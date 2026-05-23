# Kael High-Performance Desktop App Framework Plan

## Goal

Prepare Kael to support large, high-performance desktop apps in general, not only a video editor. The benchmark should be apps in the weight class of VS Code/Zed, Figma, Slack/Discord, Notion/Obsidian, Linear, Photoshop/DaVinci Resolve, OBS, Xcode, and professional data dashboards.

This plan avoids duplicating work already present in Kael. The framework already has strong foundations for GPU rendering, layout, windows, actions/keymaps, menus, webviews, storage, document lifecycle, diagnostics, workers, notifications, sharing, capture, media playback, lists, forms, modals/popovers, theming, and packaging scaffolding. The missing work is mostly in production depth, large-data ergonomics, app-scale architecture, platform parity, observability, and specialized workload engines.

## Benchmark App Classes

Use these app classes as capability benchmarks:

- IDE/workspace apps: Zed, VS Code, Xcode, JetBrains IDEs.
- Collaborative document apps: Notion, Obsidian, Craft, Figma.
- Communication apps: Slack, Discord, Teams.
- Creative/media apps: Photoshop, Final Cut, DaVinci Resolve, OBS.
- Data/operations apps: Linear, Airtable, dashboards, analytics tools.
- Utility/native apps: Raycast, Arc-style shell apps, menu-bar/background agents.

## Existing Foundations To Reuse

- Core UI/runtime: `crates/kael`.
- Storage: `crates/kael_storage`.
- Document lifecycle: `crates/kael_document`.
- Diagnostics and tracing: `crates/kael_diagnostics` plus `crates/kael/src/tracer.rs`.
- Worker/process infrastructure: `crates/kael/src/runtime`, `crates/kael/src/worker_api.rs`, `crates/kael/src/background_jobs.rs`.
- Media playback: `crates/kael-media`, `crates/kael_audio`.
- PDF/document/audio/notification/share platform crates.
- Benchmark scaffolding: `crates/kael/src/benchmark.rs`.
- Distribution tooling: `xtask`.

## Phase 1: Product-Scale Benchmark Harness

- Expand the existing benchmark scaffold instead of creating a duplicate benchmark system.
- Add benchmark scenarios for:
  - IDE workspace with file tree, tabs, editor, terminal, diagnostics panel.
  - Chat app with thousands of messages and live typing.
  - Notion-style document with nested blocks, embeds, and large undo history.
  - Figma-style canvas with thousands of nodes.
  - OBS/video editor workload with live preview, thumbnails, waveforms, and export.
  - Data dashboard with large tables, charts, filters, and real-time updates.
- Measure:
  - cold start
  - warm start
  - first interactive frame
  - frame time percentiles
  - input-to-present latency
  - scroll latency
  - resize smoothness
  - memory growth over long sessions
  - CPU and GPU usage
  - idle power
  - wakeups per second
  - asset cache hit rate
- Add regression thresholds and CI reporting.
- Export traces in Chrome Trace format and attach benchmark artifacts.

## Phase 2: App Architecture Kit

- Add a reusable app shell layer for large apps:
  - workspace model
  - command registry
  - command palette
  - menu/action synchronization
  - global search surface
  - dock/sidebar/panel layout
  - tab and split-pane model
  - status bar model
  - inspector/property panel model
- Add a typed navigation/router abstraction for multi-window apps.
- Add persisted layout state:
  - window placement
  - open tabs
  - split sizes
  - sidebars
  - panels
  - recent workspaces
- Integrate with existing `Action`, `Keymap`, `DocumentController`, and session storage rather than replacing them.

## Phase 3: Large Data And Virtualization

- Build higher-level virtualized components on top of existing list primitives:
  - virtual table
  - tree table
  - outline/tree view
  - infinite feed
  - masonry/grid collection
  - grouped list
  - sticky headers/columns
- Add selection models:
  - single selection
  - multi-selection
  - range selection
  - keyboard selection
  - drag selection
- Add large-data update strategies:
  - diffed collection updates
  - stable item identity
  - incremental measurement
  - background sorting/filtering
  - visible-range prefetch
- Add stress tests with 100k+ rows/items and rapid updates.

## Phase 4: Text, Editing, And Document Engine

- Build a production text/document editing layer for IDEs, notes apps, chat composers, and inspectors.
- Add:
  - rope-backed text storage
  - incremental syntax highlighting hooks
  - multi-cursor editing
  - code folding model
  - find/replace
  - inline diagnostics
  - minimap hooks
  - block document model
  - rich copy/paste
  - spellcheck hooks
  - IME regression tests
- Reuse the existing text/rich-text/input primitives where possible.
- Add benchmark documents:
  - large code file
  - large markdown document
  - deeply nested block document
  - chat composer with attachments and mentions.

## Phase 5: Rendering And Scene Scalability

- Add a renderer performance roadmap:
  - frame budget instrumentation
  - layout/paint invalidation counters
  - scene node count reporting
  - GPU upload byte counters
  - texture atlas pressure metrics
  - overdraw/debug overlay
  - dirty-region visualization
- Add reusable canvas/scene layers for creative apps:
  - zoomable canvas
  - pan/zoom controller
  - object selection
  - transform handles
  - snapping guides
  - spatial index for hit testing
  - tiled rendering for huge scenes
- Add GPU resource lifecycle APIs:
  - texture budget
  - explicit eviction
  - cross-platform surface abstraction
  - low-copy image/video upload path
- Extend `surface()` beyond macOS CoreVideo so Windows and Linux have equivalent high-performance paths.

## Phase 6: Background Work, Scheduling, And Cancellation

- Upgrade the existing job scheduler instead of creating a separate queue.
- Add:
  - async execution
  - progress events
  - cancellation tokens
  - priorities
  - bounded concurrency
  - retry policy
  - pause/resume
  - dependency graph
  - observable job status
  - cooperative shutdown
- Use it for:
  - indexing
  - search
  - import/export
  - thumbnailing
  - sync
  - build/test tasks
  - analytics queries
  - media processing
- Add worker-pool health checks and crash recovery for long-running apps.

## Phase 7: Asset, Cache, And Offline Data Layer

- Add a general app cache crate rather than a media-only cache.
- Support:
  - memory cache
  - disk cache
  - cache namespaces
  - content-addressed blobs
  - metadata database
  - eviction by size/age/priority
  - background warming
  - stale-while-revalidate
  - offline-first reads
  - cache corruption recovery
- Reuse `kael_storage` for metadata and migrations.
- Add specialized adapters:
  - image thumbnails
  - media thumbnails/waveforms/proxies
  - remote API responses
  - document previews
  - search indexes.

## Phase 8: Networking, Sync, And Collaboration

- Add an app-scale networking layer:
  - typed API client conventions
  - retry/backoff
  - request cancellation
  - auth token storage
  - offline queue
  - upload/download progress
  - WebSocket/session reconnection
  - background sync
- Add collaboration primitives:
  - presence model
  - local operation log
  - conflict handling hooks
  - CRDT/OT integration points
  - awareness/cursor updates
- Keep the core framework transport-agnostic so apps can bring their own backend.

## Phase 9: Accessibility And Internationalization

- Turn accessibility from role mapping into a production requirement.
- Add:
  - cross-platform accessibility tree validation
  - screen reader smoke tests
  - keyboard-only navigation tests
  - focus order validation
  - accessible names/descriptions audits
  - high contrast mode hooks
  - reduced motion hooks
  - dynamic type/text scaling policy
- Add internationalization support:
  - string catalog loading
  - pluralization
  - locale formatting
  - RTL layout checks
  - font fallback tests
  - IME tests for CJK input.

## Phase 10: Platform Parity Completion

- Track and close platform gaps without duplicating generic APIs.
- Complete or harden:
  - notification actions on macOS/Windows
  - push registration metadata and fallbacks
  - share receiver registration on macOS/Linux/Windows
  - screen/window capture runtime initialization on Windows/Linux
  - microphone capture runtime initialization on macOS/Windows/Linux
  - system audio loopback on Windows/Linux
  - Linux app activation/hide semantics where possible
  - Linux auxiliary executable lookup
  - global hotkey behavior on Wayland or explicit unsupported policy
  - status item/menu-bar APIs on macOS
  - print behavior and print dialog parity
  - progress bar/taskbar/dock progress parity
- Add capability reports so apps can query support at runtime and degrade gracefully.

## Phase 11: Security, Permissions, And Sandboxing

- Expand permission and capability handling for serious desktop apps.
- Add:
  - unified permission prompt flow
  - secure credential/keychain wrappers
  - file-scoped access tokens/bookmarks
  - plugin permission manifests
  - process capability limits
  - network permission policy
  - audit logging for sensitive operations
- Harden worker and extension processes:
  - structured lifecycle
  - crash containment
  - rate limits
  - memory limits where available
  - IPC schema versioning
  - compatibility negotiation.

## Phase 12: Plugin And Extension Platform

- Build on the existing extension/process model.
- Add:
  - extension manifest format
  - contribution points
  - commands
  - menus
  - panels/views
  - settings
  - keybindings
  - file type handlers
  - safe storage
  - extension host diagnostics
  - extension crash/restart behavior
- Add examples:
  - theme extension
  - command extension
  - panel extension
  - language/tooling extension.

## Phase 13: Observability And Developer Tools

- Build production developer tools around the existing tracer and diagnostics crates.
- Add:
  - live inspector panel
  - element tree inspection
  - layout bounds overlay
  - accessibility tree viewer
  - frame timeline
  - job queue viewer
  - asset cache viewer
  - memory snapshot hooks
  - panic/crash report viewer
  - log viewer with filtering
- Add opt-in telemetry hooks:
  - performance events
  - app lifecycle events
  - feature usage counters
  - privacy filters
  - local-only mode.

## Phase 14: Packaging, Updates, And Release Hardening

- Expand `xtask` distribution tooling.
- Add:
  - reproducible release profiles
  - app icons and metadata validation
  - entitlement generation
  - macOS hardened runtime validation
  - notarization log parsing
  - Windows installer/MSIX/WiX support
  - Linux AppImage/Flatpak/deb/rpm support
  - delta updates
  - rollback-safe updates
  - release channel support
  - symbol upload for crash reporting
  - dependency license report
  - SBOM generation.

## Phase 15: Specialized Workload Engines

Add specialized engines only after the general platform layers are in place.

### Media/Video Workloads

- Random-access decode.
- Media probe metadata.
- Thumbnail/waveform/proxy cache adapters.
- Timeline model.
- Preview compositor.
- Audio mix engine.
- FFmpeg export pipeline.

### IDE Workloads

- File watcher scaling.
- Project indexing.
- Language server process management.
- Terminal/PTY support.
- Search index.
- Diagnostics model.
- Git integration hooks.

### Canvas/Design Workloads

- Scene graph model.
- Spatial index.
- Tiled canvas rendering.
- Selection and transform handles.
- Vector path editing.
- Export pipeline.

### Data Dashboard Workloads

- Virtual tables.
- Query job scheduler.
- Streaming data updates.
- Chart primitives.
- CSV/Parquet import adapters.
- Large-filter and grouping benchmarks.

## Recommended Implementation Order

1. Product-scale benchmark harness.
2. App architecture kit.
3. Large data and virtualization.
4. Background scheduler upgrade.
5. Asset/cache/offline layer.
6. Rendering and scene scalability instrumentation.
7. Accessibility and internationalization.
8. Platform parity completion.
9. Security and permissions hardening.
10. Packaging and release hardening.
11. Specialized workload engines: media, IDE, canvas, dashboard.

## General Readiness Criteria

Kael is ready for large desktop apps when it can reliably:

- cold start a complex app shell quickly and measure it in CI
- keep idle CPU and wakeups near zero
- scroll and update 100k+ item views smoothly
- handle multi-window workspace state across restarts
- run background jobs with progress, cancellation, and crash recovery
- persist documents, layouts, caches, and recent state safely
- expose accessible, keyboard-navigable UI on all target platforms
- report platform capabilities and degrade gracefully
- capture traces, metrics, logs, and crashes for real debugging
- package, sign, update, and roll back apps on macOS, Windows, and Linux
- support at least one heavy benchmark app from each class: IDE, chat, document, canvas, media, and dashboard.

## Video Editor Readiness Criteria

The media workload is ready when Kael can:

- import media
- scrub a 10-minute video without restarting decode from zero
- display thumbnails from cache
- display waveforms from cache
- trim, split, move, and delete clips with undo
- preview a two-track timeline
- export a short edited MP4
- cancel export without corrupting output files
- recover cleanly from failed media jobs

