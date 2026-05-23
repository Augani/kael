use crate::{Pixels, ScrollDelta, ScrollWheelEvent, geometry::IsZero, px};

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

pub(crate) fn pixels_have_same_sign(left: Pixels, right: Pixels) -> bool {
    (left > Pixels::ZERO && right > Pixels::ZERO) || (left < Pixels::ZERO && right < Pixels::ZERO)
}

pub(crate) fn consume_scroll_elasticity(overscroll: &mut Pixels, delta: Pixels) -> Pixels {
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

pub(crate) fn add_scroll_elasticity(overscroll: Pixels, delta: Pixels) -> Pixels {
    const RUBBER_BAND_LIMIT: f32 = 128.0;

    let progress = (overscroll.abs().0 / RUBBER_BAND_LIMIT).clamp(0.0, 1.0);
    let damping = (0.62 - progress * 0.4).clamp(0.22, 0.62);
    let limit = px(RUBBER_BAND_LIMIT);
    (overscroll + delta * damping).clamp(-limit, limit)
}

pub(crate) fn advance_scroll_elasticity(overscroll: &mut Pixels) -> bool {
    const SPRING_BACK: f32 = 0.18;
    const STOP_EPSILON: f32 = 0.25;

    if overscroll.is_zero() {
        return false;
    }

    *overscroll *= 1.0 - SPRING_BACK;
    if overscroll.abs() <= px(STOP_EPSILON) {
        *overscroll = Pixels::ZERO;
    }

    !overscroll.is_zero()
}

pub(crate) fn apply_scroll_delta_axis(
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
