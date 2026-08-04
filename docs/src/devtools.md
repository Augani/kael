# Developer Tools

Kael ships an in-app element inspector — the desktop equivalent of a browser's
DevTools. You hover and click any element to see its id, layout bounds, style
summary, position in the element tree, and a live frame-timing strip.

The picking machinery (hit-testing, element-state reflection, the side panel
geometry) lives in the core `kael` crate. The *panel UI* is provided by
`kael_ui`, so the inspector adopts your theme tokens automatically.

## One-call setup

Call `kael_ui::devtools::install_inspector` once at startup, behind
`#[cfg(debug_assertions)]` so release builds never include it:

```rust
use kael::{Application, App, KeyBinding, actions};

#[cfg(debug_assertions)]
actions!(myapp, [ToggleInspector]);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx: &mut App| {
        kael_ui::init(cx);

        #[cfg(debug_assertions)]
        {
            kael_ui::devtools::install_inspector(cx);
            cx.bind_keys([KeyBinding::new("cmd-alt-i", ToggleInspector, None)]);
        }

        // ... open your window ...
    });
    Ok(())
}
```

`install_inspector` does two things:

1. Registers the default inspector **renderer** via `App::set_inspector_renderer`.
   Without it, toggling the inspector opens a blank 30rem strip — the renderer is
   the missing piece that turns that strip into a populated panel.
2. Registers a `DivInspectorState` **reflector** via
   `App::register_inspector_element`, so every `div` reports its bounds, content
   size, and base style to the panel when picked.

## Toggling it

`install_inspector` only registers the renderer; it does not bind a key — that
is your app's choice. Wire an action to `Window::toggle_inspector`:

```rust
#[cfg(debug_assertions)]
let root = root.on_action(|_: &ToggleInspector, window, cx| {
    window.toggle_inspector(cx);
});
```

The `dashboard` template binds **`Cmd-Alt-I`** to this in debug builds. Toggling
on enters *picking mode*: hover an element to highlight it, click to pin the
selection. Toggle again to close the panel.

## What the panel shows

| Section | Contents |
| --- | --- |
| **Path** | A breadcrumb of the picked element's `GlobalElementId`, deepening one indent per level, ending with the `file:line:col` where the element was constructed. |
| **Element** | Instance index, layout `origin`, `size`, and children `content` size — read from `DivInspectorState`. |
| **Style** | Any explicitly set `display`, `size`, `background`, and `flex` direction from the element's base style. Rows for unset properties are omitted. |
| **Frames** | Recent frame count, average frame time, derived FPS, and p95 / p99 frame times. |

## Frame timing

In debug builds (and with the `inspector` feature) every window records a
`FrameRecord` at the end of each `Window::draw`. The rolling timeline — a 300-frame
ring buffer — is exposed via `Window::frame_timeline()`:

```rust
let timeline = window.frame_timeline();
let avg_us = timeline.average_duration_us();   // Option<f64>
let p95_us = timeline.p95_duration_us();       // Option<u64>
let jank   = timeline.detect_jank(16_667);     // frames over ~60fps budget
```

The recording hook is gated behind `cfg(any(feature = "inspector", debug_assertions))`
and does nothing in release builds, so there is no runtime cost in production.

## Building without a Metal toolchain

The inspector lives behind `debug_assertions`, so a plain debug build includes
it. If you build on macOS without Xcode's `metal` compiler, add the
`runtime_shaders` feature so shaders compile at launch instead of at build time:

```sh
cargo run -p your-app --features kael/runtime_shaders
```
