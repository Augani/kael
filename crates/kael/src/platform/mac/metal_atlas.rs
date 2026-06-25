use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
    DevicePixels, PlatformAtlas, Point, Size, platform::AtlasTextureList,
};
use anyhow::{Context as _, Result};
use collections::FxHashMap;
use derive_more::{Deref, DerefMut};
use etagere::BucketedAtlasAllocator;
use metal::Device;
use parking_lot::Mutex;
use std::borrow::Cow;

pub(crate) struct MetalAtlas(Mutex<MetalAtlasState>);

impl MetalAtlas {
    pub(crate) fn new(device: Device) -> Self {
        MetalAtlas(Mutex::new(MetalAtlasState {
            device: AssertSend(device),
            monochrome_textures: Default::default(),
            polychrome_textures: Default::default(),
            tiles_by_key: Default::default(),
            last_used: Default::default(),
            frame: 0,
        }))
    }

    pub(crate) fn metal_texture(&self, id: AtlasTextureId) -> metal::Texture {
        self.0.lock().texture(id).metal_texture.clone()
    }

    /// Advance the atlas frame clock. Tiles fetched after this call are stamped with the new
    /// frame and so are protected from eviction until the following frame.
    #[allow(dead_code)]
    pub(crate) fn advance_frame(&self) {
        self.0.lock().frame += 1;
    }

    /// Evict least-recently-used tiles (reclaiming their atlas regions) until total tile
    /// bytes fit `max_bytes`, never evicting a tile used in the current frame. Returns the
    /// number of tiles evicted.
    #[allow(dead_code)]
    pub(crate) fn evict_to_budget(&self, max_bytes: u64) -> usize {
        self.0.lock().evict_to_budget(max_bytes)
    }

    /// The number of distinct tiles currently held.
    #[allow(dead_code)]
    pub(crate) fn tile_count(&self) -> usize {
        self.0.lock().tiles_by_key.len()
    }
}

struct MetalAtlasState {
    device: AssertSend<Device>,
    monochrome_textures: AtlasTextureList<MetalAtlasTexture>,
    polychrome_textures: AtlasTextureList<MetalAtlasTexture>,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    last_used: FxHashMap<AtlasKey, u64>,
    frame: u64,
}

impl PlatformAtlas for MetalAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
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
            let tile = lock
                .allocate(size, key.texture_kind(), allocation_class)
                .context("failed to allocate")?;
            let texture = lock.texture(tile.texture_id);
            texture.upload(tile.bounds, &bytes);
            lock.tiles_by_key.insert(key.clone(), tile.clone());
            lock.last_used.insert(key.clone(), frame);
            Ok(Some(tile))
        }
    }

    fn remove(&self, key: &AtlasKey) {
        let mut lock = self.0.lock();
        let Some(id) = lock.tiles_by_key.get(key).map(|v| v.texture_id) else {
            return;
        };

        let textures = match id.kind {
            AtlasTextureKind::Monochrome => &mut lock.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut lock.polychrome_textures,
        };

        let Some(texture_slot) = textures
            .textures
            .iter_mut()
            .find(|texture| texture.as_ref().is_some_and(|v| v.id == id))
        else {
            return;
        };

        if let Some(mut texture) = texture_slot.take() {
            texture.decrement_ref_count();

            if texture.is_unreferenced() {
                textures.free_list.push(id.index as usize);
                lock.tiles_by_key.remove(key);
            } else {
                *texture_slot = Some(texture);
            }
        }
    }
}

impl MetalAtlasState {
    fn allocate(
        &mut self,
        size: Size<DevicePixels>,
        texture_kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
    ) -> Option<AtlasTile> {
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
                return Some(tile);
            }
        }

        let texture = self.push_texture(size, texture_kind, allocation_class);
        texture.allocate(size)
    }

    fn push_texture(
        &mut self,
        min_size: Size<DevicePixels>,
        kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
    ) -> &mut MetalAtlasTexture {
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

        let texture_list = match kind {
            AtlasTextureKind::Monochrome => &mut self.monochrome_textures,
            AtlasTextureKind::Polychrome => &mut self.polychrome_textures,
        };

        let index = texture_list.free_list.pop();

        let atlas_texture = MetalAtlasTexture {
            id: AtlasTextureId {
                index: index.unwrap_or(texture_list.textures.len()) as u32,
                kind,
            },
            allocation_class,
            allocator: etagere::BucketedAtlasAllocator::new(size.into()),
            metal_texture: AssertSend(metal_texture),
            live_atlas_keys: 0,
        };

        if let Some(ix) = index {
            texture_list.textures[ix] = Some(atlas_texture);
            texture_list.textures.get_mut(ix)
        } else {
            texture_list.textures.push(Some(atlas_texture));
            texture_list.textures.last_mut()
        }
        .unwrap()
        .as_mut()
        .unwrap()
    }

    fn texture(&self, id: AtlasTextureId) -> &MetalAtlasTexture {
        let textures = match id.kind {
            crate::AtlasTextureKind::Monochrome => &self.monochrome_textures,
            crate::AtlasTextureKind::Polychrome => &self.polychrome_textures,
        };
        textures[id.index as usize].as_ref().unwrap()
    }

    fn tile_bytes(tile: &AtlasTile) -> u64 {
        let bytes_per_pixel = match tile.texture_id.kind {
            AtlasTextureKind::Monochrome => 1u8,
            AtlasTextureKind::Polychrome => 4u8,
        };
        u64::from(tile.bounds.size.width.to_bytes(bytes_per_pixel))
            * u64::from(tile.bounds.size.height)
    }

    fn evict_to_budget(&mut self, max_bytes: u64) -> usize {
        let entries: Vec<(AtlasKey, u64, u64)> = self
            .tiles_by_key
            .iter()
            .map(|(key, tile)| {
                let last_used = self.last_used.get(key).copied().unwrap_or(0);
                (key.clone(), last_used, Self::tile_bytes(tile))
            })
            .collect();
        let total: u64 = entries.iter().map(|(_, _, bytes)| *bytes).sum();
        let policy_input: Vec<(u64, u64)> = entries
            .iter()
            .map(|(_, last_used, bytes)| (*last_used, *bytes))
            .collect();

        let victims = crate::select_atlas_evictions(&policy_input, total, max_bytes, self.frame);
        let mut evicted = 0;
        for index in victims {
            if self.evict_tile(&entries[index].0) {
                evicted += 1;
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
        let Some(texture_slot) = textures
            .textures
            .iter_mut()
            .find(|texture| texture.as_ref().is_some_and(|v| v.id == id))
        else {
            return true;
        };

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

struct MetalAtlasTexture {
    id: AtlasTextureId,
    allocation_class: AtlasAllocationClass,
    allocator: BucketedAtlasAllocator,
    metal_texture: AssertSend<metal::Texture>,
    live_atlas_keys: u32,
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

    fn upload(&self, bounds: Bounds<DevicePixels>, bytes: &[u8]) {
        let region = metal::MTLRegion::new_2d(
            bounds.origin.x.into(),
            bounds.origin.y.into(),
            bounds.size.width.into(),
            bounds.size.height.into(),
        );
        self.metal_texture.replace_region(
            region,
            0,
            bytes.as_ptr() as *const _,
            bounds.size.width.to_bytes(self.bytes_per_pixel()) as u64,
        );
    }

    fn bytes_per_pixel(&self) -> u8 {
        match self.id.kind {
            AtlasTextureKind::Monochrome => 1,
            AtlasTextureKind::Polychrome => 4,
        }
    }

    fn decrement_ref_count(&mut self) {
        self.live_atlas_keys -= 1;
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

    fn image_key(id: usize) -> AtlasKey {
        AtlasKey::Image(RenderImageParams {
            image_id: ImageId(id),
            frame_index: 0,
        })
    }

    #[test]
    fn evicts_lru_tiles_to_budget_and_protects_the_current_frame() {
        let Some(device) = metal::Device::system_default() else {
            return;
        };
        let atlas = MetalAtlas::new(device);
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
            "the three older tiles are shed to fit a one-tile budget"
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
}
