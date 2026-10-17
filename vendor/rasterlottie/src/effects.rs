use serde_json::Value;

use crate::{
    AnimatedValue, Layer,
    timeline::{is_supported_animated_value, sample_numbers, sample_scalar},
};

#[derive(Debug, Clone)]
pub enum SupportedLayerEffect {
    Fill(FillEffect),
    SimpleChoker(SimpleChokerEffect),
}

#[derive(Debug, Clone)]
pub struct FillEffect {
    color: AnimatedValue,
    opacity: AnimatedValue,
}

#[derive(Debug, Clone)]
pub struct SimpleChokerEffect {
    choke: AnimatedValue,
}

pub fn supported_layer_effects(layer: &Layer) -> Option<Vec<SupportedLayerEffect>> {
    let mut effects = Vec::new();

    for effect in &layer.effects {
        if !effect_enabled(effect) {
            continue;
        }

        if let Some(fill) = parse_fill_effect(effect) {
            effects.push(SupportedLayerEffect::Fill(fill));
            continue;
        }

        if let Some(choker) = parse_simple_choker_effect(effect) {
            effects.push(SupportedLayerEffect::SimpleChoker(choker));
            continue;
        }

        return None;
    }

    Some(effects)
}

pub fn fill_effect_color(effect: &FillEffect, frame: f32) -> Option<[u8; 4]> {
    let values = sample_numbers(&effect.color, frame)?;
    let alpha = values.get(3).copied().unwrap_or(1.0);
    Some([
        normalized_channel(*values.first().unwrap_or(&0.0)),
        normalized_channel(*values.get(1).unwrap_or(&0.0)),
        normalized_channel(*values.get(2).unwrap_or(&0.0)),
        normalized_channel(alpha),
    ])
}

pub fn fill_effect_opacity(effect: &FillEffect, frame: f32) -> Option<f32> {
    let raw = sample_scalar(&effect.opacity, frame)?;
    Some(if raw > 1.0 { raw / 100.0 } else { raw }.clamp(0.0, 1.0))
}

pub fn simple_choker_amount(effect: &SimpleChokerEffect, frame: f32) -> Option<f32> {
    sample_scalar(&effect.choke, frame)
}

fn parse_fill_effect(effect: &Value) -> Option<FillEffect> {
    if effect.get("mn").and_then(Value::as_str) != Some("ADBE Fill") {
        return None;
    }

    let parameters = effect.get("ef")?.as_array()?;
    let fill_mask = effect_parameter(parameters, "ADBE Fill-0001")?;
    let all_masks = effect_parameter(parameters, "ADBE Fill-0007")?;
    let color = effect_parameter(parameters, "ADBE Fill-0002")?;
    let invert = effect_parameter(parameters, "ADBE Fill-0006")?;
    let horizontal_feather = effect_parameter(parameters, "ADBE Fill-0003")?;
    let vertical_feather = effect_parameter(parameters, "ADBE Fill-0004")?;
    let opacity = effect_parameter(parameters, "ADBE Fill-0005")?;

    if parse_effect_scalar(fill_mask)? != 0.0
        || parse_effect_scalar(all_masks)? != 0.0
        || parse_effect_scalar(invert)? != 0.0
        || parse_effect_scalar(horizontal_feather)? != 0.0
        || parse_effect_scalar(vertical_feather)? != 0.0
    {
        return None;
    }

    let color = parse_effect_animated_value(color)?;
    let opacity = parse_effect_animated_value(opacity)?;
    if !is_supported_animated_value(&color) || !is_supported_animated_value(&opacity) {
        return None;
    }

    Some(FillEffect { color, opacity })
}

fn parse_simple_choker_effect(effect: &Value) -> Option<SimpleChokerEffect> {
    if effect.get("mn").and_then(Value::as_str) != Some("ADBE Simple Choker") {
        return None;
    }

    let parameters = effect.get("ef")?.as_array()?;
    let view = effect_parameter(parameters, "ADBE Simple Choker-0001")?;
    let choke = effect_parameter(parameters, "ADBE Simple Choker-0002")?;

    if (parse_effect_scalar(view)? - 1.0).abs() > f32::EPSILON {
        return None;
    }

    let choke = parse_effect_animated_value(choke)?;
    if !is_supported_animated_value(&choke) {
        return None;
    }

    Some(SimpleChokerEffect { choke })
}

fn effect_parameter<'a>(parameters: &'a [Value], mnemonic: &str) -> Option<&'a Value> {
    parameters
        .iter()
        .find(|entry| entry.get("mn").and_then(Value::as_str) == Some(mnemonic))
}

fn parse_effect_scalar(value: &Value) -> Option<f32> {
    let animated = parse_effect_animated_value(value)?;
    animated.as_scalar()
}

fn parse_effect_animated_value(value: &Value) -> Option<AnimatedValue> {
    serde_json::from_value(value.get("v")?.clone()).ok()
}

fn effect_enabled(effect: &Value) -> bool {
    effect
        .get("en")
        .and_then(Value::as_i64)
        .is_none_or(|enabled| enabled != 0)
}

fn normalized_channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Animation;

    #[test]
    fn parses_supported_fill_effects() {
        let animation = Animation::from_json_str(
            r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "ef":[
                            {
                                "mn":"ADBE Fill",
                                "en":1,
                                "ef":[
                                    {"mn":"ADBE Fill-0001","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0007","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0002","v":{"a":0,"k":[1,0,0,1]}},
                                    {"mn":"ADBE Fill-0006","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0003","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0004","v":{"a":0,"k":0}},
                                    {"mn":"ADBE Fill-0005","v":{"a":0,"k":1}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
        )
        .unwrap();

        let effects = supported_layer_effects(&animation.layers[0]).unwrap();
        let SupportedLayerEffect::Fill(fill) = &effects[0] else {
            panic!("expected fill effect");
        };
        assert_eq!(fill_effect_color(fill, 0.0), Some([255, 0, 0, 255]));
        assert_eq!(fill_effect_opacity(fill, 0.0), Some(1.0));
    }

    #[test]
    fn parses_supported_simple_choker_effects() {
        let animation = Animation::from_json_str(
            r#"{
                "v":"5.7.6",
                "fr":30,
                "ip":0,
                "op":60,
                "w":64,
                "h":64,
                "layers":[
                    {
                        "nm":"Shape Layer 1",
                        "ind":1,
                        "ty":4,
                        "ef":[
                            {
                                "mn":"ADBE Simple Choker",
                                "en":1,
                                "ef":[
                                    {"mn":"ADBE Simple Choker-0001","v":{"a":0,"k":1}},
                                    {"mn":"ADBE Simple Choker-0002","v":{"a":0,"k":3}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
        )
        .unwrap();

        let effects = supported_layer_effects(&animation.layers[0]).unwrap();
        let SupportedLayerEffect::SimpleChoker(choker) = &effects[0] else {
            panic!("expected simple choker effect");
        };
        assert_eq!(simple_choker_amount(choker, 0.0), Some(3.0));
    }
}
