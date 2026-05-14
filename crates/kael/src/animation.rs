use std::{rc::Rc, time::Duration};

use crate::Styled;

/// An animation that can be applied to an element.
#[derive(Clone)]
pub struct Animation {
    duration: Duration,
    easing: Easing,
    delay: Duration,
    repeat: Repeat,
}

impl Animation {
    /// Creates a new animation with the given duration.
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            easing: Easing::Linear,
            delay: Duration::ZERO,
            repeat: Repeat::Once,
        }
    }

    /// Sets the easing used by this animation.
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Sets a custom easing function for this animation.
    pub fn with_easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self {
        self.easing = Easing::Custom(Rc::new(easing));
        self
    }

    /// Delays this animation relative to the start of its sequence.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Sets the repeat behavior for this animation.
    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// Repeats this animation forever.
    pub fn repeat_forever(self) -> Self {
        self.repeat(Repeat::Forever)
    }

    pub(crate) fn sample(&self, elapsed: Duration) -> AnimationSample {
        if elapsed < self.delay {
            return AnimationSample {
                delta: 0.0,
                started: false,
                finished: false,
            };
        }

        if self.duration.is_zero() {
            return AnimationSample {
                delta: 1.0,
                started: true,
                finished: self.repeat != Repeat::Forever,
            };
        }

        let local_elapsed = elapsed - self.delay;
        let local_seconds = local_elapsed.as_secs_f32();
        let duration_seconds = self.duration.as_secs_f32();

        match self.repeat {
            Repeat::Once => {
                let raw_delta = (local_seconds / duration_seconds).clamp(0.0, 1.0);
                AnimationSample {
                    delta: self.easing.sample(raw_delta),
                    started: true,
                    finished: raw_delta >= 1.0,
                }
            }
            Repeat::Count(count) => {
                let cycle_count = count.max(1);
                let total_seconds = duration_seconds * cycle_count as f32;
                if local_seconds >= total_seconds {
                    AnimationSample {
                        delta: 1.0,
                        started: true,
                        finished: true,
                    }
                } else {
                    AnimationSample {
                        delta: self
                            .easing
                            .sample((local_seconds / duration_seconds).fract()),
                        started: true,
                        finished: false,
                    }
                }
            }
            Repeat::Forever => AnimationSample {
                delta: self
                    .easing
                    .sample((local_seconds / duration_seconds).fract()),
                started: true,
                finished: false,
            },
        }
    }

    pub(crate) fn scheduled_end(&self) -> Duration {
        let active_duration = match self.repeat {
            Repeat::Once => self.duration,
            Repeat::Count(count) => self.duration.saturating_mul(count.max(1)),
            Repeat::Forever => self.duration,
        };

        self.delay + active_duration
    }
}

/// Supported easing curves for explicit animations.
#[derive(Clone)]
pub enum Easing {
    /// A linear curve.
    Linear,
    /// A quadratic ease-in curve.
    EaseIn,
    /// A quadratic ease-out curve.
    EaseOut,
    /// A quadratic ease-in-out curve.
    EaseInOut,
    /// A cubic Bezier curve with CSS-style control points.
    CubicBezier(f32, f32, f32, f32),
    /// A damped spring curve.
    Spring {
        /// Spring stiffness.
        stiffness: f32,
        /// Damping factor.
        damping: f32,
        /// Effective mass.
        mass: f32,
    },
    /// A custom easing callback.
    Custom(Rc<dyn Fn(f32) -> f32>),
}

impl Easing {
    pub(crate) fn sample(&self, delta: f32) -> f32 {
        let delta = delta.clamp(0.0, 1.0);

        match self {
            Self::Linear => easing::linear(delta),
            Self::EaseIn => easing::quadratic(delta),
            Self::EaseOut => easing::ease_out(delta),
            Self::EaseInOut => easing::ease_in_out(delta),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, delta),
            Self::Spring {
                stiffness,
                damping,
                mass,
            } => spring(*stiffness, *damping, *mass, delta),
            Self::Custom(callback) => callback(delta).clamp(0.0, 1.0),
        }
    }
}

/// Repeat behavior for explicit animations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeat {
    /// Play the animation once.
    Once,
    /// Play the animation a fixed number of times.
    Count(u32),
    /// Repeat the animation indefinitely.
    Forever,
}

/// A sequence of animations that can overlap with the previous step.
#[derive(Clone, Default)]
pub struct AnimationSequence {
    animations: Vec<Animation>,
}

impl AnimationSequence {
    /// Creates an empty animation sequence.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an animation after the current sequence tail.
    pub fn then(mut self, animation: Animation) -> Self {
        let start = self
            .animations
            .iter()
            .map(Animation::scheduled_end)
            .max()
            .unwrap_or(Duration::ZERO);
        let delay = animation.delay;
        self.animations.push(animation.delay(start + delay));
        self
    }

    /// Appends a new animation with the given duration.
    pub fn then_for(self, duration: Duration) -> Self {
        self.then(Animation::new(duration))
    }

    /// Starts the most recently-added animation earlier so it overlaps the previous one.
    pub fn with_overlap(mut self, overlap: Duration) -> Self {
        if let Some(last) = self.animations.last_mut() {
            last.delay = last.delay.saturating_sub(overlap);
        }
        self
    }

    /// Consumes the sequence into its scheduled animations.
    pub fn into_animations(self) -> Vec<Animation> {
        self.animations
    }

    /// Returns the scheduled animations in this sequence.
    pub fn animations(&self) -> &[Animation] {
        &self.animations
    }
}

/// A set of keyframes that target common styled element properties.
#[derive(Clone, Default)]
pub struct Keyframes {
    frames: Vec<Keyframe>,
}

impl Keyframes {
    /// Creates an empty keyframe set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a keyframe at the given normalized time.
    pub fn at(
        mut self,
        progress: f32,
        build: impl FnOnce(StyledKeyframe) -> StyledKeyframe,
    ) -> Self {
        self.frames.push(Keyframe {
            progress: progress.clamp(0.0, 1.0),
            style: build(StyledKeyframe::default()),
        });
        self.frames
            .sort_by(|left, right| left.progress.total_cmp(&right.progress));
        self
    }

    pub(crate) fn sample(&self, progress: f32) -> StyledKeyframe {
        let progress = progress.clamp(0.0, 1.0);
        let Some(first) = self.frames.first() else {
            return StyledKeyframe::default();
        };

        if progress <= first.progress {
            return first.style;
        }

        for window in self.frames.windows(2) {
            let start = &window[0];
            let end = &window[1];
            if progress <= end.progress {
                let segment_delta = if (end.progress - start.progress).abs() <= f32::EPSILON {
                    1.0
                } else {
                    (progress - start.progress) / (end.progress - start.progress)
                };
                return start.style.interpolate(end.style, segment_delta);
            }
        }

        self.frames
            .last()
            .map(|frame| frame.style)
            .unwrap_or_default()
    }

    pub(crate) fn apply<E: Styled>(&self, element: E, progress: f32) -> E {
        self.sample(progress).apply(element)
    }
}

/// Creates a keyframe builder for explicit styled animations.
pub fn keyframes() -> Keyframes {
    Keyframes::new()
}

#[derive(Clone, Copy)]
struct Keyframe {
    progress: f32,
    style: StyledKeyframe,
}

/// A single styled keyframe.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StyledKeyframe {
    opacity: Option<f32>,
    scale_x: Option<f32>,
    scale_y: Option<f32>,
    rotate_degrees: Option<f32>,
}

impl StyledKeyframe {
    /// Sets the target opacity.
    pub fn opacity(mut self, opacity: f32) -> Self {
        self.opacity = Some(opacity);
        self
    }

    /// Sets a uniform target scale.
    pub fn scale(mut self, factor: f32) -> Self {
        self.scale_x = Some(factor);
        self.scale_y = Some(factor);
        self
    }

    /// Sets a non-uniform target scale.
    pub fn scale_xy(mut self, x: f32, y: f32) -> Self {
        self.scale_x = Some(x);
        self.scale_y = Some(y);
        self
    }

    /// Sets the target rotation in degrees.
    pub fn rotate(mut self, degrees: f32) -> Self {
        self.rotate_degrees = Some(degrees);
        self
    }

    /// Applies this keyframe to a styled element.
    pub fn apply<E: Styled>(self, mut element: E) -> E {
        if let Some(opacity) = self.opacity {
            element = element.opacity(opacity);
        }
        if let (Some(scale_x), Some(scale_y)) = (self.scale_x, self.scale_y) {
            element = element.scale_xy(scale_x, scale_y);
        }
        if let Some(rotate_degrees) = self.rotate_degrees {
            element = element.rotate(rotate_degrees);
        }
        element
    }

    fn interpolate(self, other: Self, delta: f32) -> Self {
        Self {
            opacity: interpolate_optional(self.opacity, other.opacity, delta),
            scale_x: interpolate_optional(self.scale_x, other.scale_x, delta),
            scale_y: interpolate_optional(self.scale_y, other.scale_y, delta),
            rotate_degrees: interpolate_optional(self.rotate_degrees, other.rotate_degrees, delta),
        }
    }
}

pub(crate) struct AnimationSample {
    pub delta: f32,
    pub started: bool,
    pub finished: bool,
}

fn interpolate_optional(start: Option<f32>, end: Option<f32>, delta: f32) -> Option<f32> {
    match (start, end) {
        (Some(start), Some(end)) => Some(start + (end - start) * delta),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, delta: f32) -> f32 {
    let mut low = 0.0;
    let mut high = 1.0;
    let mut t = delta;

    for _ in 0..12 {
        let x = cubic_bezier_axis(x1, x2, t);
        if x < delta {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.0;
    }

    cubic_bezier_axis(y1, y2, t).clamp(0.0, 1.0)
}

fn cubic_bezier_axis(p1: f32, p2: f32, t: f32) -> f32 {
    let inverse_t = 1.0 - t;
    3.0 * inverse_t * inverse_t * t * p1 + 3.0 * inverse_t * t * t * p2 + t * t * t
}

fn spring(stiffness: f32, damping: f32, mass: f32, delta: f32) -> f32 {
    let stiffness = stiffness.max(f32::EPSILON);
    let damping = damping.max(0.0);
    let mass = mass.max(f32::EPSILON);
    let angular_frequency = (stiffness / mass).sqrt();
    let decay = (-damping * delta).exp();
    (1.0 - decay * (angular_frequency * delta).cos()).clamp(0.0, 1.0)
}

/// Common easing helpers.
pub mod easing {
    use std::f32::consts::PI;

    /// Returns the input unchanged.
    pub fn linear(delta: f32) -> f32 {
        delta
    }

    /// Applies a quadratic ease-in curve.
    pub fn quadratic(delta: f32) -> f32 {
        delta * delta
    }

    /// Applies a quadratic ease-out curve.
    pub fn ease_out(delta: f32) -> f32 {
        1.0 - (1.0 - delta).powi(2)
    }

    /// Applies a quadratic ease-in-out curve.
    pub fn ease_in_out(delta: f32) -> f32 {
        if delta < 0.5 {
            2.0 * delta * delta
        } else {
            let x = -2.0 * delta + 2.0;
            1.0 - x * x / 2.0
        }
    }

    /// Applies a quintic ease-out curve.
    pub fn ease_out_quint() -> impl Fn(f32) -> f32 {
        move |delta| 1.0 - (1.0 - delta).powi(5)
    }

    /// Plays the provided easing forward and then backward.
    pub fn bounce(easing: impl Fn(f32) -> f32) -> impl Fn(f32) -> f32 {
        move |delta| {
            if delta < 0.5 {
                easing(delta * 2.0)
            } else {
                easing((1.0 - delta) * 2.0)
            }
        }
    }

    /// Produces a soft pulsing alpha curve between two values.
    pub fn pulsating_between(min: f32, max: f32) -> impl Fn(f32) -> f32 {
        let range = max - min;

        move |delta| {
            let t = (delta * 2.0 * PI).sin();
            let breath = (t * t * t + t) / 2.0;
            let normalized_alpha = (breath + 1.0) / 2.0;
            min + normalized_alpha * range
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Animation, AnimationSequence, Repeat, keyframes};
    use crate::animation::StyledKeyframe;
    use std::time::Duration;

    #[test]
    fn animation_sequence_offsets_the_next_step() {
        let sequence = AnimationSequence::new()
            .then(Animation::new(Duration::from_millis(200)))
            .then(Animation::new(Duration::from_millis(300)))
            .with_overlap(Duration::from_millis(100));

        let animations = sequence.into_animations();
        assert_eq!(animations.len(), 2);
        assert_eq!(animations[0].scheduled_end(), Duration::from_millis(200));
        assert_eq!(animations[1].scheduled_end(), Duration::from_millis(400));
    }

    #[test]
    fn keyframes_interpolate_between_styles() {
        let frames = keyframes()
            .at(0.0, |frame| frame.scale(1.0).opacity(1.0))
            .at(1.0, |frame| frame.scale(1.2).opacity(0.5));

        let sample = frames.sample(0.5);
        assert_eq!(sample, StyledKeyframe::default().scale(1.1).opacity(0.75));
    }

    #[test]
    fn counted_animations_finish_after_the_requested_cycles() {
        let animation = Animation::new(Duration::from_millis(100)).repeat(Repeat::Count(2));

        assert!(!animation.sample(Duration::from_millis(150)).finished);
        assert!(animation.sample(Duration::from_millis(250)).finished);
    }
}
