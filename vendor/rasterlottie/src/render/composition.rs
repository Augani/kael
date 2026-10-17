use tiny_skia::{
    BlendMode, Color, FillRule, IntRect, Mask as TinyMask, MaskType, Paint, Pixmap, PixmapPaint,
    Transform as PixmapTransform,
};

use super::{
    drawing_paths::build_bezier_path,
    renderer::{LayerRenderContext, RenderTransform, Rgba8},
    values::scalar_at,
};
use crate::{
    Animation, Layer, MaskMode, RasterlottieError, TrackMatteMode, model::Mask,
    timeline::sample_shape_path,
};

#[derive(Debug, Clone)]
pub(super) struct LayerSurface {
    pub(super) pixmap: Pixmap,
    pub(super) origin_x: i32,
    pub(super) origin_y: i32,
}

pub(super) fn new_pixmap(width: u32, height: u32) -> Result<Pixmap, RasterlottieError> {
    span_enter!(
        tracing::Level::TRACE,
        "new_pixmap",
        width = width,
        height = height
    );
    Pixmap::new(width, height).ok_or(RasterlottieError::InvalidCanvasSize { width, height })
}

pub(super) fn resolve_output_canvas_size(
    animation: &Animation,
    config: crate::RenderConfig,
) -> Result<(u32, u32), RasterlottieError> {
    if !config.scale.is_finite() || config.scale <= 0.0 {
        return Err(RasterlottieError::InvalidCanvasSize {
            width: animation.width,
            height: animation.height,
        });
    }

    let width = ((animation.width as f32) * config.scale).ceil().max(1.0);
    let height = ((animation.height as f32) * config.scale).ceil().max(1.0);
    if width > u32::MAX as f32 || height > u32::MAX as f32 {
        return Err(RasterlottieError::InvalidCanvasSize {
            width: animation.width,
            height: animation.height,
        });
    }

    Ok((width as u32, height as u32))
}

pub(super) fn composite_pixmap(
    destination: &mut Pixmap,
    source: &Pixmap,
    x: i32,
    y: i32,
    blend_mode: BlendMode,
) {
    span_enter!(tracing::Level::TRACE, "composite_pixmap");
    let paint = PixmapPaint {
        blend_mode,
        ..PixmapPaint::default()
    };
    destination.draw_pixmap(
        x,
        y,
        source.as_ref(),
        &paint,
        PixmapTransform::identity(),
        None,
    );
}

pub(super) fn apply_alpha_mask(mut pixmap: Pixmap, mask: &TinyMask) -> Pixmap {
    span_enter!(tracing::Level::TRACE, "apply_alpha_mask");
    pixmap.apply_mask(mask);
    pixmap
}

pub(super) fn crop_layer_surface(
    pixmap: &Pixmap,
) -> Result<Option<LayerSurface>, RasterlottieError> {
    crop_layer_surface_with_origin(pixmap, 0, 0)
}

pub(super) fn crop_layer_surface_with_origin(
    pixmap: &Pixmap,
    origin_x: i32,
    origin_y: i32,
) -> Result<Option<LayerSurface>, RasterlottieError> {
    let Some(bounds) = pixmap_alpha_bounds(pixmap) else {
        return Ok(None);
    };

    let cropped =
        pixmap
            .as_ref()
            .clone_rect(bounds)
            .ok_or_else(|| RasterlottieError::InvalidCanvasSize {
                width: pixmap.width(),
                height: pixmap.height(),
            })?;

    Ok(Some(LayerSurface {
        pixmap: cropped,
        origin_x: origin_x + bounds.left(),
        origin_y: origin_y + bounds.top(),
    }))
}

fn pixmap_alpha_bounds(pixmap: &Pixmap) -> Option<IntRect> {
    let width = pixmap.width() as usize;
    let mut min_x = width;
    let mut min_y = pixmap.height() as usize;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut found = false;

    for (index, pixel) in pixmap.data().chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }

        let x = index % width;
        let y = index / width;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        found = true;
    }

    found.then(|| {
        let min_x = i32::try_from(min_x).ok()?;
        let min_y = i32::try_from(min_y).ok()?;
        IntRect::from_xywh(
            min_x,
            min_y,
            (max_x - usize::try_from(min_x).ok()? + 1) as u32,
            (max_y - usize::try_from(min_y).ok()? + 1) as u32,
        )
    })?
}

pub(super) fn apply_track_matte(
    mut target: LayerSurface,
    matte: Option<&LayerSurface>,
    mode: TrackMatteMode,
) -> Result<Option<LayerSurface>, RasterlottieError> {
    span_enter!(tracing::Level::TRACE, "apply_track_matte");
    let Some(matte) = matte else {
        return Ok(
            if matches!(
                mode,
                TrackMatteMode::AlphaInverted | TrackMatteMode::LumaInverted
            ) {
                Some(target)
            } else {
                None
            },
        );
    };

    let mut mask_source = new_pixmap(target.pixmap.width(), target.pixmap.height())?;
    composite_pixmap(
        &mut mask_source,
        &matte.pixmap,
        matte.origin_x - target.origin_x,
        matte.origin_y - target.origin_y,
        BlendMode::SourceOver,
    );

    let mask_type = match mode {
        TrackMatteMode::Alpha | TrackMatteMode::AlphaInverted => MaskType::Alpha,
        TrackMatteMode::Luma | TrackMatteMode::LumaInverted => MaskType::Luminance,
    };
    let mut mask = TinyMask::from_pixmap(mask_source.as_ref(), mask_type);
    if matches!(
        mode,
        TrackMatteMode::AlphaInverted | TrackMatteMode::LumaInverted
    ) {
        mask.invert();
    }

    target.pixmap.apply_mask(&mask);
    crop_layer_surface_with_origin(&target.pixmap, target.origin_x, target.origin_y)
}

pub(super) fn build_layer_mask(
    context: &LayerRenderContext<'_>,
    layer: &Layer,
    frame: f32,
    transform: RenderTransform,
) -> Result<TinyMask, RasterlottieError> {
    let mut coverage = new_pixmap(context.canvas_width, context.canvas_height)?;
    if let Some(mode) = layer.masks.iter().find_map(Mask::mask_mode)
        && matches!(mode, MaskMode::Subtract | MaskMode::Intersect)
    {
        coverage.fill(Color::from_rgba8(255, 255, 255, 255));
    }

    for mask in &layer.masks {
        let Some(mode) = mask.mask_mode() else {
            continue;
        };
        if mode == MaskMode::None {
            continue;
        }

        let mask_pixmap = build_single_mask_pixmap(context, mask, frame, transform)?;
        let blend_mode = match mode {
            MaskMode::Add => BlendMode::SourceOver,
            MaskMode::Subtract => BlendMode::DestinationOut,
            MaskMode::Intersect => BlendMode::SourceIn,
            MaskMode::None => continue,
        };
        composite_pixmap(&mut coverage, &mask_pixmap, 0, 0, blend_mode);
    }

    Ok(TinyMask::from_pixmap(coverage.as_ref(), MaskType::Alpha))
}

fn build_single_mask_pixmap(
    context: &LayerRenderContext<'_>,
    mask: &Mask,
    frame: f32,
    transform: RenderTransform,
) -> Result<Pixmap, RasterlottieError> {
    span_enter!(
        tracing::Level::TRACE,
        "build_single_mask_pixmap",
        frame = frame
    );
    let mut pixmap = new_pixmap(context.canvas_width, context.canvas_height)?;
    let alpha = scalar_at(
        mask.opacity.as_ref(),
        frame,
        100.0,
        context.timeline_sample_cache,
    )
    .clamp(0.0, 100.0)
        / 100.0;

    if mask.inverted {
        pixmap.fill(Rgba8::new(255, 255, 255, (alpha * 255.0).round() as u8).into());
    }

    let Some(path) = build_mask_path(mask, frame, context.timeline_sample_cache) else {
        return Ok(pixmap);
    };

    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(
        255,
        255,
        255,
        if mask.inverted {
            255
        } else {
            (alpha * 255.0).round() as u8
        },
    );
    if mask.inverted {
        paint.blend_mode = BlendMode::DestinationOut;
    }

    pixmap.fill_path(&path, &paint, FillRule::Winding, transform.matrix, None);
    Ok(pixmap)
}

fn build_mask_path(
    mask: &Mask,
    frame: f32,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
) -> Option<tiny_skia::Path> {
    let geometry = mask.path.as_ref().and_then(|path| {
        timeline_sample_cache.map_or_else(
            || sample_shape_path(path, frame),
            |cache| cache.sample_shape_path(path, frame),
        )
    })?;
    build_bezier_path(&geometry)
}

pub(super) fn find_track_matte_source_index(
    context: &LayerRenderContext<'_>,
    layers: &[Layer],
    current_index: usize,
    layer: &Layer,
) -> Option<usize> {
    if let Some(index) = context
        .layer_hierarchy_cache
        .and_then(|cache| cache.for_layers(layers))
        .and_then(|cache| cache.matte_source(current_index))
    {
        return Some(index);
    }

    let target_index = layer
        .matte_parent
        .or_else(|| layer.index.map(|index| index - 1));

    target_index.map_or_else(
        || current_index.checked_sub(1),
        |target_index| {
            layers
                .iter()
                .position(|candidate| candidate.index == Some(target_index))
        },
    )
}
