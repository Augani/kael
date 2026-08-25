# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Since the workspace is currently at `0.x`, the public API is not yet
stabilised — minor version bumps may include breaking changes.

## [Unreleased]

## [0.4.1] - 2026-08-25

### Added

- Added source-owned web packaging to `kael web build` and `kael web serve`.
  `--html` preserves an application's host page and `--assets` copies a
  symlink-free product asset tree without overwriting generated Wasm loader
  files. The unchanged generated-project gate proves default and customized
  deployments in a real browser.
- Rebuilt the documentation and `llms.txt` around the current one-codebase
  desktop/browser architecture, object-oriented task guides, WebAssembly
  interactivity and deployment, platform boundaries, release evidence, and
  remaining work. The docs gate now compiles the shared quick start on native
  and Wasm targets and rejects broken routes, orphaned pages, stale LLM links,
  duplicate IDs, and unlicensed or oversized font assets.

### Changed

- Documented Astryx as the design inspiration for `kael_ui` while preserving
  the boundary between Kael's visually neutral core and its optional,
  customizable component layer.
- Browser matrix reports now record the retained frames and observation time of
  a real component ripple, so animation scheduling is measured alongside
  pointer activation instead of inferred from a static rendered frame.

### Fixed

- Fixed the browser display-capture release proof freezing its delivery result
  before a valid frame that arrived after resume. The gate now accepts that
  frame only after validating its dimensions, RGBA format, byte length, alpha,
  running state, and pause/resume lifecycle.
- Fixed the macOS native renderer proof sharing one timeout across initial
  activation and every retained revision. Each required revision now has its
  own bounded deadline and is driven through the real native resize path, so a
  loaded or occluded automation host cannot consume later frame budgets while
  resize synchronization and immediate retained redraws remain covered.
- Fixed browser smoke cleanup races where Chromium child processes could
  briefly recreate a temporary profile after a successful proof and turn the
  hosted release gate red during EXIT cleanup.

## [0.4.0] - 2026-08-24

### Added

- Added native high-fidelity pointer streams to the shared `PointerInputEvent`
  contract: AppKit tablet pressure/tilt/rotation/proximity, Windows WM_POINTER
  touch/pen identity, contact geometry and bounded chronological history, X11
  XI2.2 simultaneous touch, and Wayland `wl_touch` contacts with frame batching
  and oriented geometry. Window/device teardown emits cancellation, and stable
  identified elements now retain per-pointer capture across rerenders.
- Added `PortableScene2d`, a bounded retained high-throughput 2D surface shared
  by native and browser renderers. It batches up to 100,000 quads, sprites,
  filled paths, or triangle objects with affine transforms, rectangular clips,
  source-over opacity, decoded-image/resource byte ceilings, transactional
  rollback, content-free statistics, and typed unsupported results for custom
  shaders, compute, custom blending, and 3D. Static vector transforms are
  validated and baked at record time rather than recomputed every frame.
- Added permission-gated browser display/window/tab and camera capture through
  the shared `CaptureManager` API, plus a compatibility bridge for the retained
  `App::screen_capture_sources` surface. `getDisplayMedia`/`getUserMedia` are
  invoked before the first await to preserve trusted activation; picker denial,
  ended tracks, pause/resume/stop/drop, stale async setup, bounded RGBA frame
  extraction, frame-rate/pixel/readback-throughput ceilings, and requested-audio
  rejection are explicit rather than silently diverging from desktop behavior.
- Added explicit asynchronous browser audio APIs for bounded AudioWorklet output
  using the existing Rust mixer/DSP graph, privacy-aware device enumeration, and
  credit-bounded `getUserMedia` capture. Worklet queues, chunks, channels,
  voices, events, device strings, and capture delivery are capped; pressure,
  permission, activation, routing, processor, and lifecycle failures are typed;
  owner drop and partial setup failures close contexts and microphone tracks.
  The bridge needs no shared memory or main-thread polling, while documenting
  that main-thread Rust mixing can underrun and is not worklet-owned DSP/HRTF
  parity. A local real-Chrome release gate proves graph resume, playback/control,
  frame progress, device privacy semantics, denied capture, and cleanup.
- Added one bounded `kael_net::WebSocketClient` API across native and browser
  builds with checked `ws`/`wss` URLs and subprotocols, explicit host-policy
  enforcement, ordered text/binary events, count and byte queue limits,
  non-blocking send backpressure, sanitized open/error/close metadata, bounded
  abnormal-loss reconnection, and explicit close/drop lifecycle. Native uses a
  private Tungstenite/Rustls worker without requiring Tokio; WebAssembly uses
  the browser WebSocket API. A local real-Chrome release gate proves policy,
  protocol, text/binary, ordering, size, backpressure, error, cancellation,
  and abnormal-loss reconnection behavior. SSE remains an explicit
  descriptor-only boundary; custom upgrade headers and protocol ping schedules
  are typed parity boundaries because browsers do not expose them.
- Added a maintained one-source native/browser office-suite workload covering a
  1,000,000 × 16,384 sheet, a 250,000-block document with sparse editing/search/
  undo, 10,000 virtual slide thumbnails with one retained surface, and a
  100,000-shape spatially culled whiteboard with tiled damage, bounded tile
  caching, rich pointer input, and fixed-step animation. A real-Chrome gate
  enforces mount/cache/candidate and generous latency ceilings.
- Added `DataTableSelectionSnapshot` and a compressed all-except selection
  representation. Million-row select-all is now constant-space, bulk callbacks
  remain exact, and legacy slice callbacks are bounded instead of receiving a
  misleading partial selection.
- Added `kael_office`, a native/browser OOXML/OPC byte layer for bounded
  DOCX/XLSX/PPTX detection, part and relationship access, core properties,
  paragraph/cell/slide text extraction, safe raw-part mutation, unknown-part
  preservation, and deterministic ZIP export with traversal and zip-bomb
  defenses. It is an adapter foundation, not an Office layout or calculation
  engine.
- Made `kael_pdf` available on `wasm32-unknown-unknown` without `smol` or
  `polling` in the browser graph. Portable PDF and annotation import/export byte
  APIs now share parsing, metadata, text, search, links, previews, and sidecar
  annotations across desktop and web; path operations return a typed browser
  boundary error.

- Added an opt-in `browser` backend for `kael` and `kael_ui`. The same retained
  `Scene` now renders through a dedicated WebGL2 renderer on an HTML canvas,
  with browser pointer, wheel, keyboard, focus, resize, device-pixel-ratio,
  fullscreen, appearance, timers, and animation-frame integration.
- Added deterministic browser registration for bundled Inter and JetBrains Mono
  faces, browser glyph masks for the WebGL2 atlas, a real component
  smoke application, and a documented browser capability boundary.
- Added `kael web build` and `kael web serve`. New CLI projects select native or
  browser dependencies by target and use the same `main.rs` for both; optimized
  deployable output is written to `dist/web`.
- Added sandboxed iframe WebView islands to browser builds, with stable retained
  host identity, authenticated inline/same-origin IPC, explicit origin and
  Permission Policy controls, and deterministic unsupported errors where browser
  security boundaries cannot reproduce a native WebView operation.
- Added `WebViewCapabilityReport`, an operation-level matrix for composition,
  focus, navigation/load/history, zoom/find/print, downloads/IPC, cookies,
  headers/profiles, permissions, drag/drop, devtools, and custom protocols on
  WKWebView, WebView2, WebKitGTK X11/XWayland, native Wayland, and browser
  iframe backends.
- Unified the maintained Linux WebView host on GTK4, GSK, and WebKitGTK 6 for
  native Wayland, X11, and XWayland. The portable `webview` feature now selects
  that host, with real raw handles, scale-aware retained rendering, XDG print
  parenting, and native pointer lock on both compositor families; the old
  archived GTK3/WebKitGTK 4.1 stack is no longer shipped;
  `webview-legacy-gtk3` redirects to the maintained GTK4 host.
- Added `FixedFrameClock` to `kael_engines`, with bounded fixed-timestep catch-up,
  interpolation, pause/resume/reset controls, and dropped-time telemetry for
  games and simulation-heavy applications.
- Added bulk canvas rectangle/circle submission and command reservation, plus a
  rounded-quad fast path for circles.
- Added one rich `PointerInputEvent` path across browser mouse, multi-touch, and
  pen input, including pointer identity/type, pressure, tangential pressure,
  tilt, twist, contact geometry, buttons, cancellation, capture, and bounded
  coalesced samples. Existing desktop mouse events are promoted without breaking
  compatibility.
- Added a bounded spatial-hash index, cached `SceneGraph` hit-test/viewport
  culling, deterministic tiled damage tracking, and device-pixel PNG scene export
  through browser WebGL2 and macOS Metal with typed unsupported/content-boundary
  errors.
- Added a real 1,000,000-row Kael table to the browser release smoke. The gate
  verifies the virtual list mounts at most 64 row elements and records the exact
  visible range and materialization time without allocating a million-row model;
  it also verifies a direct final-row jump and exposes a retained draggable
  scrollbar with a bounded visible thumb.
- Added a versioned, bounded typed Web Worker bridge with async health,
  request/response, progress, cancellation, termination, and a maintained
  one-million-item browser worker release probe. Shared desktop/browser code can
  use `BackgroundExecutor::spawn_worker_request` for serializable CPU work.
- Added `kael_markdown`, a structured Markdown parser that preserves nested
  blocks, images, ordered-list starts, and task-list state.
- Added a cross-platform asynchronous `kael_storage::BlobStore`: native builds
  commit SQLite BLOB records and browser builds commit raw `Uint8Array` records
  to IndexedDB. Browser settings now use the shared `PlatformKvStore` API over
  an atomic, bounded `localStorage` map.
- Added byte-oriented `kael_document` import/export on desktop and web, plus
  origin-scoped named browser documents, integrity-checked recovery snapshots,
  bounded IndexedDB versions, listing, reopen, deletion, and an explicit
  `flush_autosave` durability boundary.
- Added byte-backed desktop/browser file workflows. `prompt_for_files`,
  `show_open_files`, `ExternalFile`, `FileUpload`, and browser `DataTransfer`
  drops now share one portable intake contract, while `save_file_bytes` maps to
  native Save As plus background writes or browser Blob downloads.
- Added a bounded, diffed browser accessibility bridge that mirrors retained
  roles, labels, states, values, bounds, focusability, and actions into an ARIA
  DOM tree while leaving visual and pointer ownership with the WebGL canvas.
- Added browser `PrintJob` support through isolated multi-page printable
  snapshots for fills, strokes, text, and retained images. Hosted iframe
  documents print directly; browser security means all print requests show the
  browser dialog rather than silently selecting a printer.
- Added real Windows and Linux native `PrintJob` execution for every retained
  print command. Windows supports native dialog and silent default-printer GDI
  spooling; Linux supports XDG Print portal dialogs and explicit CUPS `lp`/`lpr`
  silent spooling through a bounded portable PDF renderer.
- Added bounded rendered-scene PNG readback for Direct3D 11 and Blade
  (Linux and optional macOS Blade), including checked dimensions and row pitch,
  BGRA/RGBA and premultiplied-alpha normalization, content-protection rejection,
  and explicit hosted/live-surface exclusions.
- Hardened native WebView integration with authenticated, main-frame bridge
  messages and origin-aware permission policies, and added keyboard-accessible
  context menus, overflow menus, tabs, tooltips, color entry, and
  viewport-clamped overlay behavior.

### Changed

- GitHub Actions used by release, readiness, and documentation workflows are
  pinned to reviewed full commit SHAs, including the current Node 24 action
  implementations. The ChromeDriver setup action is upgraded from its retained
  shell-based v2 implementation to the drop-in TypeScript v3 release.
- The 34-crate publication preflight now packages the complete selected
  workspace set together, compiles every extracted archive, and enforces the
  crates.io 10 MiB compressed archive ceiling. This catches missing packaged
  source/assets and manifest normalization failures without uploading a crate.
- macOS distribution now follows one unambiguous release order: sign the
  hardened-runtime app, create the DMG, timestamp-sign the DMG without app-only
  flags, then notarize and staple it through a required non-interactive
  `KAEL_NOTARY_PROFILE`. Bundling no longer submits an unsigned DMG early or
  duplicates the standalone notarization phase.
- macOS DMG creation now uses the current `diskutil image create` interface,
  with a capability-checked `hdiutil` fallback for older supported systems, so
  release builds no longer depend on a deprecated command on current macOS.
- Standalone signing now honors the same environment-first macOS identity and
  Windows certificate/password inputs as bundling. Windows dry-runs redact the
  certificate password instead of rendering a secret-bearing `signtool`
  command, and passwordless certificate providers no longer receive a spurious
  empty `/p` argument.
- Documentation deployment and exact-SHA platform readiness now use pinned Rust
  1.97.1 and mdBook 0.4.52 toolchains. A complete book build is release-blocking,
  so crates cannot publish while the maintainer or user documentation is broken.
- Platform readiness now runs self-terminating real-window renderer proofs for
  Metal, Direct3D 11 WARP, and Blade/lavapipe, validates multiple retained-frame
  revisions, GPU identity, device-pixel PNG readback, and a pixel-level retained
  text/glyph-atlas probe, then launches unchanged generated projects on Windows
  and Linux virtual desktops.
- Browser release packaging now requires pinned Binaryen 132 and runs
  `wasm-opt -O3` after `wasm-bindgen`; debug builds remain fast and unoptimized.
  The retained-scene smoke shrinks by roughly one quarter before transport
  compression, and release browser gates execute the optimized module. Shipped
  native and browser binaries also use fat LTO with one codegen unit; development
  profiles remain unchanged.
- New browser atlas textures are initialized with an explicit GPU clear before
  a partial glyph/image upload. This avoids Firefox's lazy texture clear and its
  render-thread warning without copying multi-megabyte zero buffers through the
  Wasm/JavaScript boundary.

- Recycling lists now retain their height index across frames, support explicit
  height revisions, use adaptive per-type reuse pools, and immediately recycle
  measurement overdraw. Uniform lists now sanitize invalid geometry and expose
  correct absolute accessibility indices.
- Lottie parsing/preparation now uses a bounded per-thread LRU cache, avoiding
  repeated preparation while preserving the renderer's thread-affinity rules.
- Animation-frame requests are coalesced per entity, and explicit animations
  complete immediately under reduced-motion, low-power, or cancellation
  policies.
- Scene frame fingerprints now cover all visual geometry and effects and are
  only computed when frame skipping is enabled. Headless benchmarks now mutate
  real scroll, interaction, resize, long-list, and memory workloads.
- Scene damage now localizes added and removed retained primitives instead of
  forcing a full-frame repaint; oversized or externally-updated cases continue
  to fail safely to full damage.
- The browser renderer now caches shader uniforms and high-water upload buffers,
  batches solid quads and sprites, uses rectangular fragment fast paths, retains
  and scissors conservative frame damage, uploads only dirty atlas regions, and
  suspends animation-frame polling while the canvas or page is hidden. Browser
  display cadence is learned from filtered animation-frame timestamps instead of
  being fixed at 60 Hz, so continuous rendering follows high-refresh displays.
- The million-row browser proof now enforces a 24-sample hardware input-to-range
  gate (80 ms p95, 160 ms p99, at most one long task) at wide and compact sizes,
  records bounded mounts/materialization/damage evidence, and labels forced
  SwiftShader runs as a separate correctness/liveness class. Renderer evidence
  prefers unmasked WebGL adapter strings, rejects known software adapters from
  hardware runs, and is release-blocking on a hosted macOS Metal browser. Matrix
  artifacts include raw retained-frame PNGs so Firefox/Xvfb screenshot
  compositing cannot hide a correctly rendered WebGL surface.
- Windows compositor pacing now tracks per-window frame demand, so idle, hidden,
  and minimized windows no longer receive refresh-rate redraw invalidations.
- Platform readiness now executes the native Windows WebView IPC/JavaScript smoke,
  builds an architecture-correct MSI with pinned WiX tooling, decompiles and
  administratively extracts it, and runs the packaged executable's version check.
- Browser readiness now includes an opt-in, one-time bounded WebGL framebuffer
  readback and rejects blank, translucent, or visually uniform output instead of
  trusting frame-presentation markers alone.
- Browser WebGL context restoration now rebuilds context-owned resources,
  repopulates textures from the retained CPU atlas, and forces a new frame
  without reloading application state.
- Release tooling now separates staging directories from uploadable artifacts,
  uses versioned tags and a distinct artifact URL base, and refuses production
  updater publishing without nonempty regular-file artifacts and a matching
  signing key/public key.
- **Breaking:** `Style::overflow_mask` and `Style::rounded_overflow_clip` now
  receive a `Window`. Internal transform composition now also receives the UI
  zoom factor. This makes borders, radii, blur, and translations respect
  application zoom consistently.
- **Breaking:** `kael_ui`'s syntax editor is now behind the `editor` feature
  (enabled by default and by `editor-languages`) so browser/minimal builds can
  omit the Tree-sitter and rope dependencies.
- Browser WebAssembly builds no longer compile `rusqlite`, `tempfile`, or
  `smol` through `kael_storage`/`kael_document`. Native path and SQL requests
  return typed platform-boundary errors instead of pretending IndexedDB is a
  filesystem or silently changing SQL semantics.

### Fixed

- Fixed small loading spinners visually wobbling on mixed-density displays.
  Spinner geometry now remains stationary while only the conic-gradient phase
  advances, and the ring center inherits its parent surface instead of painting
  a mismatched background.
- Fixed a macOS crash when a trackpad began a two-finger scroll gesture. AppKit
  `ScrollWheel`/`MayBegin` events can expose a numeric subtype that overlaps a
  tablet subtype; Kael now restricts tablet-only selectors to documented
  tablet-derived mouse events.
- Fixed Firefox WebGL2 context recovery when the context becomes lost before
  `webglcontextlost` is delivered. A queued frame can no longer replace the last
  valid recovery sample with an all-zero lost-context readback; Firefox,
  Chromium, and WebKit now reproduce the pre-loss retained pixels exactly.
- Fixed macOS frame polling after a display link was first requested while its
  window was occluded, or after a transient CoreVideo start failure. A later
  refresh now retries the missing link instead of leaving an active window stuck
  after its first AppKit frame.
- Fixed a Chromium teardown panic when `pagehide`, window hiding, or quit caused
  a synchronous canvas blur while the browser window registry was borrowed.
  Visibility callbacks are now snapshotted and invoked after releasing registry
  state, and the cross-browser matrix checks diagnostics again after context
  close so late failures cannot produce false-green reports.
- Fixed strict sandboxed browser WebViews probing an opaque iframe document in
  WebKit. Strict mode now relies solely on its pre-injected nonce-authenticated
  `postMessage` bridge, preserving the sandbox while avoiding a security-origin
  exception during load completion.
- Fixed the suite-scale browser verifier to match the shipping 16,384-column
  virtual sheet and its constant-space anchor/focus range selection. Live DOM
  mount detection now follows the current grid/row/cell accessibility roles,
  and wide/compact gates enforce bounded tiles, cells, and selection storage.
- Fixed release probe port collisions being mistaken for a started local server.
  Every shell-driven browser runtime probe now binds its HTTP server explicitly
  to `127.0.0.1`, so an occupied IPv4 port fails honestly instead of creating a
  second IPv6 listener and watching the wrong readiness log. The audio gate also
  retries once with a clean browser profile after the known short macOS Chrome
  audio-service teardown race, while persistent failures remain release-blocking.
- Fixed the `kael_net` package manifest so its two declared WebSocket examples
  ship in the crates.io tarball while the browser fixture directory remains
  excluded. The dependency-ordered preflight allowlists only those exact example
  files and continues to reject every unapproved example, benchmark, or missing
  license notice across all 34 publishable crates.

- Browser text inputs now use a hidden DOM IME bridge for composition and
  ordinary Unicode input, including marked-text updates, committed text, and
  caret-positioned candidate windows. The browser release smoke exercises
  Japanese text, emoji, and the following ordinary input event.
- Browser copy, cut, and paste now bridge the system `ClipboardEvent` payload
  into Kael's existing edit actions. Programmatic clipboard writes use Async
  Clipboard for text, HTML, and supported image types, and pasted image files
  are decoded before dispatching the Kael paste action.
- Fixed `ObjectFit::Cover` and `ObjectFit::None` images painting outside their
  element bounds. Replaced-image content is now clipped to its layout box, so
  wide images cannot overlap adjacent text or controls on native or WebGL2
  renderers.
- Fixed backing-scale/text consistency when moving windows between displays.
  macOS propagates Retina scale changes and pixel-snaps baselines; macOS and
  Windows glyph masks now use display-independent grayscale antialiasing,
  avoiding stale-resolution text and RGB/BGR subpixel color fringing on scaled
  or rotated external panels.
- Fixed scrollbar focus registration, keyboard paging/home/end behavior, and
  accessibility semantics.
- Fixed notification body activation when no custom action is registered.
- Fixed declarative WebView reconciliation undoing controller navigation on
  macOS, Windows, Linux, and browser hosts. Linux now uses one GTK-owned
  WebKitGTK 6 hierarchy across X11, XWayland, and Wayland instead of presenting
  detached pseudo-embedded overlays; raw backends without that host fail closed.
- Fixed declarative focus changes recreating Windows and Linux WebViews and
  losing live document, history, and profile state. Browser host-directed loads
  now enforce `on_navigate`, and sandbox download permission is an explicit
  `allow_downloads`/`deny_downloads` policy instead of an implicit trust-mode
  side effect.
- Fixed duplicate Enter/Space activation across controls and overlays, controlled
  tooltip/dropdown state, click-away coverage, and tab arrow-key behavior.
- Updated `h2` to a release without RUSTSEC-2026-0258.

## [0.3.1] - 2026-08-12

### Added

- Added canvas-hosted multiline text input support with controlled focus and
  selection, UTF-8 selection/composition observers, embedded navigation hooks,
  editor key policies, accessibility metadata, and bounded IME undo history.
- Added externally controlled focus and selection observation to `Textarea`.
- Added font-family previews and retained scrolling to long `Select` menus,
  plus public expanded-state support for toolbar dropdown buttons.

### Changed

- Buttons, icon buttons, and context-menu items now use the shared interaction
  path for keyboard and pointer activation, preventing duplicate callbacks.
- Select triggers keep long labels on one line while menus use a wider,
  single-scrollable viewport with a draggable scrollbar.

### Fixed

- Fixed tab and Shift+Tab traversal across inputs by preserving focus-handle
  tab metadata and stopping already-handled key propagation.
- Fixed dialogs and overlays so their full surfaces occlude pointer input to
  underlying content.
- Stabilized interactive hover feedback and select keyboard opening.

## [0.3.0] - 2026-06-11

### Production readiness

- Unified every workspace crate and starter template on Rust 1.97.1, Rust
  2024, and one shared minimum-supported Rust version.
- Kept `kael` as the standalone primitives/runtime crate and `kael_ui` as an
  optional, brandable component layer. WebView support is now opt-in.
- Consolidated the runnable example surface into the repository-only Astryx
  showcase; examples, benchmarks, screenshots, templates, and internal audit
  material are excluded from published crate archives.
- Added strict all-target/all-feature lint and test gates, dependency auditing,
  native platform CI, package-content checks, and an explicit manual publish
  gate. Repository tags do not publish crates.
- Corrected capability reporting so descriptor-only or app-owned integrations
  are not presented as complete native backends. Agent planning metadata is
  available through the opt-in `agent-tools` feature.
- Replaced inherited runtime and benchmark environment-variable names with the
  `KAEL_*` namespace.

### Added

- **Implicit style transitions** — `.transition(duration)` and
  `.transition_with(duration, easing)` on any keyed element animate
  changes to background (including gradients), border color, text color,
  opacity, corner radii, box shadows, rotation, and scale. Hover, active,
  focus, and state-driven restyles ease instead of snapping; transitions
  are interruptible and retarget from the current visual state.
- **Subtree transforms** — `rotate`, `scale`, new `translate`/
  `translate_x`/`translate_y`, and new `skew_x`/`skew_y` on a `div` now
  transform the element's whole subtree (text, icons, images, emoji, and
  children), not just its own background quad.
- **Rounded-corner clipping** — `overflow_hidden` combined with a corner
  radius clips children to the rounded corners, on all three render
  backends.
- **FLIP layout animation** — `.animate_layout(duration)` makes a keyed
  element glide to its new position when layout moves it.
- **Color filters** — `grayscale()`, `saturate()`, `brightness()`, and
  `contrast()` apply to an element's whole subtree.
- **Gradient borders** — `border_gradient()` accepts any linear, radial,
  or conic gradient.
- **Content blur and drop shadows** — `effect_layer(child)` with
  `.content_blur(px)` and `.drop_shadow(shadow)` renders a subtree to a
  texture and composites it frosted and/or with a silhouette-following
  shadow.
- **Springs and gestures** — `SpringValue`/`SpringPoint` physics drivers
  and `DraggableSpring`, a throwable container that hands pan-gesture
  velocity to a spring with optional snap points.
- **Derived state** — `Computed<T>` / `cx.computed` memoize values
  derived from entities, with dependency tracking and invalidation on
  notify.
- **Async data helpers** — `kael_ui::query` ships `Loadable<T>`,
  `QueryState<T>` (loading/error lifecycle, stale-response dropping,
  debounce, refetch), and a TTL `QueryCache`.
- **Layered soft shadows** — the `shadow_*` theme tokens are now
  two-layer stacks (`ShadowStack`), with dark-theme alphas tamed.
- New docs chapters: State Management, Async & Data Fetching, Navigation;
  the Animations chapter covers transitions, FLIP, and springs.
- The Astryx showcase now covers the maintained component and interaction
  surface in one sectioned application.

### Changed

- kael_ui core controls (button, toggle, checkbox, radio, input,
  dropdown, select, tooltip, dialog) ship with eased micro-interactions
  by default, consuming the previously unused `transition_*` tokens.
- The kael_ui theme is a kael `Global`: read it with `Theme::of(cx)`
  (zero-clone). `use_theme()` still works as a compatibility shim.
- `kael::Navigator` is the canonical navigation stack; kael_ui's
  `ViewRouter` is kept for compatibility but considered legacy.
- `PaintQuad::border_color` is now a `Background` (gradient-capable);
  `Hsla` values convert via `Into`.

### Fixed

- Transform origins were not scaled to device pixels (wrong rotation
  center on non-1x displays).
- Rotated or skewed rounded quads rendered distorted corners and
  gradients (fragment math now runs in local space).
- kael_ui scroll physics assumed 60Hz (`dt = 1/60`); momentum is now
  frame-rate correct on 120Hz displays.
- `AnimatedPresence`/`AnimatedList` exit-unmount race that could remove
  an element mid-exit-animation.

## [0.2.0] - 2026-06-10

### Added

- **Custom themes for `kael_ui`** — `Theme::custom(tokens)` plus
  `ThemeVariant::Custom` let an app brand itself by starting from any of
  the 18 preset token sets and overriding fields with struct-update
  syntax. New `custom_theme_demo` example shows the full customization
  stack (brand theme, live switching, per-component `Styled` overrides).
- **Live theme switching** — `kael_ui::install_theme` now refreshes every
  open window, so calling it again at runtime restyles the app
  immediately.
- **Real template apps** — the three `templates/` members were stub
  windows; they are now complete kael_ui applications to copy from:
  `dashboard-app` (sidebar, stat cards with sparklines, line/bar charts,
  data table), `messaging-app` (conversation list, chat bubbles,
  composer), and `workspace-app` (file tree, tabbed syntax-highlighted
  editor, toolbar, status bar).
- **CI now verifies the UI surface on all three platforms** —
  `verify-kael.sh`/`.ps1` lint (`clippy -D warnings`), unit-test, and
  type-check `kael_ui`, all 140+ examples, and the three template apps on
  macOS, Windows, and Linux.

### Changed

- **`kael_ui::prelude` is self-sufficient** — it now re-exports the Kael
  essentials (`div`, `px`, `Application`, `Render`, `Result`,
  `AssetSource`, `ClickEvent`, geometry types, …), so an app needs only
  `use kael_ui::prelude::*;`. Mixing it with `use kael::*;` is no longer
  needed or recommended: the dual-glob pattern made `Button`, `Theme`,
  `Select`, and friends ambiguous between the core legacy widgets and the
  component library (rustc and rust-analyzer resolved them differently).
  All examples migrated.
- `kael_ui` is clippy-clean under `-D warnings`; `ValidationRules`
  callback fields changed from `Arc` to `Rc` (they are UI-thread-only,
  and the stored closures were never `Send`).

### Fixed

- `Input` no longer overwrites a placeholder configured on its
  `InputState` with an empty string when the element-level placeholder is
  unset.
- `AnimatedSwitch` now implements `Styled`, accepting user style
  overrides like every other component.

- **Kael is re-centered as a general-purpose desktop application
  framework.** The project drifted toward being a video-editor toolkit;
  this release corrects course. The release vision states the
  mission, the layering rule that keeps the core domain-neutral, the
  `adabraka-gpui` → Kael naming history, and the project's relationship
  to Zed/upstream GPUI. The production roadmap was retitled and re-read
  through the general-app lens: production gates (accessibility,
  packaging, update integrity, text correctness) come first, the GPU
  substrate (render targets, custom shaders, render graph) ships as
  public framework API, and the video-editor bar is the optional media
  track.
- **BREAKING (`kael_engines`): the 15 media/NLE modules moved to the new
  `kael_media_engines` crate.** `kael_engines` now contains only
  domain-neutral engines: `bidi` (UAX#9), `linebreak` (UAX#14), `undo`,
  `crash_report`, and the `canvas`/`dashboard`/`ide` data models. Code
  using `kael_engines::{media, compositor, effects, export, transform,
  automation, audio_mix, generators, project, markers, playback,
  timecode, scopes, subtitles, frame_cache}` should depend on
  `kael_media_engines` and update the crate prefix — module contents are
  unchanged. `kael_engines` no longer depends on `kael_render_graph`.

### Added

- **New crate: `kael_media_engines`** — the optional media/NLE leaf
  stack (timeline, compositing, audio mix, export), split out of
  `kael_engines` so that crate stays domain-neutral.
- **Design proposal 0001: public render targets, passes, and custom
  shaders**
  ([docs/design/0001-render-targets-and-custom-shaders.md](docs/design/0001-render-targets-and-custom-shaders.md))
  — the API contract for app-allocatable typed render targets (incl.
  `Rgba16Float`), WGSL shader registration via `naga`, fragment +
  compute passes, and an executor for the `kael_render_graph` DAG,
  staged Metal-first with golden-image tests.

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

[0.4.1]: https://github.com/Augani/kael/releases/tag/v0.4.1
[0.4.0]: https://github.com/Augani/kael/releases/tag/v0.4.0
[0.3.1]: https://github.com/Augani/kael/releases/tag/v0.3.1
[0.3.0]: https://github.com/Augani/kael/releases/tag/v0.3.0
[0.2.0]: https://github.com/Augani/kael/releases/tag/v0.2.0
[0.1.2]: https://github.com/Augani/kael/releases/tag/v0.1.2
