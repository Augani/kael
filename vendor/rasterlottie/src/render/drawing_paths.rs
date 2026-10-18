use std::{f32::consts::PI, mem, rc::Rc};

use tiny_skia::{
    FillRule, Path, PathBuilder, PathSegment, Pixmap, Rect, StrokeDash as TinyStrokeDash,
    Transform as PixmapTransform,
};

use super::{
    renderer::{RenderTransform, RepeaterStyle, ShapeStyles, StaticPathCache, TrimStyle},
    values::{distance, position_vec2_at, scalar_at, vec2_at},
};
use crate::{AnimatedValue, BezierPath, PositionValue, ShapeItem, ShapePathValue, timeline};

const ROUND_RECT_KAPPA: f32 = 0.552_284_8;
const FULL_TRIM_EPSILON: f32 = 1e-4;

struct StarPathSpec {
    center: [f32; 2],
    points: f32,
    outer_radius: f32,
    inner_radius: f32,
    outer_roundness: f32,
    inner_roundness: f32,
    rotation: f32,
    direction: f32,
}

pub(super) fn draw_path(
    path: &Path,
    styles: &ShapeStyles,
    trim: Option<&TrimStyle>,
    transform: RenderTransform,
    pixmap: &mut Pixmap,
) {
    span_enter!(tracing::Level::TRACE, "draw_path");
    let trimmed = trim.and_then(|trim| {
        span_enter!(tracing::Level::TRACE, "trim_path");
        trim_path(path, trim)
    });
    let resolved = trimmed.as_ref().unwrap_or(path);
    if resolved.is_empty() {
        return;
    }

    if let Some(fill) = styles.fill.as_ref()
        && let Some(paint) = fill.paint(transform.opacity)
    {
        span_enter!(tracing::Level::TRACE, "fill_path");
        pixmap.fill_path(resolved, &paint, FillRule::Winding, transform.matrix, None);
    }

    if let Some(stroke_style) = styles.stroke.as_ref()
        && let Some(paint) = stroke_style.paint(transform.opacity)
    {
        span_enter!(tracing::Level::TRACE, "stroke_path");
        pixmap.stroke_path(
            resolved,
            &paint,
            stroke_style.stroke(),
            transform.matrix,
            None,
        );
    }
}

fn build_rect_path(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
) -> Option<Path> {
    let center = position_vec2_at(
        item.position.as_ref(),
        frame,
        [0.0, 0.0],
        timeline_sample_cache,
    );
    let size = vec2_at(item.size.as_ref(), frame, [0.0, 0.0], timeline_sample_cache);
    let width = size[0].abs();
    let height = size[1].abs();
    let rect = Rect::from_xywh(
        center[0] - width * 0.5,
        center[1] - height * 0.5,
        width,
        height,
    )?;

    let roundness = item.rectangle_roundness();
    let radius = scalar_at(roundness.as_ref(), frame, 0.0, timeline_sample_cache)
        .clamp(0.0, width.min(height) * 0.5);
    if radius <= f32::EPSILON {
        return Some(PathBuilder::from_rect(rect));
    }

    build_round_rect_path(rect, radius)
}

fn build_ellipse_path(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
) -> Option<Path> {
    let center = position_vec2_at(
        item.position.as_ref(),
        frame,
        [0.0, 0.0],
        timeline_sample_cache,
    );
    let size = vec2_at(item.size.as_ref(), frame, [0.0, 0.0], timeline_sample_cache);
    let width = size[0].abs();
    let height = size[1].abs();
    let rect = Rect::from_xywh(
        center[0] - width * 0.5,
        center[1] - height * 0.5,
        width,
        height,
    )?;

    PathBuilder::from_oval(rect)
}

fn build_shape_path(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
) -> Option<Path> {
    let geometry = item.path.as_ref().and_then(|path| {
        timeline_sample_cache.map_or_else(
            || timeline::sample_shape_path(path, frame),
            |cache| cache.sample_shape_path(path, frame),
        )
    })?;
    build_bezier_path(&geometry)
}

pub(super) fn build_supported_merge_path(
    items: &[ShapeItem],
    frame: f32,
    static_path_cache: Option<&StaticPathCache>,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
    merge_index_override: Option<usize>,
) -> Option<Path> {
    let merge_index = merge_index_override.or_else(|| {
        items
            .iter()
            .position(|item| !item.hidden && item.item_type == "mm" && item.merge_mode() == Some(1))
    })?;

    for (index, item) in items.iter().enumerate().skip(merge_index + 1) {
        if item.hidden {
            continue;
        }

        match item.item_type.as_str() {
            "mm" if render_merge_path_has_no_new_sources(items, index) => {}
            "fl" | "st" | "gf" | "gs" | "tr" | "tm" | "rp" => {}
            _ => return None,
        }
    }

    let mut geometry_count = 0usize;
    let mut builder = PathBuilder::new();

    for item in items.iter().take(merge_index) {
        if item.hidden {
            continue;
        }

        match item.item_type.as_str() {
            "sh" | "rc" | "el" | "sr" => {
                let path =
                    build_shape_source_path(item, frame, static_path_cache, timeline_sample_cache)?;
                builder.push_path(&path);
                geometry_count += 1;
            }
            "gr" if group_is_render_merge_noop(item) => {}
            "tr" => {}
            _ => return None,
        }
    }

    (geometry_count >= 2).then(|| builder.finish()).flatten()
}

fn render_merge_path_has_no_new_sources(items: &[ShapeItem], index: usize) -> bool {
    let mut saw_previous_merge = false;

    for candidate in items.iter().take(index).rev() {
        if candidate.hidden {
            continue;
        }

        if candidate.item_type == "mm" {
            saw_previous_merge = true;
            break;
        }

        match candidate.item_type.as_str() {
            "gr" if group_is_render_merge_noop(candidate) => {}
            "tr" => {}
            _ => return false,
        }
    }

    saw_previous_merge
}

pub(super) fn build_shape_source_path(
    item: &ShapeItem,
    frame: f32,
    static_path_cache: Option<&StaticPathCache>,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
) -> Option<Rc<Path>> {
    span_enter!(
        tracing::Level::TRACE,
        "build_shape_source_path",
        frame = frame,
        item_type = item.item_type.as_str()
    );
    if let Some(cache) = static_path_cache
        && shape_item_has_static_geometry(item)
    {
        return cache.get_or_insert(item, || build_static_shape_source_path(item));
    }

    match item.item_type.as_str() {
        "rc" => build_rect_path(item, frame, timeline_sample_cache).map(Rc::new),
        "el" => build_ellipse_path(item, frame, timeline_sample_cache).map(Rc::new),
        "sh" => build_shape_path(item, frame, timeline_sample_cache).map(Rc::new),
        "sr" => build_polystar_path(item, frame, timeline_sample_cache).map(Rc::new),
        _ => None,
    }
}

fn build_static_shape_source_path(item: &ShapeItem) -> Option<Path> {
    match item.item_type.as_str() {
        "rc" => build_rect_path(item, 0.0, None),
        "el" => build_ellipse_path(item, 0.0, None),
        "sh" => build_shape_path(item, 0.0, None),
        "sr" => build_polystar_path(item, 0.0, None),
        _ => None,
    }
}

fn shape_item_has_static_geometry(item: &ShapeItem) -> bool {
    match item.item_type.as_str() {
        "rc" => {
            position_value_is_static(item.position.as_ref())
                && animated_value_is_static(item.size.as_ref())
                && animated_value_is_static(item.rectangle_roundness().as_ref())
        }
        "el" => {
            position_value_is_static(item.position.as_ref())
                && animated_value_is_static(item.size.as_ref())
        }
        "sh" => item.path.as_ref().is_some_and(ShapePathValue::is_static),
        "sr" => {
            position_value_is_static(item.position.as_ref())
                && animated_value_is_static(item.polystar_rotation().as_ref())
                && animated_value_is_static(item.polystar_points().as_ref())
                && animated_value_is_static(item.polystar_outer_radius().as_ref())
                && animated_value_is_static(item.polystar_outer_roundness().as_ref())
                && animated_value_is_static(item.polystar_inner_radius().as_ref())
                && animated_value_is_static(item.polystar_inner_roundness().as_ref())
        }
        _ => false,
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

fn group_is_render_merge_noop(item: &ShapeItem) -> bool {
    if item.item_type != "gr" {
        return false;
    }

    item.items.iter().all(|child| {
        child.hidden
            || child.item_type == "tr"
            || (child.item_type == "gr" && group_is_render_merge_noop(child))
    })
}

fn build_polystar_path(
    item: &ShapeItem,
    frame: f32,
    timeline_sample_cache: Option<&super::sample_cache::TimelineSampleCache>,
) -> Option<Path> {
    let center = position_vec2_at(
        item.position.as_ref(),
        frame,
        [0.0, 0.0],
        timeline_sample_cache,
    );
    let rotation_value = item.polystar_rotation();
    let rotation = PI.mul_add(
        -0.5,
        scalar_at(rotation_value.as_ref(), frame, 0.0, timeline_sample_cache).to_radians(),
    );
    let direction = if item.shape_direction() == Some(3) {
        -1.0
    } else {
        1.0
    };

    match item.polystar_type()? {
        1 => build_star_path(&StarPathSpec {
            center,
            points: scalar_at(
                item.polystar_points().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            ),
            outer_radius: scalar_at(
                item.polystar_outer_radius().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            )
            .abs(),
            inner_radius: scalar_at(
                item.polystar_inner_radius().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            )
            .abs(),
            outer_roundness: scalar_at(
                item.polystar_outer_roundness().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            ) / 100.0,
            inner_roundness: scalar_at(
                item.polystar_inner_roundness().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            ) / 100.0,
            rotation,
            direction,
        }),
        2 => build_polygon_path(
            center,
            scalar_at(
                item.polystar_points().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            ),
            scalar_at(
                item.polystar_outer_radius().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            )
            .abs(),
            scalar_at(
                item.polystar_outer_roundness().as_ref(),
                frame,
                0.0,
                timeline_sample_cache,
            ) / 100.0,
            rotation,
            direction,
        ),
        _ => None,
    }
}

fn build_round_rect_path(rect: Rect, radius: f32) -> Option<Path> {
    let left = rect.left();
    let top = rect.top();
    let right = rect.right();
    let bottom = rect.bottom();
    let tangent = radius * ROUND_RECT_KAPPA;

    let mut pb = PathBuilder::new();
    pb.move_to(left + radius, top);
    pb.line_to(right - radius, top);
    pb.cubic_to(
        right - radius + tangent,
        top,
        right,
        top + radius - tangent,
        right,
        top + radius,
    );
    pb.line_to(right, bottom - radius);
    pb.cubic_to(
        right,
        bottom - radius + tangent,
        right - radius + tangent,
        bottom,
        right - radius,
        bottom,
    );
    pb.line_to(left + radius, bottom);
    pb.cubic_to(
        left + radius - tangent,
        bottom,
        left,
        bottom - radius + tangent,
        left,
        bottom - radius,
    );
    pb.line_to(left, top + radius);
    pb.cubic_to(
        left,
        top + radius - tangent,
        left + radius - tangent,
        top,
        left + radius,
        top,
    );
    pb.close();
    pb.finish()
}

pub(super) fn build_bezier_path(path: &BezierPath) -> Option<Path> {
    let first = path.vertices.first()?;

    let mut pb = PathBuilder::new();
    pb.move_to(first.vertex[0], first.vertex[1]);

    if path.vertices.len() == 1 {
        pb.line_to(first.vertex[0], first.vertex[1]);
        return pb.finish();
    }

    for window in path.vertices.windows(2) {
        push_bezier_segment(&mut pb, &window[0], &window[1]);
    }

    if path.closed {
        let last_index = path.vertices.len() - 1;
        push_bezier_segment(&mut pb, &path.vertices[last_index], &path.vertices[0]);
        pb.close();
    }

    pb.finish()
}

fn push_bezier_segment(
    pb: &mut PathBuilder,
    current: &crate::BezierVertex,
    next: &crate::BezierVertex,
) {
    let cp1 = add_vec2(current.vertex, current.out_tangent);
    let cp2 = add_vec2(next.vertex, next.in_tangent);

    if is_linear_segment(current.vertex, cp1, cp2, next.vertex) {
        pb.line_to(next.vertex[0], next.vertex[1]);
        return;
    }

    pb.cubic_to(
        cp1[0],
        cp1[1],
        cp2[0],
        cp2[1],
        next.vertex[0],
        next.vertex[1],
    );
}

fn add_vec2(lhs: [f32; 2], rhs: [f32; 2]) -> [f32; 2] {
    [lhs[0] + rhs[0], lhs[1] + rhs[1]]
}

fn is_linear_segment(start: [f32; 2], cp1: [f32; 2], cp2: [f32; 2], end: [f32; 2]) -> bool {
    approx_eq_vec2(start, cp1) && approx_eq_vec2(cp2, end)
}

fn approx_eq_vec2(lhs: [f32; 2], rhs: [f32; 2]) -> bool {
    const EPSILON: f32 = 1e-4;
    (lhs[0] - rhs[0]).abs() <= EPSILON && (lhs[1] - rhs[1]).abs() <= EPSILON
}

fn build_star_path(spec: &StarPathSpec) -> Option<Path> {
    let count = spec.points.floor().max(0.0) as usize;
    if count < 2 || spec.outer_radius <= f32::EPSILON || spec.inner_radius <= f32::EPSILON {
        return None;
    }

    let total_points = count * 2;
    let angle = (PI * 2.0) / total_points as f32;
    let long_perimeter = (2.0 * PI * spec.outer_radius) / (total_points as f32 * 2.0);
    let short_perimeter = (2.0 * PI * spec.inner_radius) / (total_points as f32 * 2.0);

    let mut current_angle = spec.rotation;
    let mut long_flag = true;
    let mut vertices = Vec::with_capacity(total_points);

    for _ in 0..total_points {
        let radius = if long_flag {
            spec.outer_radius
        } else {
            spec.inner_radius
        };
        let roundness = if long_flag {
            spec.outer_roundness
        } else {
            spec.inner_roundness
        };
        let perimeter = if long_flag {
            long_perimeter
        } else {
            short_perimeter
        };
        let local = [radius * current_angle.cos(), radius * current_angle.sin()];
        let normal = unit_perpendicular(local);
        let vertex = [spec.center[0] + local[0], spec.center[1] + local[1]];
        let tangent = perimeter * roundness * spec.direction;
        vertices.push(crate::BezierVertex {
            vertex,
            in_tangent: [normal[0] * tangent, normal[1] * tangent],
            out_tangent: [-normal[0] * tangent, -normal[1] * tangent],
        });
        long_flag = !long_flag;
        current_angle += angle * spec.direction;
    }

    build_bezier_path(&BezierPath {
        closed: true,
        vertices,
    })
}

fn build_polygon_path(
    center: [f32; 2],
    points: f32,
    radius: f32,
    roundness: f32,
    rotation: f32,
    direction: f32,
) -> Option<Path> {
    let count = points.floor().max(0.0) as usize;
    if count < 3 || radius <= f32::EPSILON {
        return None;
    }

    let angle = (PI * 2.0) / count as f32;
    let perimeter = (2.0 * PI * radius) / (count as f32 * 4.0);
    let tangent = perimeter * roundness * direction;
    let mut current_angle = rotation;
    let mut vertices = Vec::with_capacity(count);

    for _ in 0..count {
        let local = [radius * current_angle.cos(), radius * current_angle.sin()];
        let normal = unit_perpendicular(local);
        let vertex = [center[0] + local[0], center[1] + local[1]];
        vertices.push(crate::BezierVertex {
            vertex,
            in_tangent: [normal[0] * tangent, normal[1] * tangent],
            out_tangent: [-normal[0] * tangent, -normal[1] * tangent],
        });
        current_angle += angle * direction;
    }

    build_bezier_path(&BezierPath {
        closed: true,
        vertices,
    })
}

fn unit_perpendicular(point: [f32; 2]) -> [f32; 2] {
    let length = point[0].hypot(point[1]);
    if length <= f32::EPSILON {
        [0.0, 0.0]
    } else {
        [point[1] / length, -point[0] / length]
    }
}

fn trim_path(path: &Path, trim: &TrimStyle) -> Option<Path> {
    if !matches!(trim.mode, 1 | 2) {
        return None;
    }

    let total_length = approx_path_length(path);
    if total_length <= f32::EPSILON {
        return None;
    }

    let visible_segments = normalized_trim_segments(trim);
    if visible_segments.is_empty() {
        return PathBuilder::new().finish();
    }
    if is_full_trim_segments(&visible_segments) {
        return Some(path.clone());
    }

    let mut builder = PathBuilder::new();
    for (start, end) in visible_segments {
        let before = (total_length * start).max(0.0);
        let visible = (total_length * (end - start)).max(0.0);
        let after = (total_length * (1.0 - end)).max(0.0);
        let dash = TinyStrokeDash::new(vec![0.0, before, visible, after], 0.0)?;
        let trimmed = path.dash(&dash, 1.0)?;
        builder.push_path(&trimmed);
    }

    builder.finish()
}

fn normalized_trim_segments(trim: &TrimStyle) -> Vec<(f32, f32)> {
    let offset = (trim.offset_degrees.rem_euclid(360.0)) / 360.0;
    let mut start = (trim.start / 100.0).clamp(0.0, 1.0) + offset;
    let mut end = (trim.end / 100.0).clamp(0.0, 1.0) + offset;
    if start > end {
        mem::swap(&mut start, &mut end);
    }

    start = (start * 10_000.0).round() * 0.0001;
    end = (end * 10_000.0).round() * 0.0001;

    if (start - end).abs() <= 1e-4 {
        return Vec::new();
    }

    if ((start - 0.0).abs() <= FULL_TRIM_EPSILON && (end - 1.0).abs() <= FULL_TRIM_EPSILON)
        || ((start - 1.0).abs() <= FULL_TRIM_EPSILON && (end - 0.0).abs() <= FULL_TRIM_EPSILON)
    {
        return vec![(0.0, 1.0)];
    }

    if end <= 1.0 {
        vec![(start.clamp(0.0, 1.0), end.clamp(0.0, 1.0))]
    } else if start >= 1.0 {
        vec![((start - 1.0).clamp(0.0, 1.0), (end - 1.0).clamp(0.0, 1.0))]
    } else {
        vec![
            (start.clamp(0.0, 1.0), 1.0),
            (0.0, (end - 1.0).clamp(0.0, 1.0)),
        ]
    }
}

fn is_full_trim_segments(segments: &[(f32, f32)]) -> bool {
    segments.len() == 1 && segments[0].0 <= 1e-4 && (1.0 - segments[0].1).abs() <= 1e-4
}

fn approx_path_length(path: &Path) -> f32 {
    let mut length = 0.0;
    let mut last = None;
    let mut contour_start = None;

    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(point) => {
                last = Some([point.x, point.y]);
                contour_start = Some([point.x, point.y]);
            }
            PathSegment::LineTo(point) => {
                let next = [point.x, point.y];
                if let Some(current) = last {
                    length += distance(current, next);
                }
                last = Some(next);
            }
            PathSegment::QuadTo(control, point) => {
                let next = [point.x, point.y];
                if let Some(current) = last {
                    length += approx_quad_length(current, [control.x, control.y], next, 24);
                }
                last = Some(next);
            }
            PathSegment::CubicTo(control1, control2, point) => {
                let next = [point.x, point.y];
                if let Some(current) = last {
                    length += approx_cubic_length(
                        current,
                        [control1.x, control1.y],
                        [control2.x, control2.y],
                        next,
                        32,
                    );
                }
                last = Some(next);
            }
            PathSegment::Close => {
                if let (Some(current), Some(start)) = (last, contour_start) {
                    length += distance(current, start);
                }
                last = contour_start;
            }
        }
    }

    length
}

fn approx_quad_length(start: [f32; 2], control: [f32; 2], end: [f32; 2], steps: usize) -> f32 {
    let mut length = 0.0;
    let mut previous = start;
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let point = eval_quad(start, control, end, t);
        length += distance(previous, point);
        previous = point;
    }
    length
}

fn approx_cubic_length(
    start: [f32; 2],
    control1: [f32; 2],
    control2: [f32; 2],
    end: [f32; 2],
    steps: usize,
) -> f32 {
    let mut length = 0.0;
    let mut previous = start;
    for step in 1..=steps {
        let t = step as f32 / steps as f32;
        let point = eval_cubic(start, control1, control2, end, t);
        length += distance(previous, point);
        previous = point;
    }
    length
}

fn eval_quad(start: [f32; 2], control: [f32; 2], end: [f32; 2], t: f32) -> [f32; 2] {
    let one_minus_t = 1.0 - t;
    [
        (t * t).mul_add(
            end[0],
            (one_minus_t * one_minus_t).mul_add(start[0], 2.0 * one_minus_t * t * control[0]),
        ),
        (t * t).mul_add(
            end[1],
            (one_minus_t * one_minus_t).mul_add(start[1], 2.0 * one_minus_t * t * control[1]),
        ),
    ]
}

fn eval_cubic(
    start: [f32; 2],
    control1: [f32; 2],
    control2: [f32; 2],
    end: [f32; 2],
    t: f32,
) -> [f32; 2] {
    let one_minus_t = 1.0 - t;
    [
        t.powi(3).mul_add(
            end[0],
            (3.0 * one_minus_t * t.powi(2)).mul_add(
                control2[0],
                one_minus_t
                    .powi(3)
                    .mul_add(start[0], 3.0 * one_minus_t.powi(2) * t * control1[0]),
            ),
        ),
        t.powi(3).mul_add(
            end[1],
            (3.0 * one_minus_t * t.powi(2)).mul_add(
                control2[1],
                one_minus_t
                    .powi(3)
                    .mul_add(start[1], 3.0 * one_minus_t.powi(2) * t * control1[1]),
            ),
        ),
    ]
}

pub(super) fn repeater_copy_order(repeater: &RepeaterStyle) -> Vec<usize> {
    let copies = repeater.copies.ceil().max(0.0) as usize;
    let mut indices = (0..copies).collect::<Vec<_>>();
    if repeater.mode != 1 {
        indices.reverse();
    }
    indices
}

pub(super) fn repeater_transform_for_copy(
    repeater: &RepeaterStyle,
    copy_index: usize,
) -> RenderTransform {
    let mut transform =
        repeater_transform_for_iteration(repeater, repeater.offset + copy_index as f32);
    let copies = repeater.copies.ceil().max(0.0) as usize;
    transform.opacity = if copies <= 1 {
        repeater.start_opacity
    } else {
        let progress = copy_index as f32 / (copies - 1) as f32;
        (repeater.end_opacity - repeater.start_opacity).mul_add(progress, repeater.start_opacity)
    };
    transform
}

fn repeater_transform_for_iteration(repeater: &RepeaterStyle, iteration: f32) -> RenderTransform {
    if iteration.abs() <= f32::EPSILON {
        return RenderTransform::default();
    }

    let inverse = iteration.is_sign_negative();
    let magnitude = iteration.abs();
    let whole = magnitude.floor() as usize;
    let fraction = magnitude - whole as f32;

    let mut transform = RenderTransform::default();
    let full_step = repeater_step_transform(repeater, 1.0, inverse);
    for _ in 0..whole {
        transform = transform.concat(full_step);
    }

    if fraction > f32::EPSILON {
        transform = transform.concat(repeater_step_transform(repeater, fraction, inverse));
    }

    transform
}

fn repeater_step_transform(
    repeater: &RepeaterStyle,
    percent: f32,
    inverse: bool,
) -> RenderTransform {
    let direction = if inverse { -1.0 } else { 1.0 };
    let scale = [
        ((repeater.scale[0] / 100.0) - 1.0).mul_add(percent, 1.0),
        ((repeater.scale[1] / 100.0) - 1.0).mul_add(percent, 1.0),
    ];
    let applied_scale = if inverse {
        [1.0 / scale[0].max(1e-4), 1.0 / scale[1].max(1e-4)]
    } else {
        scale
    };

    let mut matrix = PixmapTransform::identity();
    matrix = matrix.pre_translate(
        repeater.position[0] * direction * percent,
        repeater.position[1] * direction * percent,
    );
    matrix = matrix.pre_translate(repeater.anchor[0], repeater.anchor[1]);
    matrix = matrix.pre_rotate(repeater.rotation * direction * percent);
    matrix = matrix.pre_scale(applied_scale[0], applied_scale[1]);
    matrix = matrix.pre_translate(-repeater.anchor[0], -repeater.anchor[1]);

    RenderTransform {
        matrix,
        opacity: 1.0,
    }
}
