# Getting Started

## Prerequisites

- **Rust 1.97.1 or newer** with Rust 2024 support. The repository pins the
  supported toolchain in `rust-toolchain.toml`.
- **macOS:** Xcode Command Line Tools. Release builds using precompiled Metal
  shaders require the full Xcode application; development builds can use the
  `kael/runtime_shaders` feature.
- **Windows:** Visual Studio Build Tools with the Desktop development with C++
  workload.
- **Linux:** Vulkan, Wayland/X11, font, keyboard, D-Bus, and udev development
  packages. WebView, audio, capture, and media features add GTK/WebKitGTK, ALSA,
  PipeWire, and FFmpeg packages. The repository's
  [`install-linux-deps.sh`](https://github.com/Augani/kael/blob/main/scripts/ci/install-linux-deps.sh)
  is the canonical Ubuntu/Debian list.
- **Browser:** the `wasm32-unknown-unknown` Rust target and
  `wasm-bindgen-cli` 0.2.122. Optimized release builds also use Binaryen 132.
  Projects created by `kael new` request the Rust target automatically through
  their toolchain file.

With only the macOS Command Line Tools installed, enable runtime shader
compilation during development:

```bash
cargo run --features kael/runtime_shaders
```

## Create an application

The CLI creates a small application using the core framework and optional UI
component layer:

```bash
cargo install kael-cli
kael new my_app
cd my_app
cargo run
```

The generated project uses that same source for the browser target. Install the
packager once, then build and open it locally:

```bash
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --global binaryen@132.0.0
kael web serve
```

Use `kael web build` for optimized `dist/web` deployment files. See
[Browser (WebAssembly)](browser.md) for target-specific dependencies and the
initial browser capability boundary.

To configure a project manually, choose the layer you need:

```toml
[dependencies]
kael = "0.4"
kael_ui = "0.4" # remove this line when building a custom component system
```

`kael_ui` depends on `kael`; the core framework never depends on `kael_ui`.

## Your first window

```rust,ignore
{{#include ../../crates/kael_ui/examples/docs_quickstart.rs}}
```

## What just happened

1. `Application::try_new()` initializes the selected native or browser platform
   and returns startup failures instead of panicking.
2. `kael_ui::init(cx)` registers the component systems used by the optional UI
   layer.
3. `cx.open_window(...)` creates a GPU-rendered native window or the browser's
   `#blade` canvas window.
4. `cx.new(...)` stores the view in a reactive `Entity<Counter>`.
5. `entity.update(...)` mutates the model, and `cx.notify()` invalidates the
   affected view so Kael can render the next state.

## Core patterns

### Compose elements in Rust

Elements use a typed builder API for layout and appearance:

```rust,ignore
div()
    .flex()
    .flex_col()
    .gap_4()
    .p_4()
    .rounded_lg()
    .bg(rgb(0x1e1e1e))
    .text_color(rgb(0xffffff))
    .child("Hello")
```

### Keep state in entities

```rust,ignore
entity.update(cx, |state, cx| {
    state.count += 1;
    cx.notify();
});
```

### Treat platform support as data

```rust,ignore
use kael::{CapabilityReport, PlatformFeature};

let capabilities = CapabilityReport::current();
if capabilities.is_supported(PlatformFeature::GlobalHotkeys) {
    // Enable the native workflow.
} else {
    // Keep a deliberate fallback or explain the platform requirement.
}
```

### Add only the batteries the product needs

WebView, media, storage, documents, diagnostics, icons, PDF, notifications,
sharing, and other integrations are feature-gated or provided by focused
support crates. Start from the smallest dependency set and add capabilities when
the product requires them.

## Next steps

- [Core Concepts](core-concepts.md) — entities, contexts, rendering, and ownership
- [API Documentation](api-documentation.md) — crate/module map and docs.rs links
- [Component Library](component-library.md) — brandable ready-made UI
- [Platform APIs](platform-apis.md) — native services and capability checks
- [Testing](testing.md) — headless and platform-aware verification
- [Examples Gallery](examples.md) — Astryx and the application templates
