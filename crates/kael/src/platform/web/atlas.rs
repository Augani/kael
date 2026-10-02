use crate::{
    AtlasAllocationClass, AtlasKey, AtlasTextureId, AtlasTextureKind, AtlasTile, Bounds,
    DevicePixels, PlatformAtlas, Size, TileId, point, size, validate_atlas_payload,
};
use anyhow::{Context as _, Result, anyhow};
use collections::FxHashMap;
use etagere::{AllocId, AtlasAllocator};
use parking_lot::Mutex;
use std::borrow::Cow;

const SHARED_PAGE_SIZE: i32 = 1_024;
const SMALL_IMAGE_PAGE_SIZE: i32 = 512;

/// A packed WebGL atlas. Glyphs and small images share page textures so a page can
/// be uploaded once and reused by every sprite that references it.
#[derive(Default)]
pub(super) struct WebAtlas(Mutex<WebAtlasState>);

#[derive(Clone)]
pub(super) struct WebAtlasUpload {
    pub(super) page_size: Size<DevicePixels>,
    pub(super) bounds: Bounds<DevicePixels>,
    pub(super) kind: AtlasTextureKind,
    pub(super) revision: u64,
    pub(super) bytes: Vec<u8>,
}

#[derive(Default)]
struct WebAtlasState {
    next_texture_index: u32,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    pages: FxHashMap<AtlasTextureId, WebAtlasPage>,
    policy: crate::AtlasPolicy,
    byte_budget: Option<u64>,
    max_texture_dimension: Option<i32>,
}

struct WebAtlasPage {
    size: Size<DevicePixels>,
    kind: AtlasTextureKind,
    allocation_class: AtlasAllocationClass,
    allocator: AtlasAllocator,
    pixels: Vec<u8>,
    revision: u64,
    dirty_bounds: Option<Bounds<DevicePixels>>,
    live_tiles: usize,
}

impl WebAtlas {
    pub(super) fn set_max_texture_dimension(&self, dimension: i32) -> Result<()> {
        anyhow::ensure!(
            dimension > 0,
            "browser driver returned an invalid texture dimension limit"
        );
        let dimension = dimension.min(crate::MAX_ATLAS_TEXTURE_DIMENSION);
        let mut state = self.0.lock();
        anyhow::ensure!(
            state
                .pages
                .values()
                .all(|page| page.size.width.0 <= dimension && page.size.height.0 <= dimension),
            "restored browser context cannot support retained atlas page dimensions"
        );
        state.max_texture_dimension = Some(dimension);
        Ok(())
    }

    pub(super) fn mark_scene_used(&self, scene: &crate::Scene) {
        self.0.lock().policy.mark_scene_used(scene);
    }
    pub(super) fn set_byte_budget(&self, bytes: Option<u64>) {
        let mut state = self.0.lock();
        state.byte_budget = bytes;
        state.policy.set_soft_budget(bytes);
    }
    pub(super) fn after_frame(&self) {
        let mut state = self.0.lock();
        if let Some(bytes) = state.byte_budget {
            state.evict_to_budget(bytes);
        }
        for tile in state.policy.advance() {
            state.release_tile(tile);
        }
    }
    pub(super) fn shed_memory(&self) {
        let mut state = self.0.lock();
        let budget = state.byte_budget.unwrap_or(0);
        state.evict_to_budget(budget);
    }

    pub(super) fn page_revision(&self, id: AtlasTextureId) -> Option<u64> {
        self.0.lock().pages.get(&id).map(|page| page.revision)
    }

    /// Return the bytes changed since the renderer's last acknowledged revision.
    ///
    /// WebGL allocates zero-initialized texture storage before applying this region, so even a
    /// new page only needs to cross the JS/Wasm boundary with its live glyph or image bytes.
    pub(super) fn upload(
        &self,
        id: AtlasTextureId,
        known_revision: Option<u64>,
    ) -> Result<Option<WebAtlasUpload>> {
        let state = self.0.lock();
        let page = state
            .pages
            .get(&id)
            .with_context(|| format!("browser atlas page {id:?} is unavailable"))?;
        if known_revision == Some(page.revision) {
            return Ok(None);
        }

        let bounds = page.dirty_bounds.unwrap_or(Bounds {
            origin: point(DevicePixels(0), DevicePixels(0)),
            size: page.size,
        });
        let bytes = copy_region(page, bounds)?;
        Ok(Some(WebAtlasUpload {
            page_size: page.size,
            bounds,
            kind: page.kind,
            revision: page.revision,
            bytes,
        }))
    }

    pub(super) fn acknowledge_upload(&self, id: AtlasTextureId, revision: u64) {
        let mut state = self.0.lock();
        if let Some(page) = state.pages.get_mut(&id)
            && page.revision == revision
        {
            page.dirty_bounds = None;
        }
    }

    /// Require the next GPU upload of every retained page to include its complete
    /// CPU backing store. This is needed after WebGL context restoration: the new
    /// texture starts empty even when only a smaller region changed while the
    /// context was unavailable.
    pub(super) fn mark_all_pages_dirty(&self) {
        let mut state = self.0.lock();
        for page in state.pages.values_mut() {
            page.dirty_bounds = Some(Bounds {
                origin: point(DevicePixels(0), DevicePixels(0)),
                size: page.size,
            });
        }
    }
}

impl PlatformAtlas for WebAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        self.0.lock().insert(key, None, build)
    }

    fn get_or_insert_with_size<'a>(
        &self,
        key: &AtlasKey,
        size: Size<DevicePixels>,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
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

    fn clear(&self) {
        let mut state = self.0.lock();
        let tiles = state
            .tiles_by_key
            .drain()
            .map(|(_, tile)| tile)
            .collect::<Vec<_>>();
        for tile in tiles {
            state.policy.retire(tile);
        }
    }
}

impl WebAtlasState {
    fn allocated_bytes(&self) -> u64 {
        self.pages
            .values()
            .map(|page| page.pixels.len() as u64 * 2)
            .sum()
    }

    fn candidates(&self) -> Vec<AtlasKey> {
        let guard = self.policy.guard(4);
        let mut candidates: Vec<_> = self
            .tiles_by_key
            .iter()
            .filter(|(key, tile)| {
                !matches!(key, AtlasKey::CachedSurface(_)) && self.policy.last_used(tile) < guard
            })
            .map(|(key, tile)| {
                (
                    key.clone(),
                    self.policy.last_used(tile),
                    tile.texture_id.index,
                    tile.tile_id.0,
                )
            })
            .collect();
        candidates.sort_by_key(|(_, age, page, tile)| (*age, *page, *tile));
        candidates.into_iter().map(|(key, ..)| key).collect()
    }

    fn evict(&mut self, key: &AtlasKey) {
        if let Some(tile) = self.tiles_by_key.remove(key) {
            self.policy.forget(&tile);
            self.release_tile(tile);
        }
    }

    fn evict_to_budget(&mut self, bytes: u64) {
        for key in self.candidates() {
            if self.allocated_bytes() <= bytes {
                break;
            }
            self.evict(&key);
        }
    }

    fn admit_page(&mut self, bytes: u64) -> Result<()> {
        if self
            .policy
            .check_page(self.allocated_bytes(), self.pages.len(), bytes)
            .is_err()
        {
            for key in self.candidates() {
                if self
                    .policy
                    .check_page(self.allocated_bytes(), self.pages.len(), bytes)
                    .is_ok()
                {
                    break;
                }
                self.evict(&key);
            }
        }
        self.policy
            .check_page(self.allocated_bytes(), self.pages.len(), bytes)
    }

    fn reserve(&mut self, key: &AtlasKey, tile_size: Size<DevicePixels>) -> Result<AtlasTile> {
        let kind = key.texture_kind();
        let allocation_class = key.allocation_class(tile_size);
        let padding = if matches!(allocation_class, AtlasAllocationClass::DedicatedLargeImage) {
            0
        } else {
            1
        };
        let allocation_size = size(
            DevicePixels(
                tile_size
                    .width
                    .0
                    .checked_add(padding * 2)
                    .context("browser atlas width overflow")?,
            ),
            DevicePixels(
                tile_size
                    .height
                    .0
                    .checked_add(padding * 2)
                    .context("browser atlas height overflow")?,
            ),
        );
        let (texture_id, allocation) = self.allocate(kind, allocation_class, allocation_size)?;
        self.pages
            .get_mut(&texture_id)
            .context("browser atlas page missing")?
            .live_tiles += 1;
        Ok(AtlasTile {
            texture_id,
            tile_id: TileId::from(allocation.id),
            padding: padding as u32,
            bounds: Bounds {
                origin: point(
                    DevicePixels(allocation.rectangle.min.x + padding),
                    DevicePixels(allocation.rectangle.min.y + padding),
                ),
                size: tile_size,
            },
        })
    }

    fn insert<'a>(
        &mut self,
        key: &AtlasKey,
        declared: Option<Size<DevicePixels>>,
        build: &mut dyn FnMut() -> Result<Option<(Size<DevicePixels>, Cow<'a, [u8]>)>>,
    ) -> Result<Option<AtlasTile>> {
        if let Some(tile) = self.tiles_by_key.get(key).cloned() {
            self.policy.touch(&tile);
            return Ok(Some(tile));
        }
        let bytes = declared
            .map(|size| crate::atlas_payload_len(size, key.texture_kind()))
            .transpose()?
            .unwrap_or(0);
        anyhow::ensure!(
            bytes as u64 <= self.policy.limits.max_bytes,
            "browser atlas raster exceeds admission limit"
        );
        if self.policy.check_tile(bytes).is_err() {
            for key in self.candidates() {
                if self.policy.check_tile(bytes).is_ok() {
                    break;
                }
                self.evict(&key);
            }
        }
        self.policy.check_tile(bytes)?;
        let reserved = declared.map(|size| self.reserve(key, size)).transpose()?;
        let (size, bytes) = match build() {
            Ok(Some(value)) => value,
            other => {
                if let Some(tile) = reserved {
                    self.release_tile(tile);
                }
                return other.map(|_| None);
            }
        };
        if let Err(error) =
            validate_atlas_payload(size, key.texture_kind(), bytes.len()).and_then(|_| {
                anyhow::ensure!(
                    declared.is_none_or(|expected| expected == size),
                    "browser atlas raster dimensions differ from reservation"
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
            self.reserve(key, size)?
        };
        let page = self
            .pages
            .get_mut(&tile.texture_id)
            .context("browser atlas page disappeared")?;
        write_region(page, tile.bounds, &bytes)?;
        page.revision = page.revision.wrapping_add(1).max(1);
        page.dirty_bounds = Some(union_bounds(page.dirty_bounds, tile.bounds));
        self.policy.touch(&tile);
        self.tiles_by_key.insert(key.clone(), tile.clone());
        Ok(Some(tile))
    }

    fn release_tile(&mut self, tile: AtlasTile) {
        let id = tile.texture_id;
        let mut remove_page = false;
        if let Some(page) = self.pages.get_mut(&id) {
            page.allocator.deallocate(AllocId::from(tile.tile_id));
            let padding = i32::try_from(tile.padding).unwrap_or_default();
            let cleared = Bounds {
                origin: point(
                    DevicePixels(tile.bounds.origin.x.0 - padding),
                    DevicePixels(tile.bounds.origin.y.0 - padding),
                ),
                size: size(
                    DevicePixels(tile.bounds.size.width.0 + padding * 2),
                    DevicePixels(tile.bounds.size.height.0 + padding * 2),
                ),
            };
            clear_region(page, cleared);
            page.revision = page.revision.wrapping_add(1).max(1);
            page.dirty_bounds = Some(union_bounds(page.dirty_bounds, cleared));
            page.live_tiles = page.live_tiles.saturating_sub(1);
            remove_page = page.live_tiles == 0;
        }
        if remove_page {
            self.pages.remove(&id);
        }
    }

    fn allocate(
        &mut self,
        kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
        allocation_size: Size<DevicePixels>,
    ) -> Result<(AtlasTextureId, etagere::Allocation)> {
        if !matches!(allocation_class, AtlasAllocationClass::DedicatedLargeImage) {
            let mut candidates = self
                .pages
                .iter()
                .filter_map(|(id, page)| {
                    (page.kind == kind && page.allocation_class == allocation_class).then_some(*id)
                })
                .collect::<Vec<_>>();
            candidates.sort_by_key(|id| id.index);
            for id in candidates {
                if let Some(allocation) = self
                    .pages
                    .get_mut(&id)
                    .and_then(|page| page.allocator.allocate(allocation_size.into()))
                {
                    return Ok((id, allocation));
                }
            }
        }

        let default_edge = match allocation_class {
            AtlasAllocationClass::Shared => SHARED_PAGE_SIZE,
            AtlasAllocationClass::SharedSmallImage => SMALL_IMAGE_PAGE_SIZE,
            AtlasAllocationClass::DedicatedLargeImage => 1,
        };
        let page_size = size(
            DevicePixels(default_edge.max(allocation_size.width.0)),
            DevicePixels(default_edge.max(allocation_size.height.0)),
        );
        anyhow::ensure!(
            page_size.width.0
                <= self
                    .max_texture_dimension
                    .unwrap_or(crate::MAX_ATLAS_TEXTURE_DIMENSION)
                && page_size.height.0
                    <= self
                        .max_texture_dimension
                        .unwrap_or(crate::MAX_ATLAS_TEXTURE_DIMENSION),
            "browser atlas allocation exceeds the maximum texture size"
        );
        let id = AtlasTextureId {
            index: self.next_texture_index,
            kind,
        };
        let next_texture_index = self
            .next_texture_index
            .checked_add(1)
            .ok_or_else(|| anyhow!("browser atlas texture id space exhausted"))?;
        let bytes_per_pixel = bytes_per_pixel(kind);
        let byte_len = usize::try_from(page_size.width.0)?
            .checked_mul(usize::try_from(page_size.height.0)?)
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
            .context("browser atlas page byte size overflow")?;
        self.admit_page(
            (byte_len as u64)
                .checked_mul(2)
                .context("browser atlas mirror byte overflow")?,
        )?;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(byte_len)
            .context("browser atlas CPU page allocation failed")?;
        pixels.resize(byte_len, 0);
        let mut page = WebAtlasPage {
            size: page_size,
            kind,
            allocation_class,
            allocator: AtlasAllocator::new(page_size.into()),
            pixels,
            revision: 0,
            dirty_bounds: None,
            live_tiles: 0,
        };
        let allocation = page
            .allocator
            .allocate(allocation_size.into())
            .context("new browser atlas page could not fit its requested tile")?;
        self.next_texture_index = next_texture_index;
        self.pages.insert(id, page);
        Ok((id, allocation))
    }
}

fn bytes_per_pixel(kind: AtlasTextureKind) -> usize {
    match kind {
        AtlasTextureKind::Monochrome => 1,
        AtlasTextureKind::Polychrome => 4,
    }
}

fn write_region(page: &mut WebAtlasPage, bounds: Bounds<DevicePixels>, bytes: &[u8]) -> Result<()> {
    let bytes_per_pixel = bytes_per_pixel(page.kind);
    let width = usize::try_from(bounds.size.width.0)?;
    let height = usize::try_from(bounds.size.height.0)?;
    let page_width = usize::try_from(page.size.width.0)?;
    let x = usize::try_from(bounds.origin.x.0)?;
    let y = usize::try_from(bounds.origin.y.0)?;
    let row_bytes = width
        .checked_mul(bytes_per_pixel)
        .context("browser atlas row size overflow")?;
    anyhow::ensure!(
        bytes.len() == row_bytes * height,
        "invalid browser atlas upload length"
    );
    for row in 0..height {
        let source_start = row * row_bytes;
        let destination_start = ((y + row) * page_width + x) * bytes_per_pixel;
        page.pixels[destination_start..destination_start + row_bytes]
            .copy_from_slice(&bytes[source_start..source_start + row_bytes]);
    }
    Ok(())
}

fn clear_region(page: &mut WebAtlasPage, bounds: Bounds<DevicePixels>) {
    let bytes_per_pixel = bytes_per_pixel(page.kind);
    let Ok(width) = usize::try_from(bounds.size.width.0) else {
        return;
    };
    let Ok(height) = usize::try_from(bounds.size.height.0) else {
        return;
    };
    let Ok(page_width) = usize::try_from(page.size.width.0) else {
        return;
    };
    let Ok(x) = usize::try_from(bounds.origin.x.0) else {
        return;
    };
    let Ok(y) = usize::try_from(bounds.origin.y.0) else {
        return;
    };
    let row_bytes = width.saturating_mul(bytes_per_pixel);
    for row in 0..height {
        let start = ((y + row) * page_width + x) * bytes_per_pixel;
        if let Some(destination) = page.pixels.get_mut(start..start.saturating_add(row_bytes)) {
            destination.fill(0);
        }
    }
}

fn copy_region(page: &WebAtlasPage, bounds: Bounds<DevicePixels>) -> Result<Vec<u8>> {
    let bytes_per_pixel = bytes_per_pixel(page.kind);
    let width = usize::try_from(bounds.size.width.0)?;
    let height = usize::try_from(bounds.size.height.0)?;
    let page_width = usize::try_from(page.size.width.0)?;
    let x = usize::try_from(bounds.origin.x.0)?;
    let y = usize::try_from(bounds.origin.y.0)?;
    let row_bytes = width
        .checked_mul(bytes_per_pixel)
        .context("browser atlas row size overflow")?;
    let mut result = Vec::with_capacity(row_bytes * height);
    for row in 0..height {
        let start = ((y + row) * page_width + x) * bytes_per_pixel;
        result.extend_from_slice(&page.pixels[start..start + row_bytes]);
    }
    Ok(result)
}

fn union_bounds(
    current: Option<Bounds<DevicePixels>>,
    next: Bounds<DevicePixels>,
) -> Bounds<DevicePixels> {
    let Some(current) = current else {
        return next;
    };
    let left = current.origin.x.0.min(next.origin.x.0);
    let top = current.origin.y.0.min(next.origin.y.0);
    let right =
        (current.origin.x.0 + current.size.width.0).max(next.origin.x.0 + next.size.width.0);
    let bottom =
        (current.origin.y.0 + current.size.height.0).max(next.origin.y.0 + next.size.height.0);
    Bounds {
        origin: point(DevicePixels(left), DevicePixels(top)),
        size: size(DevicePixels(right - left), DevicePixels(bottom - top)),
    }
}

impl From<Size<DevicePixels>> for etagere::Size {
    fn from(value: Size<DevicePixels>) -> Self {
        etagere::Size::new(value.width.0, value.height.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AtlasAdmissionLimits, ImageId, RenderImageParams};
    use wasm_bindgen_test::wasm_bindgen_test;

    fn key(id: usize) -> AtlasKey {
        AtlasKey::Image(RenderImageParams {
            image_id: ImageId(id),
            frame_index: 0,
        })
    }

    #[wasm_bindgen_test]
    fn web_atlas_admits_cpu_and_gpu_mirrors_before_build_and_rolls_back() {
        let atlas = WebAtlas::default();
        let tile_size = size(DevicePixels(256), DevicePixels(256));
        const PAYLOAD: usize = 256 * 256 * 4;
        atlas.set_hard_admission_limits(AtlasAdmissionLimits {
            max_bytes: PAYLOAD as u64,
            max_tiles: 2,
            max_pages: 2,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&key(0), tile_size, &mut || panic!(
                    "both mirrors must be admitted before work"
                ))
                .is_err()
        );
        assert!(atlas.0.lock().pages.is_empty());
        atlas.set_hard_admission_limits(AtlasAdmissionLimits {
            max_bytes: PAYLOAD as u64 * 2,
            max_tiles: 1,
            max_pages: 1,
        });
        assert!(
            atlas
                .get_or_insert_with_size(&key(0), tile_size, &mut || Ok(None))
                .unwrap()
                .is_none()
        );
        assert!(atlas.0.lock().pages.is_empty());
        let tile = atlas
            .get_or_insert_with_size(&key(0), tile_size, &mut || {
                Ok(Some((tile_size, Cow::Owned(vec![17; PAYLOAD]))))
            })
            .unwrap()
            .unwrap();
        assert_eq!(atlas.0.lock().allocated_bytes(), PAYLOAD as u64 * 2);
        atlas.remove(&key(0));
        assert!(
            atlas
                .get_or_insert_with_size(&key(1), tile_size, &mut || panic!(
                    "retired pages remain charged"
                ))
                .is_err()
        );
        for _ in 0..3 {
            atlas.after_frame();
            assert!(atlas.page_revision(tile.texture_id).is_some());
        }
        atlas.after_frame();
        assert!(atlas.page_revision(tile.texture_id).is_none());
        assert!(
            atlas
                .get_or_insert_with_size(&key(1), tile_size, &mut || Ok(Some((
                    tile_size,
                    Cow::Owned(vec![29; PAYLOAD])
                ))))
                .unwrap()
                .is_some()
        );
    }

    #[wasm_bindgen_test]
    fn web_atlas_replay_refreshes_tiles_and_pressure_recovers_after_retirement() {
        let atlas = WebAtlas::default();
        let tile_size = size(DevicePixels(256), DevicePixels(256));
        let tile = atlas
            .get_or_insert_with_size(&key(0), tile_size, &mut || {
                Ok(Some((tile_size, Cow::Owned(vec![255; 256 * 256 * 4]))))
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
            atlas.after_frame();
            atlas.shed_memory();
            assert!(atlas.page_revision(tile.texture_id).is_some());
        }
        for _ in 0..4 {
            atlas.after_frame();
        }
        atlas.shed_memory();
        assert!(atlas.page_revision(tile.texture_id).is_none());
        assert!(
            atlas
                .get_or_insert_with_size(&key(0), tile_size, &mut || Ok(Some((
                    tile_size,
                    Cow::Owned(vec![255; 256 * 256 * 4])
                ))))
                .unwrap()
                .is_some()
        );
    }
}
