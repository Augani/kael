# Kael — Vision & Direction

**Kael is a general-purpose, GPU-accelerated framework for building native desktop
applications in Rust.** Not a video-editor toolkit, not a single-app engine — a
framework for IDEs, dashboards, design tools, messaging apps, trading platforms,
media tools, and anything else that deserves to run at native speed on macOS,
Windows, and Linux.

This document exists because we drifted from that, and we are correcting course.

---

## A course correction

Through late 2025 and early 2026, a large share of Kael's engineering effort went
into media and non-linear-editing capabilities: a timeline model, a multi-track
compositor, audio loudness metering, video scopes, clip effects. That work was
real and much of it is good — but it pulled the project's center of gravity toward
*one* application domain, and it showed in our roadmap, our commit log, and how
the community read us.

The correction is not to delete that work. It is to put it back in its place:

- **The framework is the product.** Kael's roadmap is ordered around what every
  desktop application needs — rendering, text, input, accessibility, packaging,
  GPU extensibility — not around what a video editor needs.
- **The media stack is an optional, layered consumer of the framework.** It lives
  in separate crates (`kael-media`, `kael_audio`, `kael_media_engines`) behind
  opt-in feature flags. The core `kael` crate compiles without any of it, and that
  stays true by policy (see "The layering rule" below).
- **Domain work must pay general dividends.** The video-editor push forced
  genuinely general infrastructure into existence — a GPU-agnostic render-graph
  crate (`kael_render_graph`), a GPU memory-budget API (`kael_gpu_budget`),
  UAX#9 bidirectional text, UAX#14 line breaking, color-management groundwork.
  That is the standard going forward: a domain feature is welcome when the
  primitives it needs are designed as public, general framework features first.

## Where Kael came from, and where it stands with Zed

Kael is a fork of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
the UI framework Zed Industries built for the Zed editor. It was previously
distributed as the **adabraka GPUI fork** (`adabraka-gpui`) and was renamed to
Kael. To be unambiguous about that history:

- **Kael and adabraka-gpui are the same project.** The rename was a rebrand, not
  a new fork or a change of stewardship. If you depended on the adabraka fork,
  Kael is its continuation.
- **The rename is not a severing of ties with Zed.** Kael is independent and is
  not affiliated with or endorsed by Zed Industries, but it is built on their
  foundational work, credits it, keeps the Apache-2.0 license, and remains
  immensely grateful for it.
- **Kael does not track upstream.** The Zed team has been clear that GPUI takes
  only what benefits their editor, so Kael is deliberately not structured as a
  fork that follows upstream and keeps it intact. There is no need for that.
  When Kael needs a capability, Kael builds it.
- **Why fork at all?** GPUI is excellent, and it is Zed-shaped: the Zed team has
  been clear that they prioritize APIs that directly benefit their editor.
  That is a reasonable position for them — and it leaves no home for
  general-purpose features the wider community keeps asking for. Kael is that
  home.

## What the community asked for — and where it stands

These come up repeatedly in GPUI ecosystem discussions, and they are Kael's
priorities precisely *because* they serve every application, not one domain:

| Ask | Status in Kael |
|---|---|
| **Gradients beyond linear** | **Shipped.** `radial_gradient()` and `conic_gradient()` are in the core styling API today, alongside linear gradients. |
| **Custom shaders** | **Committed, top of the GPU roadmap.** A public render-target + pass API with app-registered shaders (and compute pipelines) across Metal, DirectX 11, and Vulkan/Blade. This was originally scoped as internal plumbing for the media compositor; it is now scoped as a *public framework feature* — the media stack is just one consumer of it. Design proposal: [docs/design/0001-render-targets-and-custom-shaders.md](docs/design/0001-render-targets-and-custom-shaders.md). |
| **Offscreen render targets** | Same program as custom shaders (P0-A): app-allocatable typed render targets, including ≥16-bit float formats. |
| **A component library that doesn't require a second dependency** | **Shipped.** `kael_ui` provides 100+ shadcn-inspired components, theming, icons, and fonts in-tree. |
| **Production desktop-app table stakes** | In progress, prioritized ahead of all media work: accessibility (AccessKit), real signed installers, verified auto-update (signature + hash verification has landed), native crash reporting, BiDi/complex text. |
| **API stability** | A published SemVer and deprecation policy, and a deliberate `pub` surface, are roadmap commitments before 1.0. |

## The layering rule

This is the policy that keeps Kael general:

1. **The core `kael` crate is domain-neutral.** It contains rendering, windowing,
   layout, text, input, state, and platform integration. It must build and be
   fully usable with no media, audio, or editing code compiled in. Domain crates
   may depend on core; core never requires them (optional, feature-gated
   integrations only).
2. **New GPU and platform capabilities land as public framework APIs**, designed
   for arbitrary applications, documented, and exercised by at least one
   non-media example. Domain stacks consume the public API like any other app.
3. **Domain stacks are leaf crates.** `kael-media`, `kael_audio`, and the NLE
   engines in `kael_media_engines` are maintained, but they are siblings of
   *your* application — not the framework's reason to exist. (The general
   engines — BiDi, line breaking, undo, crash reporting — live in
   `kael_engines`, which is domain-neutral.)

Today the core already honors rule 1 (media and audio are optional dependencies
behind feature flags). Rules 2 and 3 govern everything new.

## How priorities are ordered now

In order:

1. **Ship-blocking general gates** — accessibility, packaging/signing/notarization,
   update integrity, crash reporting, text correctness (BiDi, line breaking, IME).
   These block *every* serious app built on Kael.
2. **GPU extensibility as public API** — render targets, custom shaders, compute,
   the render graph. The most-requested capability in the GPUI ecosystem and the
   thing upstream won't take.
3. **Stability and community** — SemVer policy, deliberate public surface,
   contribution guidelines that welcome general-purpose features.
4. **Domain stacks (media included)** — continue as optional layers, funded by
   and subordinate to items 1–3.

[PRODUCTION_ROADMAP.md](PRODUCTION_ROADMAP.md) carries the detailed technical
audit and phase plan, read through this ordering.

## On the wider GPUI ecosystem

GPUI capability is currently spread across several efforts — component libraries
tracking upstream, mirrors tagging Zed releases, forks experimenting with wgpu
backends, and Kael. Fragmentation before maturity helps no one. Kael's position:

- We are openly a fork, and we intend to remain a *useful* one: general features,
  documented, released on crates.io, usable without adopting anyone's app.
- We are happy to collaborate with other GPUI forks and libraries on shared
  problems — custom shader APIs, versioning against upstream, platform backends.
  If you maintain one of these projects and want to align on an API, open an
  issue.
- We do not wait on upstream. Zed builds for their editor; Kael builds for every
  desktop app. When Kael needs something, we build it ourselves.

---

*This document is the project's north star. If a proposed change to Kael makes
sense only for one kind of application, it belongs in a leaf crate — or in your
app. If it makes every desktop app better, it belongs in the framework.*
