# One codebase, desktop and web

Yes: the aim is one product codebase. A Kael application keeps its views, state,
layout, components, retained drawing, virtualization, animations, and product
logic in Rust, then selects a native or browser host at build time.

That does not mean every operating-system ability exists inside a browser
sandbox. Kael keeps the shared application surface large and makes the remaining
differences explicit, so portable code is the default and platform branches stay
small and intentional.

## Start from one entry point

Projects created by `kael new` arrange target dependencies while keeping one
`main.rs`:

```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
kael = { version = "0.4", features = ["runtime_shaders"] }
kael_ui = "0.4"

[target.'cfg(target_arch = "wasm32")'.dependencies]
kael = { version = "0.4", default-features = false, features = ["browser"] }
kael_ui = { version = "0.4", default-features = false, features = ["browser"] }
```

The application entry point remains normal Kael code:

```rust,ignore
{{#include ../../crates/kael_ui/examples/docs_quickstart.rs}}
```

Build either target:

```bash
# Native desktop
cargo run

# Local browser build
kael web serve

# Optimized static deployment in dist/web
kael web build
```

## What remains the same

| Product layer | Shared contract |
| --- | --- |
| Interface | Elements, components, layout, text, images, SVG, canvas, effects |
| Application state | Entities, observation, actions, keybindings, async tasks |
| Scale | Virtual lists and grids, recycling, bounded caches, spatial culling |
| Interaction | Pointer, keyboard, wheel, focus, IME, clipboard, accessibility actions |
| Documents | Byte import/export, recovery snapshots, OOXML, PDF, Markdown |
| Network and work | HTTP, WebSockets, typed background worker requests |
| Visual behavior | Retained scenes, animation policy, reduced motion, high-DPI layout |

The browser sends the same retained `Scene` to WebGL2. It does not rebuild the
interface as HTML and it does not run the application inside a WebView.

## What the host adapts

- **Rendering:** Metal on macOS, Direct3D 11 on Windows, Vulkan through Blade on
  Linux, and WebGL2 in the browser.
- **Windows:** native OS windows on desktop; independent retained surfaces inside
  the browser page on the web.
- **Files:** native paths and Save As on desktop; byte-backed pickers, drops, and
  Blob downloads in the browser.
- **Storage:** native SQLite/JSON and OS keychains where selected; IndexedDB and
  bounded browser key/value storage on the web.
- **System services:** printing, capture, audio, notifications, sharing, and
  WebViews use the host implementation and permission model.

Keep product code on the shared APIs. Use native paths, subprocesses, raw window
handles, or system credentials only behind a capability decision.

## Branch on capability, not platform names

```rust,ignore
use kael::{CapabilityReport, PlatformFeature};

let report = CapabilityReport::current();

if report.is_supported(PlatformFeature::GlobalHotkeys) {
    enable_global_shortcut_workflow();
} else {
    enable_in_app_shortcut_workflow();
}
```

This survives more environments than `if cfg!(target_os = ...)`: Linux desktop
services vary, browser permissions can be denied, and optional features may not
be compiled into a particular build.

## Keep platform-owned surfaces deliberate

WebViews are compatibility islands for OAuth, payments, maps, hosted documents,
or vendor widgets. They are not the default way to build Kael screens. The
retained application continues to own navigation, editors, data surfaces,
commands, and long-lived product state.

Use `WebViewCapabilityReport` before depending on history, cookies, downloads,
custom headers, profiles, permissions, or custom protocols. Browser iframe,
WKWebView, WebView2, and WebKitGTK security models are not identical.

## Test both deliverables

Source parity is not release parity. Before shipping:

1. Exercise native and optimized Wasm builds.
2. Test your supported browsers and desktop backends.
3. Verify high-DPI text, keyboard navigation, IME, screen readers, and reduced
   motion on each target.
4. Measure representative data sizes rather than a blank window.
5. Record the capability report for workflows that depend on the host.

Kael's own browser gates run retained pixels, million-row virtualization,
suite-scale workloads, IME, clipboard, context loss, accessibility bounds,
workers, audio, WebSockets, capture, and lifecycle behavior. Product tests still
need to cover the features and devices the application promises.

Continue with [Browser and WebAssembly](browser.md) for the detailed host
contract or [Suite-scale applications](suite-scale-apps.md) for a maintained
one-source workload.
