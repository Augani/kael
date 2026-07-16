//! # Animation Utilities and Presets
//!
//! Professional animation system providing smooth, polished easing functions and reusable
//! animation configurations for desktop application interfaces.
//! ## Features
//!
//! - **Easing Functions**: Mathematical easing curves for natural motion
//! - **Duration Presets**: Standardized timing values following UI guidelines
//! - **Animation Presets**: Ready-to-use animations for common interactions
//! - **Spring Physics**: Realistic bouncy animations with configurable parameters
//! - **Performance**: Optimized calculations with minimal runtime overhead
//!
//! ## Easing Categories
//!
//! - **Linear**: Constant velocity (rarely natural for UI)
//! - **Quadratic/Cubic/Quartic**: Smooth acceleration/deceleration
//! - **Exponential**: Dramatic acceleration (good for entrances)
//! - **Spring**: Natural bouncy motion with physics simulation
//! - **Back**: Slight overshoot for emphasis (subtle bounce effect)
//!
//! ## Usage Examples
//!
//! ### Basic Animation
//! ```rust,ignore
//! use kael_ui::animations::*;
//!
//! // Fade in with smooth easing
//! div()
//!     .with_animation(
//!         "fade-in",
//!         fade_in(Duration::from_millis(300)),
//!         |el, delta| el.opacity(delta)
//!     )
//! ```
//!
//! ### Spring Animation
//! ```rust,ignore
//! // Natural slide with bounce
//! div()
//!     .with_animation(
//!         "slide-spring",
//!         spring_slide(Duration::from_millis(400)),
//!         |el, delta| el.ml(px(-100.0 * (1.0 - delta)))
//!     )
//! ```
//!
//! ### Preset Usage
//! ```rust,ignore
//! // Use predefined animations
//! div().with_animation(
//!     "bounce",
//!     presets::bounce_in(),
//!     |el, delta| el.scale(delta)
//! )
//! ```
//!
//! ## Design Decisions
//!
//! - **Performance First**: All calculations are lightweight and cache-friendly
//! - **Natural Motion**: Easing curves based on real-world physics observations
//! - **Consistency**: Standardized durations and easing across the library
//! - **Extensibility**: Easy to add custom easing functions and presets
//! - **Kael Integration**: Seamless integration with Kael's animation system
//!

use kael::*;
use smallvec::SmallVec;
use std::time::Duration;

/// Returns a stagger delay without overflowing for unusually large item counts.
pub(crate) fn stagger_delay(stagger: Duration, index: usize) -> Duration {
    let multiplier = u32::try_from(index).unwrap_or(u32::MAX);
    stagger.saturating_mul(multiplier)
}

/// Maps progress across a delayed animation to progress across its active phase.
///
/// `delta` is normalized over `delay + active_duration`. A zero-duration active
/// phase stays at its initial value until the delay completes, then resolves to
/// its final value. This avoids the `0 / 0` boundary that would otherwise leave
/// animated content with NaN opacity, size, or position.
pub(crate) fn delayed_animation_progress(
    delta: f32,
    delay: Duration,
    active_duration: Duration,
) -> f32 {
    if !delta.is_finite() {
        return 0.0;
    }

    let delta = delta.clamp(0.0, 1.0);
    let delay_seconds = delay.as_secs_f64();
    let active_seconds = active_duration.as_secs_f64();
    let total_seconds = delay_seconds + active_seconds;

    if total_seconds <= f64::EPSILON {
        return 1.0;
    }
    if active_seconds <= f64::EPSILON {
        return if delta >= 1.0 { 1.0 } else { 0.0 };
    }

    let delay_fraction = delay_seconds / total_seconds;
    (((delta as f64 - delay_fraction) / (1.0 - delay_fraction)).clamp(0.0, 1.0)) as f32
}

/// Standard animation durations following modern UI guidelines
pub mod durations {
    use std::time::Duration;

    /// Ultra fast (100ms) - for micro-interactions
    pub const ULTRA_FAST: Duration = Duration::from_millis(100);

    /// Very fast (150ms) - for subtle state changes
    pub const FASTEST: Duration = Duration::from_millis(150);

    /// Fast (200ms) - for quick transitions
    pub const FAST: Duration = Duration::from_millis(200);

    /// Normal (300ms) - default for most animations
    pub const NORMAL: Duration = Duration::from_millis(300);

    /// Slow (400ms) - for emphasis
    pub const SLOW: Duration = Duration::from_millis(400);

    /// Very slow (500ms) - for dramatic effects
    pub const SLOWEST: Duration = Duration::from_millis(500);

    /// Extra slow (600ms) - for very dramatic effects
    pub const EXTRA_SLOW: Duration = Duration::from_millis(600);
}

/// Easing functions for smooth animations.
///
/// These are compatibility shims that delegate to the canonical
/// [`kael::Easing`] vocabulary; prefer [`kael::Easing`] variants directly in new
/// code. Each function preserves its historical name and `fn(f32) -> f32`
/// signature so existing call sites keep compiling. The `spring`,
/// `smooth_spring`, and `cubic_bezier` curves have no core variant and remain
/// defined here as their authoritative implementation.
pub mod easings {
    use kael::Easing;

    /// Linear easing. Delegates to [`Easing::Linear`].
    pub fn linear(t: f32) -> f32 {
        Easing::Linear.ease(t)
    }

    /// Quadratic ease-in. Delegates to [`Easing::EaseIn`].
    pub fn ease_in_quad(t: f32) -> f32 {
        Easing::EaseIn.ease(t)
    }

    /// Quadratic ease-out. Delegates to [`Easing::EaseOut`].
    pub fn ease_out_quad(t: f32) -> f32 {
        Easing::EaseOut.ease(t)
    }

    /// Quadratic ease-in-out. Delegates to [`Easing::EaseInOut`].
    pub fn ease_in_out_quad(t: f32) -> f32 {
        Easing::EaseInOut.ease(t)
    }

    /// Cubic ease-in. Delegates to [`Easing::EaseInCubic`].
    pub fn ease_in_cubic(t: f32) -> f32 {
        Easing::EaseInCubic.ease(t)
    }

    /// Cubic ease-out. Delegates to [`Easing::EaseOutCubic`].
    pub fn ease_out_cubic(t: f32) -> f32 {
        Easing::EaseOutCubic.ease(t)
    }

    /// Cubic ease-in-out. Delegates to [`Easing::EaseInOutCubic`].
    pub fn ease_in_out_cubic(t: f32) -> f32 {
        Easing::EaseInOutCubic.ease(t)
    }

    /// Quartic ease-in. Delegates to [`Easing::EaseInQuart`].
    pub fn ease_in_quart(t: f32) -> f32 {
        Easing::EaseInQuart.ease(t)
    }

    /// Quartic ease-out. Delegates to [`Easing::EaseOutQuart`].
    pub fn ease_out_quart(t: f32) -> f32 {
        Easing::EaseOutQuart.ease(t)
    }

    /// Quartic ease-in-out. Delegates to [`Easing::EaseInOutQuart`].
    pub fn ease_in_out_quart(t: f32) -> f32 {
        Easing::EaseInOutQuart.ease(t)
    }

    /// Exponential ease-out. Delegates to [`Easing::EaseOutExpo`].
    pub fn ease_out_expo(t: f32) -> f32 {
        Easing::EaseOutExpo.ease(t)
    }

    /// Exponential ease-in-out. Delegates to [`Easing::EaseInOutExpo`].
    pub fn ease_in_out_expo(t: f32) -> f32 {
        Easing::EaseInOutExpo.ease(t)
    }

    /// Natural bouncy spring. No core variant; defined here.
    pub fn spring(t: f32) -> f32 {
        if t >= 1.0 {
            return 1.0;
        }
        let damping = 0.7;
        let frequency = 1.5;
        let decay = (-damping * t * 10.0).exp();
        let oscillation = (frequency * t * std::f32::consts::PI * 2.0).sin();
        (1.0 - decay * oscillation * 0.5).clamp(0.0, 1.0)
    }

    /// Clamped elastic spring. Delegates to [`Easing::Elastic`].
    pub fn elastic(t: f32) -> f32 {
        Easing::Elastic.ease(t)
    }

    /// Subtle spring suited to UI. No core variant; defined here.
    pub fn smooth_spring(t: f32) -> f32 {
        if t >= 1.0 {
            return 1.0;
        }
        let damping = 0.9;
        let frequency = 1.0;
        let decay = (-damping * t * 10.0).exp();
        let oscillation = (frequency * t * std::f32::consts::PI * 2.0).sin();
        (t + decay * oscillation * 0.1).clamp(0.0, 1.0)
    }

    /// Backing ease-out with reduced overshoot. Delegates to
    /// [`Easing::EaseOutBack`].
    pub fn ease_out_back(t: f32) -> f32 {
        Easing::EaseOutBack(1.2).ease(t)
    }

    /// Exponential ease-in. Delegates to [`Easing::EaseInExpo`].
    pub fn ease_in_expo(t: f32) -> f32 {
        Easing::EaseInExpo.ease(t)
    }

    /// Circular ease-in. Delegates to [`Easing::EaseInCirc`].
    pub fn ease_in_circ(t: f32) -> f32 {
        Easing::EaseInCirc.ease(t)
    }

    /// Circular ease-out. Delegates to [`Easing::EaseOutCirc`].
    pub fn ease_out_circ(t: f32) -> f32 {
        Easing::EaseOutCirc.ease(t)
    }

    /// Circular ease-in-out. Delegates to [`Easing::EaseInOutCirc`].
    pub fn ease_in_out_circ(t: f32) -> f32 {
        Easing::EaseInOutCirc.ease(t)
    }

    /// Backing ease-in. Delegates to [`Easing::EaseInBack`].
    pub fn ease_in_back(t: f32) -> f32 {
        Easing::EaseInBack(1.70158).ease(t)
    }

    /// Backing ease-in-out. Delegates to [`Easing::EaseInOutBack`].
    pub fn ease_in_out_back(t: f32) -> f32 {
        Easing::EaseInOutBack(1.70158).ease(t)
    }

    /// Elastic ease-in. Delegates to [`Easing::EaseInElastic`].
    pub fn ease_in_elastic(t: f32) -> f32 {
        Easing::EaseInElastic.ease(t)
    }

    /// Elastic ease-out. Delegates to [`Easing::EaseOutElastic`].
    pub fn ease_out_elastic(t: f32) -> f32 {
        Easing::EaseOutElastic.ease(t)
    }

    /// Quintic ease-in. Delegates to [`Easing::EaseInQuint`].
    pub fn ease_in_quint(t: f32) -> f32 {
        Easing::EaseInQuint.ease(t)
    }

    /// Quintic ease-out. Delegates to [`Easing::EaseOutQuint`].
    pub fn ease_out_quint(t: f32) -> f32 {
        Easing::EaseOutQuint.ease(t)
    }

    /// Quintic ease-in-out. Delegates to [`Easing::EaseInOutQuint`].
    pub fn ease_in_out_quint(t: f32) -> f32 {
        Easing::EaseInOutQuint.ease(t)
    }

    /// Stepped easing builder. Delegates to [`Easing::Steps`].
    pub fn steps(n: u32) -> impl Fn(f32) -> f32 {
        move |t: f32| Easing::Steps(n).ease(t)
    }

    /// Cubic Bezier easing builder. No core variant of equal precision;
    /// defined here.
    pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
        move |t: f32| {
            if t <= 0.0 {
                return 0.0;
            }
            if t >= 1.0 {
                return 1.0;
            }
            let mut low = 0.0_f32;
            let mut high = 1.0_f32;
            let mut mid;
            for _ in 0..20 {
                mid = (low + high) / 2.0;
                let x = cubic_bezier_sample(mid, x1, x2);
                if (x - t).abs() < 0.0001 {
                    return cubic_bezier_sample(mid, y1, y2);
                }
                if x < t {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            cubic_bezier_sample((low + high) / 2.0, y1, y2)
        }
    }

    fn cubic_bezier_sample(t: f32, p1: f32, p2: f32) -> f32 {
        let t2 = t * t;
        let t3 = t2 * t;
        3.0 * (1.0 - t) * (1.0 - t) * t * p1 + 3.0 * (1.0 - t) * t2 * p2 + t3
    }

    /// Recommended smooth default. Delegates to [`Easing::EaseInOutCubic`].
    pub fn smooth() -> impl Fn(f32) -> f32 {
        ease_in_out_cubic
    }

    /// Snappy default with overshoot. Delegates to [`Easing::EaseOutBack`].
    pub fn snappy() -> impl Fn(f32) -> f32 {
        ease_out_back
    }
}

/// Creates a smooth fade-in animation
///
/// Uses cubic easing for the most natural fade effect
pub fn fade_in(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_out_cubic)
}

/// Creates a smooth fade-out animation
pub fn fade_out(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_in_cubic)
}

/// Creates a smooth slide animation
///
/// Best for sliding panels, drawers, and menus
pub fn slide_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_out_cubic)
}

/// Creates a spring-based slide animation
///
/// Natural feeling slide with subtle bounce
pub fn spring_slide(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::smooth_spring)
}

/// Creates a scale animation with back easing
///
/// Scales with a slight overshoot for emphasis
pub fn scale_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_out_back)
}

/// Creates a smooth scale animation without overshoot
pub fn scale_smooth(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_out_cubic)
}

/// Creates a rotation animation
pub fn rotate_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::linear)
}

/// Creates a smooth, professional pulse animation
///
/// Uses sine wave for natural breathing effect
pub fn pulse_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::linear)
}

/// Creates a shake animation (horizontal movement)
///
/// Uses elastic easing for realistic shake
pub fn shake_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_out_quad)
}

/// Creates a bounce animation with spring physics
pub fn bounce_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::spring)
}

/// Creates a smooth bounce without overshoot
pub fn bounce_smooth(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::ease_out_quart)
}

/// Creates an elastic spring animation
pub fn spring_animation(duration: Duration) -> Animation {
    Animation::new(duration).with_easing(easings::smooth_spring)
}

/// Pre-configured animation presets with optimal settings
pub mod presets {
    use super::*;

    // Fade animations
    /// Ultra-quick fade in (100ms) - for tooltips
    pub fn fade_in_ultra_quick() -> Animation {
        fade_in(durations::ULTRA_FAST)
    }

    /// Quick fade in (200ms) - for fast transitions
    pub fn fade_in_quick() -> Animation {
        fade_in(durations::FAST)
    }

    /// Normal fade in (300ms) - standard UI transition
    pub fn fade_in_normal() -> Animation {
        fade_in(durations::NORMAL)
    }

    /// Slow fade in (400ms) - for emphasis
    pub fn fade_in_slow() -> Animation {
        fade_in(durations::SLOW)
    }

    /// Quick fade out (200ms)
    pub fn fade_out_quick() -> Animation {
        fade_out(durations::FAST)
    }

    /// Normal fade out (300ms)
    pub fn fade_out_normal() -> Animation {
        fade_out(durations::NORMAL)
    }

    // Slide animations with improved easing
    /// Slide in from top with smooth easing
    pub fn slide_in_top() -> Animation {
        slide_animation(durations::NORMAL)
    }

    /// Slide in from bottom with smooth easing
    pub fn slide_in_bottom() -> Animation {
        slide_animation(durations::NORMAL)
    }

    /// Slide in from left with smooth easing
    pub fn slide_in_left() -> Animation {
        slide_animation(durations::NORMAL)
    }

    /// Slide in from right with smooth easing
    pub fn slide_in_right() -> Animation {
        slide_animation(durations::NORMAL)
    }

    /// Spring slide from left - natural feeling
    pub fn spring_slide_left() -> Animation {
        spring_slide(durations::SLOW)
    }

    /// Spring slide from right - natural feeling
    pub fn spring_slide_right() -> Animation {
        spring_slide(durations::SLOW)
    }

    // Scale animations
    /// Scale up with back easing (slight overshoot)
    pub fn scale_up() -> Animation {
        scale_animation(durations::FAST)
    }

    /// Scale down with back easing
    pub fn scale_down() -> Animation {
        scale_animation(durations::FAST)
    }

    /// Smooth scale up (no overshoot)
    pub fn scale_up_smooth() -> Animation {
        scale_smooth(durations::FAST)
    }

    /// Smooth scale down (no overshoot)
    pub fn scale_down_smooth() -> Animation {
        scale_smooth(durations::FAST)
    }

    // Rotation animations
    /// Continuous spin (2 seconds per rotation)
    pub fn spin() -> Animation {
        rotate_animation(Duration::from_secs(2)).repeat_forever()
    }

    /// Fast spin (1 second per rotation)
    pub fn spin_fast() -> Animation {
        rotate_animation(Duration::from_secs(1)).repeat_forever()
    }

    /// Slow spin (3 seconds per rotation) - for loading indicators
    pub fn spin_slow() -> Animation {
        rotate_animation(Duration::from_secs(3)).repeat_forever()
    }

    // Pulse animations - improved smoothness
    /// Smooth pulse effect (1 second cycle)
    pub fn pulse() -> Animation {
        pulse_animation(Duration::from_secs(1)).repeat_forever()
    }

    /// Fast pulse (600ms cycle) - for urgent notifications
    pub fn pulse_fast() -> Animation {
        pulse_animation(durations::EXTRA_SLOW).repeat_forever()
    }

    /// Slow pulse (1.5 second cycle) - for subtle breathing effect
    pub fn pulse_slow() -> Animation {
        pulse_animation(Duration::from_millis(1500)).repeat_forever()
    }

    // Interactive animations
    /// Shake effect (error indication)
    pub fn shake() -> Animation {
        shake_animation(durations::FAST)
    }

    /// Strong shake (critical error)
    pub fn shake_strong() -> Animation {
        shake_animation(durations::NORMAL)
    }

    // Bounce animations
    /// Bounce in effect with spring physics
    pub fn bounce_in() -> Animation {
        bounce_animation(durations::SLOW)
    }

    /// Smooth bounce (no overshoot)
    pub fn bounce_smooth_preset() -> Animation {
        bounce_smooth(durations::NORMAL)
    }

    // Spring animations
    /// Spring effect (natural feeling)
    pub fn spring() -> Animation {
        spring_animation(durations::SLOW)
    }

    /// Quick spring
    pub fn spring_quick() -> Animation {
        spring_animation(durations::NORMAL)
    }
}

/// Animation state management helper
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum AnimationState {
    /// Animation hasn't started
    #[default]
    Idle,
    /// Animation is running
    Running,
    /// Animation completed
    Complete,
}

impl AnimationState {
    /// Check if animation is idle
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Check if animation is running
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if animation is complete
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// Calculate smooth pulse scale (sine wave based)
///
/// Returns a scale factor that oscillates smoothly
pub fn pulse_scale(delta: f32, min_scale: f32, max_scale: f32) -> f32 {
    let oscillation = (delta * std::f32::consts::PI * 2.0).sin();
    let normalized = (oscillation + 1.0) / 2.0; // Convert -1..1 to 0..1
    min_scale + (max_scale - min_scale) * normalized
}

/// Calculate smooth pulse opacity
///
/// Returns an opacity value that oscillates smoothly
pub fn pulse_opacity(delta: f32, min_opacity: f32, max_opacity: f32) -> f32 {
    let oscillation = (delta * std::f32::consts::PI * 2.0).sin();
    let normalized = (oscillation + 1.0) / 2.0;
    min_opacity + (max_opacity - min_opacity) * normalized
}

/// Calculate shake offset with natural decay
pub fn shake_offset(delta: f32, max_offset: f32) -> f32 {
    let frequency = 4.0;
    let decay = 1.0 - delta;
    (delta * std::f32::consts::PI * frequency).sin() * max_offset * decay
}

/// Calculate spring bounce with natural physics
pub fn spring_bounce(delta: f32, amplitude: f32) -> f32 {
    let damping = 0.7;
    let frequency = 1.5;
    let decay = (-damping * delta * 10.0).exp();
    let oscillation = (frequency * delta * std::f32::consts::PI * 2.0).sin();
    amplitude * decay * oscillation
}

/// Clamped scalar interpolation.
///
/// Prefer the canonical [`kael::interpolate::interpolate_f32`], which does not
/// clamp `t`; this shim clamps to `0..=1` and delegates.
pub fn lerp_f32(from: f32, to: f32, t: f32) -> f32 {
    kael::interpolate::interpolate_f32(from, to, t.clamp(0.0, 1.0))
}

/// Clamped length interpolation.
///
/// Prefer the canonical [`kael::interpolate::interpolate_pixels`]; this shim
/// clamps `t` to `0..=1` and delegates.
pub fn lerp_pixels(from: Pixels, to: Pixels, t: f32) -> Pixels {
    kael::interpolate::interpolate_pixels(from, to, t.clamp(0.0, 1.0))
}

/// Clamped HSL-space color interpolation.
///
/// Blends each HSL channel directly. For perceptual linear-RGB blending use
/// the canonical [`kael::interpolate::interpolate_hsla`].
pub fn lerp_color(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    Hsla {
        h: kael::interpolate::interpolate_f32(from.h, to.h, t),
        s: kael::interpolate::interpolate_f32(from.s, to.s, t),
        l: kael::interpolate::interpolate_f32(from.l, to.l, t),
        a: kael::interpolate::interpolate_f32(from.a, to.a, t),
    }
}

/// Clamped box-shadow interpolation.
///
/// Prefer the canonical [`kael::interpolate::interpolate_shadow`]; this shim
/// clamps `t` to `0..=1` and delegates.
pub fn lerp_shadow(from: &BoxShadow, to: &BoxShadow, t: f32) -> BoxShadow {
    kael::interpolate::interpolate_shadow(from, to, t.clamp(0.0, 1.0))
}

/// Clamped box-shadow stack interpolation.
///
/// Prefer the canonical [`kael::interpolate::interpolate_shadows`]; this shim
/// clamps `t` to `0..=1` and delegates.
pub fn lerp_shadows(from: &[BoxShadow], to: &[BoxShadow], t: f32) -> SmallVec<[BoxShadow; 2]> {
    let t = t.clamp(0.0, 1.0);
    let from: SmallVec<[BoxShadow; 1]> = from.iter().cloned().collect();
    let to: SmallVec<[BoxShadow; 1]> = to.iter().cloned().collect();
    kael::interpolate::interpolate_shadows(&from, &to, t)
        .into_iter()
        .collect()
}

#[cfg(test)]
mod easing_delegate_tests {
    use super::{delayed_animation_progress, easings, stagger_delay};
    use kael::Easing;
    use std::time::Duration;

    fn sweep(delegate: impl Fn(f32) -> f32, easing: Easing) {
        for step in 0..=100 {
            let t = step as f32 / 100.0;
            assert_eq!(delegate(t), easing.ease(t), "mismatch at t={t}");
        }
    }

    #[test]
    fn delegates_match_core_variants() {
        sweep(easings::linear, Easing::Linear);
        sweep(easings::ease_in_quad, Easing::EaseIn);
        sweep(easings::ease_out_quad, Easing::EaseOut);
        sweep(easings::ease_in_out_quad, Easing::EaseInOut);
        sweep(easings::ease_in_cubic, Easing::EaseInCubic);
        sweep(easings::ease_out_cubic, Easing::EaseOutCubic);
        sweep(easings::ease_in_out_cubic, Easing::EaseInOutCubic);
        sweep(easings::ease_in_quart, Easing::EaseInQuart);
        sweep(easings::ease_out_quart, Easing::EaseOutQuart);
        sweep(easings::ease_in_out_quart, Easing::EaseInOutQuart);
        sweep(easings::ease_in_quint, Easing::EaseInQuint);
        sweep(easings::ease_out_quint, Easing::EaseOutQuint);
        sweep(easings::ease_in_out_quint, Easing::EaseInOutQuint);
        sweep(easings::ease_in_expo, Easing::EaseInExpo);
        sweep(easings::ease_out_expo, Easing::EaseOutExpo);
        sweep(easings::ease_in_out_expo, Easing::EaseInOutExpo);
        sweep(easings::ease_in_circ, Easing::EaseInCirc);
        sweep(easings::ease_out_circ, Easing::EaseOutCirc);
        sweep(easings::ease_in_out_circ, Easing::EaseInOutCirc);
        sweep(easings::ease_in_back, Easing::EaseInBack(1.70158));
        sweep(easings::ease_out_back, Easing::EaseOutBack(1.2));
        sweep(easings::ease_in_out_back, Easing::EaseInOutBack(1.70158));
        sweep(easings::ease_in_elastic, Easing::EaseInElastic);
        sweep(easings::ease_out_elastic, Easing::EaseOutElastic);
        sweep(easings::elastic, Easing::Elastic);
        sweep(easings::steps(4), Easing::Steps(4));
    }

    #[test]
    fn delayed_progress_handles_zero_durations_without_nan() {
        let zero = Duration::ZERO;
        assert_eq!(delayed_animation_progress(0.0, zero, zero), 1.0);
        assert_eq!(delayed_animation_progress(1.0, zero, zero), 1.0);

        let delay = Duration::from_millis(100);
        assert_eq!(delayed_animation_progress(0.999, delay, zero), 0.0);
        assert_eq!(delayed_animation_progress(1.0, delay, zero), 1.0);
    }

    #[test]
    fn delayed_progress_is_finite_clamped_and_uses_only_the_active_phase() {
        let delay = Duration::from_millis(100);
        let duration = Duration::from_millis(300);

        assert_eq!(delayed_animation_progress(-1.0, delay, duration), 0.0);
        assert_eq!(delayed_animation_progress(0.25, delay, duration), 0.0);
        assert!((delayed_animation_progress(0.625, delay, duration) - 0.5).abs() < f32::EPSILON);
        assert_eq!(delayed_animation_progress(2.0, delay, duration), 1.0);
        assert_eq!(delayed_animation_progress(f32::NAN, delay, duration), 0.0);
    }

    #[test]
    fn stagger_delay_multiplies_without_panicking() {
        assert_eq!(
            stagger_delay(Duration::from_millis(50), 3),
            Duration::from_millis(150)
        );
        assert_eq!(stagger_delay(Duration::MAX, usize::MAX), Duration::MAX);
    }
}
