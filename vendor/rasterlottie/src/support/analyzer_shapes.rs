use super::analyzer::{
    SupportProfile, SupportReport, UnsupportedFeature, UnsupportedKind, visit_animated_value,
    visit_position_value, visit_scalar_animated_value, visit_transform,
};
use crate::{
    model::{ShapeItem, Transform},
    timeline,
};

pub(super) fn visit_shape_path_value(
    value: &crate::ShapePathValue,
    path: &str,
    field: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
) {
    if !profile.allow_expressions && value.has_expression() {
        report.push(UnsupportedFeature {
            path: format!("{path}.{field}"),
            kind: UnsupportedKind::Expressions,
            detail: "expressions are not supported by the active support profile".to_string(),
        });
    }

    if value.is_static() && value.as_bezier_path().is_none() {
        report.push(UnsupportedFeature {
            path: format!("{path}.{field}"),
            kind: UnsupportedKind::ShapeItem,
            detail: "shape path data is malformed".to_string(),
        });
        return;
    }

    if !value.is_static() && !timeline::is_supported_shape_path(value) {
        report.push(UnsupportedFeature {
            path: format!("{path}.{field}"),
            kind: UnsupportedKind::AnimatedValue,
            detail: "shape path uses unsupported keyframe features".to_string(),
        });
    }
}

pub(super) fn visit_shape_list(
    shapes: &[ShapeItem],
    profile: &SupportProfile,
    report: &mut SupportReport,
    path: &str,
) {
    for (index, shape) in shapes.iter().enumerate() {
        visit_shape(
            shape,
            shapes,
            index,
            profile,
            report,
            &format!("{path}[{index}]"),
        );
    }
}

fn visit_shape(
    shape: &ShapeItem,
    siblings: &[ShapeItem],
    index: usize,
    profile: &SupportProfile,
    report: &mut SupportReport,
    path: &str,
) {
    const SUPPORTED_TYPES: &[&str] = &[
        "gr", "fl", "st", "gf", "gs", "tr", "rc", "el", "sh", "tm", "sr", "rp",
    ];

    if !profile.allow_expressions && shape.has_expression() {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::Expressions,
            detail: "shape expressions are not supported by the active support profile".to_string(),
        });
    }

    let supported_merge_path = is_supported_merge_path(shape, siblings, index);
    if !profile.allow_unknown_shape_items
        && !SUPPORTED_TYPES.contains(&shape.item_type.as_str())
        && !supported_merge_path
    {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::ShapeItem,
            detail: format!("shape item `{}` is not supported yet", shape.item_type),
        });
    }

    match shape.item_type.as_str() {
        "rc" => {
            visit_position_value(shape.position.as_ref(), path, "position", profile, report);
            visit_animated_value(shape.size.as_ref(), path, "size", profile, report);
            let roundness = shape.rectangle_roundness();
            visit_animated_value(roundness.as_ref(), path, "roundness", profile, report);
        }
        "el" => {
            visit_position_value(shape.position.as_ref(), path, "position", profile, report);
            visit_animated_value(shape.size.as_ref(), path, "size", profile, report);
        }
        "sh" => {
            if let Some(value) = &shape.path {
                visit_shape_path_value(value, path, "path", profile, report);
            }
        }
        "fl" => {
            visit_animated_value(shape.color.as_ref(), path, "color", profile, report);
            visit_animated_value(shape.opacity.as_ref(), path, "opacity", profile, report);
        }
        "st" => {
            visit_animated_value(shape.color.as_ref(), path, "color", profile, report);
            visit_animated_value(shape.opacity.as_ref(), path, "opacity", profile, report);
            visit_animated_value(shape.width.as_ref(), path, "width", profile, report);
            visit_animated_value(
                shape.miter_limit_value.as_ref(),
                path,
                "miter_limit",
                profile,
                report,
            );

            if let Some(entries) = shape.dash_pattern() {
                let mut dash_values = 0usize;
                for (index, entry) in entries.iter().enumerate() {
                    match entry.name.as_str() {
                        "d" | "g" => {
                            dash_values += 1;
                            visit_scalar_animated_value(
                                Some(&entry.value),
                                &format!("{path}.dashes[{index}]"),
                                "value",
                                profile,
                                report,
                            );
                        }
                        "o" => visit_scalar_animated_value(
                            Some(&entry.value),
                            &format!("{path}.dashes[{index}]"),
                            "offset",
                            profile,
                            report,
                        ),
                        _ => report.push(UnsupportedFeature {
                            path: format!("{path}.dashes[{index}]"),
                            kind: UnsupportedKind::ShapeItem,
                            detail: "stroke dash entry uses an unknown kind".to_string(),
                        }),
                    }
                }
                if dash_values == 0 {
                    report.push(UnsupportedFeature {
                        path: path.to_string(),
                        kind: UnsupportedKind::ShapeItem,
                        detail: "stroke dash patterns must include at least one dash or gap"
                            .to_string(),
                    });
                }
            }
        }
        "gf" => visit_gradient_shape(shape, path, profile, report, false),
        "gs" => visit_gradient_shape(shape, path, profile, report, true),
        "tm" => {
            visit_scalar_animated_value(shape.trim_start(), path, "start", profile, report);
            visit_scalar_animated_value(shape.trim_end().as_ref(), path, "end", profile, report);
            visit_scalar_animated_value(shape.trim_offset(), path, "offset", profile, report);
            match shape.trim_mode().unwrap_or(1) {
                1 | 2 => {}
                _ => report.push(UnsupportedFeature {
                    path: path.to_string(),
                    kind: UnsupportedKind::ShapeItem,
                    detail: "trim path mode is not supported yet".to_string(),
                }),
            }
        }
        "sr" => {
            visit_position_value(shape.position.as_ref(), path, "position", profile, report);
            visit_scalar_animated_value(
                shape.polystar_points().as_ref(),
                path,
                "points",
                profile,
                report,
            );
            let rotation = shape.polystar_rotation();
            visit_scalar_animated_value(rotation.as_ref(), path, "rotation", profile, report);
            visit_scalar_animated_value(
                shape.polystar_outer_radius().as_ref(),
                path,
                "outer_radius",
                profile,
                report,
            );
            visit_scalar_animated_value(
                shape.polystar_outer_roundness().as_ref(),
                path,
                "outer_roundness",
                profile,
                report,
            );
            if shape.polystar_type() == Some(1) {
                visit_scalar_animated_value(
                    shape.polystar_inner_radius().as_ref(),
                    path,
                    "inner_radius",
                    profile,
                    report,
                );
                visit_scalar_animated_value(
                    shape.polystar_inner_roundness().as_ref(),
                    path,
                    "inner_roundness",
                    profile,
                    report,
                );
            }

            match shape.polystar_type() {
                Some(1 | 2) => {}
                _ => report.push(UnsupportedFeature {
                    path: path.to_string(),
                    kind: UnsupportedKind::ShapeItem,
                    detail: "polystar type is not supported yet".to_string(),
                }),
            }
        }
        "rp" => {
            visit_scalar_animated_value(shape.repeater_copies(), path, "copies", profile, report);
            visit_scalar_animated_value(shape.repeater_offset(), path, "offset", profile, report);
            match shape.repeater_composite_mode().unwrap_or(1) {
                1 | 2 => {}
                _ => report.push(UnsupportedFeature {
                    path: path.to_string(),
                    kind: UnsupportedKind::ShapeItem,
                    detail: "repeater composite mode is not supported yet".to_string(),
                }),
            }

            let Some(transform) = shape.repeater_transform() else {
                report.push(UnsupportedFeature {
                    path: path.to_string(),
                    kind: UnsupportedKind::ShapeItem,
                    detail: "repeater transform data is missing".to_string(),
                });
                return;
            };
            visit_animated_value(transform.anchor.as_ref(), path, "anchor", profile, report);
            visit_position_value(
                transform.position.as_ref(),
                path,
                "position",
                profile,
                report,
            );
            visit_animated_value(transform.scale.as_ref(), path, "scale", profile, report);
            visit_animated_value(
                transform.rotation.as_ref(),
                path,
                "rotation",
                profile,
                report,
            );
            visit_animated_value(transform.opacity.as_ref(), path, "opacity", profile, report);
            visit_animated_value(
                transform.start_opacity.as_ref(),
                path,
                "start_opacity",
                profile,
                report,
            );
            visit_animated_value(
                transform.end_opacity.as_ref(),
                path,
                "end_opacity",
                profile,
                report,
            );
        }
        "tr" => {
            let rotation = shape.transform_rotation();
            let transform = Transform {
                anchor: shape.anchor.clone(),
                position: shape.position.clone(),
                scale: shape.size.clone(),
                rotation,
                opacity: shape.opacity.clone(),
                skew: shape.skew.clone(),
                skew_axis: shape.skew_axis.clone(),
            };
            visit_transform(&transform, path, profile, report);
        }
        _ => {}
    }

    visit_shape_list(&shape.items, profile, report, &format!("{path}.items"));
}

fn is_supported_merge_path(shape: &ShapeItem, siblings: &[ShapeItem], index: usize) -> bool {
    if shape.item_type != "mm" || !matches!(shape.merge_mode(), Some(1..=5)) {
        return false;
    }

    if merge_path_has_no_new_sources(siblings, index) {
        return true;
    }

    if shape.merge_mode() == Some(1) {
        let mut geometry_sources = 0usize;
        for candidate in siblings.iter().take(index) {
            if candidate.hidden {
                continue;
            }

            match candidate.item_type.as_str() {
                "sh" | "rc" | "el" | "sr" => geometry_sources += 1,
                "gr" if group_is_merge_noop(candidate) => {}
                "tr" => {}
                _ => return false,
            }
        }

        return geometry_sources >= 2;
    }

    let mut geometry_sources = 0usize;
    for candidate in siblings.iter().take(index) {
        if candidate.hidden {
            continue;
        }

        match candidate.item_type.as_str() {
            "sh" | "rc" | "el" | "sr" => {
                geometry_sources += 1;
            }
            "gr" if group_is_merge_noop(candidate) => {}
            "tr" => {}
            _ => return false,
        }

        if geometry_sources > 1 {
            return false;
        }
    }

    true
}

fn merge_path_has_no_new_sources(siblings: &[ShapeItem], index: usize) -> bool {
    let mut saw_previous_merge = false;

    for candidate in siblings.iter().take(index).rev() {
        if candidate.hidden {
            continue;
        }

        if candidate.item_type == "mm" {
            saw_previous_merge = true;
            break;
        }

        match candidate.item_type.as_str() {
            "gr" if group_is_merge_noop(candidate) => {}
            "tr" => {}
            _ => return false,
        }
    }

    saw_previous_merge
}

fn group_is_merge_noop(shape: &ShapeItem) -> bool {
    if shape.item_type != "gr" {
        return false;
    }

    shape.items.iter().all(|child| {
        child.hidden
            || child.item_type == "tr"
            || (child.item_type == "gr" && group_is_merge_noop(child))
    })
}

fn visit_gradient_shape(
    shape: &ShapeItem,
    path: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
    is_stroke: bool,
) {
    let Some(gradient) = shape.gradient_data() else {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::ShapeItem,
            detail: "gradient data is missing".to_string(),
        });
        return;
    };

    if gradient.point_count == 0 {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::ShapeItem,
            detail: "gradient point count must be positive".to_string(),
        });
    }

    match shape.gradient_type() {
        Some(1 | 2) => {}
        _ => report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::ShapeItem,
            detail: "gradient type is not supported yet".to_string(),
        }),
    }

    visit_animated_value(
        Some(&gradient.colors),
        path,
        "gradient_colors",
        profile,
        report,
    );
    if shape.gradient_start_point().is_none() {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::ShapeItem,
            detail: "gradient start point is missing".to_string(),
        });
    }
    visit_animated_value(
        shape.gradient_start_point(),
        path,
        "start_point",
        profile,
        report,
    );

    let end_point = shape.gradient_end_point();
    if end_point.is_none() {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::ShapeItem,
            detail: "gradient end point is missing".to_string(),
        });
    }
    visit_animated_value(end_point.as_ref(), path, "end_point", profile, report);

    let highlight_length = shape.gradient_highlight_length();
    visit_animated_value(
        highlight_length.as_ref(),
        path,
        "highlight_length",
        profile,
        report,
    );
    visit_animated_value(
        shape.gradient_highlight_angle(),
        path,
        "highlight_angle",
        profile,
        report,
    );
    visit_animated_value(shape.opacity.as_ref(), path, "opacity", profile, report);

    if is_stroke {
        visit_animated_value(shape.width.as_ref(), path, "width", profile, report);
        visit_animated_value(
            shape.miter_limit_value.as_ref(),
            path,
            "miter_limit",
            profile,
            report,
        );

        if let Some(entries) = shape.dash_pattern() {
            let mut dash_values = 0usize;
            for (index, entry) in entries.iter().enumerate() {
                match entry.name.as_str() {
                    "d" | "g" => {
                        dash_values += 1;
                        visit_scalar_animated_value(
                            Some(&entry.value),
                            &format!("{path}.dashes[{index}]"),
                            "value",
                            profile,
                            report,
                        );
                    }
                    "o" => visit_scalar_animated_value(
                        Some(&entry.value),
                        &format!("{path}.dashes[{index}]"),
                        "offset",
                        profile,
                        report,
                    ),
                    _ => report.push(UnsupportedFeature {
                        path: format!("{path}.dashes[{index}]"),
                        kind: UnsupportedKind::ShapeItem,
                        detail: "stroke dash entry uses an unknown kind".to_string(),
                    }),
                }
            }
            if dash_values == 0 {
                report.push(UnsupportedFeature {
                    path: path.to_string(),
                    kind: UnsupportedKind::ShapeItem,
                    detail: "stroke dash patterns must include at least one dash or gap"
                        .to_string(),
                });
            }
        }
    }
}
