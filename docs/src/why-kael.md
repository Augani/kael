# Choosing Kael

Kael is for teams building substantial desktop products in Rust: software with
many screens, large data sets, multiple windows, background work, native OS
integrations, and a long operational life. Its design target is simple:
applications should become more capable without requiring proportionally more
memory, idle CPU, runtime layers, or integration code.

Kael is pre-1.0 (currently `0.3.x`). The architecture and batteries are broad,
but applications should still pin a compatible minor release and validate the
capabilities that matter to their users.

## What the architecture buys you

Kael uses a retained UI tree and reactive `Entity<T>` state. It schedules work
when state changes instead of treating every frame as a reason to rebuild the
whole interface. Virtualized collections, bounded caches, frame skipping,
headless measurement, and GPU-budget APIs give large products explicit control
over the cost of their UI.

The framework also keeps application concerns close together. UI, state,
commands, async work, accessibility, native services, diagnostics, packaging,
and updates can share Rust types and failure handling. Teams can spend their
complexity budget on product behavior instead of maintaining bridges between
unrelated runtimes.

This does not guarantee that every Kael application is small or fast. Assets,
dependencies, data models, and application code still determine real resource
use. Use the [benchmarking harness](benchmarking.md) to measure the product and
keep performance claims tied to reproducible evidence.

## Choose how much framework you want

- Use `kael` for rendering, state, elements, layout, text, input, windows,
  accessibility, and platform primitives.
- Add `kael_ui` for a broad component system that can be rethemed and restyled
  around the product's brand.
- Add focused `kael_*` crates for storage, networking, secrets, documents,
  diagnostics, notifications, sharing, media, release services, and higher-level
  application engines.

The primitive layer does not depend on the component layer. A custom design
system remains a first-class architecture, not an escape hatch.

## How it compares

This table is decision context, not Kael's identity. Figures are
order-of-magnitude guidance for trivial applications; measure a representative
build before making a product decision.

| | **Kael** | **Electron-style stack** | **Tauri** | **egui** | **Iced** |
| --- | --- | --- | --- | --- | --- |
| Architecture | Native GPU retained mode | Bundled Chromium + Node | OS WebView + Rust core | Immediate-mode GPU | Native retained mode |
| Main UI language | Rust | JavaScript/TypeScript | HTML/CSS/JS | Rust | Rust |
| Render path | Metal / DX11 / Vulkan | Chromium compositor | System WebView | wgpu/glow | wgpu/tiny-skia |
| State model | Reactive entities | Application-selected web state | Application-selected web state | Redraw each frame | Message/update/view |
| Product batteries | Framework and focused crates | Mature web/Node ecosystem | Bundler and updater ecosystem | Mostly application-owned | Mostly application-owned |
| Web deployment | No | No | No | Optional wasm | Optional wasm |
| Maturity | Pre-1.0 | Very mature | Mature | Mature | Mature |

Choose a web-based desktop stack when web compatibility, the npm ecosystem, or
shared web/desktop UI is the primary requirement. Choose an immediate-mode
toolkit when a compact tool UI benefits from that model. Choose Kael when the
product wants a native retained interface, one Rust application architecture,
deep desktop capabilities, and direct control over rendering and resource use.

## Accessibility

Kael produces one accessibility tree and serves it through the platform adapter:

- **Windows** — UI Automation through `WM_GETOBJECT`.
- **macOS** — `accesskit_macos` over the window's `NSView` for VoiceOver.
- **Linux** — AccessKit's AT-SPI2 adapter for X11 and Wayland.

This is a real implementation, but it is younger than browser accessibility
stacks. Validate the screen readers, keyboard workflows, scale factors, and
locales your application supports.

## Packaging and updates

Application packaging and updates are part of the product toolchain: DMG on
macOS, WiX/MSI on Windows, and AppImage on Linux, with signing, notarization,
checksums, update manifests, and signature verification. These APIs package
applications built with Kael; they do not change the ownership or release policy
of the Kael framework itself.

## Current boundaries

- **Touch and pen input are partial.** Pointer, scroll, magnify, and gesture
  primitives exist, but full contact streams and pressure/tilt metadata still
  need backend work.
- **Desktop only.** Kael does not provide a wasm/browser deployment target.
- **Web-specific surfaces stay explicit.** Use the optional WebView feature for
  OAuth, payments, maps, hosted documents, vendor widgets, or another
  intentionally web-owned surface.
- **Native media continues to mature.** Audio/video primitives, captions,
  controllers, and automatic routing exist; hardware decode and broader native
  streaming coverage remain platform-dependent.
- **Pre-1.0 API.** Breaking changes may occur between minor versions.
- **Smaller ecosystem.** There are fewer third-party packages and community
  answers than in older UI ecosystems.

## Good fits

Kael is a strong fit for editors, IDEs, agent workspaces, collaboration tools,
communication apps, dashboards, database clients, media tools, design software,
and other desktop products where responsiveness, resource use, native services,
and a coherent application architecture all matter.

It is a weaker fit when the product must share its primary interface with the
web, depends heavily on DOM-only packages, or needs a platform capability Kael
currently reports as unsupported. Read the
[Native Capability Bridge](native-capability-bridge.md) before committing to an
OS-dependent workflow.
