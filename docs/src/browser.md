# Browser (WebAssembly)

Kael's `browser` feature sends the same retained `Scene` used by the desktop
backends to a dedicated WebGL2 renderer. Application state, layout,
components, painting, virtualization, and animation stay in Rust; this is not a
DOM rewrite or a WebView wrapper.

Sharing that application and scene architecture is not a promise of identical
pixels, throughput, or platform capabilities. The browser uses its own WebGL2,
text, input, scheduling, and iframe integrations, while desktop builds use
native renderers and operating-system services. Treat browser support as a
separate release target: test it directly, branch on capability reports for
native-only workflows, and measure performance on the browsers and devices you
intend to support.

## Build and run the same application

Projects created with `kael new` are ready for both targets. After installing
the version-matched packager once, one command builds the application, starts a
local server with the correct WebAssembly MIME type, and opens it:

```sh
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --global binaryen@132.0.0
kael web serve
```

Use `kael web build` to produce an optimized static site under `dist/web`:

```text
dist/web/
├── index.html
├── app.js
└── app_bg.wasm
```

Deploy those files to any static host. In a Cargo workspace, select the
application with `--package <name>` or `--bin <name>`. `--debug` trades output
size and runtime optimization for a faster development build.

An existing project should select dependencies by compilation target so its
single `main.rs` gets native services on desktop and the lean browser surface on
wasm32:

```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
kael = { version = "0.4", features = ["runtime_shaders"] }
kael_ui = "0.4"

[target.'cfg(target_arch = "wasm32")'.dependencies]
kael = { version = "0.4", default-features = false, features = ["browser"] }
kael_ui = { version = "0.4", default-features = false, features = ["browser"] }
```

## Rendering and input contract

The first Kael window uses `<canvas id="blade">`. Additional calls to the same
`open_window` API used by desktop applications create independent retained
surfaces inside the page. Each surface owns its own canvas, WebGL2 renderer,
scene, atlas, input/IME bridge, accessibility tree, print manager, and hosted
WebView layer. Focus raises the surface in Kael's in-page stack; window bounds,
resize, move, minimize, maximize/fullscreen, show/hide, and close flow through
the normal `PlatformWindow` API. Application hide/page-hide pauses every
surface, including its semantic and iframe overlays, without forgetting which
windows were individually visible.

These secondary surfaces deliberately are not popup tabs or native operating
system windows. They remain bounded by the browser viewport and cannot be
detached to another display, appear in the taskbar/dock, or escape browser
z-order and permission policy. Applications that need an inspector or palette
can therefore keep their desktop multi-window state and behavior on the web;
applications that require independently movable OS windows must branch on the
capability report.

CSS pixels remain Kael logical pixels. Every canvas backing store tracks
device-pixel-ratio changes, including moving the page between displays, and
resize invalidation updates both layout and its WebGL viewport. Rendering is
scheduled with `requestAnimationFrame`; idle, minimized, application-hidden, or
page-hidden windows stop requesting frames until new work arrives or visibility
returns. Glyph and image atlases upload only their dirty regions after
allocation, keeping Wasm-to-WebGL transfer overhead bounded as the atlas grows.

If the browser loses the WebGL context, Kael preserves application state and
its CPU-side atlas, rebuilds all shaders/buffers/textures after restoration, and
forces a fresh retained frame. Applications do not need to reload the page.

Kael learns the active display cadence from a short median-filtered window of
animation-frame timestamps instead of assuming 60 Hz. Continuous simulations
can therefore follow 90/120/144 Hz panels, while isolated dropped frames and
idle/background gaps do not redefine the reported refresh rate.

The release smoke opts into a one-time bounded WebGL framebuffer readback after
the second retained frame. Readiness requires varied, opaque pixels with a stable
framebuffer digest in addition to the renderer's presentation marker, so a blank
or failed draw cannot pass the browser release gate. It then deliberately loses
and restores the WebGL2 context and requires the post-restore frame to match the
pre-loss framebuffer digest exactly, covering shader/buffer recreation and full
glyph/image atlas re-upload.

The same smoke renders a Kael table backed by 1,000,000 logical rows without
materializing a million-row collection. Its real virtual-list callback publishes
the exact visible range, row-element count, and materialization time. Release
readiness requires no more than 64 mounted rows and proves that the range length
matches that mounted count. It also jumps directly to row 1,000,000, verifies
that the final row entered the mounted range, then returns the visible showcase
to row one. The table uses Kael UI's always-visible, draggable retained scrollbar.
The three-engine release matrix then drives 24 retained scroll steps and requires
at least 16 measured range changes, no more than 64 peak mounted rows, scroll
response p95 at or below 80 ms and p99 at or below 160 ms, materialization p99 at
or below 20 ms, and at most one browser long task. The report retains p50/p95/p99,
sample count, long-task support/count, materialization p99, peak mounts, and
retained damage ratio for regression analysis. Those strict limits are the
hardware-GPU performance gate. CI also runs the same pixels through an explicitly
forced software rasterizer with a separately labelled liveness/correctness budget;
host CPU contention in SwiftShader is never reported as hardware performance.
The browser renderer batches solid retained quads, skips expensive rounded-SDF
and filter work for plain cells, preserves the previous framebuffer, and scissors
localized scene damage. The maintained workload therefore measures the real
end-to-end input-to-visible-range path rather than a DOM-only microbenchmark.

Pointer capture, mouse buttons, wheel deltas, keyboard events, focus, theme
appearance, fullscreen, and canvas resizing feed Kael's normal input and window
paths. Focused text inputs use a caret-positioned, visually hidden DOM input to
receive browser IME composition and ordinary Unicode input while Kael retains
selection, marked-text, editing, and painting ownership. The release smoke
commits Japanese text and emoji through `composition*` events, suppresses the
browser's trailing duplicate input event, and then verifies a normal text insert.
WebGL2 is required.

Clipboard shortcuts use browser `copy`, `cut`, and `paste` events so Kael's
existing synchronous edit actions see the current system payload. Plain text and
HTML round-trip synchronously; pasted image files are read before the paste
action is dispatched. Programmatic writes use Async Clipboard for supported
text, HTML, and image MIME types while retaining an in-process fallback when the
browser denies permission. Because Kael's legacy platform read method is
synchronous, reads outside a browser paste event return that latest mirror rather
than awaiting a new system read. The release smoke pastes into a real
`InputState`, selects it, copies it back through `ClipboardEvent`, and compares
the Unicode payload exactly.

Browser paste payloads are treated as hostile input and rejected atomically when
they exceed 32 advertised items, 8 MiB of plain UTF-8 text, 16 MiB of HTML, 1 MiB
of `text/uri-list`, 64 MiB for one encoded image, or 128 MiB across all retained
representations and images. Textual data must cross the synchronous browser event
boundary before Kael can measure it, but it is checked before metadata or
application-state copies are made. Image sizes are checked before reading and
each `Blob` read is sliced to the remaining budget plus one sentinel byte, so a
dishonest size report cannot trigger an unbounded Rust allocation. A rejected
event is consumed without changing the clipboard mirror or dispatching the paste
action. The release smoke exercises oversized text, image, aggregate, and item
count rejection in the real retained input path.

## Portable file workflows

Use `App::prompt_for_files` or `App::show_open_files` when application code must
compile unchanged for desktop and browser targets. They return `ExternalFile`
values containing a safe base name, optional MIME hint, and shared encoded
bytes. Desktop selections additionally retain the real source path; browser
selections deliberately do not invent one. `FileUpload` uses this contract for
picker selections and accepts the same byte-backed values from browser
`DataTransfer` drops. Existing native path drops remain supported.

Browser intake is asynchronous and bounded to 256 MiB per file and 512 MiB per
selection/drop. A file that cannot be read remains visible as an unavailable
`ExternalFile` with an explicit error instead of silently disappearing. These
bounds also apply to the default desktop byte adapter, keeping one-codebase
behavior predictable. Code that truly needs an arbitrary native path should
continue to use `prompt_for_paths` behind a capability branch.

Use `App::save_file_bytes` for generated DOCX/XLSX/PPTX/PDF/project output. It
shows a native Save As dialog and writes on a background thread on desktop; in
the browser it creates a typed Blob and initiates a download with the same
suggested filename. Browser policy still controls the final download location,
so an application cannot demand an arbitrary destination path.

Parse selected or dropped bytes with the same native/browser
`kael_office::OfficePackage` and `kael_pdf::PdfDocument` APIs. The
[Office and PDF document bytes](office-documents.md) guide covers extraction,
deterministic OOXML round trips, annotation bytes, security bounds, and the
deliberate boundary short of full Microsoft Office or PDF layout.

## Notifications and sharing

Enable `notifications-full` and `share` alongside `browser` when the same
application sends notifications or opens a share picker. Keep one async call at
the product boundary:

```rust,no_run
# async fn portable() -> Result<(), Box<dyn std::error::Error>> {
use kael::{LocalNotification, NotificationCenter, ShareFile, ShareSheet};

NotificationCenter::new()
    .schedule_local_async(LocalNotification::new("Export ready", "suite.docx"))
    .await?;

ShareSheet::builder()
    .text("Suite export")
    .memory_file(ShareFile::new(
        "suite.docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        vec![0_u8; 16],
    ))
    .build_checked()?
    .show_portable()
    .await?;
# Ok(())
# }
```

Both browser operations are activation-sensitive. A notification permission
prompt and `navigator.share` should start directly inside a click or keyboard
activation; awaiting unrelated work first can consume that browser activation.
Notification permission granted/denied/prompt/unavailable states and share
cancelled/denied/unavailable/unsupported states are typed; a permission prompt
started after transient activation is lost returns `UserActivationRequired`.
Browser notification
delivery is immediate and page-created: durable interval/calendar/location
triggers, push, custom action buttons, named sounds, and badge counts require a
product service worker or native backend and return explicit unsupported
errors. Browser sharing accepts bounded text, URL, in-memory files, and images,
but cannot read `PathBuf`, select a destination programmatically, filter the
system picker, or register a share target without a PWA manifest/service worker.

Bundled TrueType/OpenType bytes passed to `TextSystem::add_fonts` take the same
pure-Rust text path on every browser: Cosmic Text 0.19 performs advanced
HarfRust shaping, including GPOS kerning, GSUB ligatures, bidirectional layout,
clusters, and caller-supplied OpenType features, and Swash rasterizes the
resulting glyph IDs into Kael's page-packed WebGL atlas. Metrics come from the
actual selected face rather than estimated Canvas ratios. The bytes are also
registered through `FontFace`, including deterministic `.KaelSans` and
`.KaelMono` aliases, so browser-owned fallback rendering uses the same family.
A completed asynchronous `FontFace` load clears text and atlas caches and forces
relayout.

Browser and operating-system fonts whose OpenType bytes have not been passed to
Kael remain an explicit Canvas 2D fallback: Canvas prefix measurement retains
pair kerning, but that fallback emits one raster mask per Unicode scalar because
the browser does not expose its shaped glyph IDs or font bytes. Mixed
byte-backed/Canvas lines are shaped as separate segments. Cosmic Text can choose
another registered byte-backed face for missing glyphs, but it cannot reach an
unregistered system face from inside a bundled-font segment. Applications that
need complex-script coverage should bundle an appropriate font (or request that
system family directly and accept the Canvas fallback boundary). Color emoji is
not yet preserved by the monochrome glyph atlas. Swash/CoreText/DirectWrite can
also produce small antialiasing differences even when layout metrics match.

The maintained release smoke opts into a Rust-side text probe. Readiness now
requires the bundled Inter face to preserve the actual three `ffi` clusters
(that particular build has no standard `ffi` substitution), apply its
discretionary arrow ligature and `AV` kerning, and place forced-RTL clusters in
the expected visual order.

## Hosted WebView surfaces

The same `webview`, `webview_html`, and `WebViewController` APIs create iframe
overlays above the WebGL canvas in browser builds. Bounds, clipping-derived
visibility, opacity, focus, `src`, `srcdoc`, page-load observation, host-to-frame
messages, and authenticated inline/same-origin bridge messages are supported.
The maintained browser smoke app exercises a strict-sandbox inline iframe and
does not declare startup ready until its nonce-authenticated message arrives.

`BrowserWebViewPolicy` makes iframe trust explicit. Its default `Strict`
sandbox gives the hosted document an opaque origin, preventing same-origin
content from reaching Kael's host DOM; it also blocks popups and top-level
navigation. `TrustedSameOrigin` preserves origin storage for trusted content,
while `Unrestricted` most closely resembles a native WebView and must only be
used for app-controlled pages. The policy also
sets host-directed allowed origins, referrer policy, credentialless/lazy
loading, user-activated download permission, and an explicit browser Permission
Policy allowlist. `WebViewOptions::allow_downloads()` enables the sandbox
`allow-downloads` token and `deny_downloads()` removes it; this never enables
silent downloads or application-selected destinations. These presets can only
deny downloads in `Strict` and `TrustedSameOrigin`; `Unrestricted` deliberately
has no iframe sandbox for Kael to tighten and remains subject to browser policy.
Kael applies both the
origin allowlist and `on_navigate` to host-directed initial, declarative, and
controller loads. Embedded content can still self-navigate unless it cooperates
with the Kael bridge or the application constrains it with CSP, so browser
`on_navigate` is not a native-equivalent cross-origin interception boundary.

Browser security boundaries cannot reproduce every native WebView operation.
Arbitrary cross-origin frames cannot be scripted or inspected by their parent;
custom request headers and user agents, isolated profile keys, HttpOnly cookie
CRUD, profile-wide data clearing, download destinations/completion policy, app
custom URL protocols, and programmatic developer-tool state return explicit
unsupported errors. Inline and
parent-accessible same-origin content supports JavaScript/result, injected
CSS/JavaScript, URL/title/history, printing, readable cookies, and Kael's typed
DOM bridges. A cross-origin iframe can receive an exact-origin host
`postMessage`, but it cannot use Kael's nonce-authenticated frame-to-host bridge
unless a future cooperative handshake protocol is added.

Use `WebViewCapabilityReport::current()` to gate individual operations. It
reports composition, focus, navigation/policy, load state, history, reload/stop,
zoom, find, print, downloads, IPC, custom protocols, developer tools, cookies,
headers, profiles, user agent, JavaScript, permission policy, and drag/drop
separately instead of reducing the entire iframe boundary to one boolean.

## Accessibility and printing

Each retained frame exports Kael's `AccessibilityTree` into a transparent DOM
semantic layer above the canvas. Stable accessibility ids retain DOM identity;
unchanged nodes are diffed in place, hidden subtrees are pruned, and the mirror
is capped at 4,096 nodes as a safety boundary. Roles, labels, descriptions,
headings, states, text/range/toggle values, bounds, focusability, and advertised
actions map to ARIA. Tables and grids additionally carry row/column counts,
one-based logical indices, spans, header roles, and sort direction. A virtualized
million-row grid therefore announces its complete logical dimensions while
retaining only the mounted semantic rows and cells. The layer never receives
pointer input, so the WebGL canvas
remains the visual and mouse/touch owner. Screen-reader clicks, DOM focus, and
the standard activation/range/expand/scroll keys are normalized back through
Kael's existing accessibility action router instead of synthesizing duplicate
canvas pointer or text events.

Virtualized controls should expose their mounted semantic viewport rather than
their complete logical data set. The browser mirror's hard cap protects a
misbehaving custom control; a capped tree reports
`data-kael-accessibility-truncated="true"` for diagnostics. Browser and screen
reader combinations still differ in how they announce transparent retained
content. The DOM mirror does not become a second text editor: actual text and
IME input continues through Kael's hidden input bridge, and generic
`SetValue` payload entry is not synthesized by the semantic layer.

`PrintJob` uses an isolated printable iframe snapshot in browser builds. The
shared fill, stroke, line, text, wrapped-text, image, page-size, margin, and
multi-page commands are converted to bounded print HTML, including embedded
PNG snapshots for retained images. `PrintRequest::webview` continues to print
the hosted iframe document itself. Browsers do not permit silent printer
dispatch, printer selection, or application control over the user's print
settings, so both `print` and `show_print_dialog` present the browser print
dialog. Browser font availability and print-engine pagination can still produce
small differences from native Core Graphics output; use explicit page commands
and bundled fonts for release-critical document layout. Kael releases the
snapshot when the browser emits `afterprint`; engines that omit that event keep
at most the latest snapshot until the next print or the window is dropped.

## Storage and document bytes

`kael_storage::PlatformKvStore` keeps the shared typed settings API: desktop
builds use SQLite and browser builds use one atomic, origin-scoped
`localStorage` entry. Browser settings are bounded to 4 MiB and surface denied
access or quota failures instead of silently becoming memory-only. Large and
binary values use the asynchronous `BlobStore`, backed by SQLite BLOB records
on desktop and raw `Uint8Array` records in IndexedDB on the web. A completed
write means the backing transaction committed. The maintained WebAssembly test
reopens both stores and verifies settings and arbitrary binary bytes survived.

The native SQL `Database` surface is not silently emulated. Browser calls
return `BrowserSqlUnsupported`, and native path resolution returns
`BrowserPathUnsupported`; applications should use the shared key/value and blob
stores or explicitly integrate a domain-specific web database.

`kael_document` runs the application's same `Document::read`/`write` model,
undo/redo history, dirty tracking, listeners, byte import, and byte export on
desktop and web. It does not claim to implement DOCX, XLSX, or PPTX itself—the
application's format adapter owns those bytes. Browser applications create a
controller with `new_persistent`, save stable named documents to IndexedDB, list
and reopen them, retain bounded version metadata and blobs, and recover dirty
content only when its saved-baseline digest still matches. Recovery writes are
asynchronous; `flush_autosave` is the explicit durability boundary for unload
or other lifecycle transitions. Arbitrary browser paths remain unavailable, so
the app shell passes bytes from a picker/drop and sends `DocumentExport` bytes
to a user-activated download or share operation.

## Capability boundary

Arbitrary native filesystem paths, subprocesses, secure credential storage,
detached operating-system windows, custom URL schemes, and silent printer
dispatch are unavailable in the 0.4 browser backend. In-page multi-window
retained surfaces and hosted iframe WebViews are reported as partial rather
than being silently treated as native equivalents.
APIs with no safe browser equivalent return explicit unsupported errors; inspect
`CapabilityReport::current()` before exposing platform-dependent workflows.

With `screen-capture` enabled, `CaptureManager` and the retained
`App::screen_capture_sources` path both use `getDisplayMedia`. The call that
starts capture must originate from a trusted user gesture and the browser owns
the screen/window/tab picker, so sources cannot be enumerated in advance. Kael
delivers bounded RGBA8 frames and cleans up tracks on stop, source end, error,
or owner drop. Readback pressure is capped so 4K can run at 60 Hz and 1080p at
high refresh rates, while larger surfaces are sampled at a lower cadence rather
than copying gigabytes per second on the browser main thread. Capture audio is deliberately separate: use the asynchronous
`kael_audio` microphone API rather than asking a video session to change its
media contract.

## Framework smoke application

### Supported browser baseline

Kael 0.4's release-tested minimum is Chromium 151, Firefox 153, and WebKit
26.5. That maps to Chrome/Edge 151+, Firefox 153+, and Safari 26.5+ as the
product acceptance baseline. The automated WebKit build exercises the Safari
rendering engine, not every branded Safari or operating-system integration, so
applications should still acceptance-test downloads, permissions, and share UI
on the Apple devices they ship.

Every supported engine must provide WebAssembly, JavaScript modules, WebGL2,
`requestAnimationFrame`, pointer/composition/input events, `ResizeObserver`,
`MutationObserver`, Blob/File/FileReader, sandboxed iframe messaging, History
API state, and canvas Blob export. The release gate fails when any required
primitive is missing; Kael does not silently switch the retained renderer to a
DOM implementation.

Optional browser-owned services remain capability-gated. Async Clipboard, File
System Access pickers, Web Share, page-created Notifications, pointer lock, and
controller discovery differ by
engine, browser policy, secure-context state, and user permission. Kael's
portable byte picker/download paths do not require `showOpenFilePicker`, and
the share/notification APIs expose typed unavailable, denied, activation, and
unsupported outcomes. `target/browser-matrix/report.json` records the observed
optional capability booleans for each pinned engine without promoting them to
the required baseline.

Enable `game-input` for the portable `Window` controller snapshot and pointer
lock API. Controller polling is coalesced onto display frames, and pointer lock
must be requested synchronously from a trusted activation. See
[Game Input](game-input.md) for bounds, mapping, lifecycle, and the release
smoke.

### Three-engine release proof

Install the pinned runner and its browser revisions once, then execute the
matrix:

```sh
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --global binaryen@132.0.0
python3 -m venv target/browser-matrix-venv
target/browser-matrix-venv/bin/python -m pip install \
  -r scripts/browser-matrix-requirements.txt
target/browser-matrix-venv/bin/python -m playwright install \
  chromium firefox webkit
KAEL_PLAYWRIGHT_PYTHON=target/browser-matrix-venv/bin/python \
  bash scripts/verify-browser-matrix.sh
```

Linux CI sets `KAEL_BROWSER_MATRIX_SOFTWARE=1` because its browser runners do
not expose a physical GPU. On a GPU-equipped workstation, omit that variable to
run the strict hardware timing class. `KAEL_BROWSER_MATRIX_ARTIFACT_DIR` can keep
the two reports in separate evidence directories.

The verifier packages and serves one set of static artifacts, then runs it
serially in Chromium, Firefox, and WebKit. The framework smoke runs at 1280×800
and 430×720; the suite-scale app runs at 1280×800 and its under-900-pixel
compact breakpoint at 760×720. Every retained layout must render non-blank
verified pixels, fill its viewport, and keep page overflow bounded.
The proof also requires exact context-loss restoration, the million-row
viewport mount bound and last-row jump, visible retained scrollbars, pointer,
keyboard and IME routing, accessibility actions, byte-backed file
pick/drop and download, sandboxed iframe policy/IPC, suite-scale two-axis mount
bounds, secondary retained-window lifecycle, hash/popstate routing without a
reload, PNG export, and a freshly restarted local WebSocket protocol probe per
engine. It also runs the display-capture lifecycle fixture once per engine. The
fixture injects a canvas-backed `MediaStream` through the same asynchronous
picker boundary and proves privacy-safe enumeration, `Starting` to `Running`,
an exact bounded 64x32 RGBA32 frame (8192 bytes with opaque alpha),
pause/resume/stop, audio and oversized-config rejection, and asynchronous picker
denial transitioning to `Error` with a typed `last_error`. Its source and frame
dimensions are fixed, so repeating it at the wide and compact viewports would
not exercise a different layout path.

That fixture does not claim to automate a trusted activation, the browser/OS
picker UI, or permission persistence. Those remain branded-browser acceptance
tests. If an engine build lacks canvas `captureStream` or `MediaStream`, the
matrix fails explicitly and records the missing fixture primitive; it never
turns that automation limitation into a successful real-picker result.
File/drop automation is reported as unavailable only when the engine
does not expose a constructible test `DataTransfer`; the user-facing picker and
download paths remain mandatory. Clipboard payload routing is exercised when
the engine allows script-constructed `ClipboardEvent` payloads. Firefox strips
that privileged synthetic payload, so the report records
`automation-unavailable`; trusted user paste/copy remains implemented and
should be acceptance-tested against the operating-system clipboard.

Screenshots, downloaded proof files, console/page/request failures, browser
versions, semantic marker snapshots, capability differences, and the combined
JSON report are written under `target/browser-matrix`. The local WebSocket echo
server is restarted between engines so reconnect state cannot leak across
tests. CI caches Playwright's browser directory using the hash of the pinned
requirements file and still runs the engines serially for deterministic GPU and
workload evidence.
On Linux the verifier starts a private Xvfb display and opts Firefox into Mesa
software WebGL2; Firefox's native headless mode does not expose a usable WebGL2
context on GPU-less release runners. This is a renderer proof, not a
hardware-GPU benchmark.

Repository contributors can package the maintained `kael_ui` browser smoke app
directly:

```sh
bash scripts/build-web.sh \
  --package kael_ui \
  --example browser_smoke \
  --features browser \
  --out-dir target/browser-smoke \
  --out-name browser_smoke \
  --html crates/kael_ui/examples/browser_smoke/index.html
```

Release builds require Binaryen 132 (`npm install --global
binaryen@132.0.0`). Both `kael web build` and `scripts/build-web.sh` run
`wasm-opt -O3` after `wasm-bindgen`; debug builds intentionally skip that pass.
Pinning the optimizer keeps release output reproducible, and the browser matrix
exercises the optimized bytes rather than an easier unoptimized artifact.
