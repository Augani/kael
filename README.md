# Kael

A high-performance, GPU-accelerated UI framework for building native desktop applications in Rust.

Built on top of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) from [Zed Industries](https://zed.dev), extended with webviews, media playback, system tray, form controls, rich text, Lottie animations, blur effects, gesture recognition, theming, and more.

Kael targets macOS, Linux, and Windows with platform-native rendering backends (Metal, Vulkan/Blade, DirectX 11) and delivers 60fps with minimal CPU usage through dirty tracking and render-on-demand.

## Features

**Rendering & Graphics**
- GPU-accelerated rendering with per-platform shaders
- Cached surface rendering with dirty tracking (idle at 0% CPU)
- Canvas drawing API with stroked/filled paths, shapes, transforms
- Backdrop blur and frosted glass effects
- Lottie animation playback with background frame rendering
- SVG rendering and icon atlas system
- Device-pixel snapping (`PixelSnapPolicy`) for crisp output at any DPI
- sRGB-correct render pipeline (Metal color space, linear blending)
- Native macOS rendering parity (SF Pro optical sizing, continuous corners, AppKit-matched text)

**Layout & Elements**
- Flexbox and grid layout (via Taffy)
- Rich text with inline elements, entities, and selection
- Recycling lists for heterogeneous content
- Uniform lists for same-height items
- Implicit style transitions (opacity, scale, background, border)
- Explicit keyframe and spring animations
- Elastic (rubber-band) scrolling
- Gesture recognizers (pan, swipe, pinch)
- Form controls: text input, checkbox, toggle, slider, radio, select, date picker

**Platform Integration**
- System tray with menus
- Global hotkeys
- Native file/save dialogs
- Notifications
- Printing
- WebViews (macOS WKWebView, Windows WebView2, Linux WebKitGTK via wry)
- Media playback (audio/video)
- Screen and media capture
- Overlay and click-through windows
- Auto-launch, single instance, dock control
- Biometric authentication
- Power-aware rendering (battery detection, animation throttling)

**Architecture**
- Entity-based reactive state management
- Multi-window support
- In-window layer stack (modals, popovers, anchored layers)
- Focus management and keyboard navigation
- Action dispatch system with keybindings
- Theme system with hot-reload from TOML/JSON
- Extension host with WASM sandboxing
- Process isolation (worker children, extension children)
- IPC transport layer
- Session persistence

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
kael = "0.1"
```

```rust
use kael::*;

struct Counter {
    count: i32,
}

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(format!("Count: {}", self.count))
            .child(
                div()
                    .cursor_pointer()
                    .child("Increment")
                    .on_click(cx.listener(|this, _, _, _| {
                        this.count += 1;
                    })),
            )
    }
}

fn main() {
    Application::new().run(|cx| {
        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Counter { count: 0 })
        })
        .unwrap();
    });
}
```

## Examples

```bash
cargo run -p kael --example hello_world
cargo run -p kael --example animation
cargo run -p kael --example recycling_list
cargo run -p kael --example webview_demo
cargo run -p kael --example daemon_app
cargo run -p kael --example form_controls
cargo run -p kael --example platform_features
cargo run -p kael --example crispness_showcase
cargo run -p kael --example native_comparison
```

## Platform Support

| Platform | Renderer | Status |
|----------|----------|--------|
| macOS | Metal | Full support |
| Linux (X11) | Blade (Vulkan) | Full support |
| Linux (Wayland) | Blade (Vulkan) | Full support |
| Windows | DirectX 11 | Full support |

## Project Structure

```
crates/
├── kael/           # Core framework
├── kael-macros/    # Proc macros (derive Render, Action, etc.)
├── kael-media/     # Media playback integration
├── collections/    # Optimized collection types
├── refineable/     # Style refinement system
├── util/           # Shared utilities
└── ...
```

## Building

```bash
# Check
cargo check -p kael

# Run tests
cargo test -p kael

# Run an example
cargo run -p kael --example hello_world
```

### Linux Dependencies

```bash
# Ubuntu/Debian
sudo apt install libwayland-dev libxkbcommon-dev libvulkan-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libdbus-1-dev libnotify-dev

# Fedora
sudo dnf install wayland-devel libxkbcommon-devel vulkan-loader-devel \
  webkit2gtk4.1-devel gtk3-devel dbus-devel libnotify-devel
```

## Acknowledgements

Kael is built on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the UI framework created by [Zed Industries](https://zed.dev) for the Zed code editor. The original GPUI code is licensed under Apache-2.0.

## License

Apache-2.0 — see [LICENSE](crates/kael/LICENSE-APACHE) for details.

Original GPUI code: Copyright 2022-2025 Zed Industries, Inc.
