# Why Kael exists

Kael started from a personal frustration: ambitious desktop software too often
has to choose between native capability, web reach, performance, and a coherent
development model.

I wanted to build products with the depth of a document suite or creative
engine: large sheets, long documents, presentations, whiteboards, media,
collaboration, and rich input. I did not want an application that constantly
redraws, carries several runtimes, or needs a second frontend to reach the web.

That is the idea behind Kael: one Rust application architecture, a retained GPU
scene, explicit resource bounds, and platform services that tell the truth about
where they can run.

## The product I wanted to build with

Kael is designed for software that grows. A useful framework has to remain
clear when the application has many screens, many windows, background work,
large data, documents, plugins, native services, and years of product decisions.

That led to a few non-negotiable principles:

- **Do useful work.** Invalidation, frame skipping, localized damage,
  virtualization, recycling, bounded caches, and GPU budgets should prevent work
  that cannot improve the current frame.
- **Keep one architecture.** Views, state, async work, files, networking,
  documents, diagnostics, packaging, and updates should share Rust types and
  failure handling.
- **Let products own their identity.** Use `kael_ui`, reshape its tokens and
  components, mix it with custom work, or build an entire design system directly
  on `kael` primitives.
- **Treat platforms honestly.** A capability report is better than an API that
  exists everywhere but quietly does something weaker on one target.
- **Prove scale in releases.** Large tables, documents, slide decks,
  whiteboards, browser engines, and native renderers belong in maintained gates,
  not only in roadmap language.

## Why desktop and web share a foundation

The browser should not require a rewrite of the product. Kael sends the same
retained scene to native GPU backends or a dedicated WebGL2 renderer. State,
layout, components, painting, virtualization, animations, document bytes, and
workers stay in Rust.

The platform boundary remains real. A browser cannot create a detached OS
window, expose arbitrary native paths, launch subprocesses, or provide a system
keychain. Kael represents those differences through capabilities and portable
byte-oriented workflows instead of hiding them.

Read [One codebase, desktop and web](one-codebase.md) for the exact contract.

## Why retained rendering

Immediate-mode UI is excellent for many tools, but Kael is aimed at long-lived
product surfaces where identity, focus, accessibility, text, window state, and
large collections benefit from stable retained structure.

Reactive `Entity<T>` state invalidates affected views. Scene fingerprints can
skip unchanged frames. Damage can remain local. Virtual lists mount only the
visible range. Hidden and idle windows stop requesting frames. These mechanisms
do not make every application fast automatically, but they give a product direct
control over where time and memory go.

## Why the framework is layered

`kael` provides rendering, state, elements, layout, text, input, accessibility,
windows, async work, and platform primitives. `kael_ui` provides a broad,
brandable component system. Focused `kael_*` crates add storage, networking,
secrets, documents, diagnostics, notifications, sharing, media, release
services, and application engines.

The dependency direction is deliberate: `kael_ui` depends on `kael`; `kael`
never depends on `kael_ui`. A custom visual system is an architecture choice,
not an escape hatch.

## Where Kael fits

Kael is a strong fit for editors, IDEs, agent workspaces, communication apps,
dashboards, database clients, document suites, media tools, design software,
simulations, and game or creative engines where responsiveness, native services,
and product architecture all matter.

Choose another stack when a product depends primarily on DOM-only packages, the
npm ecosystem is the main advantage, an immediate-mode UI is the better mental
model, or a required platform capability is not yet implemented in Kael.

Kael 0.4 is pre-1.0. Pin a compatible minor release, validate the capabilities
your product needs, and expect refinement before 1.0.

## Foundation and independence

Kael began as a fork of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
created by Zed Industries, and was previously distributed as the adabraka GPUI
fork. It retains the required Apache-2.0 attribution for that foundational work.

Kael is now an independent project with its own application model, browser
renderer, component system, product crates, performance workloads, platform
bridges, and release process. It is not affiliated with or endorsed by Zed
Industries.

**Augustus Otu**, creator of Kael
