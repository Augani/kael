use serde_json::Value;

use crate::{Animation, Layer, ShapeItem, ShapePathValue};

#[derive(Debug, Clone, PartialEq, Eq)]
struct PathReferenceExpression {
    layer_name: String,
    content_names: Vec<String>,
}

pub fn resolve_supported_expressions(animation: &Animation) -> Animation {
    let mut resolved = animation.clone();
    resolve_comp_layers(&mut resolved.layers);
    for asset in &mut resolved.assets {
        resolve_comp_layers(&mut asset.layers);
    }
    resolved
}

fn resolve_comp_layers(layers: &mut [Layer]) {
    let max_passes = layers.len().max(1);
    for _ in 0..max_passes {
        let snapshot = layers.to_vec();
        let mut changed = false;

        for layer in layers.iter_mut() {
            changed |= resolve_layer_expressions(layer, &snapshot);
        }

        if !changed {
            break;
        }
    }
}

fn resolve_layer_expressions(layer: &mut Layer, comp_layers: &[Layer]) -> bool {
    resolve_shape_item_list(&mut layer.shapes, comp_layers)
}

fn resolve_shape_item_list(items: &mut [ShapeItem], comp_layers: &[Layer]) -> bool {
    let mut changed = false;

    for item in items {
        changed |= resolve_shape_item(item, comp_layers);
    }

    changed
}

fn resolve_shape_item(item: &mut ShapeItem, comp_layers: &[Layer]) -> bool {
    let mut changed = false;

    if let Some(path) = item.path.as_mut() {
        changed |= resolve_shape_path_value(path, comp_layers);
    }

    changed |= resolve_shape_item_list(&mut item.items, comp_layers);
    changed
}

fn resolve_shape_path_value(path: &mut ShapePathValue, comp_layers: &[Layer]) -> bool {
    let Some(expression) = path.expression.as_ref().and_then(Value::as_str) else {
        return false;
    };
    let Some(reference) = parse_path_reference_expression(expression) else {
        return false;
    };
    let Some(resolved_path) = lookup_shape_path_reference(comp_layers, &reference) else {
        return false;
    };

    *path = resolved_path;
    true
}

fn lookup_shape_path_reference(
    layers: &[Layer],
    reference: &PathReferenceExpression,
) -> Option<ShapePathValue> {
    let layer = layers
        .iter()
        .find(|layer| layer.name == reference.layer_name)?;
    lookup_shape_path_in_items(&layer.shapes, &reference.content_names).cloned()
}

fn lookup_shape_path_in_items<'a>(
    items: &'a [ShapeItem],
    content_names: &[String],
) -> Option<&'a ShapePathValue> {
    let (current_name, remaining) = content_names.split_first()?;
    let item = items.iter().find(|item| item.name == *current_name)?;
    if remaining.is_empty() {
        return item.path.as_ref();
    }

    lookup_shape_path_in_items(&item.items, remaining)
}

fn parse_path_reference_expression(expression: &str) -> Option<PathReferenceExpression> {
    let (_, mut remaining) = expression.split_once("thisComp.layer(")?;
    let (layer_name, rest) = parse_quoted_argument(remaining)?;
    remaining = rest;

    let mut content_names = Vec::new();
    loop {
        let trimmed = remaining.trim_start();
        if let Some(after_content) = trimmed.strip_prefix(".content(") {
            let (content_name, rest) = parse_quoted_argument(after_content)?;
            content_names.push(content_name);
            remaining = rest;
            continue;
        }

        let after_path = trimmed.strip_prefix(".path")?;
        let tail = after_path.trim();
        if !tail.is_empty() && tail != ";" {
            return None;
        }

        return (!content_names.is_empty()).then_some(PathReferenceExpression {
            layer_name,
            content_names,
        });
    }
}

fn parse_quoted_argument(source: &str) -> Option<(String, &str)> {
    let trimmed = source.trim_start();
    let quote = trimmed.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let after_open_quote = trimmed.strip_prefix(quote)?;

    let mut value = String::new();
    let mut escaped = false;
    let mut end_index = None;

    for (index, ch) in after_open_quote.char_indices() {
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            _ if ch == quote => {
                end_index = Some(index + ch.len_utf8());
                break;
            }
            _ => value.push(ch),
        }
    }

    let end_index = end_index?;
    let after_quote = after_open_quote.get(end_index..)?;
    let after_quote = after_quote.trim_start();
    let after_paren = after_quote.strip_prefix(')')?;
    Some((value, after_paren))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Animation;

    #[test]
    fn resolves_supported_path_reference_expressions() {
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
                        "nm":"Head",
                        "ind":1,
                        "ty":4,
                        "hd":true,
                        "shapes":[
                            {
                                "ty":"gr",
                                "nm":"Group 1",
                                "it":[
                                    {
                                        "ty":"sh",
                                        "nm":"Path 1",
                                        "ks":{
                                            "a":0,
                                            "k":{"c":true,"i":[[0,0],[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0],[0,0]],"v":[[20,20],[44,20],[44,44],[20,44]]}
                                        }
                                    }
                                ]
                            }
                        ]
                    },
                    {
                        "nm":"Mask",
                        "ind":2,
                        "ty":4,
                        "shapes":[
                            {
                                "ty":"gr",
                                "it":[
                                    {
                                        "ty":"sh",
                                        "nm":"Path 1",
                                        "ks":{
                                            "x":"var $bm_rt; $bm_rt = thisComp.layer('Head').content('Group 1').content('Path 1').path;"
                                        }
                                    },
                                    {"ty":"fl","c":{"a":0,"k":[1,0,0,1]},"o":{"a":0,"k":100}},
                                    {"ty":"tr","a":{"a":0,"k":[0,0]},"p":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0},"o":{"a":0,"k":100}}
                                ]
                            }
                        ]
                    }
                ]
            }"#,
        )
        .unwrap();

        let resolved = resolve_supported_expressions(&animation);
        let path = &resolved.layers[1].shapes[0].items[0]
            .path
            .as_ref()
            .unwrap()
            .expression;

        assert!(path.is_none());
        assert!(
            resolved.layers[1].shapes[0].items[0]
                .path
                .as_ref()
                .unwrap()
                .as_bezier_path()
                .is_some()
        );
    }

    #[test]
    fn parses_path_reference_expressions_with_multibyte_names() {
        let expression =
            "var $bm_rt; $bm_rt = thisComp.layer('頭').content('グループ').content('パス').path;";

        let parsed = parse_path_reference_expression(expression).unwrap();

        assert_eq!(parsed.layer_name, "頭");
        assert_eq!(
            parsed.content_names,
            ["グループ".to_string(), "パス".to_string()]
        );
    }
}
