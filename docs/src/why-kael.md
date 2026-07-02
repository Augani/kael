# Choosing Kael

This page is for engineers deciding whether Kael belongs in their stack. It is
not a sales pitch — it lays out where Kael sits relative to the common
alternatives, and is honest about what is missing. Kael is pre-1.0 (currently
`0.3.x`); treat it accordingly.

Kael is a fork of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui).
It targets native desktop apps (macOS, Windows, Linux) written in Rust and
rendered on the GPU. Kael is native-first, but it also provides explicit WebView
islands for workflows where web compatibility is the product requirement.

## How it compares

| | **Kael** | **Electron** | **Tauri** | **egui** | **Iced** |
|---|---|---|---|---|---|
| Architecture | Native GPU retained-mode | Chromium + Node webview | OS webview + Rust core | Immediate-mode GPU | Native retained-mode (Elm-style) |
| Render path | Metal / DX11 / Vulkan | Blink compositor | System WebView2/WKWebView/WebKitGTK | wgpu/glow | wgpu/tiny-skia |
| Language | Rust | JS/TS (+ native addons) | Rust core, JS/HTML UI | Rust | Rust |
| Styling model | Tailwind-like builder API in Rust | CSS | CSS | Code-driven, minimal theming | Code-driven stylesheets |
| UI paradigm | Retained tree, reactive entities | DOM | DOM | Redraw-every-frame | Message/update/view |
| Binary size (hello-world, ballpark) | ~10–30 MB | ~80–150 MB | ~3–10 MB (excludes the OS webview) | ~5–15 MB | ~10–20 MB |
| Memory at idle | Low (no browser) | High (full Chromium) | Low–moderate (shares OS webview) | Low | Low |
| Bundled runtime | None | Ships Chromium + Node | None (uses OS webview) | None | None |
| Accessibility | Native per platform (see below) | Chromium a11y (mature) | OS webview a11y (mature) | Limited / partial | AccessKit-based |
| Packaging / updater | Built in (see below) | Mature (electron-builder, Squirrel) | Built in (bundler + updater) | Bring your own | Bring your own |
| Web deployment | No | No (desktop) | No (desktop) | Yes (wasm) | Yes (wasm) |
| Maturity / ecosystem | Pre-1.0, small | Very mature, huge | Mature, large | Mature, focused | Mature, focused |

Size and memory figures are order-of-magnitude guidance for a trivial app, not
benchmarks; real numbers depend heavily on assets, dependencies, and build
flags. Tauri's small binary excludes the system webview it depends on at
runtime.

## What the architecture buys you

Kael draws native widgets itself on the GPU. The primary UI path has no HTML,
CSS, DOM, or JavaScript bridge — layout is flexbox/grid via Taffy, and the app
UI can live in one Rust crate. Compared to Electron you drop the bundled browser
(smaller binaries, lower idle memory, no IPC hop between a JS UI and a native
core). Compared to Tauri you do not have to build the whole app in the user's
system webview. The tradeoff is real: browser features such as mature media
playback, DOM APIs, CSS edge cases, and the npm UI ecosystem must be replaced
with native Kael APIs or isolated in explicit WebView surfaces.

Versus immediate-mode toolkits like egui, Kael is retained-mode with a reactive
`Entity<T>` state system, so it only re-renders what changed rather than
redrawing every frame. Versus Iced, the main differences are the styling model
(a Tailwind-like builder API instead of Elm-style stylesheets) and the lineage
(a GPUI fork, proven at the scale of the Zed editor).

## Accessibility

Honest status, because it is often where native GPU toolkits fall down:

- **Windows** — a hand-rolled UI Automation provider served via `WM_GETOBJECT`.
- **macOS** — the `accesskit_macos` adapter over the window's `NSView`, serving
  a full `NSAccessibility` tree to VoiceOver.
- **Linux** — the `accesskit_unix` AT-SPI2 adapter (x11 and Wayland), exposing
  the tree to Orca over D-Bus.

All three are driven from the same per-frame accessibility tree. This is real,
not a stub — but it is newer than Chromium's decade-hardened a11y, so validate
against your target screen readers.

## Packaging and updates

Packaging and auto-update are built in, not an afterthought: `.dmg` on macOS,
WiX/MSI on Windows, AppImage on Linux, with code-signing/notarization hooks. The
auto-updater verifies update signatures against a pinned public key before
installing. This is closer to Tauri's batteries-included story than to egui/Iced
(where you assemble your own).

## What Kael does not have yet

Be aware of the gaps before committing:

- **Touch / pen input is not Chromium-complete yet** — pointer, scroll, and
  magnify gestures exist, and `CapabilityReport` now separates
  `PrecisionPointerInput`, `GestureInput`, `TouchInput`, and `PenInput`, but
  direct touch contact streams and pen pressure/tilt metadata still need
  backend work.
- **No URL routing** — navigation is an in-app stack, not URL-addressable.
- **No web deployment** — desktop only; there is no wasm/browser target. If you
  need the same UI on the web, egui or Iced (or a webview stack) fit better.
- **No full browser-platform parity** — native Kael UI is not a DOM/CSS/JS
  runtime. Use WebView islands for web-shaped requirements while native
  equivalents mature.
- **Media is still bridging the gap** — audio/video primitives,
  `VideoController`, and `VideoPlayer::source(...)` exist, including parsed and
  rendered WebVTT/SRT text tracks with built-in caption selection.
  `VideoPlayer::url(...)` defaults to automatic native-vs-WebView routing for
  browser-media manifests, but hardware decode, richer native streaming,
  native audio/video stream selection, and full browser-media parity are still
  roadmap work.
- **Pre-1.0 API** — expect breaking changes between minor versions.
- **Smaller ecosystem** — fewer third-party widgets, examples, and answers than
  Electron or Tauri. You will occasionally be the first to hit something.

## When to pick Kael

Reach for Kael when you want native GPU performance and a single Rust codebase
for a desktop app — IDEs, editors, dashboards, design and media tools — and you
are comfortable on a pre-1.0 framework. Reach for Electron or Tauri when you
need the full web platform as your default UI runtime, a large ecosystem, or one
codebase for web and desktop; reach for egui when an immediate-mode tool UI is
enough; reach for Iced when you want a mature, native, Elm-style Rust toolkit.
