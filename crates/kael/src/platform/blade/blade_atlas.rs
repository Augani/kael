use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
    DevicePixels, PlatformAtlas, Point, Size,
    platform::{AtlasTextureList, AtlasTileAllocations, allocate_native_atlas_texture_id},
};
use anyhow::{Context as _, Result};
use blade_graphics as gpu;
use collections::FxHashMap;
use etagere::BucketedAtlasAllocator;
use parking_lot::Mutex;
use std::{borrow::Cow, ops, sync::Arc};

pub(crate) struct BladeAtlas(Mutex<BladeAtlasState>);

struct PendingUpload {
    id: AtlasTextureId,
    bounds: Bounds<DevicePixels>,
    data: gpu::BufferPiece,
}

struct AtlasUploadChunk {
    raw: gpu::Buffer,
    size: u64,
    used: u64,
    sync: Option<gpu::SyncPoint>,
}

struct UploadReservation {
    data: gpu::BufferPiece,
    chunk: usize,
    previous_used: u64,
}

#[derive(Default)]
struct AtlasUploadBelt {
    chunks: Vec<AtlasUploadChunk>,
}

impl AtlasUploadBelt {
    fn allocated_bytes(&self) -> u64 {
        self.chunks.iter().map(|chunk| chunk.size).sum()
    }

    fn reserve(
        &mut self,
        bytes: u64,
        max_bytes: u64,
        gpu: &gpu::Context,
    ) -> Result<UploadReservation> {
        for (index, chunk) in self.chunks.iter_mut().enumerate() {
            if let Some(sync) = &chunk.sync {
                if !gpu.wait_for(sync, 0).unwrap_or(false) {
                    continue;
                }
                chunk.sync = None;
                chunk.used = 0;
            }
            let aligned = chunk
                .used
                .checked_add(63)
                .context("atlas upload alignment overflow")?
                & !63;
            if aligned
                .checked_add(bytes)
                .is_some_and(|end| end <= chunk.size)
            {
                let previous_used = chunk.used;
                chunk.used = aligned + bytes;
                return Ok(UploadReservation {
                    data: chunk.raw.at(aligned),
                    chunk: index,
                    previous_used,
                });
            }
        }
        // Completed staging allocations are expendable; pending submissions
        // and unsubmitted copies retain their exact buffers.
        self.trim(gpu);
        let size = bytes.max(65_536);
        anyhow::ensure!(
            self.allocated_bytes()
                .checked_add(size)
                .is_some_and(|total| total <= max_bytes),
            "Blade atlas upload byte admission limit reached"
        );
        let raw = gpu.create_buffer(gpu::BufferDesc {
            name: "bounded atlas upload",
            size,
            memory: gpu::Memory::Upload,
        });
        let chunk = self.chunks.len();
        self.chunks.push(AtlasUploadChunk {
            raw,
            size,
            used: bytes,
            sync: None,
        });
        Ok(UploadReservation {
            data: raw.into(),
            chunk,
            previous_used: 0,
        })
    }

    fn rollback(&mut self, reservation: UploadReservation) {
        self.chunks[reservation.chunk].used = reservation.previous_used;
    }

    fn flush(&mut self, sync: &gpu::SyncPoint) {
        for chunk in &mut self.chunks {
            if chunk.sync.is_none() && chunk.used > 0 {
                chunk.sync = Some(sync.clone());
            }
        }
    }

    fn trim(&mut self, gpu: &gpu::Context) {
        let mut index = 0;
        while index < self.chunks.len() {
            let completed = self.chunks[index]
                .sync
                .as_ref()
                .is_some_and(|sync| gpu.wait_for(sync, 0).unwrap_or(false));
            if completed || (self.chunks[index].sync.is_none() && self.chunks[index].used == 0) {
                let chunk = self.chunks.remove(index);
                gpu.destroy_buffer(chunk.raw);
            } else {
                index += 1;
            }
        }
    }

    fn destroy(&mut self, gpu: &gpu::Context) {
        for chunk in self.chunks.drain(..) {
            gpu.destroy_buffer(chunk.raw);
        }
    }
}

struct BladeAtlasState {
    gpu: Arc<gpu::Context>,
    upload_belt: AtlasUploadBelt,
    storage: BladeAtlasStorage,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    policy: crate::AtlasPolicy,
    initializations: Vec<AtlasTextureId>,
    uploads: Vec<PendingUpload>,
}

#[cfg(gles)]
unsafe impl Send for BladeAtlasState {}

impl BladeAtlasState {
    fn destroy(&mut self) {
        self.storage.destroy(&self.gpu);
        self.upload_belt.destroy(&self.gpu);
    }
}

pub struct BladeTextureInfo {
    pub raw_texture: gpu::Texture,
    pub raw_view: gpu::TextureView,
}

impl BladeAtlas {
    pub(crate) fn new(gpu: &Arc<gpu::Context>) -> Self {
        BladeAtlas(Mutex::new(BladeAtlasState {
            gpu: Arc::clone(gpu),
            upload_belt: AtlasUploadBelt::default(),
            storage: BladeAtlasStorage::default(),
            tiles_by_key: Default::default(),
            policy: Default::default(),
            initializations: Vec::new(),
            uploads: Vec::new(),
        }))
    }

    /// Advance the atlas frame clock so tiles fetched after this call are protected from
    /// eviction until the following frame.
    #[allow(dead_code)]
    pub(crate) fn mark_scene_used(&self, scene: &crate::Scene) -> anyhow::Result<()> {
        let mut state = self.0.lock();
        anyhow::ensure!(
            scene.atlas_tiles().all(|tile| state
                .storage
                .get(tile.texture_id)
                .is_some_and(|texture| texture.allocations.contains(tile))),
            "scene contains a stale or foreign atlas tile"
        );
        state.policy.mark_scene_used(scene);
        Ok(())
    }

    pub(crate) fn set_admission_limits(&self, bytes: Option<u64>) {
        self.0.lock().policy.set_soft_budget(bytes);
    }

    pub(crate) fn advance_frame(&self) {
        let mut state = self.0.lock();
        for tile in state.policy.advance() {
            state.release_tile(tile);
        }
    }

    /// Evict least-recently-used tiles until allocated atlas texture pages fit `max_bytes`,
    /// protecting tiles used within the last `keep_recent_frames`
    /// frames. Returns the number of tiles evicted.
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

    pub(crate) fn destroy(&self) {
        self.0.lock().destroy();
    }

    pub fn before_frame(&self, gpu_encoder: &mut gpu::CommandEncoder) {
        let mut lock = self.0.lock();
        lock.flush(gpu_encoder);
    }

    pub fn after_frame(&self, sync_point: &gpu::SyncPoint) {
        let mut lock = self.0.lock();
        lock.upload_belt.flush(sync_point);
    }

    pub fn get_texture_info(&self, id: AtlasTextureId) -> Option<BladeTextureInfo> {
        let lock = self.0.lock();
        let texture = lock.storage.get(id)?;
        Some(BladeTextureInfo {
            raw_texture: texture.raw,
            raw_view: texture.raw_view,
        })
    }
}

impl PlatformAtlas for BladeAtlas {
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
        self.0.lock().policy.needs_retirement_frames()
    }

    fn remove(&self, key: &AtlasKey) {
        let mut state = self.0.lock();
        if let Some(tile) = state.tiles_by_key.remove(key) {
            state.policy.retire(tile);
        }
    }
    #[cfg(target_arch = "wasm32")]
    fn clear(&self) {
        let keys = self
            .0
            .lock()
            .tiles_by_key
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            self.remove(&key);
        }
    }
}

impl BladeAtlasState {
    fn page_count(&self) -> usize {
        self.storage
            .monochrome_textures
            .textures
            .iter()
            .chain(&self.storage.polychrome_textures.textures)
            .filter(|page| page.is_some())
            .count()
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
            .map(|size| -> anyhow::Result<_> {
                self.allocate(size, key.texture_kind(), key.allocation_class(size))
            })
            .transpose()?;
        let upload = if let Some(tile) = &reserved {
            match self.upload_belt.reserve(
                raster_bytes as u64,
                self.policy
                    .limits
                    .max_bytes
                    .saturating_sub(self.texture_bytes()),
                &self.gpu,
            ) {
                Ok(upload) => Some(upload),
                Err(error) => {
                    self.release_tile(tile.clone());
                    return Err(error);
                }
            }
        } else {
            None
        };
        let built = build();
        let (size, bytes) = match built {
            Ok(Some(value)) => value,
            other => {
                if let Some(upload) = upload {
                    self.upload_belt.rollback(upload);
                }
                if let Some(tile) = reserved {
                    self.release_tile(tile);
                }
                return other.map(|_| None);
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
            if let Some(upload) = upload {
                self.upload_belt.rollback(upload);
            }
            if let Some(tile) = reserved {
                self.release_tile(tile);
            }
            return Err(error);
        }
        let tile = if let Some(tile) = reserved {
            tile
        } else {
            self.policy.check_tile(bytes.len())?;
            self.allocate(size, key.texture_kind(), key.allocation_class(size))?
        };
        if let Err(error) = self.upload_texture(tile.texture_id, tile.bounds, &bytes, upload) {
            self.release_tile(tile);
            return Err(error);
        }
        self.policy.touch(&tile);
        self.tiles_by_key.insert(key.clone(), tile.clone());
        Ok(Some(tile))
    }

    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
    ) -> Result<AtlasTile> {
        {
            let textures = &mut self.storage[texture_kind];

            if let Some(tile) = textures.iter_mut().rev().find_map(|texture| {
                (texture.allocation_class == allocation_class)
                    .then(|| texture.allocate(size))
                    .flatten()
            }) {
                return Ok(tile);
            }
        }

        let texture = self.push_texture(size, texture_kind, allocation_class)?;
        texture
            .allocate(size)
            .ok_or_else(|| anyhow::anyhow!("new Blade atlas texture could not fit requested tile"))
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
    ) -> Result<&mut BladeAtlasTexture> {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };

        const MAX_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(16384),
            height: DevicePixels(16384),
        };

        let size = allocation_class.texture_size(min_size, DEFAULT_ATLAS_SIZE, MAX_ATLAS_SIZE);
        let added_bytes = crate::atlas_payload_len(size, kind)? as u64;
        self.upload_belt.trim(&self.gpu);
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
        let format;
        let usage;
        match kind {
            AtlasTextureKind::Monochrome => {
                format = gpu::TextureFormat::R8Unorm;
                usage = gpu::TextureUsage::COPY | gpu::TextureUsage::RESOURCE;
            }
            AtlasTextureKind::Polychrome => {
                format = gpu::TextureFormat::Bgra8Unorm;
                usage = gpu::TextureUsage::COPY | gpu::TextureUsage::RESOURCE;
            }
        }

        let id = allocate_native_atlas_texture_id(kind)?;
        let raw = self.gpu.create_texture(gpu::TextureDesc {
            name: "atlas",
            format,
            size: gpu::Extent {
                width: size.width.into(),
                height: size.height.into(),
                depth: 1,
            },
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: gpu::TextureDimension::D2,
            usage,
            external: None,
        });
        let raw_view = self.gpu.create_texture_view(
            raw,
            gpu::TextureViewDesc {
                name: "",
                format,
                dimension: gpu::ViewDimension::D2,
                subresources: &Default::default(),
            },
        );

        let texture_list = &mut self.storage[kind];
        let atlas_texture = BladeAtlasTexture {
            id,
            allocation_class,
            allocator: etagere::BucketedAtlasAllocator::new(size.into()),
            allocations: AtlasTileAllocations::default(),
            format,
            raw,
            raw_view,
            live_atlas_keys: 0,
            allocation_bytes: u64::from(size.width.0 as u32)
                .saturating_mul(u64::from(size.height.0 as u32))
                .saturating_mul(u64::from(format.block_info().size)),
        };

        self.initializations.push(atlas_texture.id);

        Ok(texture_list.insert(id.index, atlas_texture))
    }

    fn upload_texture(
        &mut self,
        id: AtlasTextureId,
        bounds: Bounds<DevicePixels>,
        bytes: &[u8],
        reserved: Option<UploadReservation>,
    ) -> Result<()> {
        let reservation = if let Some(reserved) = reserved {
            reserved
        } else {
            self.upload_belt.reserve(
                bytes.len() as u64,
                self.policy
                    .limits
                    .max_bytes
                    .saturating_sub(self.texture_bytes()),
                &self.gpu,
            )?
        };
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), reservation.data.data(), bytes.len());
        }
        self.uploads.push(PendingUpload {
            id,
            bounds,
            data: reservation.data,
        });
        Ok(())
    }

    fn evict_to_budget_with_guard(&mut self, max_bytes: u64, guard_frame: u64) -> usize {
        self.upload_belt.trim(&self.gpu);
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
            .saturating_add(self.upload_belt.allocated_bytes())
    }

    fn texture_bytes(&self) -> u64 {
        self.storage
            .monochrome_textures
            .textures
            .iter()
            .chain(&self.storage.polychrome_textures.textures)
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
        let id = tile.texture_id;
        let Some(texture) = self.storage[id.kind].get_mut(id.index) else {
            return;
        };
        if texture.id != id {
            return;
        }
        let Some(allocation) = texture.allocations.release(&tile) else {
            return;
        };
        texture.allocator.deallocate(allocation);
        texture.decrement_ref_count();
        if texture.is_unreferenced() {
            let mut texture = self.storage[id.kind]
                .remove(id.index)
                .expect("live atlas page");
            self.initializations.retain(|pending| *pending != id);
            self.uploads.retain(|pending| pending.id != id);
            for chunk in &mut self.upload_belt.chunks {
                if chunk.sync.is_none()
                    && !self
                        .uploads
                        .iter()
                        .any(|upload| upload.data.buffer == chunk.raw)
                {
                    chunk.used = 0;
                }
            }
            self.upload_belt.trim(&self.gpu);
            texture.destroy(&self.gpu);
        }
    }

    fn flush_initializations(&mut self, encoder: &mut gpu::CommandEncoder) {
        for id in self.initializations.drain(..) {
            let Some(texture) = self.storage.get(id) else {
                continue;
            };
            encoder.init_texture(texture.raw);
        }
    }

    fn flush(&mut self, encoder: &mut gpu::CommandEncoder) {
        self.flush_initializations(encoder);

        let mut transfers = encoder.transfer("atlas");
        for upload in self.uploads.drain(..) {
            let Some(texture) = self.storage.get(upload.id) else {
                continue;
            };
            transfers.copy_buffer_to_texture(
                upload.data,
                upload.bounds.size.width.to_bytes(texture.bytes_per_pixel()),
                gpu::TexturePiece {
                    texture: texture.raw,
                    mip_level: 0,
                    array_layer: 0,
                    origin: [
                        upload.bounds.origin.x.into(),
                        upload.bounds.origin.y.into(),
                        0,
                    ],
                },
                gpu::Extent {
                    width: upload.bounds.size.width.into(),
                    height: upload.bounds.size.height.into(),
                    depth: 1,
                },
            );
        }
    }
}

#[derive(Default)]
struct BladeAtlasStorage {
    monochrome_textures: AtlasTextureList<BladeAtlasTexture>,
    polychrome_textures: AtlasTextureList<BladeAtlasTexture>,
}

impl ops::Index<AtlasTextureKind> for BladeAtlasStorage {
    type Output = AtlasTextureList<BladeAtlasTexture>;
    fn index(&self, kind: AtlasTextureKind) -> &Self::Output {
        match kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        }
    }
}

impl ops::IndexMut<AtlasTextureKind> for BladeAtlasStorage {
    fn index_mut(&mut self, kind: AtlasTextureKind) -> &mut Self::Output {
        match kind {
            crate::AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        }
    }
}

impl BladeAtlasStorage {
    fn get(&self, id: AtlasTextureId) -> Option<&BladeAtlasTexture> {
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &self.monochrome_textures,
            AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures.get(id.index).filter(|texture| texture.id == id)
    }

    fn destroy(&mut self, gpu: &gpu::Context) {
        for mut texture in self.monochrome_textures.drain().flatten() {
            texture.destroy(gpu);
        }
        for mut texture in self.polychrome_textures.drain().flatten() {
            texture.destroy(gpu);
        }
    }
}

struct BladeAtlasTexture {
    id: AtlasTextureId,
    allocation_class: AtlasAllocationClass,
    allocator: BucketedAtlasAllocator,
    allocations: AtlasTileAllocations,
    raw: gpu::Texture,
    raw_view: gpu::TextureView,
    format: gpu::TextureFormat,
    live_atlas_keys: u32,
    allocation_bytes: u64,
}

impl BladeAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let tile = self
            .allocations
            .allocate(&mut self.allocator, self.id, size)?;
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn destroy(&mut self, gpu: &gpu::Context) {
        gpu.destroy_texture(self.raw);
        gpu.destroy_texture_view(self.raw_view);
    }

    fn bytes_per_pixel(&self) -> u8 {
        self.format.block_info().size
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys = self.live_atlas_keys.checked_sub(1).unwrap_or_else(|| {
            log::error!("Blade atlas live-key count underflow prevented");
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

#[cfg(test)]
mod admission_tests {
    use super::*;
    use crate::{AtlasAdmissionLimits, ImageId, RenderImageParams, size};

    #[test]
    fn blade_atlas_admission_accounts_real_upload_buffers_before_raster() {
        let gpu = Arc::new(
            unsafe {
                gpu::Context::init(gpu::ContextDesc {
                    presentation: false,
                    validation: false,
                    ..Default::default()
                })
            }
            .expect("native Blade GPU required"),
        );
        let atlas = BladeAtlas::new(&gpu);
        let key = AtlasKey::Image(RenderImageParams {
            image_id: ImageId(9700),
            frame_index: 0,
        });
        let tile_size = size(DevicePixels(256), DevicePixels(256));
        const PAGE: u64 = 256 * 256 * 4;
        atlas.set_hard_admission_limits(AtlasAdmissionLimits {
            max_bytes: PAGE,
            max_tiles: 2,
            max_pages: 2,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&key, tile_size, &mut || panic!(
                    "staging bytes must be admitted before raster"
                ))
                .is_err()
        );
        assert_eq!(
            atlas.0.lock().allocated_bytes(),
            0,
            "failed reservation releases its actual texture"
        );
        atlas.set_hard_admission_limits(AtlasAdmissionLimits {
            max_bytes: PAGE * 2,
            max_tiles: 2,
            max_pages: 2,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&key, tile_size, &mut || Ok(None))
                .unwrap()
                .is_none()
        );
        assert_eq!(atlas.0.lock().texture_bytes(), 0);
        let tile = atlas
            .get_or_insert_with_size(&key, tile_size, &mut || {
                Ok(Some((tile_size, Cow::Owned(vec![17; PAGE as usize]))))
            })
            .unwrap()
            .unwrap();
        assert_eq!(atlas.0.lock().allocated_bytes(), PAGE * 2);
        assert_eq!(atlas.0.lock().uploads.len(), 1);
        let second = AtlasKey::Image(RenderImageParams {
            image_id: ImageId(9701),
            frame_index: 0,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&second, tile_size, &mut || panic!(
                    "resident pages and uploads must remain charged"
                ))
                .is_err()
        );
        atlas.remove(&key);
        for _ in 0..3 {
            atlas.advance_frame();
            assert!(atlas.get_texture_info(tile.texture_id).is_some());
        }
        atlas.advance_frame();
        assert!(atlas.get_texture_info(tile.texture_id).is_none());
        assert!(
            atlas.0.lock().uploads.is_empty(),
            "retired unsubmitted copies do not reference destroyed texture"
        );
        atlas.destroy();
    }
}
