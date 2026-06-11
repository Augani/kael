//! Rubber-band scroll elasticity: the canonical overscroll feel shared across
//! the framework.
//!
//! When content is dragged past its scroll bounds, the boundary motion is
//! integrated into a signed overscroll amount with progressive damping
//! ([`add_scroll_elasticity`]) and, once released, decays back to zero with a
//! frame-rate-independent exponential ([`advance_scroll_elasticity`]). Using
//! these functions everywhere guarantees the rubber-band stretch and snap-back
//! curve are identical in core div scroll and in higher-level scroll
//! containers.

use crate::Pixels;
#[cfg(any(target_os = "macos", test))]
use crate::ScrollDelta;
use crate::ScrollWheelEvent;
use crate::geometry::IsZero;
use crate::px;

pub(crate) fn rubber_band_scroll_enabled(event: &ScrollWheelEvent) -> bool {
    #[cfg(any(target_os = "macos", test))]
    {
        matches!(event.delta, ScrollDelta::Pixels(_))
    }

    #[cfg(not(any(target_os = "macos", test)))]
    {
        let _ = event;
        false
    }
}

/// Returns whether both values are non-zero and point in the same direction.
pub fn pixels_have_same_sign(left: Pixels, right: Pixels) -> bool {
    (left > Pixels::ZERO && right > Pixels::ZERO) || (left < Pixels::ZERO && right < Pixels::ZERO)
}

/// Consumes scroll `delta` against an existing `overscroll` before it reaches
/// the content.
///
/// When the content is already stretched past a boundary, a reverse scroll
/// first un-stretches the rubber band rather than scrolling the content. The
/// returned value is the remaining delta (if any) that should be applied to the
/// content offset once the overscroll has been neutralized.
pub fn consume_scroll_elasticity(overscroll: &mut Pixels, delta: Pixels) -> Pixels {
    if overscroll.is_zero() || pixels_have_same_sign(*overscroll, delta) {
        return delta;
    }

    let next_overscroll = *overscroll + delta;
    if next_overscroll.is_zero() || pixels_have_same_sign(*overscroll, next_overscroll) {
        *overscroll = next_overscroll;
        Pixels::ZERO
    } else {
        *overscroll = Pixels::ZERO;
        next_overscroll
    }
}

/// Integrates boundary `delta` into the rubber-band `overscroll` with
/// progressive damping.
///
/// The further the content is already stretched, the stiffer the band becomes:
/// damping eases from `0.62` near the boundary down to `0.22` at the
/// `128px` limit, and the result is clamped to that limit. This is the canonical
/// stretch curve; share it so every scroll surface feels the same.
pub fn add_scroll_elasticity(overscroll: Pixels, delta: Pixels) -> Pixels {
    const RUBBER_BAND_LIMIT: f32 = 128.0;

    let progress = (overscroll.abs().0 / RUBBER_BAND_LIMIT).clamp(0.0, 1.0);
    let damping = (0.62 - progress * 0.4).clamp(0.22, 0.62);
    let limit = px(RUBBER_BAND_LIMIT);
    (overscroll + delta * damping).clamp(-limit, limit)
}

/// Advances the rubber-band `overscroll` one frame toward zero.
///
/// Decay is a frame-rate-independent exponential (`rate = 12.0`): `last_advance`
/// tracks the previous wall-clock instant so the snap-back covers the same
/// distance per unit time at 60Hz, 120Hz, or any frame cadence. Returns `true`
/// while the band is still settling and `false` once it has snapped flush
/// (within `0.25px`), at which point `last_advance` is reset.
pub fn advance_scroll_elasticity(
    overscroll: &mut Pixels,
    last_advance: &mut Option<std::time::Instant>,
) -> bool {
    const DECAY_RATE: f32 = 12.0;
    const STOP_EPSILON: f32 = 0.25;

    if overscroll.is_zero() {
        *last_advance = None;
        return false;
    }

    let now = std::time::Instant::now();
    let dt = last_advance
        .map(|prev| now.duration_since(prev).as_secs_f32().min(0.05))
        .unwrap_or(1.0 / 60.0);
    *last_advance = Some(now);

    let factor = (-DECAY_RATE * dt).exp();
    *overscroll *= factor;
    if overscroll.abs() <= px(STOP_EPSILON) {
        *overscroll = Pixels::ZERO;
        *last_advance = None;
    }

    !overscroll.is_zero()
}

/// Applies a single-axis scroll `delta` to an `offset`, routing any motion past
/// the bounds into rubber-band `overscroll`.
///
/// This is the full per-axis pipeline used by core scrolling: reverse deltas
/// first [`consume_scroll_elasticity`], the offset is clamped to
/// `[-max_offset, 0]`, and the portion that would exceed the boundary is fed
/// into [`add_scroll_elasticity`]. With `rubber_band_enabled` false (or no
/// scrollable range) the offset is hard-clamped and overscroll is cleared.
pub fn apply_scroll_delta_axis(
    offset: &mut Pixels,
    overscroll: &mut Pixels,
    max_offset: Pixels,
    delta: Pixels,
    rubber_band_enabled: bool,
) {
    if delta.is_zero() {
        return;
    }

    if !rubber_band_enabled || max_offset.is_zero() {
        *overscroll = Pixels::ZERO;
        *offset = (*offset + delta).clamp(-max_offset, px(0.));
        return;
    }

    let remaining_delta = consume_scroll_elasticity(overscroll, delta);
    let candidate_offset = *offset + remaining_delta;
    let clamped_offset = candidate_offset.clamp(-max_offset, px(0.));
    let boundary_delta = candidate_offset - clamped_offset;

    *offset = clamped_offset;
    if !boundary_delta.is_zero() {
        *overscroll = add_scroll_elasticity(*overscroll, boundary_delta);
    }
}
