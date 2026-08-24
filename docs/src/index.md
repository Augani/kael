# Kael

**Build ambitious desktop software that does more with less.**

Kael is a GPU-accelerated application framework for Rust. It combines a
retained renderer, reactive application state, native platform services,
production tooling, and optional product batteries in one coherent system for
macOS, Windows, Linux, and WebAssembly/WebGL2 browsers.

The goal is not merely to render a window. Kael is for long-lived PC
applications—editors, agent workspaces, communication products, dashboards,
media tools, and creative software—that must remain responsive as their data,
windows, background work, and feature surface grow.

> Kael began as a fork of
> [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), created by
> Zed Industries, and was previously distributed as the adabraka GPUI fork.
> Kael is an independent project and is not affiliated with or endorsed by Zed
> Industries.

## Two layers, one foundation

| Layer | Use it for |
| --- | --- |
| `kael` | Rendering, entities, elements, layout, text, input, accessibility, windows, async work, and native platform primitives |
| `kael_ui` | Brandable controls, data surfaces, editors, charts, navigation, overlays, feedback, media controls, and responsive layouts |

`kael_ui` depends on `kael`; the primitive crate never depends on the component
crate. A product can use Kael with its own design system, adopt the complete UI
library, or mix the two.

## Designed for efficient, capable applications

- Retained rendering, invalidation, frame skipping, virtualization, bounded
  caches, and GPU budgets avoid work that does not improve the current frame.
- Rust types flow through UI, state, async tasks, platform services,
  diagnostics, packaging, and updates without a second application runtime.
- Capability reports expose platform differences rather than hiding them behind
  APIs that may not work on a user's machine.
- Focused support crates add storage, networking, secrets, Office/PDF documents,
  diagnostics, notifications, sharing, media, release services, and application
  engines without forcing every app to compile every battery.
- Styling primitives and design tokens keep the final product's identity in the
  application's hands.

## Start building

Create an application with the CLI:

```bash
cargo install kael-cli
kael new my_app
cd my_app
cargo run
```

Or choose the layers directly:

```toml
[dependencies]
kael = "0.4"
kael_ui = "0.4" # optional
```

Continue with [Getting Started](getting-started.md), then read
[Core Concepts](core-concepts.md). Use [Choosing Kael](why-kael.md) for tradeoffs
and [Native Capability Bridge](native-capability-bridge.md) for platform-aware
product planning.

## Documentation map

- [API Documentation](api-documentation.md) — docs.rs, feature flags, and module map
- [Component Library](component-library.md) — the optional brandable UI layer
- [Platform APIs](platform-apis.md) — operating-system integrations
- [Testing](testing.md) — headless, behavioral, and platform testing
- [Benchmarking Evidence](benchmarking.md) — measuring resource use and responsiveness
- [Examples Gallery](examples.md) — Astryx and application templates

Kael 0.4 is pre-1.0. Pin a compatible minor version, validate the platform
capabilities your product depends on, and expect API refinement before 1.0.
