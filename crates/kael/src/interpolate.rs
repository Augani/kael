//! The canonical interpolation vocabulary shared across the framework.
//!
//! These primitives back the implicit-style transitions in [`crate::Div`] and
//! are re-used by higher-level crates (e.g. `kael_ui`) so that there is a
//! single, authoritative definition for each interpolated quantity.
//!
//! `progress` is the normalized blend factor; callers that require clamping to
//! `0..=1` must clamp before calling.

use crate::{BoxShadow, Hsla, Pixels, Rgba, point, px};
use anyhow::Result;
use smallvec::SmallVec;

const MAX_INTERPOLATED_SHADOWS: usize = 1_024;

/// Linearly interpolate a scalar without clamping `progress`.
pub fn interpolate_f32(from: f32, to: f32, progress: f32) -> f32 {
    try_interpolate_f32(from, to, progress).unwrap_or_else(|_| {
        let endpoint = if progress.is_finite() && progress >= 0.5 {
            to
        } else {
            from
        };
        if endpoint.is_finite() { endpoint } else { 0.0 }
    })
}

/// Linearly interpolate a scalar while rejecting non-finite inputs or output.
pub fn try_interpolate_f32(from: f32, to: f32, progress: f32) -> Result<f32> {
    anyhow::ensure!(
        from.is_finite() && to.is_finite() && progress.is_finite(),
        "interpolation inputs must be finite"
    );
    let value = from + (to - from) * progress;
    anyhow::ensure!(value.is_finite(), "interpolation result must be finite");
    Ok(value)
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

/// Interpolate a single box shadow. Mismatched `inset` flags switch discretely
/// at the halfway point because inset and outset shadows cannot be blended.
pub fn interpolate_shadow(from: &BoxShadow, to: &BoxShadow, progress: f32) -> BoxShadow {
    if from.inset != to.inset {
        return if progress.is_finite() && progress >= 0.5 {
            to.clone()
        } else {
            from.clone()
        };
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
    interpolate_shadows_inner(
        from,
        to,
        progress,
        from.len().max(to.len()).min(MAX_INTERPOLATED_SHADOWS),
    )
}

/// Interpolate a bounded shadow stack while rejecting invalid progress.
pub fn interpolate_shadows_checked(
    from: &SmallVec<[BoxShadow; 1]>,
    to: &SmallVec<[BoxShadow; 1]>,
    progress: f32,
) -> Result<SmallVec<[BoxShadow; 1]>> {
    anyhow::ensure!(
        progress.is_finite(),
        "shadow interpolation progress must be finite"
    );
    let len = from.len().max(to.len());
    anyhow::ensure!(
        len <= MAX_INTERPOLATED_SHADOWS,
        "shadow interpolation cannot exceed {MAX_INTERPOLATED_SHADOWS} shadows"
    );
    Ok(interpolate_shadows_inner(from, to, progress, len))
}

fn interpolate_shadows_inner(
    from: &SmallVec<[BoxShadow; 1]>,
    to: &SmallVec<[BoxShadow; 1]>,
    progress: f32,
    len: usize,
) -> SmallVec<[BoxShadow; 1]> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsla;

    fn shadow(inset: bool, offset: f32) -> BoxShadow {
        BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 1.0),
            offset: point(px(offset), px(0.0)),
            blur_radius: px(4.0),
            spread_radius: px(0.0),
            inset,
        }
    }

    #[test]
    fn invalid_scalar_interpolation_does_not_leak_non_finite_geometry() {
        assert!(try_interpolate_f32(0.0, 1.0, f32::NAN).is_err());
        assert!(try_interpolate_f32(f32::MAX, -f32::MAX, 2.0).is_err());
        assert_eq!(interpolate_f32(4.0, 8.0, f32::NAN), 4.0);
        assert_eq!(interpolate_f32(f32::NAN, 8.0, 0.0), 0.0);
    }

    #[test]
    fn incompatible_shadow_modes_switch_at_halfway_not_at_start() {
        let outset = shadow(false, 1.0);
        let inset = shadow(true, 9.0);
        assert_eq!(interpolate_shadow(&outset, &inset, 0.0).inset, false);
        assert_eq!(interpolate_shadow(&outset, &inset, 0.49).inset, false);
        assert_eq!(interpolate_shadow(&outset, &inset, 0.5).inset, true);
        assert_eq!(interpolate_shadow(&outset, &inset, 1.0).inset, true);
    }

    #[test]
    fn shadow_stack_interpolation_is_bounded() {
        let from = std::iter::repeat_with(|| shadow(false, 0.0))
            .take(MAX_INTERPOLATED_SHADOWS + 1)
            .collect::<SmallVec<[BoxShadow; 1]>>();
        let to = SmallVec::new();
        assert!(interpolate_shadows_checked(&from, &to, 0.5).is_err());
        assert_eq!(
            interpolate_shadows(&from, &to, 0.5).len(),
            MAX_INTERPOLATED_SHADOWS
        );
    }
}
