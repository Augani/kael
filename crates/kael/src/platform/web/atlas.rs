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
pub(super) struct WebAtlas(Mutex<WebAtlasState>);

#[derive(Clone)]
pub(super) struct WebAtlasUpload {
    pub(super) page_size: Size<DevicePixels>,
    pub(super) bounds: Bounds<DevicePixels>,
    pub(super) kind: AtlasTextureKind,
    pub(super) revision: u64,
    pub(super) bytes: Vec<u8>,
}

struct WebAtlasState {
    next_texture_index: u32,
    tiles_by_key: FxHashMap<AtlasKey, AtlasTile>,
    pages: FxHashMap<AtlasTextureId, WebAtlasPage>,
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

impl Default for WebAtlas {
    fn default() -> Self {
        Self(Mutex::new(WebAtlasState {
            next_texture_index: 0,
            tiles_by_key: FxHashMap::default(),
            pages: FxHashMap::default(),
        }))
    }
}

impl WebAtlas {
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
        let mut state = self.0.lock();
        if let Some(tile) = state.tiles_by_key.get(key) {
            return Ok(Some(tile.clone()));
        }

        let Some((tile_size, bytes)) = build()? else {
            return Ok(None);
        };
        let kind = key.texture_kind();
        validate_atlas_payload(tile_size, kind, bytes.len())?;
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
                    .context("browser atlas tile width overflow")?,
            ),
            DevicePixels(
                tile_size
                    .height
                    .0
                    .checked_add(padding * 2)
                    .context("browser atlas tile height overflow")?,
            ),
        );

        let (texture_id, allocation) = state.allocate(kind, allocation_class, allocation_size)?;
        let bounds = Bounds {
            origin: point(
                DevicePixels(allocation.rectangle.min.x + padding),
                DevicePixels(allocation.rectangle.min.y + padding),
            ),
            size: tile_size,
        };
        let tile = AtlasTile {
            texture_id,
            tile_id: TileId::from(allocation.id),
            padding: padding as u32,
            bounds,
        };
        let page = state
            .pages
            .get_mut(&texture_id)
            .context("new browser atlas page disappeared")?;
        write_region(page, bounds, bytes.as_ref())?;
        page.revision = page.revision.wrapping_add(1).max(1);
        page.dirty_bounds = Some(union_bounds(page.dirty_bounds, bounds));
        page.live_tiles += 1;
        state.tiles_by_key.insert(key.clone(), tile.clone());
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        let mut state = self.0.lock();
        let Some(tile) = state.tiles_by_key.remove(key) else {
            return;
        };
        let id = tile.texture_id;
        let mut remove_page = false;
        if let Some(page) = state.pages.get_mut(&id) {
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
            state.pages.remove(&id);
        }
    }

    fn clear(&self) {
        let mut state = self.0.lock();
        state.tiles_by_key.clear();
        state.pages.clear();
    }
}

impl WebAtlasState {
    fn allocate(
        &mut self,
        kind: AtlasTextureKind,
        allocation_class: AtlasAllocationClass,
        allocation_size: Size<DevicePixels>,
    ) -> Result<(AtlasTextureId, etagere::Allocation)> {
        if !matches!(allocation_class, AtlasAllocationClass::DedicatedLargeImage) {
            let candidates = self
                .pages
                .iter()
                .filter_map(|(id, page)| {
                    (page.kind == kind && page.allocation_class == allocation_class).then_some(*id)
                })
                .collect::<Vec<_>>();
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
            page_size.width.0 <= crate::MAX_ATLAS_TEXTURE_DIMENSION
                && page_size.height.0 <= crate::MAX_ATLAS_TEXTURE_DIMENSION,
            "browser atlas allocation exceeds the maximum texture size"
        );
        let id = AtlasTextureId {
            index: self.next_texture_index,
            kind,
        };
        self.next_texture_index = self
            .next_texture_index
            .checked_add(1)
            .ok_or_else(|| anyhow!("browser atlas texture id space exhausted"))?;
        let bytes_per_pixel = bytes_per_pixel(kind);
        let byte_len = usize::try_from(page_size.width.0)?
            .checked_mul(usize::try_from(page_size.height.0)?)
            .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
            .context("browser atlas page byte size overflow")?;
        let mut page = WebAtlasPage {
            size: page_size,
            kind,
            allocation_class,
            allocator: AtlasAllocator::new(page_size.into()),
            pixels: vec![0; byte_len],
            revision: 0,
            dirty_bounds: None,
            live_tiles: 0,
        };
        let allocation = page
            .allocator
            .allocate(allocation_size.into())
            .context("new browser atlas page could not fit its requested tile")?;
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
