use std::{ptr, rc::Rc};

use rustc_hash::FxHashMap;
use tiny_skia::{Path, Transform as PixmapTransform};

use super::{
    renderer::{
        LayerHierarchyCache, LayerSliceCache, RenderTransform, Renderer, ShapeGroupPlan,
        ShapePlanCache, ShapeRenderableItem, ShapeRenderableKind, StaticPathCache,
    },
    values::{
        decode_fill_style, decode_layer_transform, decode_repeater_style, decode_shape_transform,
        decode_stroke_style, decode_trim_style,
    },
};
use crate::{AnimatedValue, Animation, Layer, PositionValue, ShapeItem, Transform};

impl StaticPathCache {
    pub(super) fn get_or_insert<F>(&self, item: &ShapeItem, build: F) -> Option<Rc<Path>>
    where
        F: FnOnce() -> Option<Path>,
    {
        let key = ptr::from_ref(item) as usize;
        if let Some(path) = self.paths.borrow().get(&key).cloned() {
            trace!("static_path_cache hit");
            return path;
        }

        trace!("static_path_cache miss");
        let path = build().map(Rc::new);
        self.paths.borrow_mut().insert(key, path.clone());
        path
    }
}

impl LayerHierarchyCache {
    pub(super) fn from_animation(animation: &Animation) -> Self {
        let mut slices = FxHashMap::default();
        Self::register_layers(&animation.layers, &mut slices);
        for asset in &animation.assets {
            Self::register_layers(&asset.layers, &mut slices);
        }

        Self { slices }
    }

    fn register_layers(layers: &[Layer], slices: &mut FxHashMap<usize, LayerSliceCache>) {
        if layers.is_empty() {
            return;
        }

        let key = layers.as_ptr() as usize;
        slices
            .entry(key)
            .or_insert_with(|| LayerSliceCache::from_layers(layers));
    }

    pub(super) fn for_layers(&self, layers: &[Layer]) -> Option<&LayerSliceCache> {
        self.slices.get(&(layers.as_ptr() as usize))
    }
}

impl LayerSliceCache {
    fn from_layers(layers: &[Layer]) -> Self {
        let mut positions_by_ptr = FxHashMap::default();
        positions_by_ptr.reserve(layers.len());
        let mut positions_by_index = FxHashMap::default();
        positions_by_index.reserve(layers.len());
        for (position, layer) in layers.iter().enumerate() {
            positions_by_ptr.insert(ptr::from_ref(layer) as usize, position);
            if let Some(index) = layer.index {
                positions_by_index.insert(index, position);
            }
        }

        let mut parent_lineages = Vec::with_capacity(layers.len());
        let mut matte_sources = Vec::with_capacity(layers.len());
        for (position, layer) in layers.iter().enumerate() {
            let mut lineage = Vec::new();
            let mut current = Some(position);
            while let Some(current_position) = current {
                if lineage.contains(&current_position) {
                    break;
                }

                lineage.push(current_position);
                current = layers[current_position]
                    .parent
                    .and_then(|parent| positions_by_index.get(&parent).copied());
            }
            lineage.reverse();
            parent_lineages.push(lineage.into_boxed_slice());

            let target_index = layer
                .matte_parent
                .or_else(|| layer.index.map(|index| index - 1));
            matte_sources.push(
                target_index
                    .and_then(|index| positions_by_index.get(&index).copied())
                    .or_else(|| position.checked_sub(1)),
            );
        }

        let mut static_transforms = Vec::with_capacity(layers.len());
        for lineage in &parent_lineages {
            let transform = lineage
                .iter()
                .all(|position| layer_transform_is_static(layers[*position].transform.as_ref()))
                .then(|| {
                    let mut matrix = PixmapTransform::identity();
                    let mut opacity = 1.0;
                    for position in lineage.iter().copied() {
                        let transform =
                            decode_layer_transform(layers[position].transform.as_ref(), 0.0, None);
                        matrix = matrix.pre_concat(transform.matrix);
                        opacity = transform.opacity;
                    }

                    Some(RenderTransform { matrix, opacity })
                })
                .flatten();
            static_transforms.push(transform);
        }

        Self {
            positions_by_ptr,
            parent_lineages,
            matte_sources,
            static_transforms,
        }
    }

    pub(super) fn parent_lineage<'a>(
        &self,
        layers: &'a [Layer],
        layer: &Layer,
    ) -> Option<impl Iterator<Item = &'a Layer>> {
        let position = self
            .positions_by_ptr
            .get(&(ptr::from_ref(layer) as usize))?;
        let lineage = self.parent_lineages.get(*position)?;
        Some(lineage.iter().map(|position| &layers[*position]))
    }

    pub(super) fn matte_source(&self, current_index: usize) -> Option<usize> {
        self.matte_sources.get(current_index).copied().flatten()
    }

    pub(super) fn static_transform(&self, layer: &Layer) -> Option<RenderTransform> {
        let position = self
            .positions_by_ptr
            .get(&(ptr::from_ref(layer) as usize))?;
        self.static_transforms.get(*position).copied().flatten()
    }
}

impl ShapePlanCache {
    pub(super) fn from_animation(animation: &Animation) -> Self {
        let mut groups = FxHashMap::default();
        Self::register_shape_items(&animation.layers, &mut groups);
        for asset in &animation.assets {
            Self::register_shape_items(&asset.layers, &mut groups);
        }
        for glyph in &animation.chars {
            Self::register_group(&glyph.data.shapes, &mut groups);
        }

        Self { groups }
    }

    fn register_shape_items(layers: &[Layer], groups: &mut FxHashMap<usize, ShapeGroupPlan>) {
        for layer in layers {
            Self::register_group(&layer.shapes, groups);
        }
    }

    fn register_group(items: &[ShapeItem], groups: &mut FxHashMap<usize, ShapeGroupPlan>) {
        if items.is_empty() {
            return;
        }

        let key = items.as_ptr() as usize;
        if groups.contains_key(&key) {
            return;
        }

        groups.insert(key, ShapeGroupPlan::from_items(items));
        for item in items {
            if item.item_type == "gr" {
                Self::register_group(&item.items, groups);
            }
        }
    }

    pub(super) fn for_items(&self, items: &[ShapeItem]) -> Option<&ShapeGroupPlan> {
        self.groups.get(&(items.as_ptr() as usize))
    }
}

impl ShapeGroupPlan {
    fn from_items(items: &[ShapeItem]) -> Self {
        let mut local_transform = None;
        let mut trim = None;
        let mut repeater = None;
        let mut fill = None;
        let mut stroke = None;
        let mut merge_index = None;
        let mut renderables = Vec::new();

        for (index, item) in items.iter().enumerate() {
            if item.hidden {
                continue;
            }

            match item.item_type.as_str() {
                "tr" if local_transform.is_none() => local_transform = Some(index),
                "tm" if trim.is_none() => trim = Some(index),
                "rp" if repeater.is_none() => repeater = Some(index),
                "fl" | "gf" if fill.is_none() => fill = Some(index),
                "st" | "gs" if stroke.is_none() => stroke = Some(index),
                "mm" if merge_index.is_none() && item.merge_mode() == Some(1) => {
                    merge_index = Some(index);
                }
                "gr" => renderables.push(ShapeRenderableItem {
                    index,
                    kind: ShapeRenderableKind::Group,
                }),
                "rc" | "el" | "sh" | "sr" => renderables.push(ShapeRenderableItem {
                    index,
                    kind: ShapeRenderableKind::Geometry,
                }),
                _ => {}
            }
        }

        Self {
            local_transform,
            static_local_transform: local_transform
                .filter(|index| shape_transform_item_is_static(&items[*index]))
                .map(|index| decode_shape_transform(&items[index], 0.0, None)),
            trim,
            static_trim: trim
                .filter(|index| trim_item_is_static(&items[*index]))
                .and_then(|index| decode_trim_style(&items[index], 0.0, None)),
            repeater,
            static_repeater: repeater
                .filter(|index| repeater_item_is_static(&items[*index]))
                .and_then(|index| decode_repeater_style(&items[index], 0.0, None)),
            fill,
            static_fill: fill
                .filter(|index| fill_item_is_static(&items[*index]))
                .and_then(|index| decode_fill_style(&items[index], 0.0, None)),
            stroke,
            static_stroke: stroke
                .filter(|index| stroke_item_is_static(&items[*index]))
                .and_then(|index| decode_stroke_style(&items[index], 0.0, None)),
            merge_index,
            renderables: renderables.into_boxed_slice(),
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::target_corpus()
    }
}

fn animated_value_is_static(value: Option<&AnimatedValue>) -> bool {
    value.is_none_or(AnimatedValue::is_static)
}

fn position_value_is_static(value: Option<&PositionValue>) -> bool {
    match value {
        Some(PositionValue::Combined(value)) => value.is_static(),
        Some(PositionValue::Split(value)) if value.is_split() => {
            animated_value_is_static(value.x.as_ref())
                && animated_value_is_static(value.y.as_ref())
                && animated_value_is_static(value.z.as_ref())
        }
        None | Some(PositionValue::Split(_)) => true,
    }
}

fn shape_transform_item_is_static(item: &ShapeItem) -> bool {
    animated_value_is_static(item.anchor.as_ref())
        && position_value_is_static(item.position.as_ref())
        && animated_value_is_static(item.size.as_ref())
        && animated_value_is_static(item.transform_rotation().as_ref())
        && animated_value_is_static(item.opacity.as_ref())
}

fn trim_item_is_static(item: &ShapeItem) -> bool {
    animated_value_is_static(item.trim_start())
        && animated_value_is_static(item.trim_end().as_ref())
        && animated_value_is_static(item.trim_offset())
}

fn fill_item_is_static(item: &ShapeItem) -> bool {
    animated_value_is_static(item.color.as_ref())
        && animated_value_is_static(item.opacity.as_ref())
        && item
            .gradient_data()
            .is_none_or(|gradient| gradient.colors.is_static())
        && animated_value_is_static(item.gradient_start_point())
        && animated_value_is_static(item.gradient_end_point().as_ref())
        && animated_value_is_static(item.gradient_highlight_length().as_ref())
        && animated_value_is_static(item.gradient_highlight_angle())
}

fn stroke_item_is_static(item: &ShapeItem) -> bool {
    fill_item_is_static(item)
        && animated_value_is_static(item.width.as_ref())
        && animated_value_is_static(item.miter_limit_value.as_ref())
        && item.dash_pattern().is_none_or(|entries| {
            entries
                .iter()
                .all(|entry| animated_value_is_static(Some(&entry.value)))
        })
}

fn repeater_item_is_static(item: &ShapeItem) -> bool {
    animated_value_is_static(item.repeater_copies())
        && animated_value_is_static(item.repeater_offset())
        && item.repeater_transform().is_none_or(|transform| {
            animated_value_is_static(transform.anchor.as_ref())
                && position_value_is_static(transform.position.as_ref())
                && animated_value_is_static(transform.scale.as_ref())
                && animated_value_is_static(transform.rotation.as_ref())
                && animated_value_is_static(transform.start_opacity.as_ref())
                && animated_value_is_static(transform.end_opacity.as_ref())
        })
}

fn layer_transform_is_static(transform: Option<&Transform>) -> bool {
    transform.is_none_or(|transform| {
        animated_value_is_static(transform.anchor.as_ref())
            && position_value_is_static(transform.position.as_ref())
            && animated_value_is_static(transform.scale.as_ref())
            && animated_value_is_static(transform.rotation.as_ref())
            && animated_value_is_static(transform.opacity.as_ref())
    })
}
