use std::{collections::VecDeque, time::Instant};

use crate::{
    MagnifyEvent, Modifiers, MouseButton, Pixels, PlatformInput, Point, ScrollDelta,
    ScrollWheelEvent, TouchPhase, point, px,
};

const MAX_VELOCITY_SAMPLES: usize = 20;
const DEFAULT_PAN_THRESHOLD: f32 = 2.0;
const DEFAULT_SWIPE_THRESHOLD: f32 = 48.0;
const DEFAULT_SWIPE_VELOCITY_THRESHOLD: f32 = 400.0;
const PINCH_LINE_DELTA_SCALE: f32 = 0.075;
const PINCH_PIXEL_DELTA_SCALE: f32 = 1.0 / 240.0;

/// A stateful recognizer that emits higher-level gesture events from platform input.
pub trait GestureRecognizer {
    /// The gesture event emitted by this recognizer.
    type Event;

    /// Feed a platform input event into the recognizer.
    fn on_event(&mut self, event: &PlatformInput) -> Option<Self::Event>;

    /// Reset any in-flight gesture state.
    fn reset(&mut self);
}

/// Tracks recent positions and estimates a velocity in pixels per second.
#[derive(Clone, Debug, Default)]
pub struct VelocityTracker {
    samples: VecDeque<(Instant, Point<Pixels>)>,
}

impl VelocityTracker {
    /// Add a sample using the current instant.
    pub fn add_sample(&mut self, position: Point<Pixels>) {
        self.add_sample_at(Instant::now(), position);
    }

    /// Estimate the current velocity in pixels per second.
    pub fn velocity(&self) -> Point<Pixels> {
        if self.samples.len() < 2 {
            return Point::default();
        }

        let origin = self.samples.front().map(|(time, _)| *time).unwrap();
        let sample_count = self.samples.len() as f32;

        let mut sum_t = 0.0;
        let mut sum_tt = 0.0;
        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut sum_tx = 0.0;
        let mut sum_ty = 0.0;

        for (time, position) in &self.samples {
            let seconds = time.duration_since(origin).as_secs_f32();
            let x = f32::from(position.x);
            let y = f32::from(position.y);

            sum_t += seconds;
            sum_tt += seconds * seconds;
            sum_x += x;
            sum_y += y;
            sum_tx += seconds * x;
            sum_ty += seconds * y;
        }

        let denominator = sample_count * sum_tt - sum_t * sum_t;
        if denominator.abs() <= f32::EPSILON {
            return self
                .samples
                .back()
                .map_or(Point::default(), |(time, position)| {
                    let (first_time, first_position) = self.samples.front().unwrap();
                    let elapsed = time.duration_since(*first_time).as_secs_f32();
                    if elapsed <= f32::EPSILON {
                        Point::default()
                    } else {
                        point(
                            px((f32::from(position.x) - f32::from(first_position.x)) / elapsed),
                            px((f32::from(position.y) - f32::from(first_position.y)) / elapsed),
                        )
                    }
                });
        }

        point(
            px((sample_count * sum_tx - sum_t * sum_x) / denominator),
            px((sample_count * sum_ty - sum_t * sum_y) / denominator),
        )
    }

    pub(crate) fn add_sample_at(&mut self, time: Instant, position: Point<Pixels>) {
        if self.samples.len() == MAX_VELOCITY_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((time, position));
    }
}

/// The source of a pan gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanSource {
    /// A pointer drag gesture.
    Pointer,
}

/// The lifecycle state of a pan gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PanState {
    /// The gesture just started.
    Began,
    /// The gesture updated with a new translation.
    Changed,
    /// The gesture finished.
    Ended,
}

/// Data produced by a pan gesture recognizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanGestureEvent {
    /// The lifecycle state of this pan event.
    pub state: PanState,
    /// The source of the pan gesture.
    pub source: PanSource,
    /// The position where the pan began.
    pub start_position: Point<Pixels>,
    /// The current pointer position.
    pub position: Point<Pixels>,
    /// Total translation since the gesture began.
    pub translation: Point<Pixels>,
    /// Incremental translation since the previous event.
    pub delta: Point<Pixels>,
    /// Estimated velocity in pixels per second.
    pub velocity: Point<Pixels>,
}

#[derive(Clone, Debug)]
struct ActivePan {
    started: bool,
    start_position: Point<Pixels>,
    last_position: Point<Pixels>,
    velocity_tracker: VelocityTracker,
}

/// Recognizes pointer-driven pan gestures.
#[derive(Clone, Debug)]
pub struct PanGesture {
    minimum_drag_distance: Pixels,
    active: Option<ActivePan>,
}

impl Default for PanGesture {
    fn default() -> Self {
        Self::new()
    }
}

impl PanGesture {
    /// Create a new pan recognizer.
    pub fn new() -> Self {
        Self {
            minimum_drag_distance: px(DEFAULT_PAN_THRESHOLD),
            active: None,
        }
    }

    /// Set the minimum drag distance required before the gesture begins.
    pub fn minimum_drag_distance(mut self, threshold: impl Into<Pixels>) -> Self {
        self.minimum_drag_distance = threshold.into();
        self
    }

    /// Returns whether the recognizer is currently tracking an in-flight gesture.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    fn begin(&mut self, position: Point<Pixels>) {
        let mut velocity_tracker = VelocityTracker::default();
        velocity_tracker.add_sample(position);
        self.active = Some(ActivePan {
            started: false,
            start_position: position,
            last_position: position,
            velocity_tracker,
        });
    }

    fn update(&mut self, position: Point<Pixels>) -> Option<PanGestureEvent> {
        let active = self.active.as_mut()?;
        let translation = position - active.start_position;
        if !active.started && translation.magnitude() < f32::from(self.minimum_drag_distance) as f64
        {
            active.last_position = position;
            active.velocity_tracker.add_sample(position);
            return None;
        }

        let state = if active.started {
            PanState::Changed
        } else {
            active.started = true;
            PanState::Began
        };
        let delta = position - active.last_position;
        active.last_position = position;
        active.velocity_tracker.add_sample(position);

        Some(PanGestureEvent {
            state,
            source: PanSource::Pointer,
            start_position: active.start_position,
            position,
            translation,
            delta,
            velocity: active.velocity_tracker.velocity(),
        })
    }

    fn end(&mut self, position: Point<Pixels>) -> Option<PanGestureEvent> {
        let mut active = self.active.take()?;
        if !active.started {
            return None;
        }

        let delta = position - active.last_position;
        active.velocity_tracker.add_sample(position);
        Some(PanGestureEvent {
            state: PanState::Ended,
            source: PanSource::Pointer,
            start_position: active.start_position,
            position,
            translation: position - active.start_position,
            delta,
            velocity: active.velocity_tracker.velocity(),
        })
    }
}

impl GestureRecognizer for PanGesture {
    type Event = PanGestureEvent;

    fn on_event(&mut self, event: &PlatformInput) -> Option<Self::Event> {
        match event {
            PlatformInput::MouseDown(event) if event.button == MouseButton::Left => {
                self.begin(event.position);
                None
            }
            PlatformInput::MouseMove(event) if event.pressed_button == Some(MouseButton::Left) => {
                self.update(event.position)
            }
            PlatformInput::MouseUp(event) if event.button == MouseButton::Left => {
                self.end(event.position)
            }
            PlatformInput::MouseExited(_) => {
                self.reset();
                None
            }
            _ => None,
        }
    }

    fn reset(&mut self) {
        self.active = None;
    }
}

/// The direction a swipe gesture resolves to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SwipeDirection {
    /// A swipe moving toward the left edge.
    Left,
    /// A swipe moving toward the right edge.
    Right,
    /// A swipe moving upward.
    Up,
    /// A swipe moving downward.
    Down,
}

/// Data produced by a swipe gesture recognizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwipeGestureEvent {
    /// The recognized swipe direction.
    pub direction: SwipeDirection,
    /// Total translation when the swipe resolved.
    pub translation: Point<Pixels>,
    /// Estimated velocity when the swipe resolved.
    pub velocity: Point<Pixels>,
    /// The position where the swipe began.
    pub start_position: Point<Pixels>,
    /// The position where the swipe ended.
    pub position: Point<Pixels>,
}

/// Recognizes directional swipe gestures from pan input.
#[derive(Clone, Debug)]
pub struct SwipeGesture {
    direction: SwipeDirection,
    threshold: Pixels,
    velocity_threshold: f32,
    pan: PanGesture,
}

impl SwipeGesture {
    /// Create a new swipe recognizer for the given direction.
    pub fn new(direction: SwipeDirection) -> Self {
        Self {
            direction,
            threshold: px(DEFAULT_SWIPE_THRESHOLD),
            velocity_threshold: DEFAULT_SWIPE_VELOCITY_THRESHOLD,
            pan: PanGesture::new(),
        }
    }

    /// Set the minimum translation required before the swipe resolves.
    pub fn threshold(mut self, threshold: impl Into<Pixels>) -> Self {
        self.threshold = threshold.into();
        self
    }

    /// Set the minimum axis velocity, in pixels per second, required before the swipe resolves.
    pub fn velocity_threshold(mut self, velocity_threshold: f32) -> Self {
        self.velocity_threshold = velocity_threshold.max(0.0);
        self
    }

    /// Attempt to resolve a swipe from a completed pan gesture.
    pub fn recognize(&self, pan: &PanGestureEvent) -> Option<SwipeGestureEvent> {
        if pan.state != PanState::Ended {
            return None;
        }

        let (translation, cross_axis_translation, velocity) = match self.direction {
            SwipeDirection::Left | SwipeDirection::Right => {
                (pan.translation.x, pan.translation.y.abs(), pan.velocity.x)
            }
            SwipeDirection::Up | SwipeDirection::Down => {
                (pan.translation.y, pan.translation.x.abs(), pan.velocity.y)
            }
        };

        let expected_sign = match self.direction {
            SwipeDirection::Left | SwipeDirection::Up => -1.0,
            SwipeDirection::Right | SwipeDirection::Down => 1.0,
        };
        let signed_translation = f32::from(translation) * expected_sign;
        let signed_velocity = f32::from(velocity) * expected_sign;

        if signed_translation < f32::from(self.threshold) {
            return None;
        }
        if cross_axis_translation > translation.abs() {
            return None;
        }
        if signed_velocity < self.velocity_threshold {
            return None;
        }

        Some(SwipeGestureEvent {
            direction: self.direction,
            translation: pan.translation,
            velocity: pan.velocity,
            start_position: pan.start_position,
            position: pan.position,
        })
    }
}

impl GestureRecognizer for SwipeGesture {
    type Event = SwipeGestureEvent;

    fn on_event(&mut self, event: &PlatformInput) -> Option<Self::Event> {
        self.pan
            .on_event(event)
            .and_then(|pan| self.recognize(&pan))
    }

    fn reset(&mut self) {
        self.pan.reset();
    }
}

/// The source of a pinch gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinchSource {
    /// A native magnification event from the platform.
    Native,
    /// A scroll-wheel zoom gesture such as ctrl-scroll or cmd-scroll.
    ScrollWheel,
}

/// The lifecycle state of a pinch gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PinchState {
    /// The pinch gesture just started.
    Began,
    /// The pinch gesture updated.
    Changed,
    /// The pinch gesture ended.
    Ended,
}

/// Data produced by a pinch gesture recognizer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PinchGestureEvent {
    /// The lifecycle state of this pinch event.
    pub state: PinchState,
    /// The source of the pinch gesture.
    pub source: PinchSource,
    /// The gesture center in window coordinates.
    pub center: Point<Pixels>,
    /// The cumulative scale since the gesture began.
    pub scale: f32,
    /// The incremental scale delta for this event.
    pub delta_scale: f32,
}

#[derive(Clone, Debug)]
struct ActivePinch {
    scale: f32,
}

/// Recognizes pinch-to-zoom gestures from native magnification or zoom-wheel input.
#[derive(Clone, Debug, Default)]
pub struct PinchGesture {
    active: Option<ActivePinch>,
}

impl PinchGesture {
    /// Create a new pinch recognizer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether the recognizer is currently tracking an in-flight gesture.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    fn update(
        &mut self,
        source: PinchSource,
        center: Point<Pixels>,
        delta_scale: f32,
        phase: TouchPhase,
        continuous: bool,
    ) -> Option<PinchGestureEvent> {
        if delta_scale.abs() <= f32::EPSILON {
            if matches!(phase, TouchPhase::Ended) {
                self.reset();
            }
            return None;
        }

        let started = self.active.is_none();
        let mut active = self.active.take().unwrap_or(ActivePinch { scale: 1.0 });
        let next_delta_scale = (1.0 + delta_scale).max(0.01);
        active.scale *= next_delta_scale;

        let state = if !continuous || matches!(phase, TouchPhase::Ended) {
            PinchState::Ended
        } else if started {
            PinchState::Began
        } else {
            PinchState::Changed
        };

        let event = PinchGestureEvent {
            state,
            source,
            center,
            scale: active.scale,
            delta_scale: next_delta_scale,
        };

        if matches!(state, PinchState::Ended) {
            self.active = None;
        } else {
            self.active = Some(active);
        }

        Some(event)
    }

    fn on_magnify(&mut self, event: &MagnifyEvent) -> Option<PinchGestureEvent> {
        self.update(
            PinchSource::Native,
            event.position,
            event.delta,
            event.touch_phase,
            true,
        )
    }

    fn on_scroll_wheel(&mut self, event: &ScrollWheelEvent) -> Option<PinchGestureEvent> {
        if !zoom_modifiers_active(event.modifiers) {
            self.reset();
            return None;
        }

        let (delta_scale, continuous) = match event.delta {
            ScrollDelta::Pixels(delta) => (f32::from(delta.y) * PINCH_PIXEL_DELTA_SCALE, true),
            ScrollDelta::Lines(delta) => (delta.y * PINCH_LINE_DELTA_SCALE, false),
        };

        self.update(
            PinchSource::ScrollWheel,
            event.position,
            delta_scale,
            event.touch_phase,
            continuous,
        )
    }
}

impl GestureRecognizer for PinchGesture {
    type Event = PinchGestureEvent;

    fn on_event(&mut self, event: &PlatformInput) -> Option<Self::Event> {
        match event {
            PlatformInput::Magnify(event) => self.on_magnify(event),
            PlatformInput::ScrollWheel(event) => self.on_scroll_wheel(event),
            _ => None,
        }
    }

    fn reset(&mut self) {
        self.active = None;
    }
}

fn zoom_modifiers_active(modifiers: Modifiers) -> bool {
    modifiers.control || modifiers.platform
}

#[cfg(test)]
mod tests {
    use super::{
        GestureRecognizer, PanGesture, PanState, PinchGesture, PinchState, SwipeDirection,
        SwipeGesture, VelocityTracker,
    };
    use crate::{
        MagnifyEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
        PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, point, px,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn velocity_tracker_estimates_recent_velocity() {
        let mut tracker = VelocityTracker::default();
        let start = Instant::now();
        tracker.add_sample_at(start, point(px(0.0), px(0.0)));
        tracker.add_sample_at(start + Duration::from_millis(50), point(px(10.0), px(0.0)));
        tracker.add_sample_at(start + Duration::from_millis(100), point(px(20.0), px(0.0)));

        let velocity = tracker.velocity();
        assert!(f32::from(velocity.x) > 190.0);
        assert!(f32::from(velocity.x) < 210.0);
        assert!(velocity.y.abs() < px(1.0));
    }

    #[test]
    fn swipe_recognizer_resolves_completed_pan() {
        let mut recognizer = SwipeGesture::new(SwipeDirection::Left);
        let start = point(px(120.0), px(40.0));
        recognizer.on_event(&PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: start,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }));
        recognizer.on_event(&PlatformInput::MouseMove(MouseMoveEvent {
            pressed_button: Some(MouseButton::Left),
            position: point(px(40.0), px(42.0)),
            modifiers: Modifiers::default(),
        }));

        let event = recognizer
            .on_event(&PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: point(px(20.0), px(43.0)),
                modifiers: Modifiers::default(),
                click_count: 1,
            }))
            .expect("expected swipe to resolve");

        assert_eq!(event.direction, SwipeDirection::Left);
    }

    #[test]
    fn pinch_recognizer_handles_native_and_scroll_zoom_input() {
        let mut pinch = PinchGesture::new();
        let native = pinch
            .on_event(&PlatformInput::Magnify(MagnifyEvent {
                position: point(px(20.0), px(20.0)),
                delta: 0.2,
                modifiers: Modifiers::default(),
                touch_phase: TouchPhase::Started,
            }))
            .expect("expected native pinch event");
        assert_eq!(native.state, PinchState::Began);
        assert!(native.scale > 1.1);

        let scroll = pinch
            .on_event(&PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(20.0), px(20.0)),
                delta: ScrollDelta::Lines(point(0.0, 1.0)),
                modifiers: Modifiers {
                    control: true,
                    ..Modifiers::default()
                },
                touch_phase: TouchPhase::Moved,
                is_momentum: false,
            }))
            .expect("expected scroll-wheel pinch fallback");
        assert_eq!(scroll.state, PinchState::Ended);
        assert!(scroll.scale > 1.0);
    }

    #[test]
    fn pan_recognizer_begins_after_threshold() {
        let mut pan = PanGesture::new();
        pan.on_event(&PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(0.0), px(0.0)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }));

        let began = pan
            .on_event(&PlatformInput::MouseMove(MouseMoveEvent {
                pressed_button: Some(MouseButton::Left),
                position: point(px(10.0), px(0.0)),
                modifiers: Modifiers::default(),
            }))
            .expect("expected pan to begin");

        assert_eq!(began.state, PanState::Began);
    }
}
