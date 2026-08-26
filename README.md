# Kael

Kael is a GPU-accelerated application framework for Rust, built to help
ambitious software do more with less: less idle work, less memory pressure,
fewer runtime layers, and one coherent Rust codebase. The same retained UI and
Scene renderer target native macOS, Windows, and Linux applications or a
WebAssembly/WebGL2 canvas in the browser.

**[Read the Kael documentation →](https://augani.github.io/kael/)**

[Get started](https://augani.github.io/kael/getting-started.html) ·
[Desktop and web](https://augani.github.io/kael/one-codebase.html) ·
[Components](https://augani.github.io/kael/component-library.html) ·
[Web deployment](https://augani.github.io/kael/web-deployment.html) ·
[API reference](https://augani.github.io/kael/api-documentation.html)

Kael is not defined by another application stack. It is a complete foundation
for responsive PC applications: retained rendering, application state, native
platform services, production tooling, and an optional component system that a
product can reshape around its own brand.

The framework has two deliberate layers:

- `kael` provides the renderer, application runtime, reactive entities, windows,
  input, accessibility, text, layout, animation, and native platform primitives.
- `kael_ui` is optional. It provides a large, themeable component system that
  applications can adopt, restyle for their brand, or replace entirely.

Apps can therefore build their own design system directly on Kael's primitives
without depending on `kael_ui`.

> Kael began as a fork of
> [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), created by
> Zed Industries, and was previously distributed as the adabraka GPUI fork. Kael
> is an independent project and is not affiliated with or endorsed by Zed
> Industries.

## Current status

Kael 0.4 is a pre-1.0 production candidate. The workspace is pinned to Rust
1.97.1, uses Rust 2024 throughout, denies lint warnings in CI, audits its locked
dependencies, and checks macOS, Windows, Linux X11, Linux Wayland, and the
`wasm32-unknown-unknown` browser target.

The public API can still change before 1.0. Applications should pin a compatible
minor release and use `CapabilityReport::current()` when a workflow depends on a
specific OS integration.

## What Kael optimizes for

- **Useful work over background work.** Retained rendering, invalidation, frame
  skipping, virtualization, bounded caches, and GPU budgeting help large apps
  stay responsive without redrawing or retaining more than they need.
- **One application architecture.** UI, state, async work, platform services,
  diagnostics, packaging, and updates can share Rust types and error handling.
- **Product ownership.** Start from low-level primitives or adopt `kael_ui` and
  replace its tokens, composition, and styling without inheriting a fixed look.
- **Real desktop capabilities.** Windows, menus, files, accessibility, input,
  notifications, capture, IPC, plugins, and lifecycle APIs are part of the
  framework rather than unrelated integration projects.
- **Explicit platform truth.** Capability reports expose OS differences so an
  application can provide a deliberate setup path or fallback.

## Choose your layer

Use only the framework primitives:

```toml
[dependencies]
kael = "0.4"
```

WebView and the bundled Reqwest transport are opt-in. Primitive-only native
applications do not pull Wry, GTK, WebKit, Reqwest, or Tokio; enable only the
implementation batteries the product uses:

```toml
[dependencies]
kael = { version = "0.4", features = ["webview"] }
```

```toml
[dependencies]
kael = { version = "0.4", features = ["http-client"] }
```

Specialized batteries stay explicit as well: use `auto-update` for signed
updates and checked downloads, or `lottie` for native Lottie/dotLottie
playback. Applications that do not enable them do not compile their
implementation dependencies.

Or add the ready-made component system:

```toml
[dependencies]
kael = "0.4"
kael_ui = "0.4"
```

`kael_ui` depends on `kael`; `kael` never depends on `kael_ui`.

## Quick start

The CLI creates a fallible, ready-to-run application with both layers:

```bash
cargo install kael-cli
kael new my_app
cd my_app
cargo run
```

Run that same `main.rs` in a browser (the packager is a one-time install):

```bash
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --global binaryen@132.0.0
kael web serve
```

`kael web build` creates an optimized static deployment in `dist/web`.

The generated entry point is intentionally small:

```rust,no_run
use kael_ui::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::tokyo_night());

        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| HelloView)
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}

struct HelloView;

impl Render for HelloView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(Button::new("hello", "Hello, Kael!"))
    }
}
```

## What ships

The core includes:

- retained GPU rendering with Metal on macOS, DirectX 11 on Windows, Vulkan
  through Blade on Linux, and a dedicated WebGL2 Scene renderer in browsers;
- flex and grid layout, rich text, SVG, images, gradients, canvas paths, effects,
  transforms, and render-on-demand animation;
- entity-based state, actions and keybindings, focus, drag and drop, virtual and
  recycling lists, multi-window application state, and async executors;
- AccessKit-based semantics, keyboard navigation, reduced-motion support, and
  platform accessibility bridges;
- menus, dialogs, clipboard, tray, notifications, global hotkeys, deep links,
  printing, WebView islands, capture, single-instance apps, process supervision,
  typed IPC, sessions, plugins, and update plumbing;
- capability reporting so applications can select a setup path or fallback when
  an OS service is unavailable.

`kael_ui` adds theme tokens, brand overrides, form controls, data grids, charts,
editors, navigation, overlays, feedback, media controls, responsive layout
helpers, and virtualized surfaces. Components accept Kael's normal styling API,
so they are starting points rather than a fixed visual identity.

Optional support crates keep product concerns out of the core dependency graph:

| Crate | Purpose |
| --- | --- |
| `kael_storage`, `kael_cache`, `kael_secrets` | Persistence, caching, and native credential storage |
| `kael_net`, `kael_http_client` | Connectivity, authentication, sync, and HTTP |
| `kael_diagnostics` | Bounded logs, metrics, crash capture, and reports |
| `kael_document`, `kael_office`, `kael_pdf`, `kael_share` | Document lifecycle, portable Office/PDF bytes, native sharing, and browser Web Share |
| `kael_notifications`, `kael_release` | Native/browser notifications and signed update/release workflows |
| `kael_audio`, `kael-media`, `kael_media_engines` | Audio, playback, and opt-in media engines |
| `kael_render_graph`, `kael_gpu_budget` | Render scheduling and GPU memory budgets |

## Design inspiration: Astryx

The visual direction behind `kael_ui` is inspired by
[Astryx](https://astryx.atmeta.com/), an open source design system built around
customizable foundations, clear application workflows, and fast composition.
It is the reference for how Kael's components should feel together: calm,
precise, adaptable, and capable of carrying dense product interfaces without
losing hierarchy.

Kael UI translates that direction into native Rust theme tokens and reusable
components rather than a fixed Astryx skin. Applications can use the defaults,
reshape them for their own identity, or build directly on Kael's lower-level
primitives. The core `kael` crate remains visually neutral and never depends on
`kael_ui`.

The repository's maintained Astryx showcase applies this direction across
actions, inputs, data display, charts, feedback, navigation, overlays,
typography, media, and layout in one application:

```bash
git clone https://github.com/Augani/kael.git
cd kael
cargo run -p kael_ui --example astryx_showcase \
  --features "media kael/runtime_shaders"
```

The showcase is repository-only and is not part of the `kael_ui` crate package.
Core workflows are covered by focused tests and guide chapters instead of
copy-paste examples that can drift from the supported API.

Three repository-only application templates demonstrate larger composition:

```bash
cargo run -p dashboard-app
cargo run -p messaging-app
cargo run -p workspace-app
```

## Platform matrix

| Platform | Windowing | Renderer | Notes |
| --- | --- | --- | --- |
| macOS | AppKit | Metal | Primary native backend; Blade is also checked |
| Windows | Win32 | DirectX 11 | WebView2 is used for optional WebView surfaces |
| Linux | X11, XWayland, and native Wayland | Vulkan / Blade without WebViews; GTK4/GSK for the portable WebView host | The public `webview` feature uses GTK4 + WebKitGTK 6 on X11, XWayland, and Wayland |
| Browser | HTML canvas with in-page retained window surfaces | Dedicated WebGL2 Scene renderer | Same application/window API, bounded suite-scale virtualization, ARIA/IME bridges, and sandboxed iframe WebView islands; detached OS windows remain unavailable |

For a portable WebView build, enable `webview` and install GTK4 plus WebKitGTK
6.0 on Linux. GTK owns the top-level surface and composes Kael's retained GSK
scene with WebKit siblings in one window on both X11 and Wayland. The
`webview-gtk4` and `webview-wayland-gtk4` names remain Linux-specific aliases;
the deprecated pre-release `webview-legacy-gtk3` spelling now redirects to the
same maintained host and cannot enable GTK3. Set `KAEL_LINUX_BACKEND=x11` or
`wayland` to make the runtime choice explicit. See
[Linux WebView hosting](docs/src/linux-webviews.md) for packages, feature flags,
runtime contracts, and acceptance gates.

Renderer support does not imply that every desktop service exists on every OS
or desktop environment. Check capabilities at runtime for hard requirements.

## Development

Rust 1.97.1 is selected by `rust-toolchain.toml`.

```bash
# Strict workspace lint on machines without the full Xcode Metal compiler
cargo clippy --workspace --all-targets \
  --all-features -- -D warnings

# Full test suite
cargo test --workspace --all-targets \
  --all-features

# Performance workload
cargo bench -p kael --bench framework
```

macOS release builds use the Metal compiler included with full Xcode. For local
development with Command Line Tools only, enable `kael/runtime_shaders`.

Linux build packages are listed in
[`scripts/ci/install-linux-deps.sh`](scripts/ci/install-linux-deps.sh). CI checks
the default platform build plus explicit X11 and Wayland feature sets.

## Documentation

Use the [Kael guide](https://augani.github.io/kael/) as the primary documentation.
It covers the complete path from a first window to a deployed desktop and web
application.

| Goal | Guide |
| --- | --- |
| Understand the framework | [What Kael is today](https://augani.github.io/kael/framework-today.html) and [object guide](https://augani.github.io/kael/object-guide.html) |
| Build a first application | [Getting started](https://augani.github.io/kael/getting-started.html) |
| Share one codebase | [Desktop and web](https://augani.github.io/kael/one-codebase.html) |
| Build an interface | [Components](https://augani.github.io/kael/component-library.html), [layout](https://augani.github.io/kael/layout-and-styling.html), and [theming](https://augani.github.io/kael/theming.html) |
| Handle large workloads | [Lists and large data](https://augani.github.io/kael/lists-and-data.html) and [suite-scale applications](https://augani.github.io/kael/suite-scale-apps.html) |
| Build and deploy for browsers | [Web build and deployment](https://augani.github.io/kael/web-deployment.html) |
| Check platform support | [Platform APIs](https://augani.github.io/kael/platform-apis.html) and [current boundaries](https://augani.github.io/kael/remaining-work.html) |
| Find Rust APIs | [`kael` on docs.rs](https://docs.rs/kael), [`kael_ui` on docs.rs](https://docs.rs/kael_ui), and the [API map](https://augani.github.io/kael/api-documentation.html) |
| Give context to an agent | [`llms.txt`](https://raw.githubusercontent.com/Augani/kael/main/llms.txt) and the [LLM guide](https://augani.github.io/kael/llms.html) |

## Acknowledgements

Kael retains the Apache-2.0 license and attribution for the foundational GPUI
work by Zed Industries. The original GPUI code is copyright 2022–2025 Zed
Industries, Inc.

## License

Apache-2.0 — see [LICENSE](LICENSE).
