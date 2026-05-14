use crate::{
    App, AtlasKey, AtlasTile, Bounds, CachedSurfaceParams, ContentMask, DevicePixels, EntityId,
    GlobalElementId, PaintIndex, Pixels, PlatformAtlas, PrepaintStateIndex, ScaledPixels,
    TextStyle, hash, point, size,
};
use collections::FxHashSet;
use smallvec::SmallVec;
use std::{borrow::Cow, ops::Range, sync::Arc};

pub(crate) type TrackedEntityGenerations = SmallVec<[(EntityId, u64); 8]>;

pub(crate) struct SubtreeCacheState {
    pub(crate) prepaint_range: Range<PrepaintStateIndex>,
    pub(crate) paint_range: Range<PaintIndex>,
    pub(crate) cache_key: SubtreeCacheKey,
    pub(crate) accessed_entities: FxHashSet<EntityId>,
    pub(crate) surface: Option<CachedSurface>,
    tracked_entity_generations: TrackedEntityGenerations,
}

pub(crate) struct CachedSurface {
    atlas: Arc<dyn PlatformAtlas>,
    params: CachedSurfaceParams,
    pub(crate) tile: AtlasTile,
    pub(crate) bounds: Bounds<ScaledPixels>,
    pub(crate) device_bounds: Bounds<DevicePixels>,
}

#[derive(Default, PartialEq)]
pub(crate) struct SubtreeCacheKey {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) content_mask: ContentMask<Pixels>,
    pub(crate) global_generation: u64,
    pub(crate) text_style: TextStyle,
}

impl SubtreeCacheState {
    pub(crate) fn new(
        accessed_entities: FxHashSet<EntityId>,
        cache_key: SubtreeCacheKey,
        prepaint_range: Range<PrepaintStateIndex>,
        cx: &App,
    ) -> Self {
        Self {
            tracked_entity_generations: capture_entity_generations(cx, &accessed_entities),
            accessed_entities,
            cache_key,
            prepaint_range,
            paint_range: PaintIndex::default()..PaintIndex::default(),
            surface: None,
        }
    }

    pub(crate) fn is_reusable(&self, cache_key: &SubtreeCacheKey, cx: &App) -> bool {
        self.cache_key == *cache_key
            && self
                .tracked_entity_generations
                .iter()
                .all(|(entity_id, generation)| {
                    cx.entity_generation(*entity_id) == Some(*generation)
                })
    }

    pub(crate) fn prepare_surface(
        &mut self,
        global_id: &GlobalElementId,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        sprite_atlas: &Arc<dyn PlatformAtlas>,
    ) {
        let Some((device_bounds, scaled_bounds)) = align_surface_bounds(bounds, scale_factor)
        else {
            self.surface = None;
            return;
        };

        let params = CachedSurfaceParams {
            cache_id: hash(global_id),
            size: device_bounds.size,
        };

        if self.surface.as_ref().is_some_and(|surface| {
            surface.params == params && surface.device_bounds == device_bounds
        }) {
            return;
        }

        let byte_len =
            device_bounds.size.width.0 as usize * device_bounds.size.height.0 as usize * 4;
        let mut build = || Ok(Some((device_bounds.size, Cow::Owned(vec![0; byte_len]))));
        let surface = sprite_atlas
            .get_or_insert_with(&AtlasKey::from(params.clone()), &mut build)
            .ok()
            .flatten()
            .map(|tile| CachedSurface {
                atlas: Arc::clone(sprite_atlas),
                params,
                tile,
                bounds: scaled_bounds,
                device_bounds,
            });

        self.surface = surface;
    }
}

impl Drop for CachedSurface {
    fn drop(&mut self) {
        self.atlas.remove(&AtlasKey::from(self.params.clone()));
    }
}

fn align_surface_bounds(
    bounds: Bounds<Pixels>,
    scale_factor: f32,
) -> Option<(Bounds<DevicePixels>, Bounds<ScaledPixels>)> {
    let scaled_bounds = bounds.scale(scale_factor);
    let min_x = scaled_bounds.origin.x.0.floor() as i32;
    let min_y = scaled_bounds.origin.y.0.floor() as i32;
    let max_x = (scaled_bounds.origin.x.0 + scaled_bounds.size.width.0).ceil() as i32;
    let max_y = (scaled_bounds.origin.y.0 + scaled_bounds.size.height.0).ceil() as i32;

    if max_x <= min_x || max_y <= min_y {
        return None;
    }

    let device_bounds = Bounds {
        origin: point(DevicePixels(min_x), DevicePixels(min_y)),
        size: size(DevicePixels(max_x - min_x), DevicePixels(max_y - min_y)),
    };
    let scaled_bounds = Bounds {
        origin: device_bounds.origin.map(Into::into),
        size: device_bounds.size.map(Into::into),
    };

    Some((device_bounds, scaled_bounds))
}

fn capture_entity_generations(
    cx: &App,
    accessed_entities: &FxHashSet<EntityId>,
) -> TrackedEntityGenerations {
    accessed_entities
        .iter()
        .filter_map(|entity_id| {
            cx.entity_generation(*entity_id)
                .map(|generation| (*entity_id, generation))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{AppContext, TestAppContext};

    #[kael::test]
    fn notify_bumps_entity_generation(cx: &mut TestAppContext) {
        let entity = cx.new(|_| 0usize);
        let entity_id = entity.entity_id();

        assert_eq!(cx.read(|app| app.entity_generation(entity_id)), Some(0));

        entity.update(cx, |_, cx| {
            cx.notify();
        });

        assert_eq!(cx.read(|app| app.entity_generation(entity_id)), Some(1));
    }
}
