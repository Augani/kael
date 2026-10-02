use crate::{AtlasTile, DevicePixels, Size};
use anyhow::{Result, ensure};
use foreign_types::ForeignType;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const CHUNK_BYTES: u64 = 8 * 1024 * 1024;
const MIN_CHUNK_BYTES: u64 = 64 * 1024;
const MAX_STAGING_BYTES: u64 = 256 * 1024 * 1024;
const MAX_CHUNKS: usize = 1024;
const MAX_PENDING_BATCHES: usize = 2;
const COMPLETION_DEADLINE: Duration = Duration::from_secs(10);

struct Chunk {
    buffer: metal::Buffer,
    resident_bytes: u64,
    used: u64,
}

struct Copy {
    buffer: metal::Buffer,
    offset: u64,
    row_bytes: u64,
    rows: u64,
    first_row: u64,
    texture: metal::Texture,
    tile: AtlasTile,
}

struct Pending {
    command: metal::CommandBuffer,
    chunks: Vec<Chunk>,
    // Explicit ownership complements Metal's retained-reference command buffers.
    textures: Vec<metal::Texture>,
    submitted: Instant,
}

enum Source {
    Active(usize),
    Spare(usize),
    New(u64),
}

struct Segment {
    source: usize,
    offset: u64,
    row_bytes: u64,
    rows: u64,
    first_row: u64,
}

pub(super) struct Plan {
    sources: Vec<Source>,
    segments: Vec<Segment>,
    additional_bytes: u64,
}

impl Plan {
    pub(super) fn additional_bytes(&self) -> u64 {
        self.additional_bytes
    }
}

pub(super) struct Reservation {
    original_chunks: usize,
    original_used: Vec<u64>,
    segments: Vec<Segment>,
    source_chunks: Vec<usize>,
}

/// CPU writes only chunks that have never been submitted or completed already.
/// Texture modifications are exclusively blits on the renderer's scene queue.
#[derive(Default)]
pub(super) struct Uploads {
    active: Vec<Chunk>,
    spare: Vec<Chunk>,
    copies: Vec<Copy>,
    pending: VecDeque<Pending>,
    failed: Option<&'static str>,
    retry_requested: bool,
}

impl Uploads {
    #[cfg(test)]
    pub(super) fn active_chunk_lengths(&self) -> Vec<u64> {
        self.active
            .iter()
            .map(|chunk| chunk.buffer.length())
            .collect()
    }

    #[cfg(test)]
    pub(super) fn expire_pending_for_test(&mut self) {
        self.pending.front_mut().unwrap().submitted = Instant::now() - COMPLETION_DEADLINE;
    }
    pub(super) fn resident_bytes(&self) -> u64 {
        self.active
            .iter()
            .chain(&self.spare)
            .chain(self.pending.iter().flat_map(|batch| &batch.chunks))
            .map(|chunk| chunk.resident_bytes)
            .sum()
    }

    pub(super) fn retained_texture_residency(
        &self,
        live: &collections::FxHashSet<usize>,
    ) -> (u64, usize) {
        let mut retained = collections::FxHashSet::default();
        let mut bytes = 0_u64;
        for texture in self.pending.iter().flat_map(|batch| &batch.textures) {
            let pointer = texture.as_ptr() as usize;
            if !live.contains(&pointer) && retained.insert(pointer) {
                let bpp = if texture.pixel_format() == metal::MTLPixelFormat::A8Unorm {
                    1
                } else {
                    4
                };
                bytes = bytes.saturating_add(
                    texture
                        .allocated_size()
                        .max(texture.width() * texture.height() * bpp),
                );
            }
        }
        (bytes, retained.len())
    }

    fn chunk_count(&self) -> usize {
        self.active.len()
            + self.spare.len()
            + self
                .pending
                .iter()
                .map(|batch| batch.chunks.len())
                .sum::<usize>()
    }

    pub(super) fn poll(&mut self) -> Result<()> {
        if let Some(message) = self.failed {
            anyhow::bail!(message);
        }
        while let Some(batch) = self.pending.front() {
            match batch.command.status() {
                metal::MTLCommandBufferStatus::Completed => {
                    let mut batch = self.pending.pop_front().unwrap();
                    for chunk in &mut batch.chunks {
                        chunk.used = 0;
                    }
                    self.spare.extend(batch.chunks);
                }
                metal::MTLCommandBufferStatus::Error => {
                    return self.fail(
                        "Metal atlas upload command failed; resources retained until teardown",
                    );
                }
                _ if batch.submitted.elapsed() >= COMPLETION_DEADLINE => {
                    return self.fail("Metal atlas upload completion deadline exceeded; resources retained until teardown");
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn fail(&mut self, message: &'static str) -> Result<()> {
        if self.failed.is_none() {
            log::error!("{message}");
        }
        self.failed = Some(message);
        anyhow::bail!(message)
    }

    pub(super) fn needs_progress(&mut self) -> bool {
        if self.poll().is_err() {
            return false;
        }
        // Completion can race the end-of-paint query after a raster was rejected.
        // Preserve one final refresh even if every command has completed already.
        let retry = self.retry_requested && self.pending.len() < MAX_PENDING_BATCHES;
        if retry {
            self.retry_requested = false;
        }
        retry || !self.copies.is_empty() || !self.pending.is_empty()
    }

    pub(super) fn failed(&self) -> bool {
        self.failed.is_some()
    }

    pub(super) fn retry_after_pending(&mut self) {
        // Permanent limits without queued work cannot recover on a new frame.
        self.retry_requested |= !self.pending.is_empty() && self.failed.is_none();
    }

    pub(super) fn trim_spare(&mut self) {
        self.spare.clear();
        if self.copies.is_empty() {
            self.active.clear();
        }
    }

    pub(super) fn plan(&mut self, size: Size<DevicePixels>, bytes_per_pixel: u64) -> Result<Plan> {
        self.poll()?;
        if self.pending.len() >= MAX_PENDING_BATCHES {
            self.retry_after_pending();
            anyhow::bail!("Metal atlas upload capacity is awaiting GPU completion");
        }
        let row_bytes = (u64::try_from(size.width.0)? * bytes_per_pixel).div_ceil(256) * 256;
        ensure!(
            row_bytes > 0 && row_bytes <= CHUNK_BYTES,
            "Metal atlas upload row exceeds chunk limit"
        );
        let height = u64::try_from(size.height.0)?;
        ensure!(
            height > 0
                && row_bytes
                    .checked_mul(height)
                    .is_some_and(|bytes| bytes <= MAX_STAGING_BYTES),
            "Metal atlas padded upload exceeds staging byte limit"
        );
        let mut sources = Vec::new();
        let mut segments = Vec::new();
        let mut first_row = 0;
        // Existing chunks are never modified by a queued GPU command.
        for (index, chunk) in self.active.iter().enumerate() {
            let rows = ((chunk.buffer.length() - chunk.used) / row_bytes).min(height - first_row);
            if rows == 0 {
                continue;
            }
            let source = sources.len();
            sources.push(Source::Active(index));
            segments.push(Segment {
                source,
                offset: chunk.used,
                row_bytes,
                rows,
                first_row,
            });
            first_row += rows;
            if first_row == height {
                break;
            }
        }
        for (index, chunk) in self.spare.iter().enumerate() {
            if first_row == height {
                break;
            }
            let rows = (chunk.buffer.length() / row_bytes).min(height - first_row);
            if rows == 0 {
                continue;
            }
            let source = sources.len();
            sources.push(Source::Spare(index));
            segments.push(Segment {
                source,
                offset: 0,
                row_bytes,
                rows,
                first_row,
            });
            first_row += rows;
        }
        let mut additional_bytes = 0_u64;
        while first_row < height {
            let rows = (CHUNK_BYTES / row_bytes).min(height - first_row);
            let capacity = (row_bytes * rows).clamp(MIN_CHUNK_BYTES, CHUNK_BYTES);
            additional_bytes = additional_bytes
                .checked_add(capacity)
                .ok_or_else(|| anyhow::anyhow!("Metal atlas staging size overflow"))?;
            let source = sources.len();
            sources.push(Source::New(capacity));
            segments.push(Segment {
                source,
                offset: 0,
                row_bytes,
                rows,
                first_row,
            });
            first_row += rows;
        }
        ensure!(
            self.resident_bytes()
                .checked_add(additional_bytes)
                .is_some_and(|bytes| bytes <= MAX_STAGING_BYTES),
            "Metal atlas staging residency limit reached"
        );
        ensure!(
            self.chunk_count()
                + sources
                    .iter()
                    .filter(|source| matches!(source, Source::New(_)))
                    .count()
                <= MAX_CHUNKS,
            "Metal atlas staging chunk count limit reached"
        );
        Ok(Plan {
            sources,
            segments,
            additional_bytes,
        })
    }

    pub(super) fn reserve(
        &mut self,
        plan: Plan,
        device: &metal::DeviceRef,
        byte_limit: u64,
    ) -> Result<Reservation> {
        let original_chunks = self.active.len();
        let original_used = self.active.iter().map(|chunk| chunk.used).collect();
        let mut source_chunks = Vec::new();
        let mut spare = std::mem::take(&mut self.spare)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        let result = (|| {
            for source in plan.sources {
                let index = match source {
                    Source::Active(index) => index,
                    Source::Spare(index) => {
                        let chunk = spare[index].take().unwrap();
                        self.active.push(chunk);
                        self.active.len() - 1
                    }
                    Source::New(capacity) => {
                        let buffer = device
                            .new_buffer(capacity, metal::MTLResourceOptions::StorageModeShared);
                        let resident_bytes = buffer.allocated_size().max(capacity);
                        ensure!(
                            !buffer.contents().is_null(),
                            "Metal atlas staging buffer is not mapped"
                        );
                        ensure!(
                            self.resident_bytes()
                                .checked_add(resident_bytes)
                                .and_then(|bytes| bytes.checked_add(
                                    spare
                                        .iter()
                                        .flatten()
                                        .map(|chunk| chunk.resident_bytes)
                                        .sum()
                                ))
                                .is_some_and(|bytes| bytes <= byte_limit.min(MAX_STAGING_BYTES)),
                            "actual Metal staging allocation exceeds atlas byte limit"
                        );
                        self.active.push(Chunk {
                            buffer,
                            resident_bytes,
                            used: 0,
                        });
                        self.active.len() - 1
                    }
                };
                source_chunks.push(index);
            }
            Ok(())
        })();
        self.spare.extend(spare.into_iter().flatten());
        let reservation = Reservation {
            original_chunks,
            original_used,
            segments: plan.segments,
            source_chunks,
        };
        if let Err(error) = result {
            self.rollback(reservation);
            return Err(error);
        }
        for segment in &reservation.segments {
            self.active[reservation.source_chunks[segment.source]].used =
                segment.offset + segment.row_bytes * segment.rows;
        }
        Ok(reservation)
    }

    pub(super) fn rollback(&mut self, reservation: Reservation) {
        self.active.truncate(reservation.original_chunks);
        for (chunk, used) in self.active.iter_mut().zip(reservation.original_used) {
            chunk.used = used;
        }
    }

    pub(super) fn write(
        &mut self,
        reservation: Reservation,
        tile: &AtlasTile,
        texture: metal::Texture,
        bytes: &[u8],
        bytes_per_pixel: u64,
    ) {
        let packed_row = tile.bounds.size.width.0 as u64 * bytes_per_pixel;
        for segment in reservation.segments {
            let chunk = &self.active[reservation.source_chunks[segment.source]];
            unsafe {
                let pointer = chunk
                    .buffer
                    .contents()
                    .cast::<u8>()
                    .add(segment.offset as usize);
                std::ptr::write_bytes(pointer, 0, (segment.row_bytes * segment.rows) as usize);
                for row in 0..segment.rows {
                    std::ptr::copy_nonoverlapping(
                        bytes
                            .as_ptr()
                            .add(((segment.first_row + row) * packed_row) as usize),
                        pointer.add((row * segment.row_bytes) as usize),
                        packed_row as usize,
                    );
                }
            }
            self.copies.push(Copy {
                buffer: chunk.buffer.clone(),
                offset: segment.offset,
                row_bytes: segment.row_bytes,
                rows: segment.rows,
                first_row: segment.first_row,
                texture: texture.clone(),
                tile: tile.clone(),
            });
        }
    }

    pub(super) fn discard_tile(&mut self, tile: &AtlasTile) {
        self.copies.retain(|copy| {
            copy.tile.texture_id != tile.texture_id || copy.tile.tile_id != tile.tile_id
        });
        if self.copies.is_empty() {
            // Admission may have prepared a plan referencing these unsubmitted
            // chunks before an old tile was evicted. Keep their indices stable.
            for chunk in &mut self.active {
                chunk.used = 0;
            }
        }
    }

    pub(super) fn flush(&mut self, queue: &metal::CommandQueueRef) -> Result<()> {
        self.poll()?;
        if self.copies.is_empty() {
            return Ok(());
        }
        ensure!(
            self.pending.len() < MAX_PENDING_BATCHES,
            "Metal atlas upload capacity is awaiting GPU completion"
        );
        let command = queue.new_command_buffer().to_owned();
        command.set_label("Kael bounded atlas upload");
        let blit = command.new_blit_command_encoder();
        for copy in &self.copies {
            blit.copy_from_buffer_to_texture(
                &copy.buffer,
                copy.offset,
                copy.row_bytes,
                copy.row_bytes * copy.rows,
                metal::MTLSize {
                    width: copy.tile.bounds.size.width.0 as u64,
                    height: copy.rows,
                    depth: 1,
                },
                &copy.texture,
                0,
                0,
                metal::MTLOrigin {
                    x: copy.tile.bounds.origin.x.0 as u64,
                    y: copy.tile.bounds.origin.y.0 as u64 + copy.first_row,
                    z: 0,
                },
                metal::MTLBlitOption::empty(),
            );
        }
        blit.end_encoding();
        let mut seen = collections::FxHashSet::default();
        let textures = self
            .copies
            .iter()
            .filter(|copy| seen.insert(copy.texture.as_ptr() as usize))
            .map(|copy| copy.texture.clone())
            .collect();
        command.commit();
        self.pending.push_back(Pending {
            command,
            chunks: std::mem::take(&mut self.active),
            textures,
            submitted: Instant::now(),
        });
        self.copies.clear();
        Ok(())
    }
}
