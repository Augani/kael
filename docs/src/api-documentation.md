# API Documentation

Kael has two complementary documentation surfaces:

- This guide explains architecture, workflows, platform choices, and complete
  application concerns.
- [docs.rs](https://docs.rs/) renders the public Rust API for every Kael crate
  release, including searchable modules, types, traits, methods, and source
  links.

Start with the guide when deciding how a feature should fit into an application.
Use rustdoc while implementing it.

## Primary API references

| Need | API reference |
| --- | --- |
| Runtime, entities, elements, windows, input, text, layout, rendering | [`kael`](https://docs.rs/kael) |
| Ready-made controls and product UI | [`kael_ui`](https://docs.rs/kael_ui) |
| Procedural macros | [`kael_macros`](https://docs.rs/kael-macros) |
| Storage and migrations | [`kael_storage`](https://docs.rs/kael_storage) |
| HTTP and connected application patterns | [`kael_http_client`](https://docs.rs/kael_http_client) and [`kael_net`](https://docs.rs/kael_net) |
| Diagnostics and crash data | [`kael_diagnostics`](https://docs.rs/kael_diagnostics) |
| Documents, PDF, and sharing | [`kael_document`](https://docs.rs/kael_document), [`kael_pdf`](https://docs.rs/kael_pdf), and [`kael_share`](https://docs.rs/kael_share) |
| Audio and media | [`kael_audio`](https://docs.rs/kael_audio), [`kael_media`](https://docs.rs/kael-media), and [`kael_media_engines`](https://docs.rs/kael_media_engines) |
| Application engines | [`kael_engines`](https://docs.rs/kael_engines) |

## Core module map

The `kael` crate re-exports its most common types at the crate root. These
modules provide focused entry points for larger systems:

| Module | Purpose |
| --- | --- |
| `prelude` | Traits and types most views import |
| `animation`, `interpolate` | Timelines, easing, keyframes, and value interpolation |
| `app_runtime`, `runtime`, `worker_api` | Application lifecycle and background execution |
| `virtual_data` | Virtualized lists, tables, and tree data |
| `text_engine` | Editing, selection, composition, and document text behavior |
| `platform_caps` | Runtime capability truth for platform-dependent workflows |
| `security` | Permissions, policies, validation, and safe handoffs |
| `process_model`, `ipc_transport`, `supervisor` | Multi-process applications and worker supervision |
| `plugin`, `extension_host`, `extension_rpc` | In-process and external extension systems |
| `headless_render`, `golden`, `benchmark` | Rendering tests and performance evidence |
| `scene_graph`, `graphics_capabilities`, `gpu` | Creative surfaces and GPU control |
| `dev_tools` | Inspector, metrics, and development-time tooling |

The [Core Concepts](core-concepts.md), [Platform APIs](platform-apis.md), and
[Testing](testing.md) chapters explain how these pieces cooperate.

## Feature flags

The core crate keeps costly or specialized integrations optional:

| Feature | Adds |
| --- | --- |
| `auto-update` | Signed update feeds, checked download queues, and platform installers |
| `lottie` | Native Lottie and dotLottie decoding and playback |
| `webview` | Explicit hosted web surfaces |
| `media` | Native media playback integration |
| `storage` | Storage primitives through `kael_storage` |
| `icons` | Compact embedded icon catalog with application-asset overrides |
| `diagnostics` | Metrics, breadcrumbs, and crash-report integration |
| `document` | Document lifecycle helpers |
| `audio` | Audio integration |
| `pdf` | PDF services |
| `notifications-full` | Notification services |
| `share` | Platform sharing workflows |
| `screen-capture` | Screen-capture backend support |
| `agent-tools` | Structured capability-planning metadata |
| `runtime_shaders` | Runtime shader compilation for development |

`kael_ui` separately gates Markdown, native HTML rendering, audio, media, and
additional editor grammars. Feature-gated APIs appear in the relevant crate
documentation when that documentation profile enables the feature.

## Build API docs locally

Build the two primary references without documenting dependencies:

```bash
RUSTDOCFLAGS="-D warnings" \
  cargo doc -p kael -p kael_ui --all-features --no-deps
```

Open `target/doc/kael/index.html` or run `cargo doc -p kael --open` for a faster
default-feature build. The production-readiness workflow also documents every
public library crate with all features enabled, so broken intra-doc links and
other rustdoc warnings anywhere in that crate set block a release candidate.

## Documentation guarantees

- `kael` and `kael_ui` provide crate-level landing pages with dependency and
  usage guidance.
- docs.rs profiles are explicit so documentation builds do not depend on an
  accidental feature set or unsupported cross-compilation target.
- Public API links are checked with rustdoc warnings treated as errors.
- The mdBook guide is built independently, so conceptual documentation cannot
  hide API-documentation failures.
- Platform-specific support is described through `CapabilityReport`; the
  existence of a type alone is never presented as proof that every backend
  implements it.
