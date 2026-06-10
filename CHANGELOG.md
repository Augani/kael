# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the workspace is currently at `0.x`, the public API is not yet
stabilised — minor version bumps may include breaking changes.

## [Unreleased]

### Added

- **New crate: `kael_ui` — a complete shadcn-inspired component library
  built into the Kael workspace.** This is the continuation of
  [adabraka-ui](https://github.com/Augani/adabraka-ui) (which depended
  on the former adabraka GPUI fork, now Kael), ported in-tree so
  applications no longer need to combine Kael with an external
  component library. It ships 100+ components across seven modules:
  `components` (buttons, inputs, selects, sliders, date/time/color
  pickers, OTP input, file upload, tag input, rating, code editor with
  tree-sitter highlighting, and more), `display` (tables, data grids,
  cards, badges, accordions, markdown/HTML rendering), `navigation`
  (sidebars, menus, tabs, breadcrumbs, toolbars, file trees, status
  bars), `overlays` (dialogs, sheets, popovers, toasts, tooltips,
  context menus, command palettes), `charts` (line, area, bar, pie,
  donut, radar, gauge, heatmap, treemap, sparkline), `layout`
  (`VStack`, `HStack`, `Grid`, `ScrollContainer`, responsive helpers),
  and a shadcn-style `theme` system with light/dark design tokens.
  Bundles 1,600+ Lucide icons, Inter and JetBrains Mono fonts, and
  140+ runnable examples (`cargo run -p kael_ui --example button_demo`).
- `canvas_with_prepaint()` is now public in `kael`, exposing the
  non-overloaded form of `canvas()` for callers that want closure
  parameter types to be inferred.

## [0.1.2] - 2026-05-28

First release with a tracked CHANGELOG. Previous published version on
crates.io was `0.1.1`, but no git tag was cut for it; the entry below
captures everything landed on `main` between `0.1.1` and `0.1.2`.

### Changed

- **macOS platform layer is now built on `objc2`.** Migrated the entire
  mac platform layer from the deprecated `cocoa` 0.26 and `objc` 0.2
  crates onto `objc2` 0.6, `objc2-app-kit`, `objc2-foundation`, and
  `block2`. `cocoa` and `objc` are no longer dependencies of `kael`.
  The migration adopts `define_class!` with `MainThreadOnly` markers
  and `Retained`-typed ivars for the application/notification
  delegates, replaces `ConcreteBlock` with `block2::RcBlock`, and
  reframes every NS* enum to the modern variant names
  (`NSEventType::KeyDown`, `NSWindowStyleMask::Closable`, etc.).
- **Stricter runtime type checking via `objc2` caught three latent
  bugs** that the old `objc` runtime silently accepted, all fixed in
  this release:
  - `-[CAMetalLayer setColorspace:]` is now passed a properly-encoded
    `^{CGColorSpace=}` pointer instead of a generic `*mut c_void`.
  - `-[NSApplication setActivationPolicy:]` and
    `-[NSWindow makeFirstResponder:]` return types are bound as `BOOL`
    instead of `()`.
  - `isKindOfClass:` is passed a single `Class` argument instead of
    accidentally being wrapped in a 1-element array.
- Workspace dependencies refreshed: Rust 1.95 toolchain (MSRV remains
  1.87), `cosmic-text` 0.19, `blade-graphics` 0.8, `ashpd` 0.13,
  `windows` 0.62, `derive_more` 2.x.

### Added

- Runnable smoke example (`cargo run -p kael --example objc2_smoke`)
  that exercises every wired AppKit selector — window lifecycle, IME,
  menu and tray dispatch, global hotkeys, observers, URL scheme — for
  manual verification.
- `scripts/run-objc2-smoke-bundled.sh` wraps the smoke example in a
  one-off `.app` so `kael://` URLs route to it via Launch Services for
  end-to-end URL-scheme testing.

### Internal

- Reorganised the mac platform module so each formerly-cocoa file
  (`auto_launch`, `dialog`, `permissions`, `biometric`, `tray`,
  `media_capture`, `screen_capture`, `attributed_string`,
  `window_appearance`, `dispatcher`, `global_hotkey`, `keyboard`,
  `events`, `platform`, `window`, `metal_renderer`) lives entirely on
  `objc2` primitives.
- `cargo fmt --all --check`, `cargo clippy -p kael`, and
  `cargo test -p kael --lib platform::mac` (9/9) all clean.

[0.1.2]: https://github.com/Augani/kael/releases/tag/v0.1.2
