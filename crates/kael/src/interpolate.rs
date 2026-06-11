//! The canonical interpolation vocabulary shared across the framework.
//!
//! These primitives back the implicit-style transitions in [`crate::Div`] and
//! are re-used by higher-level crates (e.g. `kael_ui`) so that there is a
//! single, authoritative definition for each interpolated quantity.
//!
//! `progress` is the normalized blend factor; callers that require clamping to
//! `0..=1` must clamp before calling.

use crate::{BoxShadow, Hsla, Pixels, Rgba, point, px};
use smallvec::SmallVec;

/// Linearly interpolate a scalar without clamping `progress`.
pub fn interpolate_f32(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

/// Interpolate a length, mirroring [`interpolate_f32`] in pixel space.
pub fn interpolate_pixels(from: Pixels, to: Pixels, progress: f32) -> Pixels {
    px(interpolate_f32(from.0, to.0, progress))
}

/// Interpolate a color in linear RGB space.
pub fn interpolate_hsla(from: Hsla, to: Hsla, progress: f32) -> Hsla {
    let from = Rgba::from(from);
    let to = Rgba::from(to);
    Rgba {
        r: interpolate_f32(from.r, to.r, progress),
        g: interpolate_f32(from.g, to.g, progress),
        b: interpolate_f32(from.b, to.b, progress),
        a: interpolate_f32(from.a, to.a, progress),
    }
    .into()
}

fn transparent_shadow_like(shadow: &BoxShadow) -> BoxShadow {
    BoxShadow {
        color: shadow.color.alpha(0.0),
        ..shadow.clone()
    }
}

/// Interpolate a single box shadow. Mismatched `inset` flags snap to `to`.
pub fn interpolate_shadow(from: &BoxShadow, to: &BoxShadow, progress: f32) -> BoxShadow {
    if from.inset != to.inset {
        return to.clone();
    }
    BoxShadow {
        color: interpolate_hsla(from.color, to.color, progress),
        offset: point(
            px(interpolate_f32(from.offset.x.0, to.offset.x.0, progress)),
            px(interpolate_f32(from.offset.y.0, to.offset.y.0, progress)),
        ),
        blur_radius: px(interpolate_f32(
            from.blur_radius.0,
            to.blur_radius.0,
            progress,
        )),
        spread_radius: px(interpolate_f32(
            from.spread_radius.0,
            to.spread_radius.0,
            progress,
        )),
        inset: to.inset,
    }
}

/// Interpolate a stack of box shadows, padding the shorter side with
/// transparent equivalents so shadows fade in/out independently.
pub fn interpolate_shadows(
    from: &SmallVec<[BoxShadow; 1]>,
    to: &SmallVec<[BoxShadow; 1]>,
    progress: f32,
) -> SmallVec<[BoxShadow; 1]> {
    let len = from.len().max(to.len());
    (0..len)
        .filter_map(|ix| {
            let from_shadow = from
                .get(ix)
                .cloned()
                .or_else(|| to.get(ix).map(transparent_shadow_like));
            let to_shadow = to
                .get(ix)
                .cloned()
                .or_else(|| from.get(ix).map(transparent_shadow_like));
            match (from_shadow, to_shadow) {
                (Some(from_shadow), Some(to_shadow)) => {
                    Some(interpolate_shadow(&from_shadow, &to_shadow, progress))
                }
                _ => None,
            }
        })
        .collect()
}
