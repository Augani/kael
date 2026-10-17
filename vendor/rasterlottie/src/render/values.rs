use tiny_skia::{
    LineCap as TinyLineCap, LineJoin as TinyLineJoin, StrokeDash as TinyStrokeDash,
    Transform as PixmapTransform,
};

use super::{
    renderer::{
        BrushStyle, FillStyle, GradientKind, GradientStopStyle, GradientStyle, RenderTransform,
        RepeaterStyle, Rgba8, StrokeStyle, TrimStyle,
    },
    sample_cache::TimelineSampleCache,
};
use crate::{AnimatedValue, PositionValue, ShapeItem, Transform, timeline};

pub(super) fn distance(lhs: [f32; 2], rhs: [f32; 2]) -> f32 {
    let dx = rhs[0] - lhs[0];
    let dy = rhs[1] - lhs[1];
    dx.hypot(dy)
}

fn opacity_at(samples: &[f32], offset: f32) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }

    let points = samples.len() / 2;
    let first_offset = samples[0].clamp(0.0, 1.0);
    let first_opacity = samples[1].clamp(0.0, 1.0);
    if points == 1 || offset <= first_offset {
        return first_opacity;
    }

    for index in 1..points {
        let next_offset = samples[index * 2].clamp(0.0, 1.0);
        let next_opacity = samples[index * 2 + 1].clamp(0.0, 1.0);
        if offset <= next_offset {
            let prev_offset = samples[(index - 1) * 2].clamp(0.0, 1.0);
            let prev_opacity = samples[(index - 1) * 2 + 1].clamp(0.0, 1.0);
            let span = (next_offset - prev_offset).abs();
            if span <= f32::EPSILON {
                return next_opacity;
            }

            let progress = ((offset - prev_offset) / (next_offset - prev_offset)).clamp(0.0, 1.0);
            return (next_opacity - prev_opacity).mul_add(progress, prev_opacity);
        }
    }

    samples[samples.len() - 1].clamp(0.0, 1.0)
}

pub(super) fn decode_fill_style(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<FillStyle> {
    Some(FillStyle {
        brush: decode_brush_style(item, frame, timeline_sample_cache)?,
        opacity: scalar_at(item.opacity.as_ref(), frame, 100.0, timeline_sample_cache) / 100.0,
    })
}

pub(super) fn decode_stroke_style(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<StrokeStyle> {
    Some(StrokeStyle::new(
        decode_brush_style(item, frame, timeline_sample_cache)?,
        scalar_at(item.opacity.as_ref(), frame, 100.0, timeline_sample_cache) / 100.0,
        scalar_at(item.width.as_ref(), frame, 0.0, timeline_sample_cache),
        decode_line_cap(item.line_cap),
        decode_line_join(item.line_join),
        scalar_at(
            item.miter_limit_value.as_ref(),
            frame,
            item.miter_limit.unwrap_or(4.0),
            timeline_sample_cache,
        ),
        decode_stroke_dash(item, frame, timeline_sample_cache),
    ))
}

pub(super) fn decode_trim_style(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<TrimStyle> {
    (item.item_type == "tm").then_some(TrimStyle {
        start: scalar_at(item.trim_start(), frame, 0.0, timeline_sample_cache),
        end: scalar_at(
            item.trim_end().as_ref(),
            frame,
            100.0,
            timeline_sample_cache,
        ),
        offset_degrees: scalar_at(item.trim_offset(), frame, 0.0, timeline_sample_cache),
        mode: item.trim_mode().unwrap_or(1),
    })
}

pub(super) fn decode_repeater_style(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<RepeaterStyle> {
    let transform = item.repeater_transform()?;
    Some(RepeaterStyle {
        copies: scalar_at(item.repeater_copies(), frame, 0.0, timeline_sample_cache),
        offset: scalar_at(item.repeater_offset(), frame, 0.0, timeline_sample_cache),
        mode: item.repeater_composite_mode().unwrap_or(1),
        anchor: vec2_at(
            transform.anchor.as_ref(),
            frame,
            [0.0, 0.0],
            timeline_sample_cache,
        ),
        position: position_vec2_at(
            transform.position.as_ref(),
            frame,
            [0.0, 0.0],
            timeline_sample_cache,
        ),
        scale: vec2_at(
            transform.scale.as_ref(),
            frame,
            [100.0, 100.0],
            timeline_sample_cache,
        ),
        rotation: scalar_at(
            transform.rotation.as_ref(),
            frame,
            0.0,
            timeline_sample_cache,
        )
        .to_radians(),
        start_opacity: scalar_at(
            transform.start_opacity.as_ref(),
            frame,
            100.0,
            timeline_sample_cache,
        ) / 100.0,
        end_opacity: scalar_at(
            transform.end_opacity.as_ref(),
            frame,
            100.0,
            timeline_sample_cache,
        ) / 100.0,
    })
}

fn decode_brush_style(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<BrushStyle> {
    match item.item_type.as_str() {
        "fl" | "st" => Some(BrushStyle::Solid(color_at(
            item.color.as_ref(),
            frame,
            Rgba8::new(0, 0, 0, 255),
            timeline_sample_cache,
        ))),
        "gf" | "gs" => {
            decode_gradient_style(item, frame, timeline_sample_cache).map(BrushStyle::Gradient)
        }
        _ => None,
    }
}

fn decode_gradient_style(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<GradientStyle> {
    let gradient = item.gradient_data()?;
    let samples = sample_numbers(&gradient.colors, frame, timeline_sample_cache)?;
    let point_count = gradient.point_count;
    if point_count == 0 || samples.len() < point_count * 4 {
        return None;
    }

    let color_stop_len = point_count * 4;
    let opacity_samples = &samples[color_stop_len..];
    let mut stops = Vec::with_capacity(point_count);
    for index in 0..point_count {
        let base = index * 4;
        let offset = samples[base].clamp(0.0, 1.0);
        let alpha = opacity_at(opacity_samples, offset);
        stops.push(GradientStopStyle {
            offset,
            color: Rgba8::new(
                unit_to_u8(samples[base + 1]),
                unit_to_u8(samples[base + 2]),
                unit_to_u8(samples[base + 3]),
                unit_to_u8(alpha),
            ),
        });
    }

    let gradient_type = item.gradient_type()?;
    let end_point = item.gradient_end_point();
    let highlight_length = item.gradient_highlight_length();

    Some(GradientStyle {
        kind: match gradient_type {
            1 => GradientKind::Linear,
            2 => GradientKind::Radial,
            _ => return None,
        },
        start: vec2_at(
            item.gradient_start_point(),
            frame,
            [0.0, 0.0],
            timeline_sample_cache,
        ),
        end: vec2_at(end_point.as_ref(), frame, [0.0, 0.0], timeline_sample_cache),
        highlight_length: scalar_at(highlight_length.as_ref(), frame, 0.0, timeline_sample_cache),
        highlight_angle: scalar_at(
            item.gradient_highlight_angle(),
            frame,
            0.0,
            timeline_sample_cache,
        )
        .to_radians(),
        stops,
    })
}

fn decode_stroke_dash(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<TinyStrokeDash> {
    let entries = item.dash_pattern()?;
    let mut array = Vec::new();
    let mut offset = 0.0;

    for entry in entries {
        match entry.name.as_str() {
            "d" | "g" => array
                .push(scalar_at(Some(&entry.value), frame, 0.0, timeline_sample_cache).max(0.0)),
            "o" => offset = scalar_at(Some(&entry.value), frame, 0.0, timeline_sample_cache),
            _ => {}
        }
    }

    if array.is_empty() || array.iter().all(|value| *value <= f32::EPSILON) {
        return None;
    }

    if array.len() % 2 != 0 {
        let repeated = array.clone();
        array.extend(repeated);
    }

    TinyStrokeDash::new(array, offset)
}

fn decode_line_cap(value: Option<u8>) -> TinyLineCap {
    match value.unwrap_or(1) {
        2 => TinyLineCap::Round,
        3 => TinyLineCap::Square,
        _ => TinyLineCap::Butt,
    }
}

fn decode_line_join(value: Option<u8>) -> TinyLineJoin {
    match value.unwrap_or(1) {
        2 => TinyLineJoin::Round,
        3 => TinyLineJoin::Bevel,
        4 => TinyLineJoin::MiterClip,
        _ => TinyLineJoin::Miter,
    }
}

pub(super) fn decode_layer_transform(
    transform: Option<&Transform>,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> RenderTransform {
    let Some(transform) = transform else {
        return RenderTransform::default();
    };

    build_transform(
        transform.anchor.as_ref(),
        transform.position.as_ref(),
        transform.scale.as_ref(),
        transform.rotation.as_ref(),
        transform.opacity.as_ref(),
        frame,
        timeline_sample_cache,
    )
}

pub(super) fn decode_shape_transform(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> RenderTransform {
    let rotation = item.transform_rotation();
    build_transform(
        item.anchor.as_ref(),
        item.position.as_ref(),
        item.size.as_ref(),
        rotation.as_ref(),
        item.opacity.as_ref(),
        frame,
        timeline_sample_cache,
    )
}

fn build_transform(
    anchor: Option<&AnimatedValue>,
    position: Option<&PositionValue>,
    scale: Option<&AnimatedValue>,
    rotation: Option<&AnimatedValue>,
    opacity: Option<&AnimatedValue>,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> RenderTransform {
    let anchor = vec2_at(anchor, frame, [0.0, 0.0], timeline_sample_cache);
    let position = position_vec2_at(position, frame, [0.0, 0.0], timeline_sample_cache);
    let scale = vec2_at(scale, frame, [100.0, 100.0], timeline_sample_cache);
    let rotation = scalar_at(rotation, frame, 0.0, timeline_sample_cache);

    let mut matrix = PixmapTransform::identity();
    matrix = matrix.pre_translate(position[0], position[1]);
    matrix = matrix.pre_rotate(rotation);
    matrix = matrix.pre_scale(scale[0] / 100.0, scale[1] / 100.0);
    matrix = matrix.pre_translate(-anchor[0], -anchor[1]);

    RenderTransform {
        matrix,
        opacity: scalar_at(opacity, frame, 100.0, timeline_sample_cache).clamp(0.0, 100.0) / 100.0,
    }
}

pub(super) fn scalar_at(
    value: Option<&AnimatedValue>,
    frame: f32,
    default: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> f32 {
    if let Some(cache) = timeline_sample_cache
        && let Some(sampled) = value.and_then(|value| cache.sample_scalar(value, frame))
    {
        return sampled;
    }

    value
        .and_then(|value| sample_numbers(value, frame, timeline_sample_cache))
        .and_then(|values| values.first().copied())
        .unwrap_or(default)
}

pub(super) fn vec2_at(
    value: Option<&AnimatedValue>,
    frame: f32,
    default: [f32; 2],
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> [f32; 2] {
    if let Some(cache) = timeline_sample_cache
        && let Some(sampled) = value.and_then(|value| cache.sample_vec2(value, frame))
    {
        return sampled;
    }

    value
        .and_then(|value| sample_numbers(value, frame, timeline_sample_cache))
        .and_then(|values| (values.len() >= 2).then_some([values[0], values[1]]))
        .unwrap_or(default)
}

pub(super) fn position_vec2_at(
    value: Option<&PositionValue>,
    frame: f32,
    default: [f32; 2],
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> [f32; 2] {
    match value {
        Some(PositionValue::Combined(value)) => {
            vec2_at(Some(value), frame, default, timeline_sample_cache)
        }
        Some(PositionValue::Split(value)) if value.is_split() => [
            scalar_at(value.x.as_ref(), frame, default[0], timeline_sample_cache),
            scalar_at(value.y.as_ref(), frame, default[1], timeline_sample_cache),
        ],
        _ => default,
    }
}

pub(super) fn color_at(
    value: Option<&AnimatedValue>,
    frame: f32,
    default: Rgba8,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Rgba8 {
    value
        .and_then(|value| sample_numbers(value, frame, timeline_sample_cache))
        .and_then(|values| {
            if values.len() < 3 {
                return None;
            }

            Some(Rgba8::new(
                unit_to_u8(values[0]),
                unit_to_u8(values[1]),
                unit_to_u8(values[2]),
                unit_to_u8(values.get(3).copied().unwrap_or(1.0)),
            ))
        })
        .unwrap_or(default)
}

pub(super) fn sample_numbers(
    value: &AnimatedValue,
    frame: f32,
    timeline_sample_cache: Option<&TimelineSampleCache>,
) -> Option<Vec<f32>> {
    if let Some(cache) = timeline_sample_cache {
        return cache.sample_numbers(value, frame);
    }

    timeline::sample_numbers(value, frame)
}

fn unit_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
