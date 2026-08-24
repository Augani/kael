//! Portable, frame-synchronized input for games and other interactive canvases.
//!
//! Controller state is sampled explicitly rather than from a background timer.
//! Use [`Window::gamepads`] for an individual display-frame sample or
//! [`Window::on_gamepad_frame`] for a cancellable display-frame stream.

use std::{cell::Cell, rc::Rc};

/// Maximum controllers returned by one snapshot.
pub const MAX_GAMEPADS: usize = 16;
/// Maximum axes retained for one controller.
pub const MAX_GAMEPAD_AXES: usize = 32;
/// Maximum buttons retained for one controller.
pub const MAX_GAMEPAD_BUTTONS: usize = 64;
/// Maximum native events consumed during one display-frame sample.
pub const MAX_NATIVE_GAMEPAD_EVENTS_PER_FRAME: usize = 1_024;

/// Runtime availability of a game-input facility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameInputAvailability {
    /// The facility is available at runtime.
    Available,
    /// The target or browser does not expose the facility.
    Unsupported,
    /// The native controller feature was not enabled for this build.
    DisabledAtCompileTime,
}

impl GameInputAvailability {
    /// Return whether callers can attempt to use this facility.
    pub fn is_available(self) -> bool {
        self == Self::Available
    }
}

/// Runtime capabilities for a Kael window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameInputCapabilities {
    /// Pointer-lock availability for relative mouse-look input.
    pub pointer_lock: GameInputAvailability,
    /// Game-controller availability.
    pub gamepads: GameInputAvailability,
}

impl GameInputCapabilities {
    /// Construct a capability report.
    pub const fn new(pointer_lock: GameInputAvailability, gamepads: GameInputAvailability) -> Self {
        Self {
            pointer_lock,
            gamepads,
        }
    }
}

/// Category for a portable game-input failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameInputErrorKind {
    /// The API is not available on this runtime.
    Unsupported,
    /// A native input backend could not initialize.
    InitializationFailed,
    /// The browser or operating system rejected the request.
    Rejected,
    /// The call must originate in a trusted user gesture.
    UserGestureRequired,
    /// A platform API failed for another reason.
    Platform,
}

/// A stable, cloneable game-input error suitable for retained UI state.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct GameInputError {
    kind: GameInputErrorKind,
    message: String,
}

impl GameInputError {
    /// Construct an error with a stable category and a diagnostic message.
    pub fn new(kind: GameInputErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// Return the stable error category.
    pub fn kind(&self) -> GameInputErrorKind {
        self.kind
    }

    /// Return the platform diagnostic.
    pub fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(GameInputErrorKind::Unsupported, message)
    }
}

/// Current pointer-lock lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerLockStatus {
    /// Pointer lock is not implemented by this runtime.
    Unsupported,
    /// The pointer is free.
    Unlocked,
    /// A trusted-gesture request is waiting for the browser or OS.
    Requesting,
    /// The pointer is locked to this Kael window.
    Locked,
}

/// Backend-neutral pointer-lock lifecycle bookkeeping.
///
/// Native backends keep their operating-system resources separately, but use
/// this state machine so synchronous failures and asynchronous lock changes
/// have identical observable semantics on every platform.
#[cfg(any(
    test,
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd"
))]
#[derive(Clone, Debug)]
pub(crate) struct NativePointerLockState {
    status: PointerLockStatus,
    error: Option<GameInputError>,
}

#[cfg(any(
    test,
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd"
))]
impl NativePointerLockState {
    pub(crate) fn new(supported: bool) -> Self {
        Self {
            status: if supported {
                PointerLockStatus::Unlocked
            } else {
                PointerLockStatus::Unsupported
            },
            error: None,
        }
    }

    pub(crate) fn status(&self) -> PointerLockStatus {
        self.status
    }

    pub(crate) fn error(&self) -> Option<GameInputError> {
        self.error.clone()
    }

    /// Start a lock request. Returns `false` when this window already owns or
    /// is already waiting for the lock, making repeated requests idempotent.
    pub(crate) fn begin_request(&mut self) -> Result<bool, GameInputError> {
        match self.status {
            PointerLockStatus::Unsupported => {
                let error = GameInputError::unsupported(
                    "pointer lock is unsupported by this window backend",
                );
                self.error = Some(error.clone());
                Err(error)
            }
            PointerLockStatus::Unlocked => {
                self.status = PointerLockStatus::Requesting;
                self.error = None;
                Ok(true)
            }
            PointerLockStatus::Requesting | PointerLockStatus::Locked => Ok(false),
        }
    }

    pub(crate) fn lock(&mut self) {
        if self.status != PointerLockStatus::Unsupported {
            self.status = PointerLockStatus::Locked;
            self.error = None;
        }
    }

    pub(crate) fn fail(&mut self, error: GameInputError) -> GameInputError {
        if self.status != PointerLockStatus::Unsupported {
            self.status = PointerLockStatus::Unlocked;
        }
        self.error = Some(error.clone());
        error
    }

    pub(crate) fn unlock(&mut self) {
        if self.status != PointerLockStatus::Unsupported {
            self.status = PointerLockStatus::Unlocked;
        }
    }
}

/// Mapping applied to a controller snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GamepadMapping {
    /// Browser-standard button and axis ordering.
    Standard,
    /// Device-specific raw ordering.
    Raw,
}

/// Standard axis indices shared with the browser Gamepad specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum StandardGamepadAxis {
    /// Left stick horizontal axis: `-1` left, `1` right.
    LeftStickX = 0,
    /// Left stick vertical axis: `-1` up, `1` down.
    LeftStickY = 1,
    /// Right stick horizontal axis: `-1` left, `1` right.
    RightStickX = 2,
    /// Right stick vertical axis: `-1` up, `1` down.
    RightStickY = 3,
}

/// Standard button indices shared with the browser Gamepad specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum StandardGamepadButton {
    /// Bottom face button (A/Cross).
    South = 0,
    /// Right face button (B/Circle).
    East = 1,
    /// Left face button (X/Square).
    West = 2,
    /// Top face button (Y/Triangle).
    North = 3,
    /// Left shoulder button.
    LeftShoulder = 4,
    /// Right shoulder button.
    RightShoulder = 5,
    /// Left analog trigger.
    LeftTrigger = 6,
    /// Right analog trigger.
    RightTrigger = 7,
    /// Select/Back button.
    Select = 8,
    /// Start button.
    Start = 9,
    /// Left stick click.
    LeftStick = 10,
    /// Right stick click.
    RightStick = 11,
    /// Direction pad up.
    DpadUp = 12,
    /// Direction pad down.
    DpadDown = 13,
    /// Direction pad left.
    DpadLeft = 14,
    /// Direction pad right.
    DpadRight = 15,
    /// Home/Guide button.
    Home = 16,
}

/// State of one controller button.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GamepadButtonState {
    /// Normalized analog value in the inclusive `0..=1` range.
    pub value: f32,
    /// Whether the platform considers the button pressed.
    pub pressed: bool,
    /// Whether a touch-sensitive button is currently touched.
    pub touched: bool,
}

impl GamepadButtonState {
    #[cfg(any(
        test,
        all(target_arch = "wasm32", feature = "browser"),
        all(not(target_arch = "wasm32"), feature = "game-input")
    ))]
    pub(crate) fn sanitized(value: f64, pressed: bool, touched: bool) -> Self {
        Self {
            value: finite_clamped(value, 0.0, 1.0),
            pressed,
            touched,
        }
    }
}

/// Immutable state of one connected controller at a display-frame boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct GamepadState {
    /// Stable platform index for this connection.
    pub index: u32,
    /// Bounded platform/controller identifier.
    pub id: String,
    /// Mapping applied to [`Self::axes`] and [`Self::buttons`].
    pub mapping: GamepadMapping,
    /// Device timestamp in milliseconds, or `0` when unavailable.
    pub timestamp_ms: f64,
    /// Normalized axes in the inclusive `-1..=1` range.
    pub axes: Vec<f32>,
    /// Normalized controller buttons.
    pub buttons: Vec<GamepadButtonState>,
}

impl GamepadState {
    /// Read a browser-standard axis, returning `0` when it is absent.
    pub fn axis(&self, axis: StandardGamepadAxis) -> f32 {
        self.axes.get(axis as usize).copied().unwrap_or_default()
    }

    /// Read a browser-standard button, returning a released state when absent.
    pub fn button(&self, button: StandardGamepadButton) -> GamepadButtonState {
        self.buttons
            .get(button as usize)
            .copied()
            .unwrap_or_default()
    }
}

/// Bounded controller sample captured on one display frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadSnapshot {
    /// Connected controllers, capped at [`MAX_GAMEPADS`].
    pub gamepads: Vec<GamepadState>,
    /// Native events consumed to refresh cached state for this sample.
    pub events_drained: usize,
    /// Whether the native per-frame event budget was reached.
    pub event_budget_exhausted: bool,
}

/// Decision returned by a display-frame controller callback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GamepadFrameControl {
    /// Continue sampling on the next display frame.
    Continue,
    /// End this sampling stream after the current callback.
    Stop,
}

/// Cancellation handle for display-frame controller sampling.
///
/// Dropping the handle cancels future samples. The callback is never driven by
/// a timer and at most one sample is scheduled for a rendered display frame.
pub struct GamepadFrameSubscription {
    pub(crate) active: Rc<Cell<bool>>,
}

impl GamepadFrameSubscription {
    pub(crate) fn new(active: Rc<Cell<bool>>) -> Self {
        Self { active }
    }

    /// Cancel future display-frame samples.
    pub fn cancel(&self) {
        self.active.set(false);
    }

    /// Return whether future samples are still requested.
    pub fn is_active(&self) -> bool {
        self.active.get()
    }
}

impl Drop for GamepadFrameSubscription {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(any(
    test,
    all(target_arch = "wasm32", feature = "browser"),
    all(not(target_arch = "wasm32"), feature = "game-input")
))]
pub(crate) fn finite_clamped(value: f64, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        (value as f32).clamp(min, max)
    } else {
        0.0
    }
}

#[cfg(any(
    test,
    all(target_arch = "wasm32", feature = "browser"),
    all(not(target_arch = "wasm32"), feature = "game-input")
))]
pub(crate) fn bounded_id(value: &str) -> String {
    const MAX_ID_BYTES: usize = 256;
    if value.len() <= MAX_ID_BYTES {
        return value.to_owned();
    }
    let mut boundary = MAX_ID_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(all(not(target_arch = "wasm32"), feature = "game-input"))]
mod native {
    use super::*;
    use gilrs::{Axis, Button, Gilrs};
    use std::cell::RefCell;

    enum Runtime {
        Uninitialized,
        Ready(Gilrs),
        Failed(String),
    }

    thread_local! {
        static RUNTIME: RefCell<Runtime> = const { RefCell::new(Runtime::Uninitialized) };
    }

    const AXES: [Axis; 4] = [
        Axis::LeftStickX,
        Axis::LeftStickY,
        Axis::RightStickX,
        Axis::RightStickY,
    ];
    const BUTTONS: [Button; 17] = [
        Button::South,
        Button::East,
        Button::West,
        Button::North,
        Button::LeftTrigger,
        Button::RightTrigger,
        Button::LeftTrigger2,
        Button::RightTrigger2,
        Button::Select,
        Button::Start,
        Button::LeftThumb,
        Button::RightThumb,
        Button::DPadUp,
        Button::DPadDown,
        Button::DPadLeft,
        Button::DPadRight,
        Button::Mode,
    ];

    pub(super) fn poll() -> Result<GamepadSnapshot, GameInputError> {
        RUNTIME.with(|runtime| {
            let mut runtime = runtime.borrow_mut();
            if matches!(*runtime, Runtime::Uninitialized) {
                *runtime = match Gilrs::new() {
                    Ok(gilrs) => Runtime::Ready(gilrs),
                    Err(error) => Runtime::Failed(error.to_string()),
                };
            }
            match &mut *runtime {
                Runtime::Ready(gilrs) => {
                    let mut events_drained = 0;
                    while events_drained < MAX_NATIVE_GAMEPAD_EVENTS_PER_FRAME
                        && gilrs.next_event().is_some()
                    {
                        events_drained += 1;
                    }
                    let event_budget_exhausted =
                        events_drained == MAX_NATIVE_GAMEPAD_EVENTS_PER_FRAME;
                    let gamepads = gilrs
                        .gamepads()
                        .take(MAX_GAMEPADS)
                        .map(|(index, gamepad)| {
                            let axes = AXES
                                .into_iter()
                                .enumerate()
                                .map(|(index, axis)| {
                                    let value = f64::from(gamepad.value(axis));
                                    // gilrs uses positive-up stick Y while the browser
                                    // standard uses negative-up.
                                    let value = if index == 1 || index == 3 {
                                        -value
                                    } else {
                                        value
                                    };
                                    finite_clamped(value, -1.0, 1.0)
                                })
                                .collect();
                            let buttons = BUTTONS
                                .into_iter()
                                .map(|button| {
                                    let value = gamepad
                                        .button_data(button)
                                        .map(|data| f64::from(data.value()))
                                        .unwrap_or_else(|| {
                                            f64::from(u8::from(gamepad.is_pressed(button)))
                                        });
                                    GamepadButtonState::sanitized(
                                        value,
                                        gamepad.is_pressed(button),
                                        gamepad.is_pressed(button),
                                    )
                                })
                                .collect();
                            GamepadState {
                                index: u32::try_from(usize::from(index)).unwrap_or(u32::MAX),
                                id: bounded_id(gamepad.name()),
                                mapping: GamepadMapping::Standard,
                                timestamp_ms: 0.0,
                                axes,
                                buttons,
                            }
                        })
                        .collect();
                    Ok(GamepadSnapshot {
                        gamepads,
                        events_drained,
                        event_budget_exhausted,
                    })
                }
                Runtime::Failed(error) => Err(GameInputError::new(
                    GameInputErrorKind::InitializationFailed,
                    format!("native gamepad backend failed to initialize: {error}"),
                )),
                Runtime::Uninitialized => unreachable!("native gamepad runtime initialized above"),
            }
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "game-input"))]
pub(crate) fn native_gamepads() -> Result<GamepadSnapshot, GameInputError> {
    native::poll()
}

#[cfg(any(target_arch = "wasm32", not(feature = "game-input")))]
pub(crate) fn native_gamepads() -> Result<GamepadSnapshot, GameInputError> {
    Err(GameInputError::unsupported(
        "native gamepad support requires Kael's `game-input` feature",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analog_values_are_finite_and_bounded() {
        assert_eq!(finite_clamped(f64::NAN, -1.0, 1.0), 0.0);
        assert_eq!(finite_clamped(f64::INFINITY, -1.0, 1.0), 0.0);
        assert_eq!(finite_clamped(-2.0, -1.0, 1.0), -1.0);
        assert_eq!(finite_clamped(2.0, -1.0, 1.0), 1.0);
        assert_eq!(GamepadButtonState::sanitized(2.0, true, false).value, 1.0);
    }

    #[test]
    fn controller_ids_are_utf8_safe_and_bounded() {
        let id = "🎮".repeat(100);
        let bounded = bounded_id(&id);
        assert!(bounded.len() <= 256);
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[test]
    fn standard_accessors_default_absent_controls() {
        let state = GamepadState {
            index: 0,
            id: "test".into(),
            mapping: GamepadMapping::Standard,
            timestamp_ms: 0.0,
            axes: vec![0.25],
            buttons: Vec::new(),
        };
        assert_eq!(state.axis(StandardGamepadAxis::LeftStickX), 0.25);
        assert_eq!(state.axis(StandardGamepadAxis::RightStickY), 0.0);
        assert_eq!(
            state.button(StandardGamepadButton::South),
            GamepadButtonState::default()
        );
    }

    #[test]
    fn dropping_frame_subscription_cancels_it() {
        let active = Rc::new(Cell::new(true));
        drop(GamepadFrameSubscription::new(active.clone()));
        assert!(!active.get());
    }

    #[test]
    fn native_pointer_lock_transitions_are_deterministic() {
        let mut state = NativePointerLockState::new(true);
        assert_eq!(state.status(), PointerLockStatus::Unlocked);
        assert_eq!(state.begin_request(), Ok(true));
        assert_eq!(state.status(), PointerLockStatus::Requesting);
        assert_eq!(state.begin_request(), Ok(false));

        state.lock();
        assert_eq!(state.status(), PointerLockStatus::Locked);
        assert_eq!(state.begin_request(), Ok(false));

        state.unlock();
        assert_eq!(state.status(), PointerLockStatus::Unlocked);
        assert!(state.error().is_none());
    }

    #[test]
    fn native_pointer_lock_preserves_typed_failures() {
        let mut state = NativePointerLockState::new(true);
        state.begin_request().unwrap();
        let failure = GameInputError::new(GameInputErrorKind::Rejected, "focus was lost");
        assert_eq!(state.fail(failure.clone()), failure);
        assert_eq!(state.status(), PointerLockStatus::Unlocked);
        assert_eq!(state.error(), Some(failure));

        let mut unsupported = NativePointerLockState::new(false);
        let error = unsupported.begin_request().unwrap_err();
        assert_eq!(error.kind(), GameInputErrorKind::Unsupported);
        assert_eq!(unsupported.status(), PointerLockStatus::Unsupported);
        assert_eq!(unsupported.error(), Some(error));
    }
}
