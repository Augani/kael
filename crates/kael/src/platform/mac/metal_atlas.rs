use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
    DevicePixels, PlatformAtlas, Point, Size, platform::AtlasTextureList,
};
use anyhow::{Context as _, Result};
use collections::FxHashMap;
use derive_more::{Deref, DerefMut};
use etagere::BucketedAtlasAllocator;
use foreign_types::ForeignType;
use metal::Device;
use parking_lot::Mutex;
use std::borrow::Cow;

mod uploads;

pub(crate) struct MetalAtlas(Mutex<MetalAtlasState>);

impl MetalAtlas {
    pub(crate) fn new(device: Device, command_queue: metal::CommandQueue) -> Self {
        MetalAtlas(Mutex::new(MetalAtlasState {
            device: AssertSend(device),
            command_queue: AssertSend(command_queue),
            uploads: uploads::Uploads::default(),
            monochrome_textures: Default::default(),
            polychrome_textures: Default::default(),
            tiles_by_key: Default::default(),
            policy: Default::default(),
        }))
    }

    /// Submit all prepaint uploads on the same queue as the next scene draw.
    /// A bounded batch remains immutable until its completion status is observed.
    pub(crate) fn flush_uploads(&self) -> Result<()> {
        let mut state = self.0.lock();
        let queue = state.command_queue.clone();
        state.uploads.flush(&queue)
    }

    pub(crate) fn metal_texture(&self, id: AtlasTextureId) -> Option<metal::Texture> {
        self.0
            .lock()
            .texture(id)
            .map(|texture| texture.metal_texture.clone())
    }

    /// Refresh recency for every tile referenced by a retained scene.
    #[allow(dead_code)]
    pub(crate) fn mark_scene_used(&self, scene: &crate::Scene) {
        self.0.lock().policy.mark_scene_used(scene);
    }

    pub(crate) fn set_admission_limits(&self, bytes: Option<u64>) {
        self.0.lock().policy.set_soft_budget(bytes);
    }

    /// Advance after successful submission through the three-drawable gate.
    /// Four retained frames include the upcoming prepaint before drawable acquisition;
    /// offscreen scene paths synchronously complete without advancing this clock.
    pub(crate) fn advance_frame(&self) {
        let mut state = self.0.lock();
        if state.uploads.poll().is_err() {
            return;
        }
        for tile in state.policy.advance() {
            state.release_tile(tile);
        }
    }

    /// Evict least-recently-used tiles until allocated atlas texture pages fit `max_bytes`,
    /// never evicting a tile used in the current frame. Returns the
    /// number of tiles evicted.
    #[allow(dead_code)]
    pub(crate) fn evict_to_budget(&self, max_bytes: u64) -> usize {
        self.0.lock().evict_to_budget(max_bytes)
    }

    /// Like [`Self::evict_to_budget`], but additionally protects tiles used within the last
    /// `keep_recent_frames` frames (not just the current one). The render loop uses this with
    /// three-drawable submission bound plus the upcoming prepaint frame. Shader
    /// sampling must remain inside each retained region for that lease to hold.
    #[allow(dead_code)]
    pub(crate) fn evict_to_budget_keeping(&self, max_bytes: u64, keep_recent_frames: u64) -> usize {
        let mut lock = self.0.lock();
        let guard = lock.policy.guard(keep_recent_frames);
        lock.evict_to_budget_with_guard(max_bytes, guard)
    }

    /// The number of distinct tiles currently held.
    #[allow(dead_code)]
    pub(crate) fn tile_count(&self) -> usize {
        self.0.lock().tiles_by_key.len()
    }
}

struct MetalAtlasState {
    device: AssertSend<Device>,
    command_queue: AssertSend<metal::CommandQueue>,
    uploads: uploads::Uploads,
    monochrome_textures: AtlasTextureList<MetalAtlasTexture>,
    polychrome_textures: AtlasTextureList<MetalAtlasTexture>,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    policy: crate::AtlasPolicy,
}

impl PlatformAtlas for MetalAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> anyhow::Result<Option<AtlasTile>> {
        self.0.lock().insert(key, None, build)
    }

    fn get_or_insert_with_size<'a>(
        &self,
        key: &AtlasKey,
        size: Size<DevicePixels>,
        build: &mut dyn FnMut() -> anyhow::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> anyhow::Result<Option<AtlasTile>> {
        self.0.lock().insert(key, Some(size), build)
    }

    fn set_hard_admission_limits(&self, limits: crate::AtlasAdmissionLimits) {
        self.0.lock().policy.set_hard_limits(limits);
    }

    fn needs_retirement_frames(&self) -> bool {
        let mut state = self.0.lock();
        let upload_progress = state.uploads.needs_progress();
        !state.uploads.failed() && (state.policy.needs_retirement_frames() || upload_progress)
    }

    fn remove(&self, key: &AtlasKey) {
        let mut state = self.0.lock();
        if let Some(tile) = state.tiles_by_key.remove(key) {
            state.policy.retire(tile);
        }
    }
}

impl MetalAtlasState {
    fn retained_upload_textures(&self) -> (u64, usize) {
        let live = self
            .monochrome_textures
            .textures
            .iter()
            .chain(&self.polychrome_textures.textures)
            .filter_map(Option::as_ref)
            .map(|texture| texture.metal_texture.as_ptr() as usize)
            .collect();
        self.uploads.retained_texture_residency(&live)
    }

    fn page_count(&self) -> usize {
        self.monochrome_textures
            .textures
            .iter()
            .chain(&self.polychrome_textures.textures)
            .filter(|page| page.is_some())
            .count()
            + self.retained_upload_textures().1
    }

    fn insert<'a>(
        &mut self,
        key: &AtlasKey,
        declared: Option<Size<DevicePixels>>,
        build: &mut dyn FnMut() -> anyhow::Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> anyhow::Result<Option<AtlasTile>> {
        if let Some(tile) = self.tiles_by_key.get(key).cloned() {
            self.policy.touch(&tile);
            return Ok(Some(tile));
        }
        let raster_bytes = declared
            .map(|size| crate::atlas_payload_len(size, key.texture_kind()))
            .transpose()?
            .unwrap_or(0);
        anyhow::ensure!(
            raster_bytes as u64 <= self.policy.limits.max_bytes,
            "atlas raster exceeds byte admission limit"
        );
        if self.policy.check_tile(raster_bytes).is_err() {
            let guard = self.policy.guard(4);
            let mut candidates: Vec<_> = self
                .tiles_by_key
                .iter()
                .filter(|(key, tile)| {
                    !matches!(key, AtlasKey::CachedSurface(_))
                        && self.policy.last_used(tile) < guard
                })
                .map(|(key, tile)| {
                    (
                        key.clone(),
                        self.policy.last_used(tile),
                        tile.texture_id.kind as u32,
                        tile.texture_id.index,
                        tile.tile_id.0,
                    )
                })
                .collect();
            candidates.sort_by_key(|(_, age, kind, page, tile)| (*age, *kind, *page, *tile));
            for (key, ..) in candidates {
                if self.policy.check_tile(raster_bytes).is_ok() {
                    break;
                }
                self.evict_tile(&key);
            }
        }
        self.policy.check_tile(raster_bytes)?;
        let reserved = declared
            .map(|size| self.reserve_upload(size, key.texture_kind(), key.allocation_class(size)))
            .transpose()?;
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(build));
        let (size, bytes) = match built {
            Ok(Ok(Some(value))) => value,
            other => {
                if let Some((tile, upload)) = reserved {
                    self.uploads.rollback(upload);
                    self.release_tile(tile);
                }
                return match other {
                    Ok(result) => result.map(|_| None),
                    Err(panic) => std::panic::resume_unwind(panic),
                };
            }
        };
        if let Err(error) = crate::validate_atlas_payload(size, key.texture_kind(), bytes.len())
            .and_then(|_| {
                anyhow::ensure!(
                    declared.is_none_or(|expected| expected == size),
                    "atlas raster dimensions differ from reservation"
                );
                Ok(())
            })
        {
            if let Some((tile, upload)) = reserved {
                self.uploads.rollback(upload);
                self.release_tile(tile);
            }
            return Err(error);
        }
        let (tile, upload) = if let Some(reservation) = reserved {
            reservation
        } else {
            self.policy.check_tile(bytes.len())?;
            self.reserve_upload(size, key.texture_kind(), key.allocation_class(size))?
        };
        let texture = self
            .texture(tile.texture_id)
            .context("Metal atlas texture missing")?
            .metal_texture
            .clone();
        self.uploads.write(
            upload,
            &tile,
            texture,
            &bytes,
            match key.texture_kind() {
                AtlasTextureKind::Monochrome => 1,
                AtlasTextureKind::Polychrome => 4,
            },
        );
        self.policy.touch(&tile);
        self.tiles_by_key.insert(key.clone(), tile.clone());
        Ok(Some(tile))
    }

    fn reserve_upload(
        &mut self,
        size: Size<DevicePixels>,
        kind: AtlasTextureKind,
        class: AtlasAllocationClass,
    ) -> Result<(AtlasTile, uploads::Reservation)> {
        let result = self.try_reserve_upload(size, kind, class);
        if result.is_err() {
            self.uploads.retry_after_pending();
        }
        result
    }

    fn try_reserve_upload(
        &mut self,
        size: Size<DevicePixels>,
        kind: AtlasTextureKind,
        class: AtlasAllocationClass,
    ) -> Result<(AtlasTile, uploads::Reservation)> {
        let bpp = match kind {
            AtlasTextureKind::Monochrome => 1,
            AtlasTextureKind::Polychrome => 4,
        };
        // The build closure may own this entire payload while both destination
        // and staging storage exist. Admit that simultaneous transient peak too.
        let raster_bytes = crate::atlas_payload_len(size, kind)? as u64;
        let mut plan = self.uploads.plan(size, bpp)?;
        if self
            .allocated_bytes()
            .checked_add(plan.additional_bytes())
            .and_then(|bytes| bytes.checked_add(raster_bytes))
            .is_none_or(|bytes| bytes > self.policy.limits.max_bytes)
        {
            self.uploads.trim_spare();
            plan = self.uploads.plan(size, bpp)?;
        }
        let tile = self
            .allocate(
                size,
                kind,
                class,
                plan.additional_bytes()
                    .checked_add(raster_bytes)
                    .context("Metal atlas transient upload peak overflow")?,
            )?
            .ok_or_else(|| anyhow::anyhow!("failed to allocate atlas tile"))?;
        let byte_limit = self.policy.limits.max_bytes.saturating_sub(
            self.texture_bytes()
                .saturating_add(self.retained_upload_textures().0)
                .saturating_add(raster_bytes),
        );
        match self.uploads.reserve(plan, &self.device, byte_limit) {
            Ok(upload) => Ok((tile, upload)),
            Err(error) => {
                self.release_tile(tile);
                Err(error)
            }
        }
    }

    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
        transient_peak_bytes: u64,
    ) -> Result<Option<AtlasTile>> {
        anyhow::ensure!(
            self.allocated_bytes()
                .checked_add(transient_peak_bytes)
                .is_some_and(|bytes| bytes <= self.policy.limits.max_bytes),
            "Metal atlas destination, staging and raster peak exceed byte admission limit"
        );
        {
            let textures = match texture_kind {
                AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
                AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
            };

            if let Some(tile) = textures.iter_mut().rev().find_map(|texture| {
                (texture.allocation_class == allocation_class)
                    .then(|| texture.allocate(size))
                    .flatten()
            }) {
                return Ok(Some(tile));
            }
        }

        let texture =
            self.push_texture(size, texture_kind, allocation_class, transient_peak_bytes)?;
        Ok(texture.allocate(size))
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
        transient_peak_bytes: u64,
    ) -> Result<&mut MetalAtlasTexture> {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };
        // Max texture size on all modern Apple GPUs. Anything bigger than that crashes in validateWithDevice.
        const MAX_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(16384),
            height: DevicePixels(16384),
        };
        let size = allocation_class.texture_size(min_size, DEFAULT_ATLAS_SIZE, MAX_ATLAS_SIZE);
        let nominal_bytes = crate::atlas_payload_len(size, kind)? as u64;
        let added_bytes = nominal_bytes
            .checked_add(transient_peak_bytes)
            .context("Metal atlas destination and transient peak size overflow")?;
        if self
            .policy
            .check_page(self.allocated_bytes(), self.page_count(), added_bytes)
            .is_err()
        {
            let guard = self.policy.guard(4);
            let mut candidates: Vec<_> = self
                .tiles_by_key
                .iter()
                .filter(|(key, tile)| {
                    !matches!(key, AtlasKey::CachedSurface(_))
                        && self.policy.last_used(tile) < guard
                })
                .map(|(key, tile)| {
                    (
                        key.clone(),
                        self.policy.last_used(tile),
                        tile.texture_id.kind as u32,
                        tile.texture_id.index,
                        tile.tile_id.0,
                    )
                })
                .collect();
            candidates.sort_by_key(|(_, age, kind, page, tile)| (*age, *kind, *page, *tile));
            for (key, ..) in candidates {
                if self
                    .policy
                    .check_page(self.allocated_bytes(), self.page_count(), added_bytes)
                    .is_ok()
                {
                    break;
                }
                self.evict_tile(&key);
            }
        }
        self.policy
            .check_page(self.allocated_bytes(), self.page_count(), added_bytes)?;
        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.into());
        texture_descriptor.set_height(size.height.into());
        let pixel_format;
        let usage;
        match kind {
            AtlasTextureKind::Monochrome => {
                pixel_format = metal::MTLPixelFormat::A8Unorm;
                usage = metal::MTLTextureUsage::ShaderRead;
            }
            AtlasTextureKind::Polychrome => {
                pixel_format = metal::MTLPixelFormat::BGRA8Unorm;
                usage = metal::MTLTextureUsage::ShaderRead;
            }
        }
        texture_descriptor.set_pixel_format(pixel_format);
        texture_descriptor.set_usage(usage);
        let metal_texture = self.device.new_texture(&texture_descriptor);
        let allocation_bytes = metal_texture.allocated_size().max(nominal_bytes);
        self.policy.check_page(
            self.allocated_bytes(),
            self.page_count(),
            allocation_bytes
                .checked_add(transient_peak_bytes)
                .context("actual Metal atlas allocation size overflow")?,
        )?;

        let texture_list = match kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        };

        let index = texture_list.free_list.pop();

        let texture_index = index.unwrap_or(texture_list.textures.len());
        let texture_index =
            u32::try_from(texture_index).context("Metal atlas texture index space exhausted")?;
        let atlas_texture = MetalAtlasTexture {
            id: AtlasTextureId {
                index: texture_index,
                kind,
            },
            allocation_class,
            allocator: etagere::BucketedAtlasAllocator::new(size.into()),
            metal_texture: AssertSend(metal_texture),
            live_atlas_keys: 0,
            allocation_bytes,
        };

        let slot = if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list.textures.get_mut(ix)
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list.textures.last_mut()
        };
        slot.and_then(Option::as_mut)
            .context("Metal atlas texture slot was not initialized")
    }

    fn texture(&self, id: AtlasTextureId) -> Option<&MetalAtlasTexture> {
        let textures = match id.kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures
            .textures
            .get(id.index as usize)
            .and_then(Option::as_ref)
            .filter(|texture| texture.id == id)
    }

    fn evict_to_budget(&mut self, max_bytes: u64) -> usize {
        let guard = self.policy.guard(1);
        self.evict_to_budget_with_guard(max_bytes, guard)
    }

    fn evict_to_budget_with_guard(&mut self, max_bytes: u64, guard_frame: u64) -> usize {
        if self.uploads.poll().is_err() {
            return 0;
        }
        self.uploads.trim_spare();
        if self.allocated_bytes() <= max_bytes {
            return 0;
        }
        let mut candidates: Vec<(AtlasKey, u64)> = self
            .tiles_by_key
            .keys()
            .map(|key| {
                let last_used = self.policy.last_used(&self.tiles_by_key[key]);
                (key.clone(), last_used)
            })
            .filter(|(key, last_used)| {
                !matches!(key, AtlasKey::CachedSurface(_)) && *last_used < guard_frame
            })
            .collect();
        candidates.sort_by_key(|(key, last_used)| {
            let tile = &self.tiles_by_key[key];
            (
                *last_used,
                tile.texture_id.kind as u32,
                tile.texture_id.index,
                tile.tile_id.0,
            )
        });
        let mut evicted = 0;
        for (key, _) in candidates {
            if self.evict_tile(&key) {
                evicted += 1;
            }
            if self.allocated_bytes() <= max_bytes {
                break;
            }
        }
        evicted
    }

    fn allocated_bytes(&self) -> u64 {
        self.texture_bytes()
            .saturating_add(self.uploads.resident_bytes())
            .saturating_add(self.retained_upload_textures().0)
    }

    fn texture_bytes(&self) -> u64 {
        self.monochrome_textures
            .textures
            .iter()
            .chain(&self.polychrome_textures.textures)
            .filter_map(Option::as_ref)
            .fold(0u64, |total, texture| {
                total.saturating_add(texture.allocation_bytes)
            })
    }

    fn evict_tile(&mut self, key: &AtlasKey) -> bool {
        let Some(tile) = self.tiles_by_key.remove(key) else {
            return false;
        };
        self.policy.forget(&tile);
        self.release_tile(tile);
        true
    }

    fn release_tile(&mut self, tile: AtlasTile) {
        self.uploads.discard_tile(&tile);
        let id = tile.texture_id;
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        };
        let Some(texture_slot) = textures.textures.get_mut(id.index as usize) else {
            return;
        };
        if texture_slot.as_ref().is_none_or(|texture| texture.id != id) {
            return;
        }

        if let Some(mut texture) = texture_slot.take() {
            texture
                .allocator
                .deallocate(etagere::AllocId::from(tile.tile_id));
            texture.decrement_ref_count();
            if texture.is_unreferenced() {
                textures.free_list.push(id.index as usize);
            } else {
                *texture_slot = Some(texture);
            }
        }
    }
}

struct MetalAtlasTexture {
    id: AtlasTextureId,
    allocation_class: AtlasAllocationClass,
    allocator: BucketedAtlasAllocator,
    metal_texture: AssertSend<metal::Texture>,
    live_atlas_keys: u32,
    allocation_bytes: u64,
}

impl MetalAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let allocation = self.allocator.allocate(size.into())?;
        let tile = AtlasTile {
            texture_id: self.id,
            tile_id: allocation.id.into(),
            bounds: Bounds {
                origin: allocation.rectangle.min.into(),
                size,
            },
            padding: 0,
        };
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys = self.live_atlas_keys.checked_sub(1).unwrap_or_else(|| {
            log::error!("Metal atlas live-key count underflow prevented");
            0
        });
    }

    fn is_unreferenced(&mut self) -> bool {
        self.live_atlas_keys == 0
    }
}

impl From<Size<DevicePixels>> for etagere::Size {
    fn from(size: Size<DevicePixels>) -> Self {
        etagere::Size::new(size.width.into(), size.height.into())
    }
}

impl From<etagere::Point> for Point<DevicePixels> {
    fn from(value: etagere::Point) -> Self {
        Point {
            x: DevicePixels::from(value.x),
            y: DevicePixels::from(value.y),
        }
    }
}

impl From<etagere::Size> for Size<DevicePixels> {
    fn from(size: etagere::Size) -> Self {
        Size {
            width: DevicePixels::from(size.width),
            height: DevicePixels::from(size.height),
        }
    }
}

impl From<etagere::Rectangle> for Bounds<DevicePixels> {
    fn from(rectangle: etagere::Rectangle) -> Self {
        Bounds {
            origin: rectangle.min.into(),
            size: rectangle.size().into(),
        }
    }
}

#[derive(Deref, DerefMut)]
struct AssertSend<T>(T);

unsafe impl<T> Send for AssertSend<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageId, PlatformAtlas, RenderImageParams, size};
    use std::borrow::Cow;

    fn wait(command: &metal::CommandBufferRef) {
        let started = std::time::Instant::now();
        loop {
            match command.status() {
                metal::MTLCommandBufferStatus::Completed => return,
                metal::MTLCommandBufferStatus::Error => {
                    panic!("Metal atlas regression command failed")
                }
                _ => assert!(
                    started.elapsed() < std::time::Duration::from_secs(10),
                    "Metal atlas regression completion deadline"
                ),
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    fn read_pixel(
        queue: &metal::CommandQueueRef,
        texture: &metal::TextureRef,
        origin: Point<DevicePixels>,
    ) -> (metal::Buffer, metal::CommandBuffer) {
        let buffer = queue
            .device()
            .new_buffer(256, metal::MTLResourceOptions::StorageModeShared);
        let command = queue.new_command_buffer().to_owned();
        let blit = command.new_blit_command_encoder();
        blit.copy_from_texture_to_buffer(
            texture,
            0,
            0,
            metal::MTLOrigin {
                x: origin.x.0 as u64,
                y: origin.y.0 as u64,
                z: 0,
            },
            metal::MTLSize {
                width: 1,
                height: 1,
                depth: 1,
            },
            &buffer,
            0,
            256,
            256,
            metal::MTLBlitOption::empty(),
        );
        blit.end_encoding();
        command.commit();
        (buffer, command)
    }

    fn pixel(buffer: &metal::BufferRef) -> [u8; 4] {
        unsafe {
            std::slice::from_raw_parts(buffer.contents().cast::<u8>(), 4)
                .try_into()
                .unwrap()
        }
    }

    struct Gate(metal::SharedEvent);
    impl Drop for Gate {
        fn drop(&mut self) {
            self.0.set_signaled_value(1);
        }
    }

    fn gate(queue: &metal::CommandQueueRef) -> Gate {
        let event = queue.device().new_shared_event();
        let command = queue.new_command_buffer();
        command.encode_wait_for_event(&event, 1);
        command.commit();
        Gate(event)
    }

    #[test]
    fn atlas_texture_uploads_preserve_queued_old_read_then_publish_new_pixels() {
        let device = metal::Device::system_default().expect("Metal device required");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device.clone(), queue.clone());
        let dimensions = size(DevicePixels(8), DevicePixels(8));
        let tile = atlas
            .get_or_insert_with_size(&image_key(0), dimensions, &mut || {
                Ok(Some((dimensions, Cow::Owned(vec![17; 8 * 8 * 4]))))
            })
            .unwrap()
            .unwrap();
        let texture = atlas.metal_texture(tile.texture_id).unwrap();
        atlas.flush_uploads().unwrap();
        let (initial, command) = read_pixel(&queue, &texture, tile.bounds.origin);
        wait(&command);
        assert_eq!(pixel(&initial), [17; 4]);
        let blocker = gate(&queue);
        let (old, old_command) = read_pixel(&queue, &texture, tile.bounds.origin);

        // Replacing this queue-ordered upload with the legacy replace_region
        // produced old=[29;4], proving that a later CPU write corrupted the
        // earlier queued read even though the submission order was correct.
        {
            let mut state = atlas.0.lock();
            let plan = state.uploads.plan(dimensions, 4).unwrap();
            let reservation = state
                .uploads
                .reserve(plan, &device, 4 * 1024 * 1024)
                .unwrap();
            state
                .uploads
                .write(reservation, &tile, texture.clone(), &[29; 8 * 8 * 4], 4);
        }
        atlas.flush_uploads().unwrap();

        let (new, new_command) = read_pixel(&queue, &texture, tile.bounds.origin);
        blocker.0.set_signaled_value(1);
        wait(&old_command);
        wait(&new_command);
        assert_eq!(
            pixel(&old),
            [17; 4],
            "an earlier queued GPU read retains the previous texture version"
        );
        assert_eq!(pixel(&new), [29; 4]);
    }

    #[test]
    fn upload_capacity_rejects_before_raster_retains_pages_and_recovers_with_progress_wake() {
        let device = metal::Device::system_default().expect("Metal device required");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue.clone());
        let blocker = gate(&queue);
        let dimensions = size(DevicePixels(8), DevicePixels(8));
        let mut tiles = Vec::new();
        for id in 0..2 {
            tiles.push(
                atlas
                    .get_or_insert_with_size(&image_key(id), dimensions, &mut || {
                        Ok(Some((
                            dimensions,
                            Cow::Owned(vec![17 + id as u8; 8 * 8 * 4]),
                        )))
                    })
                    .unwrap()
                    .unwrap(),
            );
            atlas.flush_uploads().unwrap();
        }
        let before = atlas.0.lock().allocated_bytes();
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(2), dimensions, &mut || panic!(
                    "queued capacity rejection must precede raster work"
                ))
                .is_err()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), before);
        assert_eq!(atlas.tile_count(), 2);
        assert!(
            atlas.needs_retirement_frames(),
            "Window must schedule a refresh without user input"
        );
        let texture = atlas.metal_texture(tiles[0].texture_id).unwrap();
        atlas.remove(&image_key(0));
        atlas.remove(&image_key(1));
        for _ in 0..4 {
            atlas.advance_frame();
        }
        assert!(atlas.metal_texture(tiles[0].texture_id).is_none());
        assert_eq!(
            atlas.0.lock().allocated_bytes(),
            before,
            "queued orphaned pages remain charged"
        );
        assert_eq!(atlas.0.lock().page_count(), 1);
        blocker.0.set_signaled_value(1);
        let (first, command) = read_pixel(&queue, &texture, tiles[0].bounds.origin);
        wait(&command);
        assert_eq!(pixel(&first), [17; 4]);
        assert!(
            atlas.needs_retirement_frames(),
            "completion racing the end-of-paint query still schedules one retry"
        );
        assert!(
            !atlas.needs_retirement_frames(),
            "the post-completion retry is bounded"
        );
        assert_eq!(atlas.0.lock().page_count(), 0);
        let replacement = atlas
            .get_or_insert_with_size(&image_key(2), dimensions, &mut || {
                Ok(Some((dimensions, Cow::Owned(vec![29; 8 * 8 * 4]))))
            })
            .unwrap()
            .unwrap();
        atlas.flush_uploads().unwrap();
        let texture = atlas.metal_texture(replacement.texture_id).unwrap();
        let (new, command) = read_pixel(&queue, &texture, replacement.bounds.origin);
        wait(&command);
        assert_eq!(pixel(&new), [29; 4]);
        assert!(!atlas.needs_retirement_frames());

        // A single queued batch can also hold the only affordable staging chunk.
        atlas.evict_to_budget_keeping(u64::MAX, 4);
        let blocker = gate(&queue);
        let tile = atlas
            .get_or_insert_with_size(&image_key(3), dimensions, &mut || {
                Ok(Some((dimensions, Cow::Owned(vec![31; 8 * 8 * 4]))))
            })
            .unwrap()
            .unwrap();
        atlas.flush_uploads().unwrap();
        let resident = atlas.0.lock().allocated_bytes();
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
            max_bytes: resident + 8 * 8 * 4,
            max_tiles: 8,
            max_pages: 1,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(4), dimensions, &mut || panic!(
                    "queued staging byte rejection must precede raster"
                ))
                .is_err()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), resident);
        blocker.0.set_signaled_value(1);
        let texture = atlas.metal_texture(tile.texture_id).unwrap();
        let (readback, command) = read_pixel(&queue, &texture, tile.bounds.origin);
        wait(&command);
        assert_eq!(pixel(&readback), [31; 4]);
        assert!(atlas.needs_retirement_frames());
        assert!(!atlas.needs_retirement_frames());
        let tile = atlas
            .get_or_insert_with_size(&image_key(4), dimensions, &mut || {
                Ok(Some((dimensions, Cow::Owned(vec![37; 8 * 8 * 4]))))
            })
            .unwrap()
            .unwrap();
        atlas.flush_uploads().unwrap();
        let (readback, command) = read_pixel(&queue, &texture, tile.bounds.origin);
        wait(&command);
        assert_eq!(pixel(&readback), [37; 4]);
        assert!(!atlas.needs_retirement_frames());
    }

    #[test]
    fn staging_peak_admission_and_panicking_raster_reservations_roll_back() {
        let device = metal::Device::system_default().expect("Metal device required");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let dimensions = size(DevicePixels(256), DevicePixels(256));
        const PAGE: u64 = 256 * 256 * 4;
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
            max_bytes: PAGE,
            max_tiles: 1,
            max_pages: 1,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(0), dimensions, &mut || panic!(
                    "destination plus staging peak must be admitted before work"
                ))
                .is_err()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), 0);
        assert!(
            !atlas.needs_retirement_frames(),
            "permanent byte admission failures never force retry frames"
        );
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
            max_bytes: crate::AtlasAdmissionLimits::default().max_bytes,
            max_tiles: 1,
            max_pages: 1,
        });
        atlas
            .get_or_insert_with_size(&image_key(0), dimensions, &mut || {
                Ok(Some((dimensions, Cow::Owned(vec![0; PAGE as usize]))))
            })
            .unwrap()
            .unwrap();
        let resident_peak = atlas.0.lock().allocated_bytes();
        atlas.remove(&image_key(0));
        for _ in 0..4 {
            atlas.advance_frame();
        }
        atlas.evict_to_budget_keeping(0, 4);
        assert_eq!(atlas.0.lock().allocated_bytes(), 0);
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
            max_bytes: resident_peak + PAGE - 1,
            max_tiles: 1,
            max_pages: 1,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(0), dimensions, &mut || panic!(
                    "actual resident storage plus transient raster must fit simultaneously"
                ))
                .is_err()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), 0);
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
            max_bytes: crate::AtlasAdmissionLimits::default().max_bytes,
            max_tiles: 1,
            max_pages: 1,
        });
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = atlas.get_or_insert_with_size(&image_key(0), dimensions, &mut || {
                    panic!("intentional raster unwind")
                });
            }))
            .is_err()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), 0);
        assert_eq!(atlas.tile_count(), 0);
        assert!(!atlas.needs_retirement_frames());
    }

    #[test]
    fn sixty_four_mib_image_upload_uses_bounded_chunks_and_exact_boundary_pixels() {
        let device = metal::Device::system_default().expect("Metal device required");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue.clone());
        let dimensions = size(DevicePixels(4096), DevicePixels(4096));
        const ROW: usize = 4096 * 4;
        const PAYLOAD: usize = ROW * 4096;
        let tile = atlas
            .get_or_insert_with_size(&image_key(0), dimensions, &mut || {
                let mut bytes = vec![17; PAYLOAD];
                bytes[511 * ROW..511 * ROW + 4].fill(31);
                bytes[512 * ROW..512 * ROW + 4].fill(32);
                bytes[PAYLOAD - 4..].fill(99);
                Ok(Some((dimensions, Cow::Owned(bytes))))
            })
            .unwrap()
            .unwrap();
        {
            let state = atlas.0.lock();
            assert_eq!(state.uploads.active_chunk_lengths().len(), 8);
            assert!(
                state
                    .uploads
                    .active_chunk_lengths()
                    .iter()
                    .all(|bytes| *bytes <= 8 * 1024 * 1024)
            );
            assert!(state.allocated_bytes() <= crate::AtlasAdmissionLimits::default().max_bytes);
        }
        atlas.flush_uploads().unwrap();
        let texture = atlas.metal_texture(tile.texture_id).unwrap();
        for (x, y, expected) in [(0, 0, 17), (0, 511, 31), (0, 512, 32), (4095, 4095, 99)] {
            let (readback, command) = read_pixel(
                &queue,
                &texture,
                crate::point(DevicePixels(x), DevicePixels(y)),
            );
            wait(&command);
            assert_eq!(pixel(&readback), [expected; 4]);
        }
        atlas.evict_to_budget_keeping(0, 4);
        assert_eq!(
            atlas.0.lock().uploads.resident_bytes(),
            0,
            "pressure releases completed staging only"
        );
        assert!(
            atlas.metal_texture(tile.texture_id).is_some(),
            "live page remains valid under pressure"
        );
    }

    #[test]
    fn upload_deadline_freezes_retirement_and_retains_resources_without_endless_wakes() {
        let device = metal::Device::system_default().expect("Metal device required");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue.clone());
        let blocker = gate(&queue);
        let dimensions = size(DevicePixels(8), DevicePixels(8));
        let tile = atlas
            .get_or_insert_with_size(&image_key(0), dimensions, &mut || {
                Ok(Some((dimensions, Cow::Owned(vec![17; 8 * 8 * 4]))))
            })
            .unwrap()
            .unwrap();
        atlas.flush_uploads().unwrap();
        atlas.remove(&image_key(0));
        let (frame, bytes) = {
            let mut state = atlas.0.lock();
            state.uploads.expire_pending_for_test();
            (state.policy.guard(1), state.allocated_bytes())
        };
        atlas.advance_frame();
        assert_eq!(atlas.0.lock().policy.guard(1), frame);
        assert_eq!(atlas.evict_to_budget_keeping(0, 4), 0);
        assert_eq!(atlas.0.lock().allocated_bytes(), bytes);
        assert!(!atlas.needs_retirement_frames());
        assert!(atlas.flush_uploads().is_err());
        let texture = atlas.metal_texture(tile.texture_id).unwrap();
        blocker.0.set_signaled_value(1);
        let (readback, command) = read_pixel(&queue, &texture, tile.bounds.origin);
        wait(&command);
        assert_eq!(pixel(&readback), [17; 4]);
        assert_eq!(
            atlas.0.lock().allocated_bytes(),
            bytes,
            "terminal failure retains ownership until teardown"
        );
    }

    fn image_key(id: usize) -> AtlasKey {
        AtlasKey::Image(RenderImageParams {
            image_id: ImageId(id),
            frame_index: 0,
        })
    }

    #[test]
    fn checked_admission_rejects_before_raster_and_rolls_back_failed_reservations() {
        let device =
            metal::Device::system_default().expect("Metal device required for atlas regression");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let size = size(DevicePixels(256), DevicePixels(256));
        const PAGE: u64 = 256 * 256 * 4;
        let limits = crate::AtlasAdmissionLimits {
            max_bytes: crate::AtlasAdmissionLimits::default().max_bytes,
            max_tiles: 1,
            max_pages: 1,
        };
        atlas.set_hard_admission_limits(limits);
        let mut builds = 0;
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(0), size, &mut || {
                    builds += 1;
                    Err(anyhow::anyhow!("intentional build failure"))
                })
                .is_err()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), 0);
        assert_eq!(atlas.tile_count(), 0);
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(0), size, &mut || {
                    builds += 1;
                    Ok(None)
                })
                .unwrap()
                .is_none()
        );
        assert_eq!(atlas.0.lock().allocated_bytes(), 0);
        let first = atlas
            .get_or_insert_with_size(&image_key(0), size, &mut || {
                builds += 1;
                Ok(Some((size, Cow::Owned(vec![17; PAGE as usize]))))
            })
            .unwrap()
            .unwrap();
        assert_eq!(builds, 3);
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(1), size, &mut || {
                    builds += 1;
                    panic!("count admission must precede rasterization")
                })
                .is_err()
        );
        assert_eq!(builds, 3);
        atlas.remove(&image_key(0));
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(1), size, &mut || {
                    panic!("retired region must remain charged before completion")
                })
                .is_err()
        );
        let retained = atlas.metal_texture(first.texture_id).unwrap();
        atlas.flush_uploads().unwrap();
        let (readback, command) = read_pixel(
            &atlas.0.lock().command_queue,
            &retained,
            first.bounds.origin,
        );
        wait(&command);
        assert_eq!(
            pixel(&readback),
            [17; 4],
            "actual retained texture pixels survive cache ownership release"
        );
        for _ in 0..3 {
            atlas.advance_frame();
        }
        assert!(atlas.metal_texture(first.texture_id).is_some());
        atlas.advance_frame();
        assert!(atlas.metal_texture(first.texture_id).is_none());
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(1), size, &mut || {
                    Ok(Some((size, Cow::Owned(vec![29; PAGE as usize]))))
                })
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn page_bytes_and_page_count_reject_before_raster_and_scene_replay_survives_pressure() {
        let device =
            metal::Device::system_default().expect("Metal device required for atlas regression");
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let small = size(DevicePixels(256), DevicePixels(256));
        const PAGE: u64 = 256 * 256 * 4;
        atlas.set_hard_admission_limits(crate::AtlasAdmissionLimits {
            max_bytes: crate::AtlasAdmissionLimits::default().max_bytes,
            max_tiles: 10,
            max_pages: 1,
        });
        let tile = atlas
            .get_or_insert_with_size(&image_key(0), small, &mut || {
                Ok(Some((small, Cow::Owned(vec![17; PAGE as usize]))))
            })
            .unwrap()
            .unwrap();
        let mut scene = crate::Scene::default();
        scene
            .cached_surface_snapshots
            .push(crate::CachedSurfaceSnapshot {
                paint_operations: 0..0,
                source_bounds: tile.bounds,
                target: tile.clone(),
            });
        for _ in 0..8 {
            atlas.mark_scene_used(&scene);
            atlas.advance_frame();
            assert_eq!(atlas.evict_to_budget_keeping(0, 4), 0);
        }
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(1), small, &mut || panic!(
                    "page admission must precede work"
                ))
                .is_err()
        );
        assert!(
            atlas
                .get_or_insert_with_size(
                    &image_key(2),
                    size(DevicePixels(16384), DevicePixels(16384)),
                    &mut || panic!("byte admission must precede work")
                )
                .is_err()
        );
        let texture = atlas.metal_texture(tile.texture_id).unwrap();
        assert_eq!(
            atlas.0.lock().texture_bytes(),
            texture.allocated_size().max(PAGE)
        );
        for _ in 0..4 {
            atlas.advance_frame();
        }
        assert!(
            atlas
                .get_or_insert_with_size(&image_key(1), small, &mut || {
                    Ok(Some((small, Cow::Owned(vec![29; PAGE as usize]))))
                })
                .unwrap()
                .is_some()
        );
        assert_eq!(
            atlas.tile_count(),
            1,
            "safe old dedicated page is reclaimed on admission"
        );
    }

    #[test]
    fn evicts_lru_tiles_to_budget_and_protects_the_current_frame() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let tile_size = size(DevicePixels(64), DevicePixels(64));
        const TILE_BYTES: u64 = 64 * 64 * 4;

        let mut builds = 0usize;
        for id in 0..4usize {
            atlas.advance_frame();
            atlas
                .get_or_insert_with(&image_key(id), &mut || {
                    builds += 1;
                    Ok(Some((
                        tile_size,
                        Cow::Owned(vec![0u8; TILE_BYTES as usize]),
                    )))
                })
                .unwrap();
        }
        assert_eq!(atlas.tile_count(), 4);
        assert_eq!(builds, 4, "each distinct image rasterized once");

        // Current frame is 4 (image 3 used this frame, protected). Budget = 1 tile.
        let evicted = atlas.evict_to_budget(TILE_BYTES);
        assert_eq!(
            evicted, 3,
            "the three older tiles are shed when the page exceeds budget"
        );
        assert_eq!(atlas.tile_count(), 1, "only the current-frame tile remains");

        // The protected tile survived: re-requesting it is a cache hit (no re-rasterize).
        let before = builds;
        atlas
            .get_or_insert_with(&image_key(3), &mut || {
                builds += 1;
                Ok(Some((
                    tile_size,
                    Cow::Owned(vec![0u8; TILE_BYTES as usize]),
                )))
            })
            .unwrap();
        assert_eq!(builds, before, "the current-frame tile stayed cached");

        // An evicted tile re-rasterizes (and reuses reclaimed atlas space).
        atlas
            .get_or_insert_with(&image_key(0), &mut || {
                builds += 1;
                Ok(Some((
                    tile_size,
                    Cow::Owned(vec![0u8; TILE_BYTES as usize]),
                )))
            })
            .unwrap();
        assert_eq!(
            builds,
            before + 1,
            "an evicted tile is rebuilt on next request"
        );
    }

    #[test]
    fn keep_window_protects_recent_frames_from_eviction() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let tile_size = size(DevicePixels(64), DevicePixels(64));
        const TILE_BYTES: u64 = 64 * 64 * 4;

        // Five tiles across frames 1..=5.
        for id in 0..5usize {
            atlas.advance_frame();
            atlas
                .get_or_insert_with(&image_key(id), &mut || {
                    Ok(Some((
                        tile_size,
                        Cow::Owned(vec![0u8; TILE_BYTES as usize]),
                    )))
                })
                .unwrap();
        }
        assert_eq!(atlas.tile_count(), 5);

        // Current frame is 5. Keep the last 3 frames (3,4,5) protected even though the
        // A below-page budget would otherwise shed all 5 tiles. Only frames 1 and 2 are
        // evictable, so the atlas stays over budget rather than touch the in-flight window.
        let evicted = atlas.evict_to_budget_keeping(TILE_BYTES, 3);
        assert_eq!(
            evicted, 2,
            "only the two tiles outside the keep-window are evictable"
        );
        assert_eq!(
            atlas.tile_count(),
            3,
            "the three most-recent frames are protected"
        );
    }

    #[test]
    fn removing_one_tile_reclaims_only_that_tile_and_is_idempotent() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let tile_size = size(DevicePixels(16), DevicePixels(16));
        let bytes = vec![0u8; 16 * 16 * 4];
        let mut builds = 0;

        for id in 0..2 {
            atlas
                .get_or_insert_with(&image_key(id), &mut || {
                    builds += 1;
                    Ok(Some((tile_size, Cow::Borrowed(&bytes))))
                })
                .unwrap();
        }
        assert_eq!(atlas.tile_count(), 2);

        atlas.remove(&image_key(0));
        atlas.remove(&image_key(0));
        assert_eq!(atlas.tile_count(), 1);

        atlas
            .get_or_insert_with(&image_key(0), &mut || {
                builds += 1;
                Ok(Some((tile_size, Cow::Borrowed(&bytes))))
            })
            .unwrap();
        assert_eq!(builds, 3, "the removed tile is rebuilt exactly once");
        assert_eq!(atlas.tile_count(), 2);
    }

    #[test]
    fn malformed_raster_payload_is_rejected_before_gpu_upload() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let result = atlas.get_or_insert_with(&image_key(0), &mut || {
            Ok(Some((
                size(DevicePixels(16), DevicePixels(16)),
                Cow::Owned(vec![0u8; 16]),
            )))
        });
        assert!(result.is_err());
        assert_eq!(atlas.tile_count(), 0);
    }

    #[test]
    fn budget_tracks_allocated_texture_pages_not_only_live_pixel_payloads() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let queue = device.new_command_queue();
        let atlas = MetalAtlas::new(device, queue);
        let tile_size = size(DevicePixels(256), DevicePixels(256));
        const PAGE_BYTES: u64 = 256 * 256 * 4;

        let mut page_bytes = Vec::new();
        for id in 0..2 {
            atlas.advance_frame();
            let tile = atlas
                .get_or_insert_with(&image_key(id), &mut || {
                    Ok(Some((
                        tile_size,
                        Cow::Owned(vec![0u8; PAGE_BYTES as usize]),
                    )))
                })
                .unwrap()
                .unwrap();
            page_bytes.push(
                atlas
                    .metal_texture(tile.texture_id)
                    .unwrap()
                    .allocated_size()
                    .max(PAGE_BYTES),
            );
        }
        assert_eq!(
            atlas.0.lock().texture_bytes(),
            page_bytes.iter().sum::<u64>()
        );

        // Unsubmitted staging stays owned until all its copies are discarded.
        let staging_bytes = atlas.0.lock().uploads.resident_bytes();
        assert_eq!(atlas.evict_to_budget(page_bytes[1] + staging_bytes), 1);
        assert_eq!(atlas.0.lock().texture_bytes(), page_bytes[1]);
        assert_eq!(atlas.tile_count(), 1);
    }
}
