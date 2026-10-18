use std::{ptr, sync::Arc};

use tiny_skia::{Pixmap, Transform as PixmapTransform};

use super::{
    assets::ImageAssetStore,
    renderer::{LayerRenderContext, RenderTransform},
    values::decode_layer_transform,
};
use crate::{Animation, Asset, Layer, RasterlottieError, timeline::sample_scalar};

pub(super) fn lookup_precomp_asset<'a>(
    animation: &'a Animation,
    layer: &Layer,
) -> Option<&'a Asset> {
    let ref_id = layer.ref_id.as_deref()?;
    animation.assets.iter().find(|asset| asset.id == ref_id)
}

pub(super) fn lookup_image_asset<'a>(
    image_assets: &'a ImageAssetStore,
    animation: &'a Animation,
    layer: &Layer,
) -> Result<Option<Arc<Pixmap>>, RasterlottieError> {
    let Some(ref_id) = layer.ref_id.as_deref() else {
        return Ok(None);
    };
    image_assets.get(animation, ref_id)
}

pub(super) fn resolve_layer_transform_chain(
    context: &LayerRenderContext<'_>,
    layer: &Layer,
    frame: f32,
) -> RenderTransform {
    span_enter!(
        tracing::Level::TRACE,
        "resolve_layer_transform_chain",
        frame = frame,
        layer = layer.name.as_str()
    );
    let cache_key = (ptr::from_ref(layer) as usize, frame.to_bits());
    if let Some(transform) = context
        .frame_cache
        .layer_transforms
        .borrow()
        .get(&cache_key)
    {
        trace!("frame_transform_cache hit");
        return *transform;
    }

    if let Some(transform) = context
        .layer_hierarchy_cache
        .and_then(|cache| cache.for_layers(context.layers))
        .and_then(|cache| cache.static_transform(layer))
    {
        trace!("frame_transform_cache static-hit");
        return transform;
    }

    trace!("frame_transform_cache miss");
    let mut matrix = PixmapTransform::identity();
    let mut opacity = 1.0;
    if let Some(lineage) = context
        .layer_hierarchy_cache
        .and_then(|cache| cache.for_layers(context.layers))
        .and_then(|cache| cache.parent_lineage(context.layers, layer))
    {
        for node in lineage {
            let transform = decode_layer_transform(
                node.transform.as_ref(),
                frame,
                context.timeline_sample_cache,
            );
            matrix = matrix.pre_concat(transform.matrix);
            if ptr::eq(node, layer) {
                opacity = transform.opacity;
            }
        }
    } else {
        let mut lineage = Vec::new();
        let mut visited = Vec::new();
        let mut current = Some(layer);

        while let Some(node) = current {
            if let Some(index) = node.index
                && visited.contains(&index)
            {
                break;
            }

            if let Some(index) = node.index {
                visited.push(index);
            }
            lineage.push(node);
            current = node.parent.and_then(|parent| {
                context
                    .layers
                    .iter()
                    .find(|candidate| candidate.index == Some(parent))
            });
        }

        lineage.reverse();
        for node in lineage {
            let transform = decode_layer_transform(
                node.transform.as_ref(),
                frame,
                context.timeline_sample_cache,
            );
            matrix = matrix.pre_concat(transform.matrix);
            if ptr::eq(node, layer) {
                opacity = transform.opacity;
            }
        }
    }

    let transform = RenderTransform { matrix, opacity };
    context
        .frame_cache
        .layer_transforms
        .borrow_mut()
        .insert(cache_key, transform);
    transform
}

pub(super) fn layer_is_visible(layer: &Layer, frame: f32) -> bool {
    !layer.hidden && frame_in_range(frame, layer.in_point, layer.out_point)
}

pub(super) fn frame_in_range(frame: f32, in_point: Option<f32>, out_point: Option<f32>) -> bool {
    if let Some(in_point) = in_point
        && frame < in_point
    {
        return false;
    }

    if let Some(out_point) = out_point
        && frame >= out_point
    {
        return false;
    }

    true
}

pub(super) fn map_layer_frame(animation: &Animation, frame: f32, layer: &Layer) -> f32 {
    let stretch = if layer.stretch > f32::EPSILON {
        layer.stretch
    } else {
        1.0
    };

    let layer_time = frame / stretch - layer.start_time;
    let Some(time_remap) = layer.time_remap.as_ref() else {
        return layer_time;
    };

    let remapped_seconds = sample_scalar(time_remap, layer_time).unwrap_or_else(|| {
        if animation.frame_rate <= f32::EPSILON {
            0.0
        } else {
            layer_time / animation.frame_rate
        }
    });

    remapped_seconds * animation.frame_rate.max(1.0)
}
