//! Deterministic retained-window proof for portable controller and pointer-lock input.

use kael::prelude::*;
use kael::{
    App, Application, Context, GameInputAvailability, GameInputErrorKind, GamepadFrameControl,
    GamepadFrameSubscription, GamepadMapping, GamepadSnapshot, PointerInputEvent,
    PointerLockStatus, Render, StandardGamepadAxis, StandardGamepadButton, Window, WindowOptions,
    div, px,
};

struct GameInputSmoke {
    polling: Option<GamepadFrameSubscription>,
    frame_count: usize,
    gamepad_verified: bool,
    lock_seen: bool,
    relative_motion_seen: bool,
    unlock_seen: bool,
    rejection_requested: bool,
    async_error_seen: bool,
    synchronous_rejection_requested: bool,
    synchronous_error_seen: bool,
}

impl GameInputSmoke {
    fn sample_gamepad(snapshot: &GamepadSnapshot) -> bool {
        let Some(gamepad) = snapshot.gamepads.first() else {
            return false;
        };
        gamepad.mapping == GamepadMapping::Standard
            && gamepad.id.len() <= 256
            && gamepad.axes.len() <= kael::MAX_GAMEPAD_AXES
            && gamepad.buttons.len() <= kael::MAX_GAMEPAD_BUTTONS
            && gamepad.axis(StandardGamepadAxis::LeftStickX) == 0.25
            && gamepad.axis(StandardGamepadAxis::LeftStickY) == -0.5
            && gamepad.axis(StandardGamepadAxis::RightStickX) == 1.0
            && gamepad.axis(StandardGamepadAxis::RightStickY) == -1.0
            && gamepad.button(StandardGamepadButton::South).pressed
            && gamepad.button(StandardGamepadButton::South).value == 1.0
    }

    fn passed(&self) -> bool {
        self.gamepad_verified
            && self.lock_seen
            && self.relative_motion_seen
            && self.unlock_seen
            && self.async_error_seen
            && self.synchronous_error_seen
    }

    fn arm_polling(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.polling.is_some() {
            return;
        }
        let entity = cx.weak_entity();
        self.polling = Some(window.on_gamepad_frame(move |snapshot, window, cx| {
            let Some(entity) = entity.upgrade() else {
                return GamepadFrameControl::Stop;
            };
            entity.update(cx, |this, cx| {
                this.frame_count += 1;
                if let Ok(snapshot) = &snapshot {
                    this.gamepad_verified |= Self::sample_gamepad(snapshot);
                }

                if this.frame_count == 1 {
                    let _ = window.request_pointer_lock();
                    this.lock_seen |= window.pointer_lock_status() == PointerLockStatus::Locked;
                }

                if this.relative_motion_seen && this.unlock_seen && !this.rejection_requested {
                    this.rejection_requested = true;
                    reject_next_pointer_lock();
                    let _ = window.request_pointer_lock();
                    this.async_error_seen = window.pointer_lock_error().is_some()
                        && window.pointer_lock_status() == PointerLockStatus::Unlocked;
                }
                if this.async_error_seen && !this.synchronous_rejection_requested {
                    this.synchronous_rejection_requested = true;
                    throw_next_pointer_lock();
                    let request_error = window.request_pointer_lock().err();
                    let retained_error = window.pointer_lock_error();
                    this.synchronous_error_seen = request_error.as_ref().is_some_and(|error| {
                        error.kind() == GameInputErrorKind::UserGestureRequired
                    }) && retained_error.as_ref().is_some_and(
                        |error| error.kind() == GameInputErrorKind::UserGestureRequired,
                    ) && window.pointer_lock_status()
                        == PointerLockStatus::Unlocked;
                }
                cx.notify();
                if this.passed() || this.frame_count >= 120 {
                    GamepadFrameControl::Stop
                } else {
                    GamepadFrameControl::Continue
                }
            })
        }));
    }
}

impl Render for GameInputSmoke {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.arm_polling(window, cx);
        let capabilities = window.game_input_capabilities();
        let capabilities_verified = capabilities.pointer_lock == GameInputAvailability::Available
            && capabilities.gamepads == GameInputAvailability::Available;
        publish_result(capabilities_verified, self);

        div()
            .id("game-input-smoke")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .p(px(24.0))
            .bg(kael::rgb(0x111827))
            .text_color(kael::rgb(0xe5e7eb))
            .on_pointer_event(cx.listener(|this, event: &PointerInputEvent, window, cx| {
                let movement_x = f32::from(event.movement.x);
                let movement_y = f32::from(event.movement.y);
                if movement_x != 0.0 || movement_y != 0.0 {
                    this.relative_motion_seen = movement_x == 7.0 && movement_y == -4.0;
                    let _ = window.exit_pointer_lock();
                    this.unlock_seen = window.pointer_lock_status() == PointerLockStatus::Unlocked;
                    cx.notify();
                }
            }))
            .child(div().text_size(px(24.0)).child("Kael portable game input"))
            .child(format!("display-frame samples: {}", self.frame_count))
            .child(format!("capabilities: {capabilities:?}"))
            .child(format!("gamepad mapping/bounds: {}", self.gamepad_verified))
            .child(format!("pointer lock acquired: {}", self.lock_seen))
            .child(format!(
                "relative movement + release: {} / {}",
                self.relative_motion_seen, self.unlock_seen
            ))
            .child(format!(
                "async rejection surfaced: {}",
                self.async_error_seen
            ))
            .child(format!(
                "synchronous rejection surfaced: {}",
                self.synchronous_error_seen
            ))
    }
}

fn main() {
    Application::try_new()
        .expect("failed to initialize Kael")
        .run(|cx: &mut App| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| GameInputSmoke {
                    polling: None,
                    frame_count: 0,
                    gamepad_verified: false,
                    lock_seen: false,
                    relative_motion_seen: false,
                    unlock_seen: false,
                    rejection_requested: false,
                    async_error_seen: false,
                    synchronous_rejection_requested: false,
                    synchronous_error_seen: false,
                })
            })
            .expect("failed to open Kael game-input smoke window");
        });
}

#[cfg(target_arch = "wasm32")]
fn reject_next_pointer_lock() {
    use wasm_bindgen::JsValue;
    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(
            window.as_ref(),
            &JsValue::from_str("__kaelRejectPointerLock"),
            &JsValue::TRUE,
        );
    }
}

#[cfg(target_arch = "wasm32")]
fn throw_next_pointer_lock() {
    use wasm_bindgen::JsValue;
    if let Some(window) = web_sys::window() {
        let _ = js_sys::Reflect::set(
            window.as_ref(),
            &JsValue::from_str("__kaelThrowPointerLock"),
            &JsValue::TRUE,
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn reject_next_pointer_lock() {}

#[cfg(not(target_arch = "wasm32"))]
fn throw_next_pointer_lock() {}

#[cfg(target_arch = "wasm32")]
fn publish_result(capabilities: bool, state: &GameInputSmoke) {
    let gamepad = state.gamepad_verified;
    let lock = state.lock_seen;
    let movement = state.relative_motion_seen;
    let unlock = state.unlock_seen;
    let rejection = state.async_error_seen;
    let synchronous_rejection = state.synchronous_error_seen;
    let frame_count = state.frame_count;
    let passed =
        capabilities && gamepad && lock && movement && unlock && rejection && synchronous_rejection;
    if let Some(root) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.document_element())
    {
        let _ = root.set_attribute("data-kael-game-input-capabilities", status(capabilities));
        let _ = root.set_attribute("data-kael-gamepad", status(gamepad));
        let _ = root.set_attribute("data-kael-pointer-lock", status(lock));
        let _ = root.set_attribute("data-kael-relative-motion", status(movement));
        let _ = root.set_attribute("data-kael-pointer-unlock", status(unlock));
        let _ = root.set_attribute("data-kael-pointer-rejection", status(rejection));
        let _ = root.set_attribute(
            "data-kael-pointer-synchronous-rejection",
            status(synchronous_rejection),
        );
        let _ = root.set_attribute("data-kael-game-input-frames", &frame_count.to_string());
    }
    let marker = if passed {
        Some("?__kael_game_input_pass__=1&capabilities=passed&gamepad=passed&lock=passed&movement=passed&unlock=passed&rejection=passed&synchronous=passed".to_owned())
    } else if frame_count >= 120 {
        Some(format!(
            "?__kael_game_input_failed__=1&capabilities={}&gamepad={}&lock={}&movement={}&unlock={}&rejection={}&synchronous={}",
            status(capabilities),
            status(gamepad),
            status(lock),
            status(movement),
            status(unlock),
            status(rejection),
            status(synchronous_rejection),
        ))
    } else {
        None
    };
    if let Some(marker) = marker
        && let Some(window) = web_sys::window()
        && window.location().search().as_deref() != Ok(marker.as_str())
    {
        let _ = window.location().set_search(&marker);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish_result(_capabilities: bool, _state: &GameInputSmoke) {}

#[cfg(target_arch = "wasm32")]
fn status(passed: bool) -> &'static str {
    if passed { "passed" } else { "pending" }
}
