use crate::{
    Bounds, Capslock, Context, Empty, IntoElement, Keystroke, Modifiers, Pixels, Point, Render,
    Window, point, px, seal::Sealed,
};
use http_client::Url;
use smallvec::SmallVec;
use std::{
    any::Any,
    fmt::Debug,
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

/// An event from a platform input source.
pub trait InputEvent: Sealed + 'static {
    /// Convert this event into the platform input enum.
    fn to_platform_input(self) -> PlatformInput;
}

/// A key event from the platform.
pub trait KeyEvent: InputEvent {}

/// A mouse event from the platform.
pub trait MouseEvent: InputEvent {}

/// The key down event equivalent for the platform.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyDownEvent {
    /// The keystroke that was generated.
    pub keystroke: Keystroke,

    /// Whether the key is currently held down.
    pub is_held: bool,
}

impl Sealed for KeyDownEvent {}
impl InputEvent for KeyDownEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::KeyDown(self)
    }
}
impl KeyEvent for KeyDownEvent {}

/// The key up event equivalent for the platform.
#[derive(Clone, Debug)]
pub struct KeyUpEvent {
    /// The keystroke that was released.
    pub keystroke: Keystroke,
}

impl Sealed for KeyUpEvent {}
impl InputEvent for KeyUpEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::KeyUp(self)
    }
}
impl KeyEvent for KeyUpEvent {}

/// The modifiers changed event equivalent for the platform.
#[derive(Clone, Debug, Default)]
pub struct ModifiersChangedEvent {
    /// The new state of the modifier keys
    pub modifiers: Modifiers,
    /// The new state of the capslock key
    pub capslock: Capslock,
}

impl Sealed for ModifiersChangedEvent {}
impl InputEvent for ModifiersChangedEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::ModifiersChanged(self)
    }
}
impl KeyEvent for ModifiersChangedEvent {}

impl Deref for ModifiersChangedEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

/// The phase of a touch motion event.
/// Based on the winit enum of the same name.
#[derive(Clone, Copy, Debug, Default)]
pub enum TouchPhase {
    /// The touch started.
    Started,
    /// The touch event is moving.
    #[default]
    Moved,
    /// The touch phase has ended
    Ended,
}

/// Stable identifier for one active pointer.
///
/// Browsers and native pointer APIs may reuse an identifier after the pointer has
/// ended. Applications should therefore scope any per-pointer state to the span
/// between [`PointerPhase::Down`] and [`PointerPhase::Up`] or
/// [`PointerPhase::Cancel`].
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct PointerId(i64);

impl PointerId {
    /// The conventional identifier used when a platform only exposes a legacy mouse stream.
    pub const LEGACY_MOUSE: Self = Self(1);

    /// Construct an identifier from a platform-provided value.
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Return the platform-provided numeric value.
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// Physical pointer device category.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PointerType {
    /// A mouse, trackball, or mouse-compatible pointing device.
    #[default]
    Mouse,
    /// A direct touch contact.
    Touch,
    /// A pen, stylus, or eraser-capable digitizer.
    Pen,
    /// A device category the current Kael version does not recognize.
    Unknown,
}

/// Lifecycle phase of a rich pointer event.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum PointerPhase {
    /// The pointer became active or a button was pressed.
    Down,
    /// The pointer moved or its analog properties changed.
    #[default]
    Move,
    /// The pointer or button was released normally.
    Up,
    /// The platform cancelled the pointer sequence.
    Cancel,
    /// The pointer entered the window's interactive surface.
    Enter,
    /// The pointer left the window's interactive surface.
    Leave,
}

bitflags::bitflags! {
    /// Simultaneously pressed pointer buttons.
    ///
    /// Values intentionally match the W3C Pointer Events `buttons` bit field, so
    /// browser input can be forwarded without a lossy remap.
    #[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
    pub struct PointerButtons: u16 {
        /// Primary button, normally the left mouse button or pen contact.
        const PRIMARY = 1;
        /// Secondary button, normally the right mouse button or pen barrel button.
        const SECONDARY = 1 << 1;
        /// Auxiliary button, normally the middle mouse button.
        const AUXILIARY = 1 << 2;
        /// Back navigation button.
        const BACK = 1 << 3;
        /// Forward navigation button.
        const FORWARD = 1 << 4;
        /// Pen eraser button when reported separately by the platform.
        const ERASER = 1 << 5;
    }
}

impl PointerButtons {
    /// Return the first legacy mouse button represented by this mask.
    pub fn primary_legacy_button(self) -> Option<MouseButton> {
        if self.contains(Self::PRIMARY) {
            Some(MouseButton::Left)
        } else if self.contains(Self::SECONDARY) {
            Some(MouseButton::Right)
        } else if self.contains(Self::AUXILIARY) {
            Some(MouseButton::Middle)
        } else if self.contains(Self::BACK) {
            Some(MouseButton::Navigate(NavigationDirection::Back))
        } else if self.contains(Self::FORWARD) {
            Some(MouseButton::Navigate(NavigationDirection::Forward))
        } else {
            None
        }
    }

    fn from_mouse_button(button: MouseButton) -> Self {
        match button {
            MouseButton::Left => Self::PRIMARY,
            MouseButton::Right => Self::SECONDARY,
            MouseButton::Middle => Self::AUXILIARY,
            MouseButton::Navigate(NavigationDirection::Back) => Self::BACK,
            MouseButton::Navigate(NavigationDirection::Forward) => Self::FORWARD,
        }
    }
}

/// One high-frequency sample contained in a coalesced pointer move.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerSample {
    /// Position in logical window pixels.
    pub position: Point<Pixels>,
    /// Relative motion in logical pixels for this event.
    ///
    /// Pointer lock keeps absolute coordinates constrained, so game and camera
    /// controls should consume this field for mouse-look movement.
    pub movement: Point<Pixels>,
    /// Normalized contact pressure in the inclusive `0..=1` range.
    pub pressure: f32,
    /// Normalized barrel pressure in the inclusive `-1..=1` range.
    pub tangential_pressure: f32,
    /// Pen tilt away from the Y-Z plane, in degrees from `-90..=90`.
    pub tilt_x: f32,
    /// Pen tilt away from the X-Z plane, in degrees from `-90..=90`.
    pub tilt_y: f32,
    /// Clockwise pen rotation in degrees from `0..360`.
    pub twist: f32,
    /// Contact geometry width in logical pixels.
    pub width: Pixels,
    /// Contact geometry height in logical pixels.
    pub height: Pixels,
    /// Platform event timestamp in milliseconds when available.
    pub timestamp_ms: f64,
}

/// A device-independent, high-fidelity pointer event.
///
/// Browser Pointer Events populate every field and preserve coalesced samples.
/// Legacy desktop mouse streams are promoted to this type with
/// [`PointerId::LEGACY_MOUSE`], unit contact geometry, and conservative analog
/// values. This lets one handler support mouse, touch, and pen without changing
/// existing [`MouseDownEvent`], [`MouseMoveEvent`], or [`MouseUpEvent`] APIs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointerInputEvent {
    /// Pointer lifecycle phase.
    pub phase: PointerPhase,
    /// Identifier of the active pointer.
    pub pointer_id: PointerId,
    /// Physical pointer device category.
    pub pointer_type: PointerType,
    /// Position in logical window pixels.
    pub position: Point<Pixels>,
    /// Relative motion in logical pixels for this event.
    ///
    /// Browser pointer lock keeps absolute coordinates fixed, so game and
    /// camera controls should consume this field for mouse-look movement.
    pub movement: Point<Pixels>,
    /// Button whose state changed, if this phase represents a button transition.
    pub button: Option<MouseButton>,
    /// Complete set of buttons held after this event.
    pub buttons: PointerButtons,
    /// Keyboard modifiers held for this event.
    pub modifiers: Modifiers,
    /// Number of consecutive clicks reported by the platform, starting at one.
    pub click_count: usize,
    /// Whether this is the pointer designated to synthesize legacy mouse events.
    pub is_primary: bool,
    /// Normalized contact pressure in the inclusive `0..=1` range.
    pub pressure: f32,
    /// Normalized barrel pressure in the inclusive `-1..=1` range.
    pub tangential_pressure: f32,
    /// Pen tilt away from the Y-Z plane, in degrees from `-90..=90`.
    pub tilt_x: f32,
    /// Pen tilt away from the X-Z plane, in degrees from `-90..=90`.
    pub tilt_y: f32,
    /// Clockwise pen rotation in degrees from `0..360`.
    pub twist: f32,
    /// Contact geometry width in logical pixels.
    pub width: Pixels,
    /// Contact geometry height in logical pixels.
    pub height: Pixels,
    /// Platform event timestamp in milliseconds when available.
    pub timestamp_ms: f64,
    /// Higher-frequency samples coalesced by the platform into this event.
    pub coalesced: Vec<PointerSample>,
}

impl PointerInputEvent {
    /// Return the current event as a sample suitable for a stroke pipeline.
    pub fn sample(&self) -> PointerSample {
        PointerSample {
            position: self.position,
            movement: self.movement,
            pressure: self.pressure,
            tangential_pressure: self.tangential_pressure,
            tilt_x: self.tilt_x,
            tilt_y: self.tilt_y,
            twist: self.twist,
            width: self.width,
            height: self.height,
            timestamp_ms: self.timestamp_ms,
        }
    }

    /// Iterate coalesced samples in chronological order followed by the current event.
    pub fn stroke_samples(&self) -> impl Iterator<Item = PointerSample> + '_ {
        self.coalesced
            .iter()
            .copied()
            .chain(std::iter::once(self.sample()))
    }

    /// Return true while the primary button or direct contact is active.
    pub fn dragging(&self) -> bool {
        self.buttons.contains(PointerButtons::PRIMARY)
    }

    /// Return true for a direct touch contact.
    pub fn is_touch(&self) -> bool {
        self.pointer_type == PointerType::Touch
    }

    /// Return true for pen or stylus input.
    pub fn is_pen(&self) -> bool {
        self.pointer_type == PointerType::Pen
    }

    /// Build the compatibility mouse event for a primary pointer, if this phase has one.
    ///
    /// Kael uses this internally so existing mouse-only elements continue to work
    /// when a browser or native backend supplies rich pointer input directly.
    pub fn legacy_mouse_event(&self) -> Option<PlatformInput> {
        if !self.is_primary {
            return None;
        }
        let button = self
            .button
            .or_else(|| self.buttons.primary_legacy_button())
            .unwrap_or(MouseButton::Left);
        match self.phase {
            PointerPhase::Down => Some(PlatformInput::MouseDown(MouseDownEvent {
                button,
                position: self.position,
                modifiers: self.modifiers,
                click_count: self.click_count.max(1),
                first_mouse: false,
            })),
            PointerPhase::Move => Some(PlatformInput::MouseMove(MouseMoveEvent {
                position: self.position,
                pressed_button: self.buttons.primary_legacy_button(),
                modifiers: self.modifiers,
            })),
            PointerPhase::Up | PointerPhase::Cancel => Some(PlatformInput::MouseUp(MouseUpEvent {
                button,
                position: self.position,
                modifiers: self.modifiers,
                click_count: self.click_count.max(1),
            })),
            PointerPhase::Leave => Some(PlatformInput::MouseExited(MouseExitEvent {
                position: self.position,
                pressed_button: self.buttons.primary_legacy_button(),
                modifiers: self.modifiers,
            })),
            PointerPhase::Enter => None,
        }
    }
}

impl Sealed for PointerInputEvent {}
impl InputEvent for PointerInputEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::Pointer(self)
    }
}
impl MouseEvent for PointerInputEvent {}

/// A mouse down event from the platform
#[derive(Clone, Debug, Default)]
pub struct MouseDownEvent {
    /// Which mouse button was pressed.
    pub button: MouseButton,

    /// The position of the mouse on the window.
    pub position: Point<Pixels>,

    /// The modifiers that were held down when the mouse was pressed.
    pub modifiers: Modifiers,

    /// The number of times the button has been clicked.
    pub click_count: usize,

    /// Whether this is the first, focusing click.
    pub first_mouse: bool,
}

impl Sealed for MouseDownEvent {}
impl InputEvent for MouseDownEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::MouseDown(self)
    }
}
impl MouseEvent for MouseDownEvent {}

/// A mouse up event from the platform
#[derive(Clone, Debug, Default)]
pub struct MouseUpEvent {
    /// Which mouse button was released.
    pub button: MouseButton,

    /// The position of the mouse on the window.
    pub position: Point<Pixels>,

    /// The modifiers that were held down when the mouse was released.
    pub modifiers: Modifiers,

    /// The number of times the button has been clicked.
    pub click_count: usize,
}

impl Sealed for MouseUpEvent {}
impl InputEvent for MouseUpEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::MouseUp(self)
    }
}
impl MouseEvent for MouseUpEvent {}

/// A click event, generated when a mouse button is pressed and released.
#[derive(Clone, Debug, Default)]
pub struct MouseClickEvent {
    /// The mouse event when the button was pressed.
    pub down: MouseDownEvent,

    /// The mouse event when the button was released.
    pub up: MouseUpEvent,
}

/// A click event that was generated by a keyboard button being pressed and released.
#[derive(Clone, Debug, Default)]
pub struct KeyboardClickEvent {
    /// The keyboard button that was pressed to trigger the click.
    pub button: KeyboardButton,

    /// The bounds of the element that was clicked.
    pub bounds: Bounds<Pixels>,
}

/// A click event, generated when a mouse button or keyboard button is pressed and released.
#[derive(Clone, Debug)]
pub enum ClickEvent {
    /// A click event trigger by a mouse button being pressed and released.
    Mouse(MouseClickEvent),
    /// A click event trigger by a keyboard button being pressed and released.
    Keyboard(KeyboardClickEvent),
}

impl Default for ClickEvent {
    fn default() -> Self {
        ClickEvent::Keyboard(KeyboardClickEvent::default())
    }
}

impl ClickEvent {
    /// Returns the modifiers that were held during the click event
    ///
    /// `Keyboard`: The keyboard click events never have modifiers.
    /// `Mouse`: Modifiers that were held during the mouse key up event.
    pub fn modifiers(&self) -> Modifiers {
        match self {
            // Click events are only generated from keyboard events _without any modifiers_, so we know the modifiers are always Default
            ClickEvent::Keyboard(_) => Modifiers::default(),
            // Click events on the web only reflect the modifiers for the keyup event,
            // tested via observing the behavior of the `ClickEvent.shiftKey` field in Chrome 138
            // under various combinations of modifiers and keyUp / keyDown events.
            ClickEvent::Mouse(event) => event.up.modifiers,
        }
    }

    /// Returns the position of the click event
    ///
    /// `Keyboard`: The bottom left corner of the clicked hitbox
    /// `Mouse`: The position of the mouse when the button was released.
    pub fn position(&self) -> Point<Pixels> {
        match self {
            ClickEvent::Keyboard(event) => event.bounds.bottom_left(),
            ClickEvent::Mouse(event) => event.up.position,
        }
    }

    /// Returns the mouse position of the click event
    ///
    /// `Keyboard`: None
    /// `Mouse`: The position of the mouse when the button was released.
    pub fn mouse_position(&self) -> Option<Point<Pixels>> {
        match self {
            ClickEvent::Keyboard(_) => None,
            ClickEvent::Mouse(event) => Some(event.up.position),
        }
    }

    /// Returns if this was a right click
    ///
    /// `Keyboard`: false
    /// `Mouse`: Whether the right button was pressed and released
    pub fn is_right_click(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => false,
            ClickEvent::Mouse(event) => {
                event.down.button == MouseButton::Right && event.up.button == MouseButton::Right
            }
        }
    }

    /// Returns whether the click was a standard click
    ///
    /// `Keyboard`: Always true
    /// `Mouse`: Left button pressed and released
    pub fn standard_click(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => true,
            ClickEvent::Mouse(event) => {
                event.down.button == MouseButton::Left && event.up.button == MouseButton::Left
            }
        }
    }

    /// Returns whether the click focused the element
    ///
    /// `Keyboard`: false, keyboard clicks only work if an element is already focused
    /// `Mouse`: Whether this was the first focusing click
    pub fn first_focus(&self) -> bool {
        match self {
            ClickEvent::Keyboard(_) => false,
            ClickEvent::Mouse(event) => event.down.first_mouse,
        }
    }

    /// Returns the click count of the click event
    ///
    /// `Keyboard`: Always 1
    /// `Mouse`: Count of clicks from MouseUpEvent
    pub fn click_count(&self) -> usize {
        match self {
            ClickEvent::Keyboard(_) => 1,
            ClickEvent::Mouse(event) => event.up.click_count,
        }
    }

    /// Returns whether the click event is generated by a keyboard event
    pub fn is_keyboard(&self) -> bool {
        match self {
            ClickEvent::Mouse(_) => false,
            ClickEvent::Keyboard(_) => true,
        }
    }
}

/// An enum representing the keyboard button that was pressed for a click event.
#[derive(Hash, PartialEq, Eq, Copy, Clone, Debug, Default)]
pub enum KeyboardButton {
    /// Enter key was clicked
    #[default]
    Enter,
    /// Space key was clicked
    Space,
}

/// An enum representing the mouse button that was pressed.
#[derive(Hash, PartialEq, Eq, Copy, Clone, Debug, Default)]
pub enum MouseButton {
    /// The left mouse button.
    #[default]
    Left,

    /// The right mouse button.
    Right,

    /// The middle mouse button.
    Middle,

    /// A navigation button, such as back or forward.
    Navigate(NavigationDirection),
}

impl MouseButton {
    /// Get all the mouse buttons in a list.
    pub fn all() -> Vec<Self> {
        vec![
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Navigate(NavigationDirection::Back),
            MouseButton::Navigate(NavigationDirection::Forward),
        ]
    }
}

/// A navigation direction, such as back or forward.
#[derive(Hash, PartialEq, Eq, Copy, Clone, Debug, Default)]
pub enum NavigationDirection {
    /// The back button.
    #[default]
    Back,

    /// The forward button.
    Forward,
}

/// A mouse move event from the platform
#[derive(Clone, Debug, Default)]
pub struct MouseMoveEvent {
    /// The position of the mouse on the window.
    pub position: Point<Pixels>,

    /// The mouse button that was pressed, if any.
    pub pressed_button: Option<MouseButton>,

    /// The modifiers that were held down when the mouse was moved.
    pub modifiers: Modifiers,
}

impl Sealed for MouseMoveEvent {}
impl InputEvent for MouseMoveEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::MouseMove(self)
    }
}
impl MouseEvent for MouseMoveEvent {}

impl MouseMoveEvent {
    /// Returns true if the left mouse button is currently held down.
    pub fn dragging(&self) -> bool {
        self.pressed_button == Some(MouseButton::Left)
    }
}

impl From<&MouseDownEvent> for PointerInputEvent {
    fn from(event: &MouseDownEvent) -> Self {
        Self {
            phase: PointerPhase::Down,
            pointer_id: PointerId::LEGACY_MOUSE,
            pointer_type: PointerType::Mouse,
            position: event.position,
            button: Some(event.button),
            buttons: PointerButtons::from_mouse_button(event.button),
            modifiers: event.modifiers,
            click_count: event.click_count.max(1),
            is_primary: true,
            pressure: 0.5,
            width: px(1.0),
            height: px(1.0),
            ..Default::default()
        }
    }
}

impl From<&MouseMoveEvent> for PointerInputEvent {
    fn from(event: &MouseMoveEvent) -> Self {
        let buttons = event
            .pressed_button
            .map(PointerButtons::from_mouse_button)
            .unwrap_or_default();
        Self {
            phase: PointerPhase::Move,
            pointer_id: PointerId::LEGACY_MOUSE,
            pointer_type: PointerType::Mouse,
            position: event.position,
            buttons,
            modifiers: event.modifiers,
            is_primary: true,
            pressure: if buttons.is_empty() { 0.0 } else { 0.5 },
            width: px(1.0),
            height: px(1.0),
            ..Default::default()
        }
    }
}

impl From<&MouseUpEvent> for PointerInputEvent {
    fn from(event: &MouseUpEvent) -> Self {
        Self {
            phase: PointerPhase::Up,
            pointer_id: PointerId::LEGACY_MOUSE,
            pointer_type: PointerType::Mouse,
            position: event.position,
            button: Some(event.button),
            modifiers: event.modifiers,
            click_count: event.click_count.max(1),
            is_primary: true,
            width: px(1.0),
            height: px(1.0),
            ..Default::default()
        }
    }
}

/// A mouse wheel event from the platform
#[derive(Clone, Debug, Default)]
pub struct ScrollWheelEvent {
    /// The position of the mouse on the window.
    pub position: Point<Pixels>,

    /// The change in scroll wheel position for this event.
    pub delta: ScrollDelta,

    /// The modifiers that were held down when the mouse was moved.
    pub modifiers: Modifiers,

    /// The phase of the touch event.
    pub touch_phase: TouchPhase,

    /// Whether this event is part of inertial scrolling after direct touch input ends.
    pub is_momentum: bool,
}

impl Sealed for ScrollWheelEvent {}
impl InputEvent for ScrollWheelEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::ScrollWheel(self)
    }
}
impl MouseEvent for ScrollWheelEvent {}

impl Deref for ScrollWheelEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

/// A magnification gesture event from the platform.
#[derive(Clone, Debug, Default)]
pub struct MagnifyEvent {
    /// The position of the gesture in window coordinates.
    pub position: Point<Pixels>,

    /// The incremental magnification delta for this event.
    pub delta: f32,

    /// The modifiers that were held when the gesture fired.
    pub modifiers: Modifiers,

    /// The touch phase associated with the gesture.
    pub touch_phase: TouchPhase,
}

impl Sealed for MagnifyEvent {}
impl InputEvent for MagnifyEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::Magnify(self)
    }
}
impl MouseEvent for MagnifyEvent {}

impl Deref for MagnifyEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

/// The scroll delta for a scroll wheel event.
#[derive(Clone, Copy, Debug)]
pub enum ScrollDelta {
    /// An exact scroll delta in pixels.
    Pixels(Point<Pixels>),
    /// An inexact scroll delta in lines.
    Lines(Point<f32>),
}

impl Default for ScrollDelta {
    fn default() -> Self {
        Self::Lines(Default::default())
    }
}

impl ScrollDelta {
    /// Returns true if this is a precise scroll delta in pixels.
    pub fn precise(&self) -> bool {
        match self {
            ScrollDelta::Pixels(_) => true,
            ScrollDelta::Lines(_) => false,
        }
    }

    /// Converts this scroll event into exact pixels.
    pub fn pixel_delta(&self, line_height: Pixels) -> Point<Pixels> {
        match self {
            ScrollDelta::Pixels(delta) => *delta,
            ScrollDelta::Lines(delta) => point(line_height * delta.x, line_height * delta.y),
        }
    }

    pub(crate) fn trace_label(&self) -> String {
        match self {
            ScrollDelta::Pixels(delta) => {
                format!(
                    "pixels({:.2},{:.2})",
                    f32::from(delta.x),
                    f32::from(delta.y)
                )
            }
            ScrollDelta::Lines(delta) => format!("lines({:.2},{:.2})", delta.x, delta.y),
        }
    }

    /// Combines two scroll deltas into one.
    /// If the signs of the deltas are the same (both positive or both negative),
    /// the deltas are added together. If the signs are opposite, the second delta
    /// (other) is used, effectively overriding the first delta.
    pub fn coalesce(self, other: ScrollDelta) -> ScrollDelta {
        match (self, other) {
            (ScrollDelta::Pixels(a), ScrollDelta::Pixels(b)) => {
                let x = if a.x.signum() == b.x.signum() {
                    a.x + b.x
                } else {
                    b.x
                };

                let y = if a.y.signum() == b.y.signum() {
                    a.y + b.y
                } else {
                    b.y
                };

                ScrollDelta::Pixels(point(x, y))
            }

            (ScrollDelta::Lines(a), ScrollDelta::Lines(b)) => {
                let x = if a.x.signum() == b.x.signum() {
                    a.x + b.x
                } else {
                    b.x
                };

                let y = if a.y.signum() == b.y.signum() {
                    a.y + b.y
                } else {
                    b.y
                };

                ScrollDelta::Lines(point(x, y))
            }

            _ => other,
        }
    }
}

pub(crate) fn scroll_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("KAEL_SCROLL_TRACE")
            .map(|value| {
                let value = value.trim().to_ascii_lowercase();
                !value.is_empty() && value != "0" && value != "false" && value != "off"
            })
            .unwrap_or(false)
    })
}

/// A mouse exit event from the platform, generated when the mouse leaves the window.
#[derive(Clone, Debug, Default)]
pub struct MouseExitEvent {
    /// The position of the mouse relative to the window.
    pub position: Point<Pixels>,
    /// The mouse button that was pressed, if any.
    pub pressed_button: Option<MouseButton>,
    /// The modifiers that were held down when the mouse was moved.
    pub modifiers: Modifiers,
}

impl Sealed for MouseExitEvent {}
impl InputEvent for MouseExitEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::MouseExited(self)
    }
}
impl MouseEvent for MouseExitEvent {}

impl Deref for MouseExitEvent {
    type Target = Modifiers;

    fn deref(&self) -> &Self::Target {
        &self.modifiers
    }
}

impl From<&MouseExitEvent> for PointerInputEvent {
    fn from(event: &MouseExitEvent) -> Self {
        let buttons = event
            .pressed_button
            .map(PointerButtons::from_mouse_button)
            .unwrap_or_default();
        Self {
            phase: PointerPhase::Leave,
            pointer_id: PointerId::LEGACY_MOUSE,
            pointer_type: PointerType::Mouse,
            position: event.position,
            buttons,
            modifiers: event.modifiers,
            is_primary: true,
            pressure: if buttons.is_empty() { 0.0 } else { 0.5 },
            width: px(1.0),
            height: px(1.0),
            ..Default::default()
        }
    }
}

/// A collection of paths from the platform, such as from a file drop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalPaths(pub(crate) SmallVec<[PathBuf; 2]>);

impl ExternalPaths {
    /// Create an empty collection of external paths.
    pub fn new() -> Self {
        Self(SmallVec::new())
    }

    /// Create a collection from platform or test paths.
    pub fn from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self(paths.into_iter().collect())
    }

    /// Convert this collection of paths into a slice.
    pub fn paths(&self) -> &[PathBuf] {
        &self.0
    }

    /// Iterate over the paths.
    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.0.iter()
    }

    /// Return the first path, if any.
    pub fn first(&self) -> Option<&PathBuf> {
        self.0.first()
    }

    /// Return the number of paths.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return whether no paths are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Content-safe summary that avoids logging local paths or filenames.
    pub fn to_text(&self) -> String {
        format!(
            "external paths: {} paths, empty {}",
            self.len(),
            self.is_empty()
        )
    }

    /// Return an owned vector of paths.
    pub fn to_vec(&self) -> Vec<PathBuf> {
        self.0.to_vec()
    }

    /// Apply a drop-zone filter to this path collection.
    pub fn filter_with(&self, filter: &FileDropFilter) -> FileDropMatch {
        filter.evaluate(self)
    }

    /// Return accepted paths when every dropped path is accepted by the filter.
    pub fn accepted_by(&self, filter: &FileDropFilter) -> Option<Vec<PathBuf>> {
        self.filter_with(filter).into_clean_accept()
    }

    /// Convert this file-only payload into a general external drop payload.
    pub fn into_drop_data(self) -> ExternalDropData {
        ExternalDropData::from_paths(self.0)
    }
}

impl Render for ExternalPaths {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // the platform will render icons for the dragged files
        Empty
    }
}

/// An external file whose bytes are already available to the application.
///
/// Browser drops and file pickers cannot expose native filesystem paths, so
/// this is the portable counterpart to [`ExternalPaths`]. Native applications
/// may also use it after reading a user-selected path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalFile {
    name: String,
    mime_type: Option<String>,
    bytes: Arc<[u8]>,
    read_error: Option<String>,
    source_path: Option<PathBuf>,
}

impl ExternalFile {
    /// Create an external file from a display name, optional MIME type, and bytes.
    pub fn new(
        name: impl Into<String>,
        mime_type: Option<impl Into<String>>,
        bytes: impl Into<Vec<u8>>,
    ) -> Self {
        let name = name.into();
        let name = Path::new(&name)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("unnamed")
            .to_string();
        Self {
            name,
            mime_type: mime_type.map(Into::into),
            bytes: Arc::from(bytes.into()),
            read_error: None,
            source_path: None,
        }
    }

    /// Create metadata for a file whose bytes could not be read.
    pub fn unavailable(
        name: impl Into<String>,
        mime_type: Option<impl Into<String>>,
        error: impl Into<String>,
    ) -> Self {
        let mut file = Self::new(name, mime_type, Vec::new());
        file.read_error = Some(error.into());
        file
    }

    /// Create a file without a MIME hint.
    pub fn from_bytes(name: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self::new(name, None::<String>, bytes)
    }

    /// Retain the native source path used to read this file.
    ///
    /// Browser files intentionally never receive a source path because web
    /// file pickers and drops do not expose one.
    pub fn with_source_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.source_path = Some(path.into());
        self
    }

    /// User-visible base file name without a directory component.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Browser or platform MIME hint, when supplied.
    pub fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    /// File bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Shared ownership of the encoded file bytes.
    pub fn shared_bytes(&self) -> Arc<[u8]> {
        self.bytes.clone()
    }

    /// Encoded byte length.
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// Whether this file has no bytes.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether the platform successfully read this file's bytes.
    pub fn is_available(&self) -> bool {
        self.read_error.is_none()
    }

    /// Bounded platform read error, when bytes are unavailable.
    pub fn read_error(&self) -> Option<&str> {
        self.read_error.as_deref()
    }

    /// Native path used to read the file, when the platform has one.
    pub fn source_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// File extension without the leading dot, when present.
    pub fn extension(&self) -> Option<&str> {
        Path::new(&self.name)
            .extension()
            .and_then(|extension| extension.to_str())
    }

    /// Content-safe summary that omits the file name and bytes.
    pub fn to_text(&self) -> String {
        format!(
            "external file: bytes {}, mime {}, extension {}, available {}, source-path {}",
            self.byte_len(),
            self.mime_type.is_some(),
            self.extension().is_some(),
            self.is_available(),
            self.source_path.is_some()
        )
    }
}

/// Data dragged from outside the app, such as browser-style file, text, or URL payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalDropData {
    paths: ExternalPaths,
    files: Vec<ExternalFile>,
    text: Option<String>,
    urls: Vec<String>,
}

impl ExternalDropData {
    /// Create an empty external drop payload.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a file-only external drop payload.
    pub fn from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            paths: ExternalPaths::from_paths(paths),
            files: Vec::new(),
            text: None,
            urls: Vec::new(),
        }
    }

    /// Create a byte-backed file payload, as produced by browser file drops.
    pub fn from_files(files: impl IntoIterator<Item = ExternalFile>) -> Self {
        Self {
            paths: ExternalPaths::new(),
            files: files.into_iter().collect(),
            text: None,
            urls: Vec::new(),
        }
    }

    /// Create a text-only external drop payload.
    pub fn text(text: impl Into<String>) -> Self {
        Self::new().with_text(text)
    }

    /// Create a URL-only external drop payload.
    pub fn url(url: impl Into<String>) -> Self {
        Self::new().with_url(url)
    }

    /// Normalize a typed drag payload into browser-style external drop data.
    ///
    /// This accepts both file-only [`ExternalPaths`] payloads and richer
    /// [`ExternalDropData`] payloads emitted by native text/URL drops or custom
    /// WebView bridges.
    pub fn from_drag_value(value: &dyn Any) -> Option<Self> {
        value.downcast_ref::<Self>().cloned().or_else(|| {
            value
                .downcast_ref::<ExternalPaths>()
                .cloned()
                .map(Into::into)
        })
    }

    /// Parse `text/uri-list` data into file paths and URLs.
    ///
    /// Lines beginning with `#` are comments per the freedesktop/URI-list
    /// convention. `file://` entries become paths; other URI schemes stay in
    /// the URL list.
    pub fn from_uri_list(uri_list: &str) -> Self {
        let mut paths = SmallVec::<[PathBuf; 2]>::new();
        let mut urls = Vec::new();

        for line in uri_list.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            match Url::parse(line) {
                #[cfg(not(target_arch = "wasm32"))]
                Ok(url) => match url.to_file_path() {
                    Ok(path) => paths.push(path),
                    Err(_) => urls.push(line.to_string()),
                },
                #[cfg(target_arch = "wasm32")]
                Ok(_) => urls.push(line.to_string()),
                Err(_) => urls.push(line.to_string()),
            }
        }

        Self {
            paths: ExternalPaths(paths),
            files: Vec::new(),
            text: None,
            urls,
        }
    }

    /// Create a payload from plain text and extract URL-looking lines.
    pub fn from_plain_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let urls = text
            .lines()
            .map(str::trim)
            .filter(|line| Url::parse(line).is_ok())
            .map(str::to_string)
            .collect();
        Self {
            paths: ExternalPaths::new(),
            files: Vec::new(),
            text: Some(text),
            urls,
        }
    }

    /// Set plain text carried by the drop.
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Add one URL carried by the drop.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.urls.push(url.into());
        self
    }

    /// Add many URLs carried by the drop.
    pub fn with_urls(mut self, urls: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.urls.extend(urls.into_iter().map(Into::into));
        self
    }

    /// Add one byte-backed file.
    pub fn with_file(mut self, file: ExternalFile) -> Self {
        self.files.push(file);
        self
    }

    /// Add byte-backed files.
    pub fn with_files(mut self, files: impl IntoIterator<Item = ExternalFile>) -> Self {
        self.files.extend(files);
        self
    }

    /// Return file paths carried by the drop.
    pub fn paths(&self) -> &ExternalPaths {
        &self.paths
    }

    /// Byte-backed files carried by the drop.
    pub fn files(&self) -> &[ExternalFile] {
        &self.files
    }

    /// Return plain text carried by the drop, if any.
    pub fn text_value(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Return URLs carried by the drop.
    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    /// Return true when file paths are present.
    pub fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    /// Return true when byte-backed files are present.
    pub fn has_files(&self) -> bool {
        !self.files.is_empty()
    }

    /// Return true when text is present and not empty.
    pub fn has_text(&self) -> bool {
        self.text_value().is_some_and(|text| !text.is_empty())
    }

    /// Return true when URLs are present.
    pub fn has_urls(&self) -> bool {
        !self.urls.is_empty()
    }

    /// Number of URLs carried by the drop.
    pub fn url_count(&self) -> usize {
        self.urls.len()
    }

    /// Number of file paths carried by the drop.
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// Number of byte-backed files.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Total bytes carried by byte-backed files.
    pub fn file_bytes(&self) -> usize {
        self.files.iter().map(ExternalFile::byte_len).sum()
    }

    /// Length of the plain text payload in UTF-8 bytes, or zero when absent.
    pub fn text_len_bytes(&self) -> usize {
        self.text_value().map(str::len).unwrap_or(0)
    }

    /// Whether the payload carries no paths, text, or URLs.
    pub fn is_empty(&self) -> bool {
        !self.has_paths() && !self.has_files() && !self.has_text() && !self.has_urls()
    }

    /// Content-safe summary that avoids logging paths, text, URLs, and filenames.
    pub fn to_text(&self) -> String {
        format!(
            "external drop data: paths {}, files {}, file-bytes {}, text {}, text-bytes {}, urls {}, empty {}",
            self.path_count(),
            self.file_count(),
            self.file_bytes(),
            self.has_text(),
            self.text_len_bytes(),
            self.url_count(),
            self.is_empty()
        )
    }

    /// Return accepted file paths when every dropped file path is accepted by the filter.
    pub fn accepted_paths_by(&self, filter: &FileDropFilter) -> Option<Vec<PathBuf>> {
        self.paths.accepted_by(filter)
    }

    /// Return whether this payload can be accepted by a file-oriented drop zone.
    ///
    /// File payloads must pass the filter. Text/URL-only payloads are accepted
    /// because there are no file paths to reject.
    pub fn accepted_by(&self, filter: &FileDropFilter) -> bool {
        if self.has_paths() || self.has_files() {
            (!self.has_paths() || self.accepted_paths_by(filter).is_some())
                && filter.accepts_files(&self.files, self.path_count())
        } else {
            self.has_text() || self.has_urls()
        }
    }
}

impl Render for ExternalDropData {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl From<ExternalPaths> for ExternalDropData {
    fn from(paths: ExternalPaths) -> Self {
        paths.into_drop_data()
    }
}

/// Builder for accepting dropped files by count and extension.
#[derive(Debug, Clone, Default)]
pub struct FileDropFilter {
    allowed_extensions: Vec<String>,
    max_files: Option<usize>,
}

impl FileDropFilter {
    /// Create a filter that accepts any path count and extension.
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept one file with any extension.
    pub fn single_file() -> Self {
        Self::new().max_files(1)
    }

    /// Accept common image file extensions.
    pub fn images() -> Self {
        Self::new().extensions([
            "avif", "bmp", "gif", "heic", "heif", "jpg", "jpeg", "png", "webp",
        ])
    }

    /// Accept common audio file extensions.
    pub fn audio() -> Self {
        Self::new().extensions(["aac", "aiff", "flac", "m4a", "mp3", "ogg", "opus", "wav"])
    }

    /// Accept common video file extensions.
    pub fn video() -> Self {
        Self::new().extensions([
            "avi", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "ogv", "webm",
        ])
    }

    /// Accept common audio and video file extensions.
    pub fn media() -> Self {
        Self::new().extensions([
            "aac", "aiff", "avi", "flac", "m4a", "m4v", "mkv", "mov", "mp3", "mp4", "mpeg", "mpg",
            "ogg", "ogv", "opus", "wav", "webm",
        ])
    }

    /// Accept only paths with one of the provided extensions.
    ///
    /// Extensions are matched case-insensitively and may be passed with or
    /// without a leading dot.
    pub fn extensions(mut self, extensions: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        self.allowed_extensions = extensions
            .into_iter()
            .map(|extension| normalize_file_extension(extension.as_ref()))
            .filter(|extension| !extension.is_empty())
            .collect();
        self.allowed_extensions.sort();
        self.allowed_extensions.dedup();
        self
    }

    /// Accept at most this many paths.
    pub fn max_files(mut self, max_files: usize) -> Self {
        self.max_files = Some(max_files);
        self
    }

    /// Return the allowed extensions.
    pub fn allowed_extensions(&self) -> &[String] {
        &self.allowed_extensions
    }

    /// Return the configured max file count.
    pub fn configured_max_files(&self) -> Option<usize> {
        self.max_files
    }

    /// Number of allowed extensions configured on this filter.
    pub fn extension_count(&self) -> usize {
        self.allowed_extensions.len()
    }

    /// Whether this filter restricts accepted extensions.
    pub fn has_extension_filter(&self) -> bool {
        !self.allowed_extensions.is_empty()
    }

    /// Whether this filter limits the number of accepted files.
    pub fn has_max_files(&self) -> bool {
        self.max_files.is_some()
    }

    /// Content-safe summary that avoids logging extension labels.
    pub fn to_text(&self) -> String {
        format!(
            "file drop filter: extensions {}, extension-filter {}, max-files {}",
            self.extension_count(),
            self.has_extension_filter(),
            self.has_max_files()
        )
    }

    /// Validate the filter configuration.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(max_files) = self.max_files {
            anyhow::ensure!(
                max_files > 0,
                "file drop max_files must be greater than zero"
            );
        }
        Ok(())
    }

    /// Return whether the given path would be accepted by this filter before
    /// considering max file count.
    pub fn accepts_path(&self, path: &std::path::Path) -> bool {
        if self.allowed_extensions.is_empty() {
            return true;
        }

        path.extension()
            .and_then(|extension| extension.to_str())
            .map(normalize_file_extension)
            .is_some_and(|extension| self.allowed_extensions.contains(&extension))
    }

    /// Return whether a byte-backed external file matches this extension filter.
    pub fn accepts_file(&self, file: &ExternalFile) -> bool {
        if self.allowed_extensions.is_empty() {
            return true;
        }
        file.extension()
            .map(normalize_file_extension)
            .is_some_and(|extension| self.allowed_extensions.contains(&extension))
    }

    /// Return whether all byte-backed files and any already accepted path count
    /// fit this filter.
    pub fn accepts_files(&self, files: &[ExternalFile], accepted_path_count: usize) -> bool {
        self.max_files
            .is_none_or(|max_files| accepted_path_count + files.len() <= max_files)
            && files.iter().all(|file| self.accepts_file(file))
    }

    /// Evaluate a set of dropped paths.
    pub fn evaluate(&self, paths: &ExternalPaths) -> FileDropMatch {
        let mut accepted = Vec::new();
        let mut rejected = Vec::new();

        for path in paths.iter() {
            let allowed_by_extension = self.accepts_path(path);
            let allowed_by_count = self
                .max_files
                .is_none_or(|max_files| accepted.len() < max_files);

            if allowed_by_extension && allowed_by_count {
                accepted.push(path.clone());
            } else {
                rejected.push(path.clone());
            }
        }

        FileDropMatch { accepted, rejected }
    }
}

/// Accepted and rejected paths from a file drop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDropMatch {
    accepted: Vec<PathBuf>,
    rejected: Vec<PathBuf>,
}

impl FileDropMatch {
    /// Paths accepted by the filter.
    pub fn accepted(&self) -> &[PathBuf] {
        &self.accepted
    }

    /// Paths rejected by the filter.
    pub fn rejected(&self) -> &[PathBuf] {
        &self.rejected
    }

    /// Consume this match into accepted paths.
    pub fn into_accepted(self) -> Vec<PathBuf> {
        self.accepted
    }

    /// Return true when all dropped paths were accepted and at least one path was present.
    pub fn is_clean_accept(&self) -> bool {
        !self.accepted.is_empty() && self.rejected.is_empty()
    }

    /// Return true when no path was accepted.
    pub fn is_empty_accept(&self) -> bool {
        self.accepted.is_empty()
    }

    /// Number of accepted paths.
    pub fn accepted_count(&self) -> usize {
        self.accepted.len()
    }

    /// Number of rejected paths.
    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }

    /// Whether any path was rejected.
    pub fn has_rejections(&self) -> bool {
        !self.rejected.is_empty()
    }

    /// Content-safe summary that avoids logging local paths or filenames.
    pub fn to_text(&self) -> String {
        format!(
            "file drop match: accepted {}, rejected {}, clean {}, empty-accept {}",
            self.accepted_count(),
            self.rejected_count(),
            self.is_clean_accept(),
            self.is_empty_accept()
        )
    }

    /// Consume this match into accepted paths only when the drop was clean.
    pub fn into_clean_accept(self) -> Option<Vec<PathBuf>> {
        self.is_clean_accept().then_some(self.accepted)
    }
}

fn normalize_file_extension(extension: &str) -> String {
    extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

/// A file drop event from the platform, generated when files are dragged and dropped onto the window.
#[derive(Debug, Clone)]
pub enum FileDropEvent {
    /// The files have entered the window.
    Entered {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
        /// The paths of the files that are being dragged.
        paths: ExternalPaths,
    },
    /// External drop data has entered the window.
    DataEntered {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
        /// The browser-style files/text/URLs payload being dragged.
        data: ExternalDropData,
    },
    /// The files are being dragged over the window
    Pending {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
    },
    /// The files have been dropped onto the window.
    Submit {
        /// The position of the mouse relative to the window.
        position: Point<Pixels>,
    },
    /// The user has stopped dragging the files over the window.
    Exited,
}

impl Sealed for FileDropEvent {}
impl InputEvent for FileDropEvent {
    fn to_platform_input(self) -> PlatformInput {
        PlatformInput::FileDrop(self)
    }
}
impl MouseEvent for FileDropEvent {}

/// An enum corresponding to all kinds of platform input events.
#[derive(Clone, Debug)]
pub enum PlatformInput {
    /// A key was pressed.
    KeyDown(KeyDownEvent),
    /// A key was released.
    KeyUp(KeyUpEvent),
    /// The keyboard modifiers were changed.
    ModifiersChanged(ModifiersChangedEvent),
    /// A high-fidelity mouse, touch, or pen event was received.
    Pointer(PointerInputEvent),
    /// The mouse was pressed.
    MouseDown(MouseDownEvent),
    /// The mouse was released.
    MouseUp(MouseUpEvent),
    /// The mouse was moved.
    MouseMove(MouseMoveEvent),
    /// The mouse exited the window.
    MouseExited(MouseExitEvent),
    /// The scroll wheel was used.
    ScrollWheel(ScrollWheelEvent),
    /// A magnification gesture was used.
    Magnify(MagnifyEvent),
    /// Files were dragged and dropped onto the window.
    FileDrop(FileDropEvent),
}

impl PlatformInput {
    pub(crate) fn mouse_event(&self) -> Option<&dyn Any> {
        match self {
            PlatformInput::KeyDown { .. } => None,
            PlatformInput::KeyUp { .. } => None,
            PlatformInput::ModifiersChanged { .. } => None,
            PlatformInput::Pointer(event) => Some(event),
            PlatformInput::MouseDown(event) => Some(event),
            PlatformInput::MouseUp(event) => Some(event),
            PlatformInput::MouseMove(event) => Some(event),
            PlatformInput::MouseExited(event) => Some(event),
            PlatformInput::ScrollWheel(event) => Some(event),
            PlatformInput::Magnify(event) => Some(event),
            PlatformInput::FileDrop(event) => Some(event),
        }
    }

    pub(crate) fn keyboard_event(&self) -> Option<&dyn Any> {
        match self {
            PlatformInput::KeyDown(event) => Some(event),
            PlatformInput::KeyUp(event) => Some(event),
            PlatformInput::ModifiersChanged(event) => Some(event),
            PlatformInput::Pointer(_) => None,
            PlatformInput::MouseDown(_) => None,
            PlatformInput::MouseUp(_) => None,
            PlatformInput::MouseMove(_) => None,
            PlatformInput::MouseExited(_) => None,
            PlatformInput::ScrollWheel(_) => None,
            PlatformInput::Magnify(_) => None,
            PlatformInput::FileDrop(_) => None,
        }
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use super::{
        MouseButton, MouseDownEvent, PlatformInput, PointerButtons, PointerId, PointerInputEvent,
        PointerPhase, PointerSample, PointerType,
    };
    use crate::{
        AppContext as _, Context, ExternalDropData, ExternalFile, ExternalPaths, FileDropFilter,
        FocusHandle, InteractiveElement, IntoElement, KeyBinding, Keystroke, ParentElement, Render,
        TestAppContext, Window, div, point, px,
    };

    struct TestView {
        saw_key_down: bool,
        saw_action: bool,
        focus_handle: FocusHandle,
    }

    actions!(test_only, [TestAction]);

    #[test]
    fn legacy_mouse_events_promote_to_stable_rich_pointer_input() {
        let mouse = MouseDownEvent {
            button: MouseButton::Right,
            position: point(px(12.0), px(34.0)),
            click_count: 2,
            ..Default::default()
        };

        let pointer = PointerInputEvent::from(&mouse);
        assert_eq!(pointer.phase, PointerPhase::Down);
        assert_eq!(pointer.pointer_id, PointerId::LEGACY_MOUSE);
        assert_eq!(pointer.pointer_type, PointerType::Mouse);
        assert_eq!(pointer.button, Some(MouseButton::Right));
        assert_eq!(pointer.buttons, PointerButtons::SECONDARY);
        assert_eq!(pointer.pressure, 0.5);
        assert_eq!(pointer.width, px(1.0));
        assert_eq!(pointer.height, px(1.0));
        assert!(pointer.is_primary);
    }

    #[test]
    fn rich_pointer_preserves_coalesced_stroke_samples_and_mouse_compatibility() {
        let pointer = PointerInputEvent {
            phase: PointerPhase::Move,
            pointer_id: PointerId::new(42),
            pointer_type: PointerType::Pen,
            position: point(px(20.0), px(30.0)),
            buttons: PointerButtons::PRIMARY | PointerButtons::SECONDARY,
            is_primary: true,
            pressure: 0.75,
            tilt_x: 12.0,
            timestamp_ms: 3.0,
            coalesced: vec![
                PointerSample {
                    position: point(px(10.0), px(15.0)),
                    pressure: 0.25,
                    timestamp_ms: 1.0,
                    ..Default::default()
                },
                PointerSample {
                    position: point(px(15.0), px(22.0)),
                    pressure: 0.5,
                    timestamp_ms: 2.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let samples = pointer.stroke_samples().collect::<Vec<_>>();
        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].timestamp_ms, 1.0);
        assert_eq!(samples[1].timestamp_ms, 2.0);
        assert_eq!(samples[2].timestamp_ms, 3.0);
        assert_eq!(samples[2].pressure, 0.75);
        assert!(pointer.is_pen());
        assert!(pointer.dragging());

        let Some(PlatformInput::MouseMove(mouse)) = pointer.legacy_mouse_event() else {
            panic!("primary pointer move should synthesize a mouse move");
        };
        assert_eq!(mouse.position, pointer.position);
        assert_eq!(mouse.pressed_button, Some(MouseButton::Left));

        let mut secondary_touch = pointer;
        secondary_touch.pointer_type = PointerType::Touch;
        secondary_touch.is_primary = false;
        assert!(secondary_touch.is_touch());
        assert!(secondary_touch.legacy_mouse_event().is_none());
    }

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().id("testview").child(
                div()
                    .key_context("parent")
                    .on_key_down(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.saw_key_down = true
                    }))
                    .on_action(cx.listener(|this: &mut TestView, _: &TestAction, _, _| {
                        this.saw_action = true
                    }))
                    .child(
                        div()
                            .key_context("nested")
                            .track_focus(&self.focus_handle)
                            .into_element(),
                    ),
            )
        }
    }

    #[kael::test]
    fn test_on_events(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(crate::WindowOptions::default(), |_, cx| {
                cx.new(|cx| TestView {
                    saw_key_down: false,
                    saw_action: false,
                    focus_handle: cx.focus_handle(),
                })
            })
            .unwrap()
        });

        cx.update(|cx| {
            cx.bind_keys(vec![KeyBinding::new("ctrl-g", TestAction, Some("parent"))]);
        });

        window
            .update(cx, |test_view, window, _cx| {
                window.focus(&test_view.focus_handle)
            })
            .unwrap();

        cx.dispatch_keystroke(*window, Keystroke::parse("a").unwrap());
        cx.dispatch_keystroke(*window, Keystroke::parse("ctrl-g").unwrap());

        window
            .update(cx, |test_view, _, _| {
                assert!(test_view.saw_key_down || test_view.saw_action);
                assert!(test_view.saw_key_down);
                assert!(test_view.saw_action);
            })
            .unwrap();
    }

    #[test]
    fn external_paths_exposes_path_helpers() {
        let paths = ExternalPaths::from_paths([
            PathBuf::from("/tmp/clip.mp4"),
            PathBuf::from("/tmp/subtitles.vtt"),
        ]);

        assert_eq!(paths.len(), 2);
        assert_eq!(paths.first().unwrap(), &PathBuf::from("/tmp/clip.mp4"));
        assert_eq!(
            paths.to_vec(),
            vec![
                PathBuf::from("/tmp/clip.mp4"),
                PathBuf::from("/tmp/subtitles.vtt")
            ]
        );
        assert!(!paths.is_empty());
    }

    #[test]
    fn file_drop_filter_normalizes_extensions_and_limits_count() {
        let filter = FileDropFilter::new()
            .extensions([".MP4", "mov", "mp4"])
            .max_files(2);
        assert_eq!(
            filter.allowed_extensions(),
            &["mov".to_string(), "mp4".to_string()]
        );
        assert_eq!(filter.configured_max_files(), Some(2));
        assert!(filter.validate().is_ok());

        let paths = ExternalPaths::from_paths([
            PathBuf::from("/tmp/a.MP4"),
            PathBuf::from("/tmp/b.mov"),
            PathBuf::from("/tmp/c.mp4"),
            PathBuf::from("/tmp/readme.txt"),
        ]);

        let matched = paths.filter_with(&filter);
        assert_eq!(
            matched.accepted(),
            &[PathBuf::from("/tmp/a.MP4"), PathBuf::from("/tmp/b.mov")]
        );
        assert_eq!(
            matched.rejected(),
            &[
                PathBuf::from("/tmp/c.mp4"),
                PathBuf::from("/tmp/readme.txt")
            ]
        );
        assert_eq!(filter.extension_count(), 2);
        assert!(filter.has_extension_filter());
        assert!(filter.has_max_files());
        assert_eq!(
            filter.to_text(),
            "file drop filter: extensions 2, extension-filter true, max-files true"
        );
        assert!(!filter.to_text().contains("mp4"));
        assert_eq!(matched.accepted_count(), 2);
        assert_eq!(matched.rejected_count(), 2);
        assert!(matched.has_rejections());
        assert_eq!(
            matched.to_text(),
            "file drop match: accepted 2, rejected 2, clean false, empty-accept false"
        );
        assert!(!matched.to_text().contains("/tmp"));
        assert!(!matched.to_text().contains("readme"));
        assert!(!matched.is_clean_accept());
        assert!(!matched.is_empty_accept());
    }

    #[test]
    fn file_drop_filter_accepts_any_extension_by_default() {
        let filter = FileDropFilter::new();
        let paths = ExternalPaths::from_paths([PathBuf::from("/tmp/archive.unknown")]);
        let matched = filter.evaluate(&paths);

        assert_eq!(paths.to_text(), "external paths: 1 paths, empty false");
        assert!(!paths.to_text().contains("archive"));
        assert_eq!(
            filter.to_text(),
            "file drop filter: extensions 0, extension-filter false, max-files false"
        );
        assert!(matched.is_clean_accept());
        assert_eq!(
            matched.to_text(),
            "file drop match: accepted 1, rejected 0, clean true, empty-accept false"
        );
        assert_eq!(
            matched.into_accepted(),
            vec![PathBuf::from("/tmp/archive.unknown")]
        );
    }

    #[test]
    fn external_paths_can_return_clean_accepted_paths() {
        let filter = FileDropFilter::video().max_files(2);
        let clean = ExternalPaths::from_paths([
            PathBuf::from("/tmp/trailer.mp4"),
            PathBuf::from("/tmp/clip.MOV"),
        ]);
        let mixed = ExternalPaths::from_paths([
            PathBuf::from("/tmp/trailer.mp4"),
            PathBuf::from("/tmp/notes.txt"),
        ]);

        assert_eq!(
            clean.accepted_by(&filter),
            Some(vec![
                PathBuf::from("/tmp/trailer.mp4"),
                PathBuf::from("/tmp/clip.MOV")
            ])
        );
        assert_eq!(mixed.accepted_by(&filter), None);
    }

    #[test]
    fn external_drop_data_models_files_text_and_urls() {
        let empty = ExternalDropData::new();
        assert!(empty.is_empty());
        assert_eq!(
            empty.to_text(),
            "external drop data: paths 0, files 0, file-bytes 0, text false, text-bytes 0, urls 0, empty true"
        );

        let filter = FileDropFilter::images().max_files(1);
        let data = ExternalDropData::from_paths([PathBuf::from("/tmp/poster.png")])
            .with_text("Poster")
            .with_url("https://example.com/poster.png");

        assert!(data.has_paths());
        assert!(data.has_text());
        assert!(data.has_urls());
        assert_eq!(data.path_count(), 1);
        assert_eq!(data.text_len_bytes(), 6);
        assert_eq!(data.url_count(), 1);
        assert!(!data.is_empty());
        assert_eq!(
            data.to_text(),
            "external drop data: paths 1, files 0, file-bytes 0, text true, text-bytes 6, urls 1, empty false"
        );
        assert!(!data.to_text().contains("/tmp"));
        assert!(!data.to_text().contains("Poster"));
        assert!(!data.to_text().contains("example.com"));
        assert_eq!(data.text_value(), Some("Poster"));
        assert_eq!(data.urls(), &["https://example.com/poster.png".to_string()]);
        assert_eq!(
            data.accepted_paths_by(&filter),
            Some(vec![PathBuf::from("/tmp/poster.png")])
        );
    }

    #[test]
    fn external_drop_data_accepts_portable_file_bytes_without_exposing_names() {
        let source_path = PathBuf::from("/private/report.DOCX");
        let file = ExternalFile::new(
            "../report.DOCX",
            Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            b"PK\x03\x04".to_vec(),
        )
        .with_source_path(source_path.clone());
        assert_eq!(file.name(), "report.DOCX");
        assert_eq!(file.extension(), Some("DOCX"));
        assert_eq!(file.bytes(), b"PK\x03\x04");
        assert_eq!(file.source_path(), Some(source_path.as_path()));
        assert!(!file.to_text().contains("report"));
        assert!(!file.to_text().contains("/private"));

        let data = ExternalDropData::from_files([file]);
        assert!(data.has_files());
        assert!(!data.has_paths());
        assert_eq!(data.file_count(), 1);
        assert_eq!(data.file_bytes(), 4);
        assert!(data.accepted_by(&FileDropFilter::new().extensions(["docx"])));
        assert!(!data.accepted_by(&FileDropFilter::new().extensions(["xlsx"])));
        assert_eq!(
            data.to_text(),
            "external drop data: paths 0, files 1, file-bytes 4, text false, text-bytes 0, urls 0, empty false"
        );
    }

    #[test]
    fn external_drop_data_parses_uri_lists() {
        let data = ExternalDropData::from_uri_list(
            "# comment\nfile:///tmp/photo.png\nhttps://example.com/item\n\n",
        );

        assert_eq!(data.paths().paths(), &[PathBuf::from("/tmp/photo.png")]);
        assert_eq!(data.urls(), &["https://example.com/item".to_string()]);
        assert!(!data.has_text());
    }

    #[test]
    fn external_drop_data_extracts_urls_from_plain_text() {
        let data = ExternalDropData::from_plain_text(
            "Read this:\nhttps://example.com/a\nnot a url\nfile:///tmp/local.txt",
        );

        assert_eq!(
            data.text_value(),
            Some("Read this:\nhttps://example.com/a\nnot a url\nfile:///tmp/local.txt")
        );
        assert_eq!(
            data.urls(),
            &[
                "https://example.com/a".to_string(),
                "file:///tmp/local.txt".to_string()
            ]
        );
        assert!(data.paths().is_empty());
    }

    #[test]
    fn external_paths_convert_to_general_drop_data() {
        let paths = ExternalPaths::from_paths([PathBuf::from("/tmp/archive.zip")]);
        let data = paths.clone().into_drop_data();

        assert_eq!(ExternalDropData::from(paths), data);
        assert_eq!(data.paths().paths(), &[PathBuf::from("/tmp/archive.zip")]);
        assert!(!data.has_text());
        assert!(!data.has_urls());
    }

    #[test]
    fn external_drop_data_normalizes_drag_values() {
        let paths = ExternalPaths::from_paths([PathBuf::from("/tmp/movie.mp4")]);
        let from_paths = ExternalDropData::from_drag_value(&paths).unwrap();
        assert_eq!(
            from_paths.paths().paths(),
            &[PathBuf::from("/tmp/movie.mp4")]
        );

        let data = ExternalDropData::url("https://example.com/movie");
        let from_data = ExternalDropData::from_drag_value(&data).unwrap();
        assert_eq!(from_data, data);

        assert!(ExternalDropData::from_drag_value(&42usize).is_none());
    }

    #[test]
    fn external_drop_data_accepts_text_or_url_without_files() {
        let filter = FileDropFilter::video();

        assert!(ExternalDropData::text("label").accepted_by(&filter));
        assert!(ExternalDropData::url("https://example.com/video").accepted_by(&filter));
        assert!(
            ExternalDropData::from_paths([PathBuf::from("/tmp/trailer.mp4")]).accepted_by(&filter)
        );
        assert!(
            !ExternalDropData::from_paths([PathBuf::from("/tmp/poster.png")]).accepted_by(&filter)
        );
    }

    #[test]
    fn file_drop_filter_presets_cover_common_media_extensions() {
        assert!(FileDropFilter::images().accepts_path(std::path::Path::new("poster.webp")));
        assert!(FileDropFilter::audio().accepts_path(std::path::Path::new("track.flac")));
        assert!(FileDropFilter::video().accepts_path(std::path::Path::new("movie.mkv")));
        assert!(FileDropFilter::media().accepts_path(std::path::Path::new("movie.mp4")));
        assert!(FileDropFilter::media().accepts_path(std::path::Path::new("voice.opus")));
        assert!(!FileDropFilter::video().accepts_path(std::path::Path::new("notes.txt")));
        assert_eq!(
            FileDropFilter::single_file().configured_max_files(),
            Some(1)
        );
    }
}
