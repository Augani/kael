//! Parameter automation curves — keyframed envelopes for animating a clip or track
//! property over time (opacity, volume, pan, effect parameters).
//!
//! An [`Automation`] holds time-ordered [`Keyframe`]s and samples a value at any time by
//! interpolating between the surrounding keyframes; before the first and after the last
//! keyframe the curve holds the endpoint value.

use serde::{Deserialize, Serialize};

/// How a keyframe's value transitions to the next keyframe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Interpolation {
    /// Hold this value until the next keyframe (step).
    Hold,
    /// Linearly ramp to the next keyframe's value.
    #[default]
    Linear,
    /// Smoothstep ease in/out toward the next keyframe's value.
    Smooth,
    /// Quadratic ease-in (slow start).
    EaseInQuad,
    /// Quadratic ease-out (slow end).
    EaseOutQuad,
    /// Cubic ease-in (slower start).
    EaseInCubic,
    /// Cubic ease-out (slower end).
    EaseOutCubic,
}

/// A single control point: a value at a time, plus how it transitions onward.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    /// Time of the keyframe in milliseconds.
    pub time_ms: u64,
    /// Parameter value at this keyframe.
    pub value: f32,
    /// Interpolation from this keyframe to the next.
    #[serde(default)]
    pub interpolation: Interpolation,
}

/// A keyframed automation curve for one scalar parameter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Automation {
    keyframes: Vec<Keyframe>,
    default: f32,
}

impl Automation {
    /// An empty curve that samples `default` everywhere.
    pub fn constant(default: f32) -> Self {
        Self {
            keyframes: Vec::new(),
            default,
        }
    }

    /// Add a keyframe, keeping the curve ordered by time. A keyframe at an existing
    /// time replaces the previous one there.
    pub fn add(&mut self, keyframe: Keyframe) {
        match self
            .keyframes
            .binary_search_by_key(&keyframe.time_ms, |existing| existing.time_ms)
        {
            Ok(index) => self.keyframes[index] = keyframe,
            Err(index) => self.keyframes.insert(index, keyframe),
        }
    }

    /// The keyframes in time order.
    pub fn keyframes(&self) -> &[Keyframe] {
        &self.keyframes
    }

    /// Sample the curve at `time_ms`. With no keyframes this is the default value; before
    /// the first / after the last keyframe it holds that endpoint.
    pub fn sample(&self, time_ms: u64) -> f32 {
        if self.keyframes.is_empty() {
            return self.default;
        }
        let first = &self.keyframes[0];
        if time_ms <= first.time_ms {
            return first.value;
        }
        let last = &self.keyframes[self.keyframes.len() - 1];
        if time_ms >= last.time_ms {
            return last.value;
        }

        let upper = self
            .keyframes
            .partition_point(|keyframe| keyframe.time_ms <= time_ms);
        let start = &self.keyframes[upper - 1];
        let end = &self.keyframes[upper];

        let span = (end.time_ms - start.time_ms) as f32;
        let progress = if span <= 0.0 {
            0.0
        } else {
            (time_ms - start.time_ms) as f32 / span
        };
        let eased = match start.interpolation {
            Interpolation::Hold => return start.value,
            Interpolation::Linear => progress,
            Interpolation::Smooth => progress * progress * (3.0 - 2.0 * progress),
            Interpolation::EaseInQuad => progress * progress,
            Interpolation::EaseOutQuad => progress * (2.0 - progress),
            Interpolation::EaseInCubic => progress * progress * progress,
            Interpolation::EaseOutCubic => 1.0 - (1.0 - progress).powi(3),
        };
        start.value + (end.value - start.value) * eased
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyframe(time_ms: u64, value: f32, interpolation: Interpolation) -> Keyframe {
        Keyframe {
            time_ms,
            value,
            interpolation,
        }
    }

    #[test]
    fn empty_curve_returns_default() {
        assert_eq!(Automation::constant(0.7).sample(1234), 0.7);
    }

    #[test]
    fn single_keyframe_holds_everywhere() {
        let mut curve = Automation::constant(0.0);
        curve.add(keyframe(100, 0.5, Interpolation::Linear));
        assert_eq!(curve.sample(0), 0.5);
        assert_eq!(curve.sample(100), 0.5);
        assert_eq!(curve.sample(9999), 0.5);
    }

    #[test]
    fn linear_interpolates_between_keyframes() {
        let mut curve = Automation::constant(0.0);
        curve.add(keyframe(0, 0.0, Interpolation::Linear));
        curve.add(keyframe(100, 1.0, Interpolation::Linear));
        assert!((curve.sample(50) - 0.5).abs() < 1e-6);
        assert!((curve.sample(25) - 0.25).abs() < 1e-6);
        // Endpoints clamp.
        assert_eq!(curve.sample(0), 0.0);
        assert_eq!(curve.sample(100), 1.0);
    }

    #[test]
    fn hold_steps_at_the_next_keyframe() {
        let mut curve = Automation::constant(0.0);
        curve.add(keyframe(0, 0.2, Interpolation::Hold));
        curve.add(keyframe(100, 0.9, Interpolation::Linear));
        assert_eq!(curve.sample(50), 0.2);
        assert_eq!(curve.sample(99), 0.2);
        assert_eq!(curve.sample(100), 0.9);
    }

    #[test]
    fn smooth_matches_linear_at_midpoint_but_eases_at_quarter() {
        let mut curve = Automation::constant(0.0);
        curve.add(keyframe(0, 0.0, Interpolation::Smooth));
        curve.add(keyframe(100, 1.0, Interpolation::Linear));
        assert!((curve.sample(50) - 0.5).abs() < 1e-6);
        // Smoothstep(0.25) = 0.15625, below the linear 0.25.
        assert!((curve.sample(25) - 0.15625).abs() < 1e-5);
    }

    #[test]
    fn add_keeps_order_and_replaces_duplicates() {
        let mut curve = Automation::constant(0.0);
        curve.add(keyframe(100, 0.1, Interpolation::Linear));
        curve.add(keyframe(0, 0.0, Interpolation::Linear));
        curve.add(keyframe(50, 0.5, Interpolation::Linear));
        let times: Vec<u64> = curve.keyframes().iter().map(|k| k.time_ms).collect();
        assert_eq!(times, vec![0, 50, 100]);
        // Replace the keyframe at 50.
        curve.add(keyframe(50, 0.9, Interpolation::Hold));
        assert_eq!(curve.keyframes().len(), 3);
        assert_eq!(curve.sample(50), 0.9);
    }

    #[test]
    fn easing_curves_have_exact_endpoints_and_known_midpoints() {
        let curve = |interpolation| {
            let mut c = Automation::constant(0.0);
            c.add(keyframe(0, 0.0, interpolation));
            c.add(keyframe(100, 1.0, Interpolation::Linear));
            c
        };
        let eases = [
            Interpolation::EaseInQuad,
            Interpolation::EaseOutQuad,
            Interpolation::EaseInCubic,
            Interpolation::EaseOutCubic,
        ];
        for ease in eases {
            let c = curve(ease);
            assert_eq!(c.sample(0), 0.0, "{ease:?} at 0");
            assert_eq!(c.sample(100), 1.0, "{ease:?} at 1");
        }
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(close(curve(Interpolation::EaseInQuad).sample(50), 0.25)); // 0.5^2
        assert!(close(curve(Interpolation::EaseOutQuad).sample(50), 0.75)); // 0.5*(2-0.5)
        assert!(close(curve(Interpolation::EaseInCubic).sample(50), 0.125)); // 0.5^3
        assert!(close(curve(Interpolation::EaseOutCubic).sample(50), 0.875)); // 1-0.5^3
    }
}
