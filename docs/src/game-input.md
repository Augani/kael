# Game input

Kael exposes one window-level API for controller state and relative mouse input
on desktop and WebAssembly builds. Enable `game-input`; it is also included by
`portable-services` and `browser-full`.

```toml
kael = { version = "0.4", features = ["game-input"] }
```

Browser builds read `navigator.getGamepads()` directly. Native builds use gilrs
for mapped controllers on macOS, Windows, Linux, and FreeBSD. Both paths produce
the browser-standard four-axis and 17-button ordering when
`GamepadMapping::Standard` is reported. Stick Y is normalized to `-1` up and `1`
down on every target.

## Poll on display frames

Controller APIs are state snapshots, not UI events. Polling from a millisecond
timer wastes wakeups, can sample the same device state repeatedly, and drifts
away from rendering. `Window::on_gamepad_frame` coalesces onto Kael's display
frame callback instead:

```rust,ignore
let polling = window.on_gamepad_frame(|sample, window, cx| {
    match sample {
        Ok(snapshot) => {
            for pad in &snapshot.gamepads {
                let steer = pad.axis(StandardGamepadAxis::LeftStickX);
                let jump = pad.button(StandardGamepadButton::South).pressed;
                update_game(steer, jump, window, cx);
            }
            GamepadFrameControl::Continue
        }
        Err(error) => {
            show_input_error(error, cx);
            GamepadFrameControl::Stop
        }
    }
});
```

Keep the returned `GamepadFrameSubscription` alive. Dropping or cancelling it
prevents the already-coalesced next-frame callback from polling. For an existing
game loop, call `Window::gamepads()` once inside its own `on_next_frame`
callback instead.

Every snapshot is bounded:

- at most 16 connected controllers;
- at most 32 axes and 64 buttons per controller;
- at most 1,024 native controller events drained per display frame;
- controller identifiers truncated at a valid UTF-8 boundary to 256 bytes;
- non-finite analog values become zero and all values are clamped.

`event_budget_exhausted` reports when a native event storm reached the per-frame
drain limit. The next display frame continues the drain without blocking the
current render.

## Pointer lock and relative motion

Check `window.game_input_capabilities()` before presenting a lock affordance.
In a browser, call `request_pointer_lock()` synchronously from a trusted click or
key handler; do not await unrelated work first.

```rust,ignore
div()
    .on_click(|_, window, _| {
        if let Err(error) = window.request_pointer_lock() {
            eprintln!("pointer lock unavailable: {error}");
        }
    })
    .on_pointer_event(|event, _, _| {
        rotate_camera(event.movement.x.0, event.movement.y.0);
    })
```

The lifecycle is explicit:

- `Unlocked` before a request;
- `Requesting` while a browser or Wayland compositor decides;
- `Locked` only after `pointerlockchange` or the Wayland `locked` event confirms
  ownership; macOS, Windows, and X11 acquire synchronously;
- `pointer_lock_error()` contains the latest synchronous or asynchronous
  rejection. A synchronous browser exception restores `Unlocked` immediately
  and is also returned as a typed `GameInputError`; `NotAllowedError` maps to
  `UserGestureRequired` so applications can request a fresh trusted activation.

`exit_pointer_lock()` only releases resources owned by that Kael window.
Destroying a window or losing focus releases native confinement, restores the
cursor, and returns the state to `Unlocked`; browser window destruction removes
the document listeners. `PointerInputEvent::movement` carries the unbounded
relative delta on every supported backend, while absolute `position` remains a
stable in-window coordinate.

Native implementations use CoreGraphics cursor disassociation on macOS, Raw
Input plus client-area confinement on Windows, and XI2 raw motion plus an X11
pointer grab on X11. Wayland support is intentionally runtime-conditional: the
active seat must expose a pointer and the compositor must advertise both
`pointer-constraints-v1` and `relative-pointer-v1`. A Wayland compositor may
defer or revoke the lock; compositors lacking either protocol report
`GameInputAvailability::Unsupported`. Applications keep one code path by
branching on `GameInputCapabilities::pointer_lock`.

The maintained GTK4 WebView host binds those protocols on GTK's exact GDK-owned
Wayland connection and uses the active GDK surface and pointer. On X11 it uses
XI2 raw motion plus a pointer grab tied to the GTK surface XID. Relative motion,
focus-loss cleanup, cursor restoration, the retained GSK scene, and WebKitGTK 6
children therefore share one window lifecycle on either compositor. Wayland
motion is queued until after protocol dispatch releases backend state,
preventing input callbacks from re-entering the native host borrow.

## Release proof

Run the deterministic retained-window smoke:

```bash
bash scripts/verify-browser-game-input-smoke.sh
```

On a macOS desktop host, exercise the real native cursor and focus-loss cleanup
path with:

```bash
cargo run -p kael --example native_pointer_lock_smoke --no-default-features
```

The Linux compositor-backed gates verify Wayland protocol discovery and the
GTK4 X11/XWayland selection, raw handles, pointer-lock backend, and an
event-driven idle interval with no permanent frame clock:

```bash
bash scripts/ci/run-linux-webview-wayland-gtk4.sh
bash scripts/ci/run-linux-webview-xwayland.sh
```

It packages the real Wasm example and verifies capability discovery, standard
axis/button mapping, input bounds, pointer lock/change/release/error lifecycle,
typed synchronous DOM exceptions, relative movement, and display-frame polling
in headless Chromium. The mock device and pointer-lock provider make CI
deterministic. Products should also acceptance-test real controller visibility
and pointer-lock policy in the exact browser embedding modes they ship.
