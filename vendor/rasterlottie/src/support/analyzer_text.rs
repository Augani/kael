#[cfg(feature = "text")]
use unicode_segmentation::UnicodeSegmentation;

use super::analyzer::{SupportProfile, SupportReport, UnsupportedFeature, UnsupportedKind};
#[cfg(feature = "text")]
use crate::model::TextDocument;
use crate::{
    effects::supported_layer_effects,
    model::{Animation, Layer, LayerType},
};

#[cfg(feature = "text")]
pub(super) fn visit_text_layer(
    animation: &Animation,
    layer: &Layer,
    path: &str,
    report: &mut SupportReport,
) {
    let Some(text) = layer.text.as_ref() else {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::Text,
            detail: "text layer data is missing".to_string(),
        });
        return;
    };

    if text.document.keyframes.is_empty() {
        report.push(UnsupportedFeature {
            path: format!("{path}.text"),
            kind: UnsupportedKind::Text,
            detail: "text layer has no document keyframes".to_string(),
        });
    }

    if text.has_animators() {
        report.push(UnsupportedFeature {
            path: format!("{path}.text"),
            kind: UnsupportedKind::Text,
            detail: "text animators are not supported yet".to_string(),
        });
    }

    if text.has_path() {
        report.push(UnsupportedFeature {
            path: format!("{path}.text"),
            kind: UnsupportedKind::Text,
            detail: "text path data is not supported yet".to_string(),
        });
    }

    for (index, keyframe) in text.document.keyframes.iter().enumerate() {
        visit_text_document(
            animation,
            &keyframe.document,
            &format!("{path}.text.keyframes[{index}]"),
            report,
        );
    }
}

#[cfg(not(feature = "text"))]
pub(super) fn visit_text_layer(
    _animation: &Animation,
    _layer: &Layer,
    path: &str,
    report: &mut SupportReport,
) {
    report.push(UnsupportedFeature {
        path: path.to_string(),
        kind: UnsupportedKind::Text,
        detail: "text support is disabled because the `text` feature is not enabled".to_string(),
    });
}

#[cfg(feature = "text")]
fn visit_text_document(
    animation: &Animation,
    document: &TextDocument,
    path: &str,
    report: &mut SupportReport,
) {
    if document.size <= f32::EPSILON {
        report.push(UnsupportedFeature {
            path: path.to_string(),
            kind: UnsupportedKind::Text,
            detail: "text size must be positive".to_string(),
        });
    }

    if document.justify > 2 {
        report.push(UnsupportedFeature {
            path: format!("{path}.justify"),
            kind: UnsupportedKind::Text,
            detail: "text justification mode is not supported yet".to_string(),
        });
    }

    if document.box_size.is_some() {
        report.push(UnsupportedFeature {
            path: format!("{path}.box_size"),
            kind: UnsupportedKind::Text,
            detail: "boxed text is not supported yet".to_string(),
        });
    }

    let Some(font) = animation.lookup_font(&document.font) else {
        report.push(UnsupportedFeature {
            path: format!("{path}.font"),
            kind: UnsupportedKind::Text,
            detail: format!("missing font `{}`", document.font),
        });
        return;
    };

    for grapheme in document.text.graphemes(true) {
        if is_text_newline(grapheme) {
            continue;
        }

        let Some(glyph) = animation.lookup_glyph(grapheme, font) else {
            report.push(UnsupportedFeature {
                path: path.to_string(),
                kind: UnsupportedKind::Text,
                detail: format!("missing glyph data for `{grapheme}`"),
            });
            continue;
        };

        if glyph.size <= f32::EPSILON {
            report.push(UnsupportedFeature {
                path: path.to_string(),
                kind: UnsupportedKind::Text,
                detail: format!("glyph `{grapheme}` has non-positive size"),
            });
        }
    }
}

pub(super) fn layer_effects_are_supported(layer: &Layer, profile: &SupportProfile) -> bool {
    profile.allow_effects
        || layer.layer_type == LayerType::NULL
        || supported_layer_effects(layer).is_some()
}

#[cfg(feature = "text")]
fn is_text_newline(grapheme: &str) -> bool {
    matches!(grapheme, "\r" | "\n" | "\r\n" | "\u{0003}")
}
