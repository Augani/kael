# Getting Started

## Prerequisites

- **Rust** 1.87+ (edition 2024) — [install via rustup](https://rustup.rs/)
- **Platform dependencies:**

**macOS:** the default build precompiles Metal shaders, which requires the **full Xcode application** — the standalone Command Line Tools do **not** include the Metal compiler. With only Command Line Tools installed, the build fails with `xcrun: error: unable to find utility "metal"`.

```bash
# Install Xcode from the App Store, then point the toolchain at it:
sudo xcode-select -s /Applications/Xcode.app
```

Alternatively, skip the full-Xcode requirement entirely by compiling shaders at app launch — recommended for development:

```bash
cargo run --features kael/runtime_shaders
```

Use `runtime_shaders` for day-to-day development; build with full Xcode for release builds.

**Linux (Ubuntu/Debian):**
```bash
sudo apt-get install -y \
  libwayland-dev libxkbcommon-dev libvulkan-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libdbus-1-dev libnotify-dev
```

**Windows:** Visual Studio Build Tools with C++ workload

## Create a new project

```bash
cargo new my_app
cd my_app
```

Add Kael to `Cargo.toml`:

```toml
[dependencies]
kael = "0.3"
```

## Your first window

Replace `src/main.rs` with:

```rust
use kael::*;
use kael::prelude::*;

struct Counter {
    count: i32,
}

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .bg(rgb(0x1E1E1E))
            .text_color(rgb(0xFFFFFF))
            .child(
                div().text_3xl().child(format!("Count: {}", self.count))
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("decrement")
                            .label("-1")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.count -= 1;
                                        cx.notify();
                                    });
                                }
                            })
                    )
                    .child(
                        button("increment")
                            .label("+1")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.count += 1;
                                        cx.notify();
                                    });
                                }
                            })
                    )
            )
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
            |_, cx| cx.new(|_| Counter { count: 0 }),
        ).unwrap();
        cx.activate(true);
    });
}
```

Run it:
```bash
cargo run
```

## What just happened

1. **`Application::new().run()`** — boots the platform event loop
2. **`cx.open_window()`** — creates a native window with GPU rendering
3. **`cx.new(|_| Counter { count: 0 })`** — creates a reactive `Entity<Counter>`
4. **`impl Render for Counter`** — defines what the entity draws each frame
5. **`cx.notify()`** — tells the framework to re-render after state changes

## Key patterns

### Builder pattern for UI

Every element uses method chaining. No JSX, no templates — just Rust:

```rust
div()
    .flex()
    .flex_col()
    .gap_4()
    .p_4()
    .bg(rgb(0x1E1E1E))
    .text_color(rgb(0xFFFFFF))
    .child("Hello")
```

### Reactive state

State lives in your struct. Mutate it, call `cx.notify()`, and the framework re-renders:

```rust
entity.update(cx, |this, cx| {
    this.count += 1;
    cx.notify();
});
```

### Custom rendering

Every widget accepts `.render_with()` for full visual control:

```rust
button("save")
    .label("Save")
    .render_with(|state, _window, _cx| {
        div()
            .px_4().py_2()
            .rounded(px(8.0))
            .bg(if state.focused { rgb(0x2563eb) } else { rgb(0x3b82f6) })
            .text_color(rgb(0xffffff))
            .child(state.label.unwrap_or_default())
            .into_any_element()
    })
```

## Next steps

- [Core Concepts](core-concepts.md) — understand Entity, Context, and the render cycle
- [Form Controls](form-controls.md) — buttons, inputs, checkboxes, sliders, and more
- [Platform APIs](platform-apis.md) — file dialogs, system tray, notifications
