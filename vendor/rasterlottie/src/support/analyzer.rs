use std::fmt::{self, Write as _};

use super::{
    analyzer_shapes::{visit_shape_list, visit_shape_path_value},
    analyzer_text::{layer_effects_are_supported, visit_text_layer},
};
use crate::{
    expression::resolve_supported_expressions,
    model::{AnimatedValue, Animation, Asset, Layer, LayerType, Mask, PositionValue, Transform},
    timeline::{is_supported_animated_value, is_supported_scalar_value},
};

/// Feature gates used by the support analyzer.
///
/// The default renderer is intentionally scoped to a validated target corpus,
/// and each flag enables a family of Lottie features that may otherwise be
/// rejected during analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupportProfile {
    /// Whether layer masks are accepted.
    pub allow_masks: bool,
    /// Whether track mattes are accepted.
    pub allow_track_mattes: bool,
    /// Whether parented layers are accepted.
    pub allow_parenting: bool,
    /// Whether supported layer effects are accepted.
    pub allow_effects: bool,
    /// Whether text layers are accepted.
    pub allow_text_layers: bool,
    /// Whether image layers are accepted.
    pub allow_image_layers: bool,
    /// Whether image assets of any kind are accepted.
    pub allow_image_assets: bool,
    /// Whether non-embedded image assets are accepted.
    pub allow_external_image_assets: bool,
    /// Whether supported expression forms are accepted.
    pub allow_expressions: bool,
    /// Whether animated properties are accepted.
    pub allow_animated_values: bool,
    /// Whether unknown shape item tags are ignored instead of rejected.
    pub allow_unknown_shape_items: bool,
}

/// Categories of unsupported features reported by the analyzer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnsupportedKind {
    /// The layer type itself is unsupported.
    LayerType,
    /// One or more masks are unsupported.
    Masks,
    /// One or more track matte features are unsupported.
    TrackMatte,
    /// Layer parenting is unsupported.
    Parenting,
    /// Layer timing or stretch semantics are unsupported.
    LayerTiming,
    /// Time remapping is unsupported.
    TimeRemap,
    /// Layer effects are unsupported.
    Effects,
    /// Expressions are unsupported.
    Expressions,
    /// An animated property uses unsupported semantics.
    AnimatedValue,
    /// A transform feature such as skew is unsupported.
    TransformFeature,
    /// An image asset is unsupported or malformed.
    ImageAsset,
    /// A text feature is unsupported.
    Text,
    /// A shape item is unsupported.
    ShapeItem,
    /// A referenced asset is missing.
    MissingAsset,
}

/// A single support issue discovered while analyzing an animation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct UnsupportedFeature {
    /// Path-like identifier that points to the offending JSON location.
    pub path: String,
    /// High-level category of the unsupported feature.
    pub kind: UnsupportedKind,
    /// Human-readable explanation of the rejection.
    pub detail: String,
}

/// Result of running the support analyzer against an animation.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct SupportReport {
    /// Collected unsupported features in discovery order.
    pub issues: Vec<UnsupportedFeature>,
}

impl SupportReport {
    /// Returns `true` when the analyzer found no issues.
    #[must_use]
    pub const fn is_supported(&self) -> bool {
        self.issues.is_empty()
    }

    /// Appends one issue to the report.
    pub fn push(&mut self, issue: UnsupportedFeature) {
        self.issues.push(issue);
    }

    /// Returns the number of collected issues.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.issues.len()
    }

    /// Returns `true` when the report contains no issues.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.issues.is_empty()
    }
}

impl fmt::Display for SupportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.issues.is_empty() {
            return write!(f, "no issues");
        }

        write!(f, "{} issue(s)", self.issues.len())?;
        for issue in self.issues.iter().take(3) {
            write!(f, "; {}: {}", issue.path, issue.detail)?;
        }
        if self.issues.len() > 3 {
            write!(f, "; ...")?;
        }
        Ok(())
    }
}

impl Default for SupportProfile {
    fn default() -> Self {
        Self::target_corpus()
    }
}

impl SupportProfile {
    /// Returns the default profile used for the validated target corpus.
    #[must_use]
    pub const fn target_corpus() -> Self {
        Self {
            allow_masks: true,
            allow_track_mattes: true,
            allow_parenting: true,
            allow_effects: false,
            allow_text_layers: cfg!(feature = "text"),
            allow_image_layers: cfg!(feature = "images"),
            allow_image_assets: cfg!(feature = "images"),
            allow_external_image_assets: false,
            allow_expressions: false,
            allow_animated_values: true,
            allow_unknown_shape_items: false,
        }
    }

    /// Returns a copy of this profile with external image assets enabled or disabled.
    #[must_use]
    pub const fn with_external_image_assets(mut self, allow: bool) -> Self {
        self.allow_external_image_assets = allow;
        self
    }
}

/// Analyzes an animation with the default target-corpus profile.
#[must_use]
pub fn analyze_animation(animation: &Animation) -> SupportReport {
    analyze_animation_with_profile(animation, SupportProfile::target_corpus())
}

/// Analyzes an animation with the provided support profile.
#[must_use]
pub fn analyze_animation_with_profile(
    animation: &Animation,
    profile: SupportProfile,
) -> SupportReport {
    let resolved = resolve_supported_expressions(animation);
    let mut report = SupportReport::default();

    for asset in &resolved.assets {
        visit_asset(asset, &profile, &mut report);
    }

    for (index, layer) in resolved.layers.iter().enumerate() {
        visit_layer(
            &resolved,
            layer,
            &resolved.layers,
            index,
            &profile,
            &mut report,
            &format!("layers[{index}]"),
        );
    }

    report
}

fn visit_asset(asset: &Asset, profile: &SupportProfile, report: &mut SupportReport) {
    let path = format!("assets[{}]", asset.id);
    if asset.is_image_asset() && !cfg!(feature = "images") {
        report.push(UnsupportedFeature {
            path,
            kind: UnsupportedKind::ImageAsset,
            detail: "image support is disabled because the `images` feature is not enabled"
                .to_string(),
        });
    } else if asset.is_image_asset() && !profile.allow_image_assets {
        report.push(UnsupportedFeature {
            path,
            kind: UnsupportedKind::ImageAsset,
            detail: "image assets are not supported by the active support profile".to_string(),
        });
    } else if asset.is_image_asset()
        && !asset.is_embedded_image_asset()
        && !profile.allow_external_image_assets
    {
        report.push(UnsupportedFeature {
            path,
            kind: UnsupportedKind::ImageAsset,
            detail: "external image assets are not supported yet".to_string(),
        });
    }
}

fn visit_layer(
    animation: &Animation,
    layer: &Layer,
    layers: &[Layer],
    list_index: usize,
    profile: &SupportProfile,
    report: &mut SupportReport,
    base_path: &str,
) {
    let path = qualify(base_path, &layer.name, layer.index);

    match layer.layer_type {
        LayerType::SHAPE | LayerType::PRECOMP | LayerType::NULL => {}
        LayerType::TEXT if !cfg!(feature = "text") => report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::Text,
            detail: "text support is disabled because the `text` feature is not enabled"
                .to_string(),
        }),
        LayerType::TEXT if profile.allow_text_layers => {
            visit_text_layer(animation, layer, &path, report);
        }
        LayerType::TEXT => report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::Text,
            detail: "text layers are not supported by the active support profile".to_string(),
        }),
        LayerType::IMAGE if !cfg!(feature = "images") => report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::ImageAsset,
            detail: "image support is disabled because the `images` feature is not enabled"
                .to_string(),
        }),
        LayerType::IMAGE if profile.allow_image_layers => {}
        LayerType::IMAGE => report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::ImageAsset,
            detail: "image layers are not supported by the active support profile".to_string(),
        }),
        other => report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::LayerType,
            detail: format!("layer type `{}` is not supported yet", other.name()),
        }),
    }

    if !profile.allow_track_mattes
        && (layer.track_matte.is_some()
            || layer.matte_parent.is_some()
            || layer.matte_source.is_some())
    {
        report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::TrackMatte,
            detail: "track mattes are not supported by the active support profile".to_string(),
        });
    } else if profile.allow_track_mattes {
        visit_track_matte(layer, layers, list_index, &path, report);
    }

    if !profile.allow_parenting && layer.parent.is_some() {
        report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::Parenting,
            detail: "layer parenting is not supported by the active support profile".to_string(),
        });
    } else if let Some(parent_index) = layer.parent
        && layers
            .iter()
            .all(|candidate| candidate.index != Some(parent_index))
    {
        report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::Parenting,
            detail: format!("missing parent layer `{parent_index}`"),
        });
    }

    if layer.stretch <= 0.0 {
        report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::LayerTiming,
            detail: "non-positive layer stretch is not supported".to_string(),
        });
    }

    if let Some(time_remap) = layer.time_remap.as_ref() {
        visit_time_remap(layer, time_remap, &path, profile, report);
    }

    if !layer.effects.is_empty() && !layer_effects_are_supported(layer, profile) {
        report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::Effects,
            detail: "layer effects are not supported by the active support profile".to_string(),
        });
    }

    if !profile.allow_masks && !layer.masks.is_empty() {
        report.push(UnsupportedFeature {
            path: path.clone(),
            kind: UnsupportedKind::Masks,
            detail: "masks are not supported by the active support profile".to_string(),
        });
    } else if profile.allow_masks {
        for (mask_index, mask) in layer.masks.iter().enumerate() {
            visit_mask(
                mask,
                &format!("{path}.masks[{mask_index}]"),
                profile,
                report,
            );
        }
    }

    if let Some(transform) = &layer.transform {
        visit_transform(transform, &path, profile, report);
    }

    visit_shape_list(&layer.shapes, profile, report, &format!("{path}.shapes"));

    if layer.layer_type == LayerType::PRECOMP {
        let Some(ref_id) = layer.ref_id.as_deref() else {
            return;
        };

        let Some(asset) = animation.assets.iter().find(|asset| asset.id == ref_id) else {
            report.push(UnsupportedFeature {
                path: path.clone(),
                kind: UnsupportedKind::MissingAsset,
                detail: format!("missing precomp asset `{ref_id}`"),
            });
            return;
        };

        for (index, child) in asset.layers.iter().enumerate() {
            visit_layer(
                animation,
                child,
                &asset.layers,
                index,
                profile,
                report,
                &format!("{path}.asset_layers[{index}]"),
            );
        }
    } else if layer.layer_type == LayerType::IMAGE {
        let Some(ref_id) = layer.ref_id.as_deref() else {
            report.push(UnsupportedFeature {
                path: path.clone(),
                kind: UnsupportedKind::MissingAsset,
                detail: "missing image asset reference".to_string(),
            });
            return;
        };

        let Some(asset) = animation.assets.iter().find(|asset| asset.id == ref_id) else {
            report.push(UnsupportedFeature {
                path: path.clone(),
                kind: UnsupportedKind::MissingAsset,
                detail: format!("missing image asset `{ref_id}`"),
            });
            return;
        };

        if !asset.is_image_asset() {
            report.push(UnsupportedFeature {
                path,
                kind: UnsupportedKind::ImageAsset,
                detail: format!("asset `{ref_id}` is not an image asset"),
            });
        }
    }
}

fn visit_track_matte(
    layer: &Layer,
    layers: &[Layer],
    list_index: usize,
    path: &str,
    report: &mut SupportReport,
) {
    if layer.track_matte.is_none() && layer.matte_parent.is_none() && layer.matte_source.is_none() {
        return;
    }

    if layer.track_matte.is_some() && layer.track_matte_mode().is_none() {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::TrackMatte,
            detail: "track matte mode is not supported yet".to_string(),
        });
        return;
    }

    if layer.track_matte_mode().is_some()
        && find_track_matte_source_index(layers, list_index, layer).is_none()
    {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::TrackMatte,
            detail: "track matte source layer is missing".to_string(),
        });
    }
}

fn visit_mask(mask: &Mask, path: &str, profile: &SupportProfile, report: &mut SupportReport) {
    if mask.mask_mode().is_none() {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::Masks,
            detail: "mask mode is not supported yet".to_string(),
        });
    }

    match mask.path.as_ref() {
        Some(value) => visit_shape_path_value(value, path, "path", profile, report),
        None => report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::Masks,
            detail: "mask path is missing".to_string(),
        }),
    }

    visit_animated_value(mask.opacity.as_ref(), path, "opacity", profile, report);
}

fn find_track_matte_source_index(
    layers: &[Layer],
    list_index: usize,
    layer: &Layer,
) -> Option<usize> {
    let target_index = layer
        .matte_parent
        .or_else(|| layer.index.map(|index| index - 1));

    target_index.map_or_else(
        || list_index.checked_sub(1),
        |target_index| {
            layers
                .iter()
                .position(|candidate| candidate.index == Some(target_index))
        },
    )
}

pub(super) fn visit_transform(
    transform: &Transform,
    path: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
) {
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
    visit_animated_value(transform.skew.as_ref(), path, "skew", profile, report);
    visit_animated_value(
        transform.skew_axis.as_ref(),
        path,
        "skew_axis",
        profile,
        report,
    );

    if transform
        .skew
        .as_ref()
        .and_then(AnimatedValue::as_scalar)
        .unwrap_or(0.0)
        != 0.0
    {
        report.push(UnsupportedFeature {
            path: format!("{path}.skew"),
            kind: UnsupportedKind::TransformFeature,
            detail: "skew is not supported by the active support profile".to_string(),
        });
    }
}

pub(super) fn visit_position_value(
    value: Option<&PositionValue>,
    path: &str,
    field: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
) {
    let Some(value) = value else {
        return;
    };

    match value {
        PositionValue::Combined(value) => {
            visit_animated_value(Some(value), path, field, profile, report);
        }
        PositionValue::Split(value) => {
            if !value.is_split() || value.x.is_none() || value.y.is_none() {
                report.push(UnsupportedFeature {
                    path: format!("{path}.{field}"),
                    kind: UnsupportedKind::AnimatedValue,
                    detail: "split position data is malformed".to_string(),
                });
                return;
            }

            visit_animated_value(
                value.x.as_ref(),
                &format!("{path}.{field}"),
                "x",
                profile,
                report,
            );
            visit_animated_value(
                value.y.as_ref(),
                &format!("{path}.{field}"),
                "y",
                profile,
                report,
            );
            visit_animated_value(
                value.z.as_ref(),
                &format!("{path}.{field}"),
                "z",
                profile,
                report,
            );
        }
    }
}

pub(super) fn visit_animated_value(
    value: Option<&AnimatedValue>,
    path: &str,
    field: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
) {
    let Some(value) = value else {
        return;
    };

    if !profile.allow_expressions && value.has_expression() {
        report.push(UnsupportedFeature {
            path: format!("{path}.{field}"),
            kind: UnsupportedKind::Expressions,
            detail: "expressions are not supported by the active support profile".to_string(),
        });
    }

    if !profile.allow_animated_values {
        if !value.is_static() {
            report.push(UnsupportedFeature {
                path: format!("{path}.{field}"),
                kind: UnsupportedKind::AnimatedValue,
                detail: "animated values are not supported by the active support profile"
                    .to_string(),
            });
        }
        return;
    }

    if !is_supported_animated_value(value) {
        report.push(UnsupportedFeature {
            path: format!("{path}.{field}"),
            kind: UnsupportedKind::AnimatedValue,
            detail: "animated value uses unsupported keyframe features".to_string(),
        });
    }
}

pub(super) fn visit_scalar_animated_value(
    value: Option<&AnimatedValue>,
    path: &str,
    field: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
) {
    let Some(value) = value else {
        return;
    };

    visit_animated_value(Some(value), path, field, profile, report);
    if !is_supported_scalar_value(value) {
        report.push(UnsupportedFeature {
            path: format!("{path}.{field}"),
            kind: UnsupportedKind::AnimatedValue,
            detail: "value must be a scalar animated value".to_string(),
        });
    }
}

fn visit_time_remap(
    layer: &Layer,
    value: &AnimatedValue,
    path: &str,
    profile: &SupportProfile,
    report: &mut SupportReport,
) {
    if layer.layer_type != LayerType::PRECOMP {
        report.push(UnsupportedFeature {
            path: format!("{path}.time_remap"),
            kind: UnsupportedKind::TimeRemap,
            detail: "time remap is only supported on precomp layers".to_string(),
        });
        return;
    }

    visit_animated_value(Some(value), path, "time_remap", profile, report);

    if !is_supported_scalar_value(value) {
        report.push(UnsupportedFeature {
            path: format!("{path}.time_remap"),
            kind: UnsupportedKind::TimeRemap,
            detail: "time remap must be a scalar animated value".to_string(),
        });
    }
}

fn qualify(base_path: &str, name: &str, index: Option<i64>) -> String {
    let mut path = base_path.to_string();
    if let Some(index) = index {
        let _ = write!(path, "#{index}");
    }
    if !name.is_empty() {
        let _ = write!(path, " `{name}`");
    }
    path
}
