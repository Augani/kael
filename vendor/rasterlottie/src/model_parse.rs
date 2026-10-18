use serde_json::Value;

use super::model_types::{BezierPath, BezierVertex};

pub fn f32_unit_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn color_components_to_rgba(values: &[f32]) -> Option<[u8; 4]> {
    if values.len() < 3 {
        return None;
    }

    Some([
        f32_unit_to_u8(values[0]),
        f32_unit_to_u8(values[1]),
        f32_unit_to_u8(values[2]),
        f32_unit_to_u8(values.get(3).copied().unwrap_or(1.0)),
    ])
}

pub fn parse_bezier_keyframe_value(value: &Value) -> Option<BezierPath> {
    match value {
        Value::Object(_) => parse_bezier_path(value),
        Value::Array(items) if items.len() == 1 => parse_bezier_path(items.first()?),
        _ => None,
    }
}

pub fn parse_bezier_path(value: &Value) -> Option<BezierPath> {
    let object = value.as_object()?;
    let vertices = parse_vec2_list(object.get("v")?)?;
    let in_tangents = parse_vec2_list(object.get("i")?)?;
    let out_tangents = parse_vec2_list(object.get("o")?)?;

    if vertices.is_empty()
        || vertices.len() != in_tangents.len()
        || vertices.len() != out_tangents.len()
    {
        return None;
    }

    Some(BezierPath {
        closed: object.get("c").and_then(value_as_bool).unwrap_or(false),
        vertices: vertices
            .into_iter()
            .zip(in_tangents)
            .zip(out_tangents)
            .map(|((vertex, in_tangent), out_tangent)| BezierVertex {
                vertex,
                in_tangent,
                out_tangent,
            })
            .collect(),
    })
}

fn parse_vec2_list(value: &Value) -> Option<Vec<[f32; 2]>> {
    value.as_array()?.iter().map(parse_vec2).collect()
}

fn parse_vec2(value: &Value) -> Option<[f32; 2]> {
    let items = value.as_array()?;
    if items.len() < 2 {
        return None;
    }

    Some([items[0].as_f64()? as f32, items[1].as_f64()? as f32])
}

fn value_as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::Number(value) => value.as_i64().map(|number| number != 0),
        _ => None,
    }
}
