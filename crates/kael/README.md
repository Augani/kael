# Kael

A GPU-accelerated desktop UI framework for building native applications in Rust.

Renders via Metal (macOS), DirectX 11 (Windows), and Vulkan (Linux). Apps are pure Rust with
native performance and 120fps rendering through dirty tracking and render-on-demand.

## Quick Start

```toml
[dependencies]
kael = "0.1"
```

```rust
use kael::*;
use kael::prelude::*;

struct Counter { count: i32 }

impl Render for Counter {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        div()
            .flex().flex_col().gap_2()
            .child(format!("Count: {}", self.count))
            .child(
                button("inc", "Increment")
                    .on_click(move |_, window, cx| {
                        entity.update(cx, |this, cx| {
                            this.count += 1;
                            cx.notify();
                        });
                    }),
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

## What's Included

- **42+ UI primitives**: Button, TextInput, Checkbox, Toggle, Slider, Select, DatePicker, Modal, Popover, Tabs, Disclosure, lists, and more
- **Flexbox layout** via Taffy with responsive styling
- **Entity-based reactive state**: `Entity<T>`, `cx.new()`, `cx.notify()`, `cx.observe()`, `cx.subscribe()`
- **Platform APIs**: file dialogs, system tray, notifications, global hotkeys, printing, clipboard, auto-updates, WebViews
- **Theming**: JSON/TOML themes with hot-reload
- **Accessibility**: screen reader roles, keyboard navigation, focus management
- **Animation**: keyframe and spring animations, Lottie playback
- **Canvas**: stroked/filled paths, shapes, transforms
- **Plugin system**: WASM-sandboxed extensions

## Platform Support

| Platform | Renderer | Status |
|----------|----------|--------|
| macOS | Metal | Full support |
| Linux (X11) | Vulkan | Full support |
| Linux (Wayland) | Vulkan | Full support |
| Windows | DirectX 11 | Full support |

## Documentation

- [Guide & API Reference](https://augani.github.io/kael/)
- [Examples](https://augani.github.io/kael/examples.html)
- [LLM-friendly reference](https://augani.github.io/kael/llms.html)

## Acknowledgements

Kael is built on top of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), the GPU-accelerated UI framework originally created by [Zed Industries](https://zed.dev) for the Zed code editor. We are grateful for their foundational work. The original GPUI code is copyright 2022-2025 Zed Industries, Inc. and licensed under Apache-2.0.

## License

Apache-2.0 — see [LICENSE](LICENSE-APACHE) for details.
