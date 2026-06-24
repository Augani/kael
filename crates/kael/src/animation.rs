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
///
/// This is the canonical easing vocabulary for the workspace. The `quad`
/// variants ([`Easing::EaseIn`], [`Easing::EaseOut`], [`Easing::EaseInOut`])
/// are quadratic; the cubic/quart/quint/expo/circ/back/elastic variants extend
/// the set with the standard CSS-style curves.
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
    /// A cubic ease-in curve.
    EaseInCubic,
    /// A cubic ease-out curve (natural-feeling deceleration).
    EaseOutCubic,
    /// A cubic ease-in-out curve.
    EaseInOutCubic,
    /// A quartic ease-in curve.
    EaseInQuart,
    /// A quartic ease-out curve.
    EaseOutQuart,
    /// A quartic ease-in-out curve.
    EaseInOutQuart,
    /// A quintic ease-in curve.
    EaseInQuint,
    /// A quintic ease-out curve.
    EaseOutQuint,
    /// A quintic ease-in-out curve.
    EaseInOutQuint,
    /// An exponential ease-in curve.
    EaseInExpo,
    /// An exponential ease-out curve.
    EaseOutExpo,
    /// An exponential ease-in-out curve.
    EaseInOutExpo,
    /// A circular ease-in curve.
    EaseInCirc,
    /// A circular ease-out curve.
    EaseOutCirc,
    /// A circular ease-in-out curve.
    EaseInOutCirc,
    /// A backing ease-in curve with the given overshoot constant.
    EaseInBack(f32),
    /// A backing ease-out curve with the given overshoot constant.
    EaseOutBack(f32),
    /// A backing ease-in-out curve with the given overshoot constant.
    EaseInOutBack(f32),
    /// An elastic ease-in curve.
    EaseInElastic,
    /// An elastic ease-out curve.
    EaseOutElastic,
    /// A clamped elastic curve with a single decaying overshoot.
    Elastic,
    /// A stepped curve with the given number of discrete steps.
    Steps(u32),
    /// A cubic Bezier curve with CSS-style control points.
    CubicBezier(f32, f32, f32, f32),
    /// A custom easing callback.
    Custom(Rc<dyn Fn(f32) -> f32>),
}

impl Easing {
    pub(crate) fn sample(&self, delta: f32) -> f32 {
        use std::f32::consts::PI;
        let delta = delta.clamp(0.0, 1.0);

        match self {
            Self::Linear => easing::linear(delta),
            Self::EaseIn => easing::quadratic(delta),
            Self::EaseOut => easing::ease_out(delta),
            Self::EaseInOut => easing::ease_in_out(delta),
            Self::EaseInCubic => delta * delta * delta,
            Self::EaseOutCubic => {
                let t = delta - 1.0;
                t * t * t + 1.0
            }
            Self::EaseInOutCubic => {
                if delta < 0.5 {
                    4.0 * delta * delta * delta
                } else {
                    let t = 2.0 * delta - 2.0;
                    1.0 + t * t * t / 2.0
                }
            }
            Self::EaseInQuart => delta * delta * delta * delta,
            Self::EaseOutQuart => {
                let t = delta - 1.0;
                1.0 - t * t * t * t
            }
            Self::EaseInOutQuart => {
                if delta < 0.5 {
                    8.0 * delta * delta * delta * delta
                } else {
                    let t = delta - 1.0;
                    1.0 - 8.0 * t * t * t * t
                }
            }
            Self::EaseInQuint => delta * delta * delta * delta * delta,
            Self::EaseOutQuint => {
                let t = delta - 1.0;
                1.0 + t * t * t * t * t
            }
            Self::EaseInOutQuint => {
                if delta < 0.5 {
                    16.0 * delta * delta * delta * delta * delta
                } else {
                    let t = 2.0 * delta - 2.0;
                    1.0 + t * t * t * t * t / 2.0
                }
            }
            Self::EaseInExpo => {
                if delta == 0.0 {
                    0.0
                } else {
                    2_f32.powf(10.0 * delta - 10.0)
                }
            }
            Self::EaseOutExpo => {
                if delta >= 1.0 {
                    1.0
                } else {
                    1.0 - 2_f32.powf(-10.0 * delta)
                }
            }
            Self::EaseInOutExpo => {
                if delta == 0.0 {
                    0.0
                } else if delta >= 1.0 {
                    1.0
                } else if delta < 0.5 {
                    2_f32.powf(20.0 * delta - 10.0) / 2.0
                } else {
                    (2.0 - 2_f32.powf(-20.0 * delta + 10.0)) / 2.0
                }
            }
            Self::EaseInCirc => 1.0 - (1.0 - delta * delta).sqrt(),
            Self::EaseOutCirc => {
                let t = delta - 1.0;
                (1.0 - t * t).sqrt()
            }
            Self::EaseInOutCirc => {
                if delta < 0.5 {
                    (1.0 - (1.0 - (2.0 * delta).powi(2)).sqrt()) / 2.0
                } else {
                    ((1.0 - (-2.0 * delta + 2.0).powi(2)).sqrt() + 1.0) / 2.0
                }
            }
            Self::EaseInBack(overshoot) => {
                let c1 = *overshoot;
                let c3 = c1 + 1.0;
                (c3 * delta * delta * delta - c1 * delta * delta).max(0.0)
            }
            Self::EaseOutBack(overshoot) => {
                if delta >= 1.0 {
                    return 1.0;
                }
                let c1 = *overshoot;
                let c3 = c1 + 1.0;
                let t = delta - 1.0;
                (1.0 + c3 * t * t * t + c1 * t * t).clamp(0.0, 1.0)
            }
            Self::EaseInOutBack(overshoot) => {
                let c1 = *overshoot;
                let c2 = c1 * 1.525;
                if delta < 0.5 {
                    ((2.0 * delta).powi(2) * ((c2 + 1.0) * 2.0 * delta - c2)) / 2.0
                } else {
                    (((2.0 * delta - 2.0).powi(2) * ((c2 + 1.0) * (delta * 2.0 - 2.0) + c2) + 2.0)
                        / 2.0)
                        .clamp(0.0, 1.0)
                }
            }
            Self::EaseInElastic => {
                if delta == 0.0 {
                    return 0.0;
                }
                if delta >= 1.0 {
                    return 1.0;
                }
                let c4 = (2.0 * PI) / 3.0;
                (-(2_f32.powf(10.0 * delta - 10.0) * ((delta * 10.0 - 10.75) * c4).sin()))
                    .clamp(0.0, 1.0)
            }
            Self::EaseOutElastic => {
                if delta == 0.0 {
                    return 0.0;
                }
                if delta >= 1.0 {
                    return 1.0;
                }
                let c4 = (2.0 * PI) / 3.0;
                (2_f32.powf(-10.0 * delta) * ((delta * 10.0 - 0.75) * c4).sin() + 1.0)
                    .clamp(0.0, 1.0)
            }
            Self::Elastic => {
                if delta == 0.0 {
                    return 0.0;
                }
                if delta >= 1.0 {
                    return 1.0;
                }
                let p = 0.3;
                let s = p / 4.0;
                let t = delta - 1.0;
                (1.0 + (2_f32.powf(10.0 * t)) * ((t - s) * (2.0 * PI) / p).sin()).clamp(0.0, 1.0)
            }
            Self::Steps(count) => {
                let n = (*count).max(1) as f32;
                (delta * n).floor() / n
            }
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, delta),
            Self::Custom(callback) => callback(delta).clamp(0.0, 1.0),
        }
    }

    /// Sample this easing at `delta` in `0..=1` (public for media keyframes).
    pub fn ease(&self, delta: f32) -> f32 {
        self.sample(delta)
    }
}

/// How to interpolate from one media keyframe toward the next.
#[derive(Clone)]
pub enum KeyframeInterpolation {
    /// Hold the value until the next keyframe (step).
    Hold,
    /// Interpolate with the given easing curve (includes `CubicBezier` handles).
    Eased(Easing),
}

/// A single media keyframe: a value at a time, with the curve used to reach the
/// following keyframe.
#[derive(Clone)]
pub struct MediaKeyframe {
    /// Keyframe time, in seconds.
    pub time: f64,
    /// Keyframe value.
    pub value: f32,
    /// Interpolation toward the following keyframe.
    pub interpolation: KeyframeInterpolation,
}

/// A keyframed scalar track sampled by the render/playback clock.
///
/// Generalizes the UI [`Keyframes`] machinery to media use — transform, opacity,
/// effect parameters, audio automation — reusing [`Easing`] (including
/// `CubicBezier` handles) for interpolation.
#[derive(Clone, Default)]
pub struct KeyframeTrack {
    keys: Vec<MediaKeyframe>,
}

impl KeyframeTrack {
    /// An empty track.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a keyframe (kept sorted by time) and return the track.
    pub fn with_key(mut self, time: f64, value: f32, interpolation: KeyframeInterpolation) -> Self {
        self.insert(MediaKeyframe {
            time,
            value,
            interpolation,
        });
        self
    }

    /// Insert a keyframe, keeping the track sorted by time.
    pub fn insert(&mut self, key: MediaKeyframe) {
        let index = self
            .keys
            .partition_point(|existing| existing.time <= key.time);
        self.keys.insert(index, key);
    }

    /// Number of keyframes.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the track has no keyframes.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Sample the track at `time` (seconds). Returns `default` for an empty
    /// track; clamps to the first/last value outside the keyframe range.
    pub fn sample(&self, time: f64, default: f32) -> f32 {
        let Some(first) = self.keys.first() else {
            return default;
        };
        if time <= first.time {
            return first.value;
        }
        let last = &self.keys[self.keys.len() - 1];
        if time >= last.time {
            return last.value;
        }
        let upper = self.keys.partition_point(|key| key.time <= time);
        let (k0, k1) = (&self.keys[upper - 1], &self.keys[upper]);
        match &k0.interpolation {
            KeyframeInterpolation::Hold => k0.value,
            KeyframeInterpolation::Eased(easing) => {
                let span = (k1.time - k0.time) as f32;
                let t = if span <= f32::EPSILON {
                    0.0
                } else {
                    ((time - k0.time) as f32 / span).clamp(0.0, 1.0)
                };
                k0.value + (k1.value - k0.value) * easing.ease(t)
            }
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
    translate_x: Option<f32>,
    translate_y: Option<f32>,
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

    /// Sets the target translation in logical pixels along both axes.
    pub fn translate(mut self, x: f32, y: f32) -> Self {
        self.translate_x = Some(x);
        self.translate_y = Some(y);
        self
    }

    /// Sets the target horizontal translation in logical pixels.
    pub fn translate_x(mut self, x: f32) -> Self {
        self.translate_x = Some(x);
        self
    }

    /// Sets the target vertical translation in logical pixels.
    pub fn translate_y(mut self, y: f32) -> Self {
        self.translate_y = Some(y);
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
        if let Some(translate_x) = self.translate_x {
            element = element.translate_x(crate::px(translate_x));
        }
        if let Some(translate_y) = self.translate_y {
            element = element.translate_y(crate::px(translate_y));
        }
        element
    }

    fn interpolate(self, other: Self, delta: f32) -> Self {
        Self {
            opacity: interpolate_optional(self.opacity, other.opacity, delta),
            scale_x: interpolate_optional(self.scale_x, other.scale_x, delta),
            scale_y: interpolate_optional(self.scale_y, other.scale_y, delta),
            rotate_degrees: interpolate_optional(self.rotate_degrees, other.rotate_degrees, delta),
            translate_x: interpolate_optional(self.translate_x, other.translate_x, delta),
            translate_y: interpolate_optional(self.translate_y, other.translate_y, delta),
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
mod easing_variant_tests {
    use super::Easing;

    mod oracle {
        use std::f32::consts::PI;

        pub fn ease_in_cubic(t: f32) -> f32 {
            t * t * t
        }
        pub fn ease_out_cubic(t: f32) -> f32 {
            let t = t - 1.0;
            t * t * t + 1.0
        }
        pub fn ease_in_out_cubic(t: f32) -> f32 {
            if t < 0.5 {
                4.0 * t * t * t
            } else {
                let t = 2.0 * t - 2.0;
                1.0 + t * t * t / 2.0
            }
        }
        pub fn ease_in_quart(t: f32) -> f32 {
            t * t * t * t
        }
        pub fn ease_out_quart(t: f32) -> f32 {
            let t = t - 1.0;
            1.0 - t * t * t * t
        }
        pub fn ease_in_out_quart(t: f32) -> f32 {
            if t < 0.5 {
                8.0 * t * t * t * t
            } else {
                let t = t - 1.0;
                1.0 - 8.0 * t * t * t * t
            }
        }
        pub fn ease_in_quint(t: f32) -> f32 {
            t * t * t * t * t
        }
        pub fn ease_out_quint(t: f32) -> f32 {
            let t = t - 1.0;
            1.0 + t * t * t * t * t
        }
        pub fn ease_in_out_quint(t: f32) -> f32 {
            if t < 0.5 {
                16.0 * t * t * t * t * t
            } else {
                let t = 2.0 * t - 2.0;
                1.0 + t * t * t * t * t / 2.0
            }
        }
        pub fn ease_in_expo(t: f32) -> f32 {
            if t == 0.0 {
                0.0
            } else {
                2_f32.powf(10.0 * t - 10.0)
            }
        }
        pub fn ease_out_expo(t: f32) -> f32 {
            if t >= 1.0 {
                1.0
            } else {
                1.0 - 2_f32.powf(-10.0 * t)
            }
        }
        pub fn ease_in_out_expo(t: f32) -> f32 {
            if t == 0.0 {
                0.0
            } else if t >= 1.0 {
                1.0
            } else if t < 0.5 {
                2_f32.powf(20.0 * t - 10.0) / 2.0
            } else {
                (2.0 - 2_f32.powf(-20.0 * t + 10.0)) / 2.0
            }
        }
        pub fn ease_in_circ(t: f32) -> f32 {
            1.0 - (1.0 - t * t).sqrt()
        }
        pub fn ease_out_circ(t: f32) -> f32 {
            let t = t - 1.0;
            (1.0 - t * t).sqrt()
        }
        pub fn ease_in_out_circ(t: f32) -> f32 {
            if t < 0.5 {
                (1.0 - (1.0 - (2.0 * t).powi(2)).sqrt()) / 2.0
            } else {
                ((1.0 - (-2.0 * t + 2.0).powi(2)).sqrt() + 1.0) / 2.0
            }
        }
        pub fn ease_in_back(t: f32) -> f32 {
            let c1 = 1.70158;
            let c3 = c1 + 1.0;
            (c3 * t * t * t - c1 * t * t).max(0.0)
        }
        pub fn ease_out_back(t: f32) -> f32 {
            if t >= 1.0 {
                return 1.0;
            }
            let c1 = 1.2;
            let c3 = c1 + 1.0;
            let t_adj = t - 1.0;
            (1.0 + c3 * t_adj * t_adj * t_adj + c1 * t_adj * t_adj).clamp(0.0, 1.0)
        }
        pub fn ease_in_out_back(t: f32) -> f32 {
            let c1 = 1.70158;
            let c2 = c1 * 1.525;
            if t < 0.5 {
                ((2.0 * t).powi(2) * ((c2 + 1.0) * 2.0 * t - c2)) / 2.0
            } else {
                (((2.0 * t - 2.0).powi(2) * ((c2 + 1.0) * (t * 2.0 - 2.0) + c2) + 2.0) / 2.0)
                    .clamp(0.0, 1.0)
            }
        }
        pub fn ease_in_elastic(t: f32) -> f32 {
            if t == 0.0 {
                return 0.0;
            }
            if t >= 1.0 {
                return 1.0;
            }
            let c4 = (2.0 * PI) / 3.0;
            (-(2_f32.powf(10.0 * t - 10.0) * ((t * 10.0 - 10.75) * c4).sin())).clamp(0.0, 1.0)
        }
        pub fn ease_out_elastic(t: f32) -> f32 {
            if t == 0.0 {
                return 0.0;
            }
            if t >= 1.0 {
                return 1.0;
            }
            let c4 = (2.0 * PI) / 3.0;
            (2_f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0).clamp(0.0, 1.0)
        }
        pub fn elastic(t: f32) -> f32 {
            if t == 0.0 {
                return 0.0;
            }
            if t >= 1.0 {
                return 1.0;
            }
            let p = 0.3;
            let s = p / 4.0;
            let t_adj = t - 1.0;
            (1.0 + (2_f32.powf(10.0 * t_adj)) * ((t_adj - s) * (2.0 * PI) / p).sin())
                .clamp(0.0, 1.0)
        }
        pub fn steps(n: u32, t: f32) -> f32 {
            let n = n.max(1) as f32;
            (t * n).floor() / n
        }
    }

    const SAMPLES: [f32; 3] = [0.0, 0.5, 1.0];

    fn assert_matches(easing: &Easing, oracle: impl Fn(f32) -> f32) {
        for delta in SAMPLES {
            assert_eq!(easing.sample(delta), oracle(delta), "at delta={delta}");
        }
    }

    #[test]
    fn cubic_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInCubic, oracle::ease_in_cubic);
        assert_matches(&Easing::EaseOutCubic, oracle::ease_out_cubic);
        assert_matches(&Easing::EaseInOutCubic, oracle::ease_in_out_cubic);
    }

    #[test]
    fn quart_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInQuart, oracle::ease_in_quart);
        assert_matches(&Easing::EaseOutQuart, oracle::ease_out_quart);
        assert_matches(&Easing::EaseInOutQuart, oracle::ease_in_out_quart);
    }

    #[test]
    fn quint_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInQuint, oracle::ease_in_quint);
        assert_matches(&Easing::EaseOutQuint, oracle::ease_out_quint);
        assert_matches(&Easing::EaseInOutQuint, oracle::ease_in_out_quint);
    }

    #[test]
    fn expo_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInExpo, oracle::ease_in_expo);
        assert_matches(&Easing::EaseOutExpo, oracle::ease_out_expo);
        assert_matches(&Easing::EaseInOutExpo, oracle::ease_in_out_expo);
    }

    #[test]
    fn circ_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInCirc, oracle::ease_in_circ);
        assert_matches(&Easing::EaseOutCirc, oracle::ease_out_circ);
        assert_matches(&Easing::EaseInOutCirc, oracle::ease_in_out_circ);
    }

    #[test]
    fn back_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInBack(1.70158), oracle::ease_in_back);
        assert_matches(&Easing::EaseOutBack(1.2), oracle::ease_out_back);
        assert_matches(&Easing::EaseInOutBack(1.70158), oracle::ease_in_out_back);
    }

    #[test]
    fn elastic_variants_match_relocated_math() {
        assert_matches(&Easing::EaseInElastic, oracle::ease_in_elastic);
        assert_matches(&Easing::EaseOutElastic, oracle::ease_out_elastic);
        assert_matches(&Easing::Elastic, oracle::elastic);
    }

    #[test]
    fn steps_variant_matches_relocated_math() {
        assert_matches(&Easing::Steps(4), |delta| oracle::steps(4, delta));
        assert_matches(&Easing::Steps(1), |delta| oracle::steps(1, delta));
    }

    #[test]
    fn quad_aliases_remain_quadratic() {
        assert_eq!(Easing::EaseIn.sample(0.5), 0.25);
        assert_eq!(Easing::EaseOut.sample(0.5), 0.75);
    }
}

#[cfg(test)]
mod tests {
    use super::{keyframes, Animation, AnimationSequence, Repeat};
    use crate::animation::StyledKeyframe;
    use std::time::Duration;

    #[test]
    fn keyframe_translate_interpolates() {
        let frames = keyframes()
            .at(0.0, |frame| frame.translate(0.0, 0.0))
            .at(1.0, |frame| frame.translate(100.0, 40.0));

        let midpoint = frames.sample(0.5);
        assert_eq!(midpoint.translate_x, Some(50.0));
        assert_eq!(midpoint.translate_y, Some(20.0));
    }

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

#[cfg(test)]
mod media_keyframe_tests {
    use super::*;

    #[test]
    fn empty_track_returns_default() {
        let track = KeyframeTrack::new();
        assert_eq!(track.sample(1.0, 42.0), 42.0);
        assert!(track.is_empty());
    }

    #[test]
    fn single_key_is_constant() {
        let track = KeyframeTrack::new().with_key(1.0, 5.0, KeyframeInterpolation::Hold);
        assert_eq!(track.sample(0.0, 0.0), 5.0);
        assert_eq!(track.sample(2.0, 0.0), 5.0);
    }

    #[test]
    fn linear_interpolates_between_keys() {
        let track = KeyframeTrack::new()
            .with_key(0.0, 0.0, KeyframeInterpolation::Eased(Easing::Linear))
            .with_key(1.0, 10.0, KeyframeInterpolation::Hold);
        assert!((track.sample(0.5, 0.0) - 5.0).abs() < 1e-5);
        assert!((track.sample(0.25, 0.0) - 2.5).abs() < 1e-5);
    }

    #[test]
    fn hold_steps_until_next_key() {
        let track = KeyframeTrack::new()
            .with_key(0.0, 1.0, KeyframeInterpolation::Hold)
            .with_key(1.0, 9.0, KeyframeInterpolation::Hold);
        assert_eq!(track.sample(0.5, 0.0), 1.0);
        assert_eq!(track.sample(0.99, 0.0), 1.0);
        assert_eq!(track.sample(1.0, 0.0), 9.0);
    }

    #[test]
    fn easing_differs_from_linear() {
        let eased = KeyframeTrack::new()
            .with_key(0.0, 0.0, KeyframeInterpolation::Eased(Easing::EaseIn))
            .with_key(1.0, 10.0, KeyframeInterpolation::Hold);
        assert!(eased.sample(0.5, 0.0) < 5.0);
    }

    #[test]
    fn clamps_outside_range() {
        let track = KeyframeTrack::new()
            .with_key(1.0, 2.0, KeyframeInterpolation::Eased(Easing::Linear))
            .with_key(3.0, 8.0, KeyframeInterpolation::Hold);
        assert_eq!(track.sample(-5.0, 0.0), 2.0);
        assert_eq!(track.sample(100.0, 0.0), 8.0);
    }

    #[test]
    fn out_of_order_insertion_stays_sorted() {
        let mut track = KeyframeTrack::new();
        track.insert(MediaKeyframe {
            time: 2.0,
            value: 20.0,
            interpolation: KeyframeInterpolation::Hold,
        });
        track.insert(MediaKeyframe {
            time: 0.0,
            value: 0.0,
            interpolation: KeyframeInterpolation::Eased(Easing::Linear),
        });
        track.insert(MediaKeyframe {
            time: 1.0,
            value: 10.0,
            interpolation: KeyframeInterpolation::Eased(Easing::Linear),
        });
        assert!((track.sample(0.5, 0.0) - 5.0).abs() < 1e-5);
        assert!((track.sample(1.5, 0.0) - 15.0).abs() < 1e-5);
        assert_eq!(track.len(), 3);
    }
}
