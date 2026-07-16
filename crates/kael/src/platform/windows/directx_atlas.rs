use collections::FxHashMap;
use etagere::BucketedAtlasAllocator;
use parking_lot::Mutex;
use windows::Win32::Graphics::{
    Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
    },
    Dxgi::Common::*,
};

use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
    DevicePixels, PlatformAtlas, Point, Size, platform::AtlasTextureList,
};

pub(crate) struct DirectXAtlas(Mutex<DirectXAtlasState>);

struct DirectXAtlasState {
    device: ID3D11Device,
    device_context: ID3D11DeviceContext,
    monochrome_textures: AtlasTextureList<DirectXAtlasTexture>,
    polychrome_textures: AtlasTextureList<DirectXAtlasTexture>,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    last_used: FxHashMap<AtlasKey, u64>,
    frame: u64,
}

struct DirectXAtlasTexture {
    id: AtlasTextureId,
    allocation_class: AtlasAllocationClass,
    bytes_per_pixel: u32,
    allocator: BucketedAtlasAllocator,
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
            last_used: Default::default(),
            frame: 0,
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
        lock.last_used.clear();
    }

    pub(crate) fn advance_frame(&self) {
        let mut state = self.0.lock();
        if state.frame == u64::MAX {
            state.frame = 1;
            state.last_used.values_mut().for_each(|frame| *frame = 0);
        } else {
            state.frame += 1;
        }
    }

    pub(crate) fn evict_to_budget_keeping(&self, max_bytes: u64, keep_recent_frames: u64) -> usize {
        let mut state = self.0.lock();
        let guard = state
            .frame
            .saturating_sub(keep_recent_frames.saturating_sub(1));
        state.evict_to_budget_with_guard(max_bytes, guard)
    }
}

impl PlatformAtlas for DirectXAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<
            Option<(Size<DevicePixels>, std::borrow::Cow<'a, [u8]>)>,
        >,
    ) -> anyhow::Result<Option<AtlasTile>> {
        let mut lock = self.0.lock();
        let frame = lock.frame;
        if let Some(tile) = lock.tiles_by_key.get(key).cloned() {
            lock.last_used.insert(key.clone(), frame);
            Ok(Some(tile))
        } else {
            let Some((size, bytes)) = build()? else {
                return Ok(None);
            };
            let allocation_class = key.allocation_class(size);
            crate::validate_atlas_payload(size, key.texture_kind(), bytes.len())?;
            let tile = lock
                .allocate(size, key.texture_kind(), allocation_class)?
                .ok_or_else(|| anyhow::anyhow!("failed to allocate"))?;
            let texture = lock.texture(tile.texture_id)?;
            texture.upload(&lock.device_context, tile.bounds, &bytes);
            lock.tiles_by_key.insert(key.clone(), tile.clone());
            lock.last_used.insert(key.clone(), frame);
            Ok(Some(tile))
        }
    }

    fn remove(&self, key: &AtlasKey) {
        let mut lock = self.0.lock();

        let Some(tile) = lock.tiles_by_key.remove(key) else {
            return;
        };
        lock.last_used.remove(key);
        let id = tile.texture_id;

        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut lock.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut lock.polychrome_textures,
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
                textures.free_list.push(texture.id.index as usize);
            } else {
                *texture_slot = Some(texture);
            }
        }
    }
}

impl DirectXAtlasState {
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
        let index = texture_list.free_list.pop();
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
        let texture_index = index.unwrap_or(texture_list.textures.len());
        let texture_index = u32::try_from(texture_index)
            .map_err(|_| anyhow::anyhow!("DirectX atlas texture index space exhausted"))?;
        let atlas_texture = DirectXAtlasTexture {
            id: AtlasTextureId {
                index: texture_index,
                kind,
            },
            allocation_class,
            bytes_per_pixel,
            allocator: etagere::BucketedAtlasAllocator::new(size.into()),
            texture,
            view,
            live_atlas_keys: 0,
            allocation_bytes: u64::from(texture_desc.Width)
                .saturating_mul(u64::from(texture_desc.Height))
                .saturating_mul(u64::from(bytes_per_pixel)),
        };
        let slot = if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list.textures.get_mut(ix)
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list.textures.last_mut()
        };
        slot.and_then(Option::as_mut)
            .ok_or_else(|| anyhow::anyhow!("DirectX atlas texture slot was not initialized"))
    }

    fn texture(&self, id: AtlasTextureId) -> anyhow::Result<&DirectXAtlasTexture> {
        let textures = match id.kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures
            .textures
            .get(id.index as usize)
            .and_then(Option::as_ref)
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
            .map(|key| {
                (
                    key.clone(),
                    self.last_used.get(key).copied().unwrap_or_default(),
                )
            })
            .filter(|(_, last_used)| *last_used < guard_frame)
            .collect();
        candidates.sort_by_key(|(_, last_used)| *last_used);

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
        self.last_used.remove(key);
        let id = tile.texture_id;
        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        };
        let Some(texture_slot) = textures.textures.get_mut(id.index as usize) else {
            return true;
        };
        if texture_slot.as_ref().is_none_or(|texture| texture.id != id) {
            return true;
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
        true
    }
}

impl DirectXAtlasTexture {
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
