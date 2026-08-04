# kael

The native, GPU-accelerated application framework at the center of
[Kael](https://github.com/Augani/kael).

`kael` provides the application runtime and low-level UI primitives: retained
rendering, layout, text, elements, state, windows, input, accessibility,
animation, async work, and native platform services. It does not depend on the
optional `kael_ui` component library, so applications can build and brand their
own component system directly on these primitives.

```toml
[dependencies]
kael = "0.3"
```

WebView support is opt-in, so ordinary native applications do not pull Wry,
GTK, or WebKit. Enable it only when the application embeds web content:

```toml
[dependencies]
kael = { version = "0.3", features = ["webview"] }
```

```rust,no_run
use kael::prelude::*;
use kael::{Application, Window, WindowOptions, div};

struct Hello;

impl Render for Hello {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().items_center().justify_center().child("Hello, Kael!")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx| {
        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Hello)
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}
```

Kael targets macOS (Metal), Windows (DirectX 11), and Linux X11/Wayland
(Vulkan through Blade). OS integrations differ by host; use
`CapabilityReport::current()` when a product requires a specific service.

The minimum supported Rust version is 1.97. The crate uses Rust 2024.

The optional `agent-tools` feature exposes structured capability-planning
metadata. It is disabled by default and is not required to build applications.

Kael began as a fork of GPUI, created by Zed Industries. It is an independent
project and is not affiliated with or endorsed by Zed Industries.

Licensed under Apache-2.0. See `LICENSE-APACHE` in this package.
