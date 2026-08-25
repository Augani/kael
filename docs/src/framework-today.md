# What Kael is today

Kael 0.4 is a pre-1.0 application framework for substantial Rust products. It
is larger than a widget toolkit and more focused than a collection of unrelated
libraries: the renderer, application runtime, product services, and release
tooling are designed to share types, bounds, errors, and platform truth.

This page is the current map. Follow the linked guides when you need the API
contract or platform details.

## The application foundation

`kael` owns the retained application model:

- GPU scenes on Metal, Direct3D 11, Vulkan through Blade, and browser WebGL2;
- windows, elements, flex and grid layout, text shaping, images, SVG, canvas,
  paths, effects, clipping, transforms, and high-DPI presentation;
- reactive `Entity<T>` state, contexts, observation, async executors, actions,
  keybindings, focus, drag and drop, and multi-window lifecycle;
- mouse, keyboard, wheel, gesture, game, touch, and pen input through shared
  contracts;
- AccessKit semantics on desktop and a bounded retained ARIA mirror in browsers;
- invalidation, frame skipping, localized damage, bounded atlases, renderer
  batching, and display-aware scheduling.

Start with [Core concepts](core-concepts.md), [Layout and styling](layout-and-styling.md),
and [Canvas and graphics](graphics.md).

## The product interface layer

`kael_ui` is optional and brandable. It adds controls and compositions without
making the primitive crate depend on a visual identity:

- buttons, text inputs, text areas, selection controls, date and color inputs;
- menus, popovers, dialogs, tooltips, tabs, splitters, scrollbars, and toasts;
- virtual lists, recycling lists, data tables, charts, editors, navigation, and
  responsive layout helpers;
- theme tokens, runtime theme switching, accessibility behavior, and reduced
  motion policies;
- opt-in media, Markdown/HTML rendering, syntax editing, game input, and screen
  capture feature sets.

Use the [Component library](component-library.md) as the index. Building an
entire design system directly on `kael` remains a first-class choice.

## Scale and graphics

Large logical workloads do not need large retained trees. Kael provides uniform
and variable-height virtualization, adaptive recycling pools, compressed table
selection, spatial indexing, tile damage, bounded caches, and GPU memory budget
queries.

`PortableScene2d` records a bounded high-throughput 2D scene shared by native
and browser renderers. It supports up to 100,000 retained quads, sprites, filled
paths, or triangle objects with transforms, clips, opacity, and transactional
rollback. Unsupported custom shaders, compute, blend modes, or 3D work return
typed results instead of pretending to be portable.

For simulations and engines, `FixedFrameClock` provides bounded fixed-timestep
catch-up, interpolation, pause and resume, and dropped-time telemetry. Read
[Lists and large data](lists-and-data.md), [Animations](animations.md), and
[Game input](game-input.md).

## Product services

Focused crates keep capabilities optional. Applications compile only the
batteries they select.

| Area | Crates and capabilities |
| --- | --- |
| Data | `kael_storage`, `kael_cache`, `kael_secrets`: SQLite, IndexedDB, JSON, bounded caches, OS credentials |
| Documents | `kael_document`, `kael_office`, `kael_pdf`, `kael_markdown`: recovery, versions, byte import and export, OOXML, PDF, structured Markdown |
| Network | `kael_http_client`, `kael_net`: HTTP, bounded WebSockets, sync primitives, host policy |
| Media | `kael_audio`, `kael-media`, `kael_media_engines`: mixing, capture, playback, timelines, compositing, export foundations |
| Operations | `kael_diagnostics`, `kael_notifications`, `kael_share`, `kael_release`: logs, metrics, crashes, notifications, sharing, signed updates |
| Rendering | `kael_render_graph`, `kael_gpu_budget`: pass scheduling, invalidation, GPU memory budgets |
| Product foundations | `kael_engines`, `kael_i18n`, `kael_icons`: bounded editor and workload state, localization, typed icons |

The [Platform APIs](platform-apis.md), [Office and PDF](office-documents.md), and
[Realtime networking](realtime-networking.md) guides cover the shared contracts
and their platform boundaries.

## Desktop and browser delivery

New CLI projects use one `main.rs` for native and browser builds. The same view,
state, layout, components, retained scenes, virtualization, animation, document
bytes, and worker requests compile to both targets. The host adapts GPU
presentation, windows, files, storage, printing, capture, audio, notifications,
sharing, and WebView composition.

The browser backend is not a DOM rewrite or an application inside a desktop
WebView. It is a dedicated WebGL2 renderer with its own text atlas, IME and
clipboard bridge, retained accessibility mirror, file-byte workflows, Web
Workers, IndexedDB storage, AudioWorklets, WebSockets, capture, printing, and
sandboxed iframe WebView islands.

Read [One codebase, desktop and web](one-codebase.md) before selecting a
platform-sensitive workflow.

## Testing and release engineering

Kael treats release readiness as code. The repository checks extracted
crates.io packages, platform compilation, real native renderer windows,
browser engines, WebView hosts, optimized Wasm, accessibility bounds, large
workloads, installer contents, and signed update metadata.

Application tooling covers DMG, MSI, and AppImage packaging; macOS signing and
notarization; Windows signing; checksums; update manifests; and atomic signed
installation. These tools package Kael applications. The application owner still
defines release policy and credentials.

Use [Testing](testing.md), [Benchmarking evidence](benchmarking.md), and the
[Release process](releasing.md) when preparing a product.

## Current boundaries

Kael is broad, but it does not erase the operating system:

- browser secondary windows are retained surfaces inside the page, not detached
  operating-system windows;
- browser builds cannot expose arbitrary native paths, subprocesses, or a system
  keychain;
- full Office layout, spreadsheet calculation, and slide layout engines remain
  product layers above Kael's bounded OOXML byte foundation;
- custom shaders, compute, custom blending, and 3D are native or
  application-specific extensions rather than part of `PortableScene2d`;
- some native touch, pen, media, sharing, and desktop-environment services vary
  by backend and must be checked at runtime;
- the public API is pre-1.0 and can change between minor releases.

`CapabilityReport::current()` and `WebViewCapabilityReport` make those
differences inspectable. Kael prefers an actionable `Unsupported` result over an
API that silently behaves like a different feature.
