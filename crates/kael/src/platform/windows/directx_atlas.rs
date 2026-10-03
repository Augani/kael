use collections::FxHashMap;
use etagere::BucketedAtlasAllocator;
use parking_lot::Mutex;
use std::borrow::Cow;
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    },
    Dxgi::Common::*,
};

use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
    DevicePixels, PlatformAtlas, Point, Size,
    platform::{AtlasTextureList, AtlasTileAllocations, allocate_native_atlas_texture_id},
};

pub(crate) struct DirectXAtlas(Mutex<DirectXAtlasState>);

struct DirectXAtlasState {
    device: ID3D11Device,
    device_context: ID3D11DeviceContext,
    monochrome_textures: AtlasTextureList<DirectXAtlasTexture>,
    polychrome_textures: AtlasTextureList<DirectXAtlasTexture>,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    policy: crate::AtlasPolicy,
}

struct DirectXAtlasTexture {
    id: AtlasTextureId,
    allocation_class: AtlasAllocationClass,
    bytes_per_pixel: u32,
    allocator: BucketedAtlasAllocator,
    allocations: AtlasTileAllocations,
    texture: ID3D11Texture2D,
    view: [Option<ID3D11ShaderResourceView>; 1],
    live_atlas_keys: u32,
    allocation_bytes: u64,
}

impl DirectXAtlas {
    pub(crate) fn new(device: &ID3D11Device, device_context: &ID3D11DeviceContext) -> Self {
        DirectXAtlas(Mutex::new(DirectXAtlasState {
            device: device.clone(),
            device_context: device_context.clone(),
            monochrome_textures: Default::default(),
            polychrome_textures: Default::default(),
            tiles_by_key: Default::default(),
            policy: Default::default(),
        }))
    }

    pub(crate) fn get_texture_view(
        &self,
        id: AtlasTextureId,
    ) -> anyhow::Result<[Option<ID3D11ShaderResourceView>; 1]> {
        let lock = self.0.lock();
        Ok(lock.texture(id)?.view.clone())
    }

    pub(crate) fn get_texture(&self, id: AtlasTextureId) -> anyhow::Result<ID3D11Texture2D> {
        let lock = self.0.lock();
        Ok(lock.texture(id)?.texture.clone())
    }

    pub(crate) fn handle_device_lost(
        &self,
        device: &ID3D11Device,
        device_context: &ID3D11DeviceContext,
    ) {
        let mut lock = self.0.lock();
        lock.device = device.clone();
        lock.device_context = device_context.clone();
        lock.monochrome_textures = AtlasTextureList::default();
        lock.polychrome_textures = AtlasTextureList::default();
        lock.tiles_by_key.clear();
        lock.policy.reset_runtime();
    }

    pub(crate) fn mark_scene_used(&self, scene: &crate::Scene) -> anyhow::Result<()> {
        let mut state = self.0.lock();
        anyhow::ensure!(
            scene.atlas_tiles().all(|tile| state
                .texture(tile.texture_id)
                .ok()
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

    pub(crate) fn evict_to_budget_keeping(&self, max_bytes: u64, keep_recent_frames: u64) -> usize {
        let mut state = self.0.lock();
        let guard = state.policy.guard(keep_recent_frames);
        state.evict_to_budget_with_guard(max_bytes, guard)
    }
}

impl PlatformAtlas for DirectXAtlas {
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
}

impl DirectXAtlasState {
    fn page_count(&self) -> usize {
        self.monochrome_textures
            .textures
            .iter()
            .chain(&self.polychrome_textures.textures)
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
                self.allocate(size, key.texture_kind(), key.allocation_class(size))?
                    .ok_or_else(|| anyhow::anyhow!("failed to allocate atlas tile"))
            })
            .transpose()?;
        let built = build();
        let (size, bytes) = match built {
            Ok(Some(value)) => value,
            other => {
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
                .ok_or_else(|| anyhow::anyhow!("failed to allocate atlas tile"))?
        };
        self.texture(tile.texture_id)?
            .upload(&self.device_context, tile.bounds, &bytes);
        self.policy.touch(&tile);
        self.tiles_by_key.insert(key.clone(), tile.clone());
        Ok(Some(tile))
    }

    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
    ) -> anyhow::Result<Option<AtlasTile>> {
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

        let texture = self.push_texture(size, texture_kind, allocation_class)?;
        Ok(texture.allocate(size))
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
    ) -> anyhow::Result<&mut DirectXAtlasTexture> {
        const DEFAULT_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(1024),
            height: DevicePixels(1024),
        };
        // Max texture size for DirectX. See:
        // https://learn.microsoft.com/en-us/windows/win32/direct3d11/overviews-direct3d-11-resources-limits
        const MAX_ATLAS_SIZE: Size<DevicePixels> = Size {
            width: DevicePixels(16384),
            height: DevicePixels(16384),
        };
        let size = allocation_class.texture_size(min_size, DEFAULT_ATLAS_SIZE, MAX_ATLAS_SIZE);
        let added_bytes = crate::atlas_payload_len(size, kind)? as u64;
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
        let pixel_format;
        let bind_flag;
        let bytes_per_pixel;
        match kind {
            AtlasTextureKind::Monochrome => {
                pixel_format = DXGI_FORMAT_R8_UNORM;
                bind_flag = D3D11_BIND_SHADER_RESOURCE;
                bytes_per_pixel = 1;
            }
            AtlasTextureKind::Polychrome => {
                pixel_format = DXGI_FORMAT_B8G8R8A8_UNORM;
                bind_flag = D3D11_BIND_SHADER_RESOURCE;
                bytes_per_pixel = 4;
            }
        }
        let id = allocate_native_atlas_texture_id(kind)?;
        let texture_desc = D3D11_TEXTURE2D_DESC {
            Width: u32::try_from(size.width.0)
                .map_err(|_| anyhow::anyhow!("invalid DirectX atlas width"))?,
            Height: u32::try_from(size.height.0)
                .map_err(|_| anyhow::anyhow!("invalid DirectX atlas height"))?,
            MipLevels: 1,
            ArraySize: 1,
            Format: pixel_format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind_flag.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut texture: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&texture_desc, None, Some(&mut texture))
                .map_err(|error| anyhow::anyhow!("creating DirectX atlas texture: {error}"))?;
        }
        let texture = texture.ok_or_else(|| {
            anyhow::anyhow!("CreateTexture2D succeeded without returning an atlas texture")
        })?;

        let texture_list = match kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        };
        let view = unsafe {
            let mut view = None;
            self.device
                .CreateShaderResourceView(&texture, None, Some(&mut view))
                .map_err(|error| anyhow::anyhow!("creating DirectX atlas view: {error}"))?;
            [Some(view.ok_or_else(|| {
                anyhow::anyhow!(
                    "CreateShaderResourceView succeeded without returning an atlas view"
                )
            })?)]
        };
        let atlas_texture = DirectXAtlasTexture {
            id,
            allocation_class,
            bytes_per_pixel,
            allocator: etagere::BucketedAtlasAllocator::new(size.into()),
            allocations: AtlasTileAllocations::default(),
            texture,
            view,
            live_atlas_keys: 0,
            allocation_bytes: u64::from(texture_desc.Width)
                .saturating_mul(u64::from(texture_desc.Height))
                .saturating_mul(u64::from(bytes_per_pixel)),
        };
        Ok(texture_list.insert(id.index, atlas_texture))
    }

    fn texture(&self, id: AtlasTextureId) -> anyhow::Result<&DirectXAtlasTexture> {
        let textures = match id.kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures
            .get(id.index)
            .filter(|texture| texture.id == id)
            .ok_or_else(|| anyhow::anyhow!("stale or invalid DirectX atlas texture id: {id:?}"))
    }

    fn allocated_bytes(&self) -> u64 {
        self.monochrome_textures
            .textures
            .iter()
            .chain(&self.polychrome_textures.textures)
            .filter_map(Option::as_ref)
            .fold(0u64, |total, texture| {
                total.saturating_add(texture.allocation_bytes)
            })
    }

    fn evict_to_budget_with_guard(&mut self, max_bytes: u64, guard_frame: u64) -> usize {
        if self.allocated_bytes() <= max_bytes {
            return 0;
        }
        let mut candidates: Vec<(AtlasKey, u64)> = self
            .tiles_by_key
            .keys()
            .map(|key| (key.clone(), self.policy.last_used(&self.tiles_by_key[key])))
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
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        };
        let Some(texture) = textures.get_mut(id.index) else {
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
            textures.remove(id.index);
        }
    }
}

impl DirectXAtlasTexture {
    fn allocate(&mut self, size: Size<DevicePixels>) -> Option<AtlasTile> {
        let tile = self
            .allocations
            .allocate(&mut self.allocator, self.id, size)?;
        self.live_atlas_keys += 1;
        Some(tile)
    }

    fn upload(
        &self,
        device_context: &ID3D11DeviceContext,
        bounds: Bounds<DevicePixels>,
        bytes: &[u8],
    ) {
        unsafe {
            device_context.UpdateSubresource(
                &self.texture,
                0,
                Some(&D3D11_BOX {
                    left: bounds.left().0 as u32,
                    top: bounds.top().0 as u32,
                    front: 0,
                    right: bounds.right().0 as u32,
                    bottom: bounds.bottom().0 as u32,
                    back: 1,
                }),
                bytes.as_ptr() as _,
                bounds.size.width.to_bytes(self.bytes_per_pixel as u8),
                0,
            );
        }
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys = self.live_atlas_keys.checked_sub(1).unwrap_or_else(|| {
            log::error!("DirectX atlas live-key count underflow prevented");
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
