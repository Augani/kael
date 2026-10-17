use serde_json::Value;

use crate::{
    AnimatedValue, BezierPath, BezierVertex, ShapePathValue, model::parse_bezier_keyframe_value,
};

#[derive(Debug, Clone)]
pub struct NumericKeyframe {
    time: f32,
    start: Vec<f32>,
    end: Option<Vec<f32>>,
    spatial: Option<SpatialTangent>,
    hold: bool,
    out_easing: EasingHandle,
    in_easing: EasingHandle,
}

#[derive(Debug, Clone, Default)]
struct EasingHandle {
    x: Vec<f32>,
    y: Vec<f32>,
}

#[derive(Debug, Clone)]
struct SpatialTangent {
    out_tangent: Vec<f32>,
    in_tangent: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct ShapeKeyframe {
    time: f32,
    start: BezierPath,
    end: Option<BezierPath>,
    hold: bool,
    out_easing: EasingHandle,
    in_easing: EasingHandle,
}

#[derive(Debug, Clone)]
struct SpatialSample {
    distance: f32,
    point: Vec<f32>,
}

impl EasingHandle {
    fn component_x(&self, index: usize, default: f32) -> f32 {
        self.x
            .get(index)
            .copied()
            .or_else(|| self.x.first().copied())
            .unwrap_or(default)
    }

    fn component_y(&self, index: usize, default: f32) -> f32 {
        self.y
            .get(index)
            .copied()
            .or_else(|| self.y.first().copied())
            .unwrap_or(default)
    }
}

pub fn is_supported_animated_value(value: &AnimatedValue) -> bool {
    value.is_static() || parse_numeric_keyframes(value).is_some()
}

pub fn is_supported_scalar_value(value: &AnimatedValue) -> bool {
    if value.is_static() {
        return value.as_scalar().is_some();
    }

    let Some(keyframes) = parse_numeric_keyframes(value) else {
        return false;
    };

    keyframes.iter().all(|keyframe| {
        keyframe.start.len() == 1
            && keyframe.end.as_ref().is_none_or(|end| end.len() == 1)
            && keyframe.spatial.is_none()
    })
}

pub fn sample_numbers(value: &AnimatedValue, frame: f32) -> Option<Vec<f32>> {
    if value.is_static() {
        return sample_static_numbers(value);
    }

    let keyframes = parse_numeric_keyframes(value)?;
    sample_numbers_from_keyframes(&keyframes, frame)
}

pub fn sample_numbers_from_keyframes(
    keyframes: &[NumericKeyframe],
    frame: f32,
) -> Option<Vec<f32>> {
    let first = keyframes.first()?;
    if frame <= first.time {
        return Some(first.start.clone());
    }

    for window in keyframes.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if frame >= next.time {
            continue;
        }

        if current.hold || next.time <= current.time {
            return Some(current.start.clone());
        }

        let duration = next.time - current.time;
        let progress = ((frame - current.time) / duration).clamp(0.0, 1.0);
        let end = current.end.as_ref().unwrap_or(&next.start);
        if current.start.len() != end.len() {
            return None;
        }

        let values = current
            .start
            .iter()
            .zip(end.iter())
            .enumerate()
            .map(|(index, (start, end))| {
                let eased =
                    eased_progress(progress, &current.out_easing, &current.in_easing, index);
                start + (end - start) * eased
            })
            .collect();
        if let Some(spatial) = current.spatial.as_ref() {
            return sample_spatial_bezier(
                &current.start,
                end,
                spatial,
                eased_progress(progress, &current.out_easing, &current.in_easing, 0),
            );
        }
        return Some(values);
    }

    Some(keyframes.last()?.start.clone())
}

pub fn sample_scalar(value: &AnimatedValue, frame: f32) -> Option<f32> {
    let values = sample_numbers(value, frame)?;
    (values.len() == 1).then_some(values[0])
}

pub fn sample_scalar_from_keyframes(keyframes: &[NumericKeyframe], frame: f32) -> Option<f32> {
    let first = keyframes.first()?;
    if frame <= first.time {
        return first.start.first().copied();
    }

    for window in keyframes.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if frame >= next.time {
            continue;
        }

        if current.hold || next.time <= current.time {
            return current.start.first().copied();
        }

        let end = current.end.as_ref().unwrap_or(&next.start);
        let start = *current.start.first()?;
        let end = *end.first()?;
        let duration = next.time - current.time;
        let progress = ((frame - current.time) / duration).clamp(0.0, 1.0);
        let eased = eased_progress(progress, &current.out_easing, &current.in_easing, 0);
        return Some((end - start).mul_add(eased, start));
    }

    keyframes.last()?.start.first().copied()
}

pub fn sample_vec2_from_keyframes(keyframes: &[NumericKeyframe], frame: f32) -> Option<[f32; 2]> {
    let first = keyframes.first()?;
    if frame <= first.time {
        return (first.start.len() >= 2).then_some([first.start[0], first.start[1]]);
    }

    for window in keyframes.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if frame >= next.time {
            continue;
        }

        if current.hold || next.time <= current.time {
            return (current.start.len() >= 2).then_some([current.start[0], current.start[1]]);
        }

        let end = current.end.as_ref().unwrap_or(&next.start);
        if current.start.len() < 2 || end.len() < 2 {
            return None;
        }

        let duration = next.time - current.time;
        let progress = ((frame - current.time) / duration).clamp(0.0, 1.0);
        if let Some(spatial) = current.spatial.as_ref() {
            let sampled = sample_spatial_bezier(
                &current.start,
                end,
                spatial,
                eased_progress(progress, &current.out_easing, &current.in_easing, 0),
            )?;
            return (sampled.len() >= 2).then_some([sampled[0], sampled[1]]);
        }

        let eased_x = eased_progress(progress, &current.out_easing, &current.in_easing, 0);
        let eased_y = eased_progress(progress, &current.out_easing, &current.in_easing, 1);
        return Some([
            (end[0] - current.start[0]).mul_add(eased_x, current.start[0]),
            (end[1] - current.start[1]).mul_add(eased_y, current.start[1]),
        ]);
    }

    let last = keyframes.last()?;
    (last.start.len() >= 2).then_some([last.start[0], last.start[1]])
}

pub fn is_supported_shape_path(value: &ShapePathValue) -> bool {
    value.is_static() && value.as_bezier_path().is_some() || parse_shape_keyframes(value).is_some()
}

pub fn sample_shape_path(value: &ShapePathValue, frame: f32) -> Option<BezierPath> {
    if value.is_static() {
        return value.as_bezier_path();
    }

    let keyframes = parse_shape_keyframes(value)?;
    sample_shape_path_from_keyframes(&keyframes, frame)
}

pub fn sample_shape_path_from_keyframes(
    keyframes: &[ShapeKeyframe],
    frame: f32,
) -> Option<BezierPath> {
    let first = keyframes.first()?;
    if frame <= first.time {
        return Some(first.start.clone());
    }

    for window in keyframes.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if frame >= next.time {
            continue;
        }

        if current.hold || next.time <= current.time {
            return Some(current.start.clone());
        }

        let duration = next.time - current.time;
        let progress = ((frame - current.time) / duration).clamp(0.0, 1.0);
        let eased = eased_progress(progress, &current.out_easing, &current.in_easing, 0);
        let end = current.end.as_ref().unwrap_or(&next.start);
        return interpolate_bezier_path(&current.start, end, eased);
    }

    Some(keyframes.last()?.start.clone())
}

pub fn parse_numeric_keyframes(value: &AnimatedValue) -> Option<Vec<NumericKeyframe>> {
    let items = value.keyframes.as_ref()?.as_array()?;
    let mut keyframes = Vec::with_capacity(items.len());

    for item in items {
        let object = item.as_object()?;
        let start = parse_numeric_components(object.get("s")?)?;
        let end = object.get("e").and_then(parse_numeric_components);

        keyframes.push(NumericKeyframe {
            time: object.get("t")?.as_f64()? as f32,
            start: start.clone(),
            end: end.clone(),
            spatial: parse_spatial_tangent(object, &start).ok()?,
            hold: object.get("h").and_then(value_as_bool).unwrap_or(false),
            out_easing: object
                .get("o")
                .and_then(parse_easing_handle)
                .unwrap_or_default(),
            in_easing: object
                .get("i")
                .and_then(parse_easing_handle)
                .unwrap_or_default(),
        });
    }

    if keyframes.is_empty() {
        return None;
    }

    for window in keyframes.windows(2) {
        let current = &window[0];
        let next = &window[1];
        let end = current.end.as_ref().unwrap_or(&next.start);
        if current.start.len() != end.len() {
            return None;
        }

        if let Some(spatial) = current.spatial.as_ref()
            && (spatial.out_tangent.len() != current.start.len()
                || spatial.in_tangent.len() != current.start.len())
        {
            return None;
        }
    }

    Some(keyframes)
}

fn parse_spatial_tangent(
    object: &serde_json::Map<String, Value>,
    start: &[f32],
) -> Result<Option<SpatialTangent>, ()> {
    let out_tangent = match object.get("to") {
        Some(value) => Some(parse_numeric_components(value).ok_or(())?),
        None => None,
    };
    let in_tangent = match object.get("ti") {
        Some(value) => Some(parse_numeric_components(value).ok_or(())?),
        None => None,
    };

    match (out_tangent, in_tangent) {
        (None, None) => Ok(None),
        (Some(out_tangent), Some(in_tangent)) => {
            let dimensions = start.len();
            if out_tangent.len() != dimensions
                || in_tangent.len() != dimensions
                || !(2..=3).contains(&dimensions)
            {
                return Err(());
            }

            Ok(Some(SpatialTangent {
                out_tangent,
                in_tangent,
            }))
        }
        _ => Err(()),
    }
}

pub fn parse_shape_keyframes(value: &ShapePathValue) -> Option<Vec<ShapeKeyframe>> {
    let items = value.keyframes.as_ref()?.as_array()?;
    let mut keyframes = Vec::with_capacity(items.len());

    for item in items {
        let object = item.as_object()?;
        let start = parse_bezier_keyframe_value(object.get("s")?)?;
        let end = object.get("e").and_then(parse_bezier_keyframe_value);
        if let Some(end) = end.as_ref()
            && !bezier_paths_are_compatible(&start, end)
        {
            return None;
        }

        keyframes.push(ShapeKeyframe {
            time: object.get("t")?.as_f64()? as f32,
            start,
            end,
            hold: object.get("h").and_then(value_as_bool).unwrap_or(false),
            out_easing: object
                .get("o")
                .and_then(parse_easing_handle)
                .unwrap_or_default(),
            in_easing: object
                .get("i")
                .and_then(parse_easing_handle)
                .unwrap_or_default(),
        });
    }

    if keyframes.is_empty() {
        return None;
    }

    for window in keyframes.windows(2) {
        let current = &window[0];
        let next = &window[1];
        let current_end = current.end.as_ref().unwrap_or(&next.start);
        if !bezier_paths_are_compatible(&current.start, current_end)
            || !bezier_paths_are_compatible(current_end, &next.start)
        {
            return None;
        }
    }

    Some(keyframes)
}

pub fn sample_static_numbers(value: &AnimatedValue) -> Option<Vec<f32>> {
    match value.keyframes.as_ref()? {
        Value::Number(number) => Some(vec![number.as_f64()? as f32]),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_f64().map(|number| number as f32))
            .collect(),
        _ => None,
    }
}

fn parse_numeric_components(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Number(number) => Some(vec![number.as_f64()? as f32]),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_f64().map(|number| number as f32))
            .collect(),
        _ => None,
    }
}

fn parse_easing_handle(value: &Value) -> Option<EasingHandle> {
    let object = value.as_object()?;
    Some(EasingHandle {
        x: parse_handle_component(object.get("x")?)?,
        y: parse_handle_component(object.get("y")?)?,
    })
}

fn parse_handle_component(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Number(number) => Some(vec![number.as_f64()? as f32]),
        Value::Array(items) => items
            .iter()
            .map(|item| item.as_f64().map(|number| number as f32))
            .collect(),
        _ => None,
    }
}

fn value_as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => value.as_i64().map(|number| number != 0),
        _ => None,
    }
}

fn cubic_bezier_ease(progress: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    if progress <= 0.0 {
        return 0.0;
    }
    if progress >= 1.0 {
        return 1.0;
    }

    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..24 {
        let mid = (low + high) * 0.5;
        let x = cubic_bezier_point(mid, 0.0, x1, x2, 1.0);
        if x < progress {
            low = mid;
        } else {
            high = mid;
        }
    }

    cubic_bezier_point((low + high) * 0.5, 0.0, y1, y2, 1.0)
}

fn eased_progress(
    progress: f32,
    out_easing: &EasingHandle,
    in_easing: &EasingHandle,
    index: usize,
) -> f32 {
    cubic_bezier_ease(
        progress,
        out_easing.component_x(index, 0.0),
        out_easing.component_y(index, 0.0),
        in_easing.component_x(index, 1.0),
        in_easing.component_y(index, 1.0),
    )
}

fn cubic_bezier_point(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let one_minus_t = 1.0 - t;
    t.powi(3).mul_add(
        p3,
        (3.0 * one_minus_t * t.powi(2)).mul_add(
            p2,
            one_minus_t
                .powi(3)
                .mul_add(p0, 3.0 * one_minus_t.powi(2) * t * p1),
        ),
    )
}

fn sample_spatial_bezier(
    start: &[f32],
    end: &[f32],
    spatial: &SpatialTangent,
    progress: f32,
) -> Option<Vec<f32>> {
    const CURVE_SEGMENTS: usize = 150;

    if start.len() != end.len()
        || spatial.out_tangent.len() != start.len()
        || spatial.in_tangent.len() != start.len()
    {
        return None;
    }

    let control1 = add_components(start, &spatial.out_tangent);
    let control2 = add_components(end, &spatial.in_tangent);
    let samples = build_spatial_samples(start, end, &control1, &control2, CURVE_SEGMENTS);
    let total_length = samples.last()?.distance;
    if total_length <= f32::EPSILON {
        return Some(start.to_vec());
    }

    let target_distance = total_length * progress.clamp(0.0, 1.0);
    for window in samples.windows(2) {
        let current = &window[0];
        let next = &window[1];
        if target_distance > next.distance {
            continue;
        }

        let segment_length = next.distance - current.distance;
        if segment_length <= f32::EPSILON {
            return Some(current.point.clone());
        }

        let segment_progress = (target_distance - current.distance) / segment_length;
        return Some(
            current
                .point
                .iter()
                .zip(next.point.iter())
                .map(|(start, end)| start + (end - start) * segment_progress)
                .collect(),
        );
    }

    Some(samples.last()?.point.clone())
}

fn build_spatial_samples(
    start: &[f32],
    end: &[f32],
    control1: &[f32],
    control2: &[f32],
    segments: usize,
) -> Vec<SpatialSample> {
    let segments = segments.max(2);
    let mut samples = Vec::with_capacity(segments);
    let mut last_point: Option<Vec<f32>> = None;
    let mut distance = 0.0;

    for index in 0..segments {
        let t = index as f32 / (segments - 1) as f32;
        let point = cubic_bezier_point_vec(t, start, control1, control2, end);
        if let Some(previous) = last_point.as_ref() {
            distance += euclidean_distance(previous, &point);
        }

        samples.push(SpatialSample {
            distance,
            point: point.clone(),
        });
        last_point = Some(point);
    }

    samples
}

fn cubic_bezier_point_vec(
    t: f32,
    start: &[f32],
    control1: &[f32],
    control2: &[f32],
    end: &[f32],
) -> Vec<f32> {
    start
        .iter()
        .zip(control1.iter())
        .zip(control2.iter())
        .zip(end.iter())
        .map(|(((start, control1), control2), end)| {
            cubic_bezier_point(t, *start, *control1, *control2, *end)
        })
        .collect()
}

fn add_components(lhs: &[f32], rhs: &[f32]) -> Vec<f32> {
    lhs.iter()
        .zip(rhs.iter())
        .map(|(lhs, rhs)| lhs + rhs)
        .collect()
}

fn euclidean_distance(lhs: &[f32], rhs: &[f32]) -> f32 {
    lhs.iter()
        .zip(rhs.iter())
        .map(|(lhs, rhs)| (rhs - lhs).powi(2))
        .sum::<f32>()
        .sqrt()
}

const fn bezier_paths_are_compatible(lhs: &BezierPath, rhs: &BezierPath) -> bool {
    lhs.vertices.len() == rhs.vertices.len() && !lhs.vertices.is_empty()
}

fn interpolate_bezier_path(
    start: &BezierPath,
    end: &BezierPath,
    progress: f32,
) -> Option<BezierPath> {
    if !bezier_paths_are_compatible(start, end) {
        return None;
    }

    Some(BezierPath {
        closed: start.closed,
        vertices: start
            .vertices
            .iter()
            .zip(end.vertices.iter())
            .map(|(start, end)| BezierVertex {
                vertex: lerp_vec2(start.vertex, end.vertex, progress),
                in_tangent: lerp_vec2(start.in_tangent, end.in_tangent, progress),
                out_tangent: lerp_vec2(start.out_tangent, end.out_tangent, progress),
            })
            .collect(),
    })
}

fn lerp_vec2(start: [f32; 2], end: [f32; 2], progress: f32) -> [f32; 2] {
    [
        (end[0] - start[0]).mul_add(progress, start[0]),
        (end[1] - start[1]).mul_add(progress, start[1]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq_vec2(lhs: [f32; 2], rhs: [f32; 2]) -> bool {
        const EPSILON: f32 = 1e-4;
        (lhs[0] - rhs[0]).abs() <= EPSILON && (lhs[1] - rhs[1]).abs() <= EPSILON
    }

    #[test]
    fn samples_static_scalars() {
        let value = serde_json::from_str::<AnimatedValue>(r#"{"a":0,"k":12}"#).unwrap();
        assert_eq!(sample_numbers(&value, 5.0), Some(vec![12.0]));
    }

    #[test]
    fn interpolates_basic_animated_scalars() {
        let value = serde_json::from_str::<AnimatedValue>(
            r#"{
                "a":1,
                "k":[
                    {"t":0,"s":[0],"e":[10],"i":{"x":[1],"y":[1]},"o":{"x":[0],"y":[0]}},
                    {"t":10,"s":[10]}
                ]
            }"#,
        )
        .unwrap();

        let sampled = sample_numbers(&value, 5.0).unwrap();
        assert!((sampled[0] - 5.0).abs() < 1e-3, "{sampled:?}");
    }

    #[test]
    fn accepts_supported_scalar_keyframes() {
        let value = serde_json::from_str::<AnimatedValue>(
            r#"{
                "a":1,
                "k":[
                    {"t":0,"s":[0],"e":[10]},
                    {"t":10,"s":[10]}
                ]
            }"#,
        )
        .unwrap();

        assert!(is_supported_scalar_value(&value));
        let sampled = sample_scalar(&value, 5.0).unwrap();
        assert!((sampled - 5.0).abs() < 1e-3, "{sampled:?}");
    }

    #[test]
    fn rejects_non_scalar_keyframes_for_scalar_sampling() {
        let value = serde_json::from_str::<AnimatedValue>(
            r#"{
                "a":1,
                "k":[
                    {"t":0,"s":[0,10],"e":[20,30]},
                    {"t":10,"s":[20,30]}
                ]
            }"#,
        )
        .unwrap();

        assert!(!is_supported_scalar_value(&value));
        assert_eq!(sample_scalar(&value, 5.0), None);
    }

    #[test]
    fn respects_hold_keyframes() {
        let value = serde_json::from_str::<AnimatedValue>(
            r#"{
                "a":1,
                "k":[
                    {"t":0,"s":[4],"h":1},
                    {"t":10,"s":[10]}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(sample_numbers(&value, 5.0), Some(vec![4.0]));
    }

    #[test]
    fn samples_spatial_keyframes_along_arc_length() {
        let value = serde_json::from_str::<AnimatedValue>(
            r#"{
                "a":1,
                "k":[
                    {
                        "t":0,
                        "s":[20,80],
                        "e":[80,80],
                        "to":[0,-80],
                        "ti":[0,-80],
                        "i":{"x":1,"y":1},
                        "o":{"x":0,"y":0}
                    },
                    {"t":10,"s":[80,80]}
                ]
            }"#,
        )
        .unwrap();

        assert!(is_supported_animated_value(&value));

        let sampled = sample_numbers(&value, 5.0).unwrap();
        assert!((sampled[0] - 50.0).abs() < 1e-2, "{sampled:?}");
        assert!((sampled[1] - 20.0).abs() < 1e-2, "{sampled:?}");
    }

    #[test]
    fn samples_animated_shape_paths() {
        let value = serde_json::from_str::<ShapePathValue>(
            r#"{
                "a":1,
                "k":[
                    {
                        "t":0,
                        "s":[{
                            "c":true,
                            "i":[[0,0],[0,0],[0,0]],
                            "o":[[0,0],[0,0],[0,0]],
                            "v":[[10,10],[20,10],[15,20]]
                        }],
                        "e":[{
                            "c":true,
                            "i":[[0,0],[0,0],[0,0]],
                            "o":[[0,0],[0,0],[0,0]],
                            "v":[[30,10],[40,10],[35,20]]
                        }],
                        "i":{"x":1,"y":1},
                        "o":{"x":0,"y":0}
                    },
                    {
                        "t":10,
                        "s":[{
                            "c":true,
                            "i":[[0,0],[0,0],[0,0]],
                            "o":[[0,0],[0,0],[0,0]],
                            "v":[[30,10],[40,10],[35,20]]
                        }]
                    }
                ]
            }"#,
        )
        .unwrap();

        let sampled = sample_shape_path(&value, 5.0).unwrap();

        assert!(approx_eq_vec2(sampled.vertices[0].vertex, [20.0, 10.0]));
        assert!(approx_eq_vec2(sampled.vertices[1].vertex, [30.0, 10.0]));
        assert!(approx_eq_vec2(sampled.vertices[2].vertex, [25.0, 20.0]));
    }

    #[test]
    fn samples_animated_shape_paths_when_closed_state_changes() {
        let value = serde_json::from_str::<ShapePathValue>(
            r#"{
                "a":1,
                "k":[
                    {
                        "t":0,
                        "s":[{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[10,10],[20,10],[15,20]]}],
                        "e":[{"c":false,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[30,10],[40,10],[35,20]]}]
                    },
                    {
                        "t":10,
                        "s":[{"c":false,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[30,10],[40,10],[35,20]]}]
                    }
                ]
            }"#,
        )
        .unwrap();

        assert!(is_supported_shape_path(&value));

        let sampled = sample_shape_path(&value, 5.0).unwrap();
        assert!(sampled.closed);
        assert!(approx_eq_vec2(sampled.vertices[0].vertex, [20.0, 10.0]));
    }

    #[test]
    fn supports_single_vertex_animated_shape_paths() {
        let value = serde_json::from_str::<ShapePathValue>(
            r#"{
                "a":1,
                "k":[
                    {
                        "t":0,
                        "s":[{"c":false,"i":[[0,0]],"o":[[0,0]],"v":[[10,10]]}],
                        "e":[{"c":false,"i":[[0,0]],"o":[[0,0]],"v":[[20,20]]}]
                    },
                    {
                        "t":10,
                        "s":[{"c":false,"i":[[0,0]],"o":[[0,0]],"v":[[20,20]]}]
                    }
                ]
            }"#,
        )
        .unwrap();

        assert!(is_supported_shape_path(&value));

        let sampled = sample_shape_path(&value, 5.0).unwrap();
        assert_eq!(sampled.vertices.len(), 1);
        assert!(approx_eq_vec2(sampled.vertices[0].vertex, [15.0, 15.0]));
    }

    #[test]
    fn rejects_incompatible_animated_shape_paths() {
        let value = serde_json::from_str::<ShapePathValue>(
            r#"{
                "a":1,
                "k":[
                    {
                        "t":0,
                        "s":[{
                            "c":true,
                            "i":[[0,0],[0,0],[0,0]],
                            "o":[[0,0],[0,0],[0,0]],
                            "v":[[10,10],[20,10],[15,20]]
                        }],
                        "e":[{
                            "c":true,
                            "i":[[0,0],[0,0],[0,0],[0,0]],
                            "o":[[0,0],[0,0],[0,0],[0,0]],
                            "v":[[10,10],[20,10],[20,20],[10,20]]
                        }]
                    },
                    {
                        "t":10,
                        "s":[{
                            "c":true,
                            "i":[[0,0],[0,0],[0,0],[0,0]],
                            "o":[[0,0],[0,0],[0,0],[0,0]],
                            "v":[[10,10],[20,10],[20,20],[10,20]]
                        }]
                    }
                ]
            }"#,
        )
        .unwrap();

        assert!(!is_supported_shape_path(&value));
    }
}
