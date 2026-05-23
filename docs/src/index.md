# Kael

**GPU-accelerated UI framework for native desktop apps in Rust.**

Kael replaces Electron with a single Rust crate that gives you everything you need to build production desktop applications — IDEs, video editors, dashboards, design tools — with native GPU performance on macOS, Windows, and Linux.

## What you get

| Layer | What Kael provides |
|-------|-------------------|
| **Widgets** | Button, TextInput, Checkbox, Toggle, RadioGroup, Slider, Select, DatePicker, Modal, Popover, Tabs, Disclosure, Progress, Toast, Splitter, and more |
| **Layout** | GPU-accelerated flexbox via Taffy, responsive sizing, scroll containers |
| **Rendering** | Metal (macOS), DirectX 11 (Windows), Vulkan (Linux) — 120fps capable |
| **State** | Reactive `Entity<T>` system with automatic re-rendering on change |
| **Platform** | File dialogs, system tray, native menus, global hotkeys, notifications, clipboard, printing, auto-updates, session persistence |
| **Advanced** | Plugin system (WASM sandboxed), multi-process IPC, accessibility, theming, gestures |

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
kael = "0.1"
```

Write your first app:

```rust
use kael::*;
use kael::prelude::*;

struct Hello {
    name: SharedString,
}

impl Render for Hello {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x1E1E1E))
            .text_xl()
            .text_color(rgb(0xFFFFFF))
            .child(format!("Hello, {}!", self.name))
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(400.0), px(300.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| Hello { name: "World".into() }),
        ).unwrap();
        cx.activate(true);
    });
}
```

## Platform support

| Platform | Renderer | Status |
|----------|----------|--------|
| macOS | Metal | Stable |
| Windows | DirectX 11 | Stable |
| Linux (X11) | Vulkan/Blade | Stable |
| Linux (Wayland) | Vulkan/Blade | Stable |
