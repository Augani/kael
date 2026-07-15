# Kael crate audit ledger

This ledger tracks the workspace-wide quality audit after the Astryx showcase
component pass. A crate is only marked complete after its public API and unsafe
boundaries are reviewed, invalid inputs and platform fallbacks are checked, and
its focused tests and lint target pass.

Status key: `pending`, `in progress`, `complete`.

Current aggregate gate (2026-07-15): `cargo test --workspace --no-fail-fast` passes across all
36 packages, including core worker/extension process tests, native crash integration tests,
templates, macros/compile-fail fixtures, and doctests. Explicit real-device/keychain and slow
nested-build tests remain ignored by design and are documented in their owning crates.
`cargo clippy --workspace --all-targets -- -D warnings` also passes, including every showcase and
standalone example target; workspace Rustfmt and scoped diff checks are clean.

| # | Package | Path | Status | Verification / notes |
|---:|---|---|---|---|
| 1 | `kael_ui` | `crates/kael_ui` | complete | 330 unit and 15 interaction tests pass under runtime shaders. The 226 top-level UI modules are reconciled in `ui-component-coverage.md`; entitlement-signed native inspection now covers every isolated showcase category and open state, including corrected charts, data display, time picker, layout, feedback, navigation, overlays, typography, media, and drag/drop surfaces. |
| 2 | `kael_semantic_version` | `crates/semantic_version` | complete | Strict core-triplet parsing now rejects extra components, metadata, leading zeroes, non-ASCII digits, and overflow; 3 tests and strict Clippy passed. |
| 3 | `kael_collections` | `crates/collections` | complete | Fx-hasher trust boundary documented; alias behavior regression-tested; 1 test and strict Clippy passed. |
| 4 | `kael_sum_tree` | `crates/sum_tree` | complete | Defensive ordered-map construction now sorts malformed input and resolves duplicate keys without corrupting search invariants; 11 tests and strict Clippy passed. |
| 5 | `kael_refineable` | `crates/refineable` | complete | Cascade slots now carry cascade identity and return a typed error instead of panicking or mutating another cascade; 1 test and strict Clippy passed. |
| 6 | `kael_derive_refineable` | `crates/derive_refineable` | complete | Macro errors are compile diagnostics instead of panics; nested paths, empty structs, dependency renames, generic bounds, attributes, and `is_some` docs were corrected; 3 macro and 3 downstream tests, strict Clippy, and a `kael` runtime check passed. |
| 7 | `kael_util` | `crates/util` | complete | ZIP traversal/symlink escapes and expanded-size limits, relative-path deserialization, shell input handling, passwd/session errors, zero limits, and overflow edges were fixed; 52 unit tests, 3 doc tests, and strict all-feature/all-target Clippy passed. |
| 8 | `kael_util_macros` | `crates/util_macros` | complete | Literal macros now expand for the consumer target rather than the proc-macro host, preserve spans, and avoid duplicate URI drives/CRLFs; 3 tests in both feature modes and strict all-feature Clippy passed. |
| 9 | `kael-macros` | `crates/kael-macros` | complete | Generated paths support renamed `kael` dependencies; action/context/test inputs now return precise diagnostics instead of panicking; 3 parser tests, 5 compile-fail cases, integration/doc tests, and strict all-feature/all-target Clippy passed with runtime shaders. |
| 10 | `kael_perf` | `crates/perf` | complete | Sub-ms throughput, comparison direction, deterministic report ordering, malformed saved results, and metadata protocol validation were corrected; 3 library + 2 binary parser tests and strict all-target Clippy passed. |
| 11 | `kael_gpu_budget` | `crates/kael_gpu_budget` | complete | Zero/over-budget readings and Linux device/heap selection were corrected; 4 macOS tests, strict Clippy, and explicit Linux + Windows cross-target checks passed. |
| 12 | `kael_render_graph` | `crates/kael_render_graph` | complete | Unproduced transient reads, order-sensitive cache topology, PTS collisions, imported alias candidates, cross-kind slot reuse, image allocation/import validation, and unbounded filter parameters were corrected; 109 tests and strict all-target Clippy passed. |
| 13 | `kael_media_sys` | `crates/media` | complete | Nullable CoreMedia/CoreVideo outputs, signed sample indices, H.264 zero-length pointers, non-contiguous block buffers, allocation failures, and SDK discovery diagnostics were corrected; 1 macOS test, strict Clippy, a runtime-shader downstream `kael` check, and Linux cross-target check passed. |
| 14 | `kael-media` | `crates/kael-media` | complete | Pause/resume position rebasing, saturating duration math, FFmpeg error handling, bounded audio/video allocation, frame stride validation, non-seekable restart, non-finite controls, and staged-file races/trust were corrected; 14 tests, strict all-target Clippy, and downstream `kael`/`kael_audio` checks passed. |
| 15 | `kael_media_engines` | `crates/kael_media_engines` | complete | Full-domain playback/timecode, transactional timeline edits, project/subtitle/automation ordering and validation, finite/bounded audio/scopes/generators/effects, compositor preview/export opacity parity, cache counter rollover, and checked PPM/WAV handling were corrected; 244 tests, doc tests, and strict all-target Clippy passed. |
| 16 | `kael_audio` | `crates/kael_audio` | complete | Non-finite DSP/mixer containment, bounded resampling/offline rendering, saturating device clock and IDs, malformed looping sources, playback-rate/seek state, playlist selection, session listeners, and spatial vectors were corrected; 47 tests passed (1 explicit real-device smoke ignored), strict all-target Clippy and downstream `kael` runtime-shader check passed. Cross-target checks reached native ALSA/FFmpeg sysroot discovery and were blocked by unavailable target pkg-config sysroots. |
| 17 | `kael_engines` | `crates/kael_engines` | complete | Unicode bidi properties, deterministic bounded IDE search, non-finite chart statistics, quoted CSV parsing, query lifecycle/ID rollover, finite canvas geometry/viewports, transactional tile-cache accounting, panic-hook containment, and CRLF wrapping were corrected; 93 tests, doc tests, and strict all-target Clippy passed. |
| 18 | `kael_http_client` | `crates/http_client` | complete | Request/response and GitHub API buffering are bounded; proxy fallback, URL construction/encoding, all non-success statuses, SHA-256 validation, staged downloads, and compressed/expanded archive limits were corrected; 7 tests, strict all-feature/all-target Clippy, and a downstream `kael` runtime-shader check passed. |
| 19 | `kael_net` | `crates/kael_net` | complete | Request/response debug output now redacts query data, sensitive headers, and bodies; retry math contains non-finite configuration and huge attempts; offline IDs wrap without panic/collision; presence ordering is deterministic and non-finite cursors are discarded; 76 tests, strict all-feature/all-target Clippy, and a downstream `kael` runtime-shader check passed. |
| 20 | `kael_storage` | `crates/kael_storage` | complete | App/database path traversal, zero/ahead-of-code migration versions, JSON memory/disk divergence after failed persistence, temporary-file collisions/cleanup, observer ID rollover, relative paths, and unbounded JSON store loads were corrected; 21 tests, strict all-feature/all-target Clippy, and a downstream `kael` runtime-shader check passed. Windows/Linux cross-checks reached bundled SQLite compilation but were blocked by unavailable target C toolchains/sysroots. |
| 21 | `kael_cache` | `crates/kael_cache` | complete | Disk writes are staged and serialized with stale-temp cleanup; byte/index/counter rollover is contained; manager writes no longer poison memory after disk failure; namespace/key memory collisions and overbroad namespace invalidation were corrected; 39 tests, strict all-feature/all-target Clippy, and a downstream `kael` runtime-shader check passed. |
| 22 | `kael_secrets` | `crates/kael_secrets` | complete | Empty/NUL/oversized identifiers and secrets, Windows target-name collisions/blob truncation/null pointers, credential allocation ownership, Linux locked-item reads/deletes, and in-memory secret zeroization were corrected; 6 native tests passed (1 explicit real-keychain smoke ignored), strict native/Windows/Linux all-target Clippy, and a downstream `kael` runtime-shader check passed. |
| 23 | `kael_i18n` | `crates/kael_i18n` | complete | Non-finite/huge number formatting, bounded precision/catalog parsing, validated locale dates, language-aware RTL detection, deterministic catalog keys, locale negotiation/fallback mutation, named arguments, and Arabic/Slavic/European/Asian plural/format presets were added or corrected; 42 tests, strict all-feature/all-target Clippy, and a downstream `kael` runtime-shader check passed. |
| 24 | `kael_icons` | `crates/kael_icons` | complete | Bundled SVGs now inherit `currentColor`; unimplemented native bridges no longer report availability; icon weights expose renderable stroke metrics; generated catalogs validate assets/slugs/collisions and provide deterministic `ALL`, `Display`, and `FromStr` APIs; 3 tests, strict all-feature/all-target Clippy, Windows/Linux cross-target checks, and a downstream `kael` runtime-shader check passed. |
| 25 | `kael_document` | `crates/kael_document` | complete | Document/autosave/version payloads and metadata are bounded; edits, undo/redo, restores, saves, and reverts preserve state across persistence failures; version paths use trusted metadata; atomic writes preserve permissions and reject directories; listener panics/ID rollover and corrupt recent metadata are contained; platform support metadata is truthful. 20 tests, strict all-feature/all-target Clippy, and a downstream `kael` runtime-shader check passed. Windows cross-check reached bundled SQLite compilation but was blocked by the unavailable target C toolchain/sysroot. |
| 26 | `kael_pdf` | `crates/kael_pdf` | complete | PDF/object/page/text/sidecar/render-cache inputs are bounded; MediaBox, parent/reference cycles, non-finite scales, RGBA invariants, annotation geometry/counts/IDs, UTF-8 search offsets, and zero-capacity caches were corrected. Saves and sidecars are staged/durable, preserve permissions, and sidecars are bound to the exact PDF SHA-256. URI links and bounded nested outlines are now extracted, metadata text decoding is PDF-aware, and platform preview claims are truthful. 15 tests, strict all-feature/all-target Clippy, Windows/Linux cross-target checks, and a downstream `kael` PDF/runtime-shader check passed. |
| 27 | `kael_notifications` | `crates/kael_notifications` | complete | Notification/category/attachment/trigger inputs are validated and bounded; durations and `Instant` math cannot panic; notification IDs fail closed at exhaustion and listener IDs wrap without replacement; cancellations purge queued entries and retain payloads; delivery failures clean scheduled state; listener panics are isolated; scheduler threads receive explicit shutdown; badge failures surface; platform capability metadata no longer advertises planned actions/push. 8 tests, strict all-feature/all-target Clippy, Windows/Linux cross-target checks, and a downstream `kael` notifications/runtime-shader check passed. |
| 28 | `kael_share` | `crates/kael_share` | complete | Share items/text/URLs/images/files/receiver types are validated and bounded; directories and malformed URLs/MIME/name inputs are rejected; materialized images use unique durable files with failure cleanup; stale-temp cleanup will not follow symlinks. Linux handoffs report process exit status, clipboard writes wait for completion, support detection is PATH-aware, blocking commands leave the async caller, and exclusions no longer fall through to an unmodeled open action. Windows clipboard ownership/closure is RAII-safe and rejected images no longer materialize; macOS moved off deprecated Cocoa/objc bindings and obsolete social claims. 18 tests, strict all-feature/all-target Clippy, Windows/Linux cross-target checks, and a downstream `kael` share/runtime-shader check passed. |
| 29 | `kael_diagnostics` | `crates/kael_diagnostics` | complete | Breadcrumb, trace-event, metric-cardinality, histogram, crash-report, native-meta, and dump inputs are bounded; zero-capacity retention, counter/time/identifier overflow, non-finite samples, sampling-rate semantics, path-like app IDs, callback panics, repeated panic-hook installs, failed native installs, mismatched metadata, symlinked pending reports, non-success submissions, and non-durable direct writes were corrected. Platform labels now describe the installed signal/exception mechanism, and the missing real-crash helper was restored. 23 unit tests and 2 real SIGSEGV/SIGABRT integration tests, strict all-feature/all-target Clippy, and a downstream `kael` diagnostics/runtime-shader check passed. Windows/Linux cross-checks reached `ring` but were blocked by unavailable target C toolchains/sysroots. |
| 30 | `kael_release` | `crates/kael_release` | complete | Update signatures now use an unambiguous length-prefixed payload covering every manifest field and channel variant; verification rejects invalid manifests. Strict versions, credential-free HTTPS URLs, artifact/release-note/channel bounds, collision-resistant staging names, normalized icon metadata, bundle identifiers, Cargo features, and compound SPDX expressions were corrected. Swap plans reject aliases, preserve preexisting backups, verify restore sources, and distinguish post-install cleanup failures; code-signature verification fails closed off macOS and bounds subprocess output. 76 tests, strict all-feature/all-target Clippy, Windows/Linux cross-target checks, and a downstream `kael` runtime-shader check passed. |
| 31 | `kael` | `crates/kael` | complete | Core pass: crash reports, tracing, IPC frames/transports, sessions, helper-process supervision, auto-update, and request/entity/accessibility/render/task identifier exhaustion are bounded or fail closed; callback panics and poisoned worker transports no longer escape their trust boundaries. macOS native strings/pasteboard data are null-checked and capped, mixed text/image clipboard entries publish RTFD without leaking temporary objects, and window/input/frame/tab plus app-delegate tray/menu/launch/quit/notification/power/hotkey/network/media callbacks are contained before crossing Objective-C, dispatch, or block boundaries. Metal device, color-space, shader-library, pipeline, CoreVideo-cache, and window/view setup failures now propagate through framework-owned window creation; headless rendering falls back to CPU instead of terminating the process, while the legacy infallible constructor remains compatible. Windows clipboard/display/window creation and vsync/close-message paths fail safely; both Win32 procedures contain unwinds at the ABI boundary, platform/drag-drop callbacks are isolated, and DirectX object outputs, structured-buffer sizes, shader paths, view availability, and DirectWrite fallback no longer rely on unchecked outputs or truncating arithmetic. DirectWrite renderer callbacks now reject null pointers, unreasonable counts, malformed cluster maps, and overflowing glyph ranges before forming native slices. DirectX shader blobs/diagnostics and GPU-vendor strings are null-checked and bounded, AMD AGS contexts are released on every exit path, and Win32 creation/enumeration payloads reject null or contain unwinds. DirectX resize restores its backbuffer on failure and device-loss recovery constructs replacement state transactionally before releasing the old renderer. Linux external file-descriptor/clipboard reads are capped, Wayland pipe/flush/writer/global failures surface cleanly, and dispatcher shutdown no longer panics. Linux font loading is time-bounded and tolerates missing faces/families/fallbacks/empty layouts; dispatcher and X11 clipboard/timer thread failures surface without crashing. Wayland protocol-order gaps, invalid repeat metadata, absent keymaps/pointers/drag offers, missing surfaces, and DnD pipe failures now fail safely, while X11 refresh recovery avoids zero-rate and unbounded catch-up loops. Wayland, X11, headless, tray, menu, lifecycle, input, hotkey, network, power, media, and notification callbacks release borrowed/locked state before invocation, contain panics, and restore reusable handlers, preventing reentrant `RefCell` panics and tray mutex deadlocks. Webview IPC, navigation/new-window policy, drag/drop, page/title, and download handlers fail closed across macOS, Windows, and Linux. Permission and biometric completions now validate native objects, reclaim abandoned callback ownership, avoid unbounded subprocess capture, and contain user callbacks on every implemented backend. macOS camera/screen capture validates native samples, dimensions, strides, allocation/device-enumeration bounds, timestamps, object creation, and callback ownership; frame/start/source callbacks are contained and screen-capture delegates no longer leak. Native keyboard/hotkey strings avoid unchecked C-string scans and reject null events. Windows IME buffers and cursor results are bounded and validated, message payload pointers reject null, and clipboard global-memory locks are RAII-safe. Metal, DirectX, and optional Blade atlases validate raster dimensions and exact payload lengths, reject stale texture IDs, reclaim tiles idempotently, contain identifier/counter exhaustion, and evict by real allocated texture-page bytes with an in-flight-frame guard. Metal rendering bounds drawable/offscreen textures, readback/compute/clip-mask memory, mapped pointers, and instance-buffer arithmetic and skips invalid surface resources safely. CoreText rejects stale fonts, malformed runs/attributes/indices, and oversized glyph rasters. Native macOS alerts now translate reordered NSAlert response ordinals back to the original framework answer index instead of returning 1000-based or incorrect selections. 1,942 library tests and strict native runtime-shader Clippy pass in both standard and `macos-blade` configurations. Windows/Linux cross-checks still stop in target-native C/system dependencies because MSVC headers and Linux pkg-config sysroots are unavailable. Remaining target: continue the platform-specific unsafe/native-object audit beyond the completed callback, credential/global-memory, URL/string, window-handle, and DirectWrite boundaries; Windows/Linux-only edits still need native target type-checks when their toolchains are available. |
| 32 | `kael-cli` | `crates/kael-cli` | complete | Project names are bounded and diagnostics content-safe; destination creation is atomic against dangling links/races, files use create-new durable writes, partial scaffolds are cleaned, extra CLI arguments fail, and generated apps use fallible application/window startup instead of `unwrap`. 5 tests and strict all-target Clippy passed. |
| 33 | `xtask` | `xtask` | complete | Distribution configs and update feeds use bounded UTF-8 regular-file reads; config schemas reject unknown fields and unsafe identifiers, names, versions, URLs, and channels. Feed output is staged, synced, and atomically replaced. Scaffolding reserves targets/files without following symlink targets or overwriting content, cleans only its own partial work, and is regression-tested under concurrent creation. Tool discovery no longer captures unbounded subprocess output. 37 default tests, the ignored real generated-app compile, and strict all-target Clippy passed. |
| 34 | `messaging-app` | `templates/messaging` | complete | Startup and window creation are fallible, asset reads/listing are path-confined and bounded, icon controls and text fields have useful accessible labels, conversation rows expose button semantics plus keyboard activation and focus rings, and Send appends the draft then clears the composer. The README now matches the UI and documents runtime shaders. 1 test and strict all-target runtime-shader Clippy passed; an ad-hoc entitlement-signed `.app` launch was visually inspected through macOS accessibility, and conversation selection plus message submission were exercised successfully. |
| 35 | `workspace-app` | `templates/workspace` | complete | Fallible startup and bounded, path-confined assets match the messaging template. File-tree selection now loads real in-memory sample content and language metadata, the active tab/status update with selection, Undo/Redo are wired, and unavailable demo actions are visibly and semantically disabled. Embedded sample code no longer teaches panic-on-window-open. 2 tests and strict runtime-shader/editor-language Clippy passed; the entitlement-signed `.app` was visually inspected and keyboard file selection was exercised successfully. |
| 36 | `dashboard-app` | `templates/dashboard` | complete | Startup/window creation is fallible; assets are path-confined and bounded; order search is normalized and tested; sidebar sections, notifications, and View all are wired; section content is honest; icon/search controls are labeled; dashboard rows wrap responsively above a practical minimum window size. 2 tests and strict runtime-shader Clippy passed. The entitlement-signed `.app` was visually and semantically inspected: sparklines, the 12-point revenue line/area chart, regional bar chart, and data table all rendered inside their cards with detailed accessible summaries. |

Row 31 continuation note: Wayland protocol callbacks and window state access now treat an
expired weak client as normal teardown, returning safe defaults or dropping late events instead
of panicking after the owning client has gone away. Untrusted keymaps are capped at 16 MiB, and
repeat attempts to start the Wayland or headless event loop are ignored safely.

Row 31 native-lifecycle note: poisoned cross-platform tab-manager state now recovers without
disabling later tab operations. Windows native prompts bound their button surface, use unique
stable IDs, and resolve dialog failures safely; window-class/module and performance-counter
initialization now propagate or fall back instead of terminating. macOS display-link start/stop
is idempotent and balances dispatch-source suspension during normal and failed teardown.
The current native gate is 1,949 passing library tests plus strict standard and `macos-blade`
all-target runtime-shader Clippy; the older aggregate count in row 31 is superseded by this note.

Row 31 AppKit note: dock attention/badge and modal-dialog calls now fail safely when invoked off
the main thread, and unexpected modal response ordinals are bounded to a valid answer instead of
underflowing into an arbitrary `usize`.

Row 31 startup note: applications and background executors now have fallible constructors;
Linux headless event-loop/source registration and Windows platform creation propagate errors, and
off-main-thread application construction is rejected before reaching the legacy assertion.

Row 31 single-instance note: Unix ownership now uses a race-free advisory lock in a private,
owner-checked runtime directory; lock/socket errors remain distinguishable from a true duplicate,
public IDs are validated before path construction, activation reads are timed and exact, duplicate
listeners are rejected, and callback panics no longer kill future activation delivery.

Row 31 supervision note: health-monitor creation is fallible, monitor waits and restart backoffs
are interruptible, stop/drop cannot race a child back into existence, owner teardown terminates and
reaps children, failed status queries clean up conservatively, and poisoned process/callback state
is recovered without permanently disabling supervision.

Row 31 worker-runtime note: progress streams now surface terminal worker errors, report reader
thread creation failures, and send cancellation when consumers disappear or progress payloads are
invalid. Bootstrap messages validate protocol fields and capability labels without logging payloads;
Unix worker startup uses a bounded accept/read window and a mode-0600 socket, and every failed
bootstrap stops the supervised child and removes the socket. Worker-side poisoned transport locks
return errors instead of panicking. The current gate is 1,951 passing library tests, 4 passing real
worker-process integration tests, and strict standard plus `macos-blade` all-target runtime-shader
Clippy; the older aggregate counts above are superseded by this note.

Row 31 extension-runtime note: external extension activation now uses random mode-0600 Unix
endpoints, bounded accept/handshake waits, correct installed/dev working-directory resolution, and
transactional child/socket cleanup. RPC requests use exhaustion-safe unique IDs, enforce response
correlation, validate identifiers/errors/handshakes/contributions, and keep unexpected-message
diagnostics content-safe. Checked runtime construction validates app IDs and directory creation.
Plugin manifests are regular-file-only and capped at 1 MiB; manifest IDs cannot escape the install
root, API compatibility is checked before copying, and installs are staged, symlink-rejecting,
depth/count/byte bounded, failure-cleaning, and deterministically listed. Capability/argument and
contribution surfaces are bounded and duplicate capabilities are rejected. Deactivation surfaces
shutdown/stop failures without falsely marking a live process inactive. The current gate is 1,959
passing library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; the
older aggregate counts above are superseded by this note.

Row 31 IPC follow-up note: typed worker and extension transports now require exactly one complete
inner frame and reject trailing-byte smuggling. Native stream transports frame without duplicating
the full payload allocation and accept the complete documented inner-frame envelope; close is
idempotent and disables subsequent I/O, including in-memory transports used by protocol tests.
Public IPC endpoint construction has a fallible validated API, while the legacy path constructor
maps invalid path-like identifiers to a deterministic safe endpoint. Unix listener creation no
longer deletes a caller-supplied preexisting path. The current gate is 1,962 passing library tests
plus strict standard and `macos-blade` all-target runtime-shader Clippy; the older aggregate counts
above are superseded by this note.

Row 31 custom-protocol note: request URLs and path depth, route/header counts, response bodies, and
file-backed assets are bounded; file reads are metadata-checked and limited instead of using an
unbounded allocation. Existing-file resolution returns the canonical in-root target, rejects
out-of-root symlinks, and uses no-follow final opens on Unix. MIME types require valid type/subtype
tokens. Handler panics are contained after restoring the route, including the existing reentrant
replacement semantics, so one faulty request cannot silently unregister future handling. The
current gate is 1,963 passing library tests plus strict standard and `macos-blade` all-target
runtime-shader Clippy; the older aggregate counts above are superseded by this note.

Row 31 open-dispatch note: platform open-request batches and individual request strings, plus
deep-link route counts, are bounded. Schemes are normalized case-insensitively and colon-form URLs
such as `myapp:item` route correctly. Panics are isolated independently at every observer tier and
in registry handlers; callback collections are restored after dispatch while preserving reentrant
registrations, and checked deep-link dispatch reports handler failure. The current gate is 1,965
passing library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; the
older aggregate counts above are superseded by this note.

Row 31 updater note: update/feed URLs are bounded and feed decoding now rejects malformed UTF-8
instead of parsing lossy text. Download-start and staging failures move the updater to an error
state and clear stale install eligibility; failed staging creation is cleaned up. Progress
fractions remain finite and in range for zero or inconsistent totals. Install-time package opening
uses no-follow semantics on Unix in addition to regular-file and streaming size/hash checks. The
current gate is 1,967 passing library tests plus strict standard and `macos-blade` all-target
runtime-shader Clippy; the older aggregate counts above are superseded by this note.

Row 31 session-store note: session reads are bounded on the open file handle, reject malformed
UTF-8, and use no-follow opens on Unix, closing metadata/open races and growing-file allocation
bypasses. Dot-path application IDs cannot collapse or escape the app-specific directory. Dangling
snapshot links are treated as invalid persisted state and can be cleared. Atomic temp files use
private permissions on Unix and are removed after write or sync failure. The current gate is 1,970
passing library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; the
older aggregate counts above are superseded by this note.

Row 31 crash-reporter note: the complete reporter panic hook is unwind-contained and captured
message/backtrace fields are truncated at UTF-8 boundaries so pathological payloads do not defeat
the report-size gate. Report directories/files are private on Unix, failed temp writes are cleaned,
and upload reads are size-bounded on the open no-follow handle. Submission endpoints require HTTPS
without credentials or fragments, and pending uploads are capped at 256 per batch to bound crash-
loop startup work. The current gate is 1,974 passing library tests plus strict standard and
`macos-blade` all-target runtime-shader Clippy; the older aggregate counts above are superseded by
this note.

Row 31 file-watcher note: watcher delivery now uses a bounded 1,024-event queue and contains each
callback panic so later events continue to arrive. Registrations and grouped watch sets are capped
at 1,024, duplicate roots are rejected, and failed native registration rolls back framework state.
Translated notify path batches are capped at 256, while unwatching no longer requires a deleted
watched path to exist for canonicalization. The current gate is 1,976 passing library tests plus
strict standard and `macos-blade` all-target runtime-shader Clippy; the older aggregate counts above
are superseded by this note.

Row 31 security-token note: permission prompt and decision callbacks are panic-contained and fail
closed, while prompt reasons are bounded and validated. In-memory credential entries validate
identifier/secret sizes, cap store growth, redact debug output, reject oversized lookup keys, and
zero secret buffers on drop. File access tokens now use random opaque UUID identifiers instead of
predictable counters/path hashes, enforce absolute bounded paths, reject zero or overflowing TTLs,
purge expired entries before a capped issue, and redact paths/tokens from debug output. A compatible
`ThreatModel::strict()` provides explicit high-risk-default denial without changing established app
defaults. The current gate is 1,980 passing library tests plus strict standard and `macos-blade`
all-target runtime-shader Clippy; the older aggregate counts above are superseded by this note.

Row 31 security-policy note: plugin permission manifests validate bounded identifiers, reject
duplicate/overlapping declarations, fail closed when malformed, and return permissions in stable
declaration order. Process limits reject zero, non-finite, negative, and out-of-range values; PID
and name validation plus the missing open-file check are available through checked construction.
CPU NaN/negative measurements fail closed, and violation histories/text are bounded, UTF-8 safe,
and control-character sanitized. Network policies cap host lists, compare DNS names case-
insensitively, validate DNS labels and IPv4/IPv6 literals, and make malformed deny policies fail
closed. HTTP/realtime URLs, request headers/names, and declared body sizes are bounded. The current
gate is 1,984 passing library tests plus strict standard and `macos-blade` all-target runtime-shader
Clippy; the older aggregate counts above are superseded by this note.

Row 31 realtime/schema note: reconnect policies now require canonical disabled state and expose a
validated capped exponential delay schedule. Realtime connection groups are capped at 64 and detect
duplicates by bounded structural comparison instead of allocating fingerprints containing URLs and
header values. IPC schemas validate nonzero compatible version ranges, bounded unique message-type
identifiers, and fail closed for compatibility, negotiation, and message intersection when either
schema is malformed. The current gate is 1,985 passing library tests plus strict standard and
`macos-blade` all-target runtime-shader Clippy; the older aggregate counts above are superseded by
this note.

Row 31 background-job note: jobs queued behind dependencies or concurrency limits now retain their
bounded serialized request and typed result decoder, so resume, dependency completion, and explicit
retry can execute real work instead of leaving permanently inert status records. Retry attempts and
backoff are bounded, terminal jobs expose explicit cleanup, dependency cycles and oversized graphs
are rejected, and progress payloads use the same finite/text validation as checked handoffs.
Concurrency slots use atomic compare-and-exchange plus an unwind-safe release guard; cancellation
cannot be overwritten by a late worker response, poisoned bookkeeping locks recover, and scheduler
state is capped. The current gate is 1,990 passing library tests plus strict standard and
`macos-blade` all-target runtime-shader Clippy; the older aggregate counts above are superseded by
this note.

Row 31 benchmark-evidence note: collector sample retention, harness history, imported JSON, result
measurements, sample interactions, text fields, and platform sysctl strings are bounded. Cache
counters rescale before overflow, CPU sampling uses real wall intervals, native CPU-time conversion
rejects invalid timevals, empty smoothness minima return zero, and invalid smoothness metrics have a
fallible constructor. Workload panics still propagate but all collectors are stopped first.
Measurements now require finite values and metric-compatible units; malformed required metrics are
reported as evidence issues, scenario/unit mismatches are not compared, zero baselines no longer
hide changes, and invalid thresholds fail closed. The current gate is 1,995 passing library tests
plus strict standard and `macos-blade` all-target runtime-shader Clippy; the older aggregate counts
above are superseded by this note.

Row 31 command-palette note: direct palette registration now enforces the same bounded identifier,
label, category, shortcut, and icon validation as checked generated handoffs, rejects duplicates
without echoing identifiers, and caps palette growth at 4,096 descriptors. Palette identifiers have
a fallible constructor. Search queries and category filters are bounded and reject control text;
search covers labels, categories, command ids, and shortcut hints. Search, category, and complete
command lists now use deterministic label/category/id ordering instead of HashMap iteration order,
eliminating palette order flicker. The executable app registry likewise caps growth, validates even
legacy infallible registrations, preserves the first command instead of silently replacing
duplicates, bounds and stabilizes search, redacts command ids from Debug/errors, contains metadata
and handler panics, and returns handler failure instead of crashing the app. The current gate is
1,999 passing library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy;
the older aggregate counts above are superseded by this note.

Row 31 app-runtime state note: settings files are read through bounded no-follow handles, require a
regular file and representable schema version, and validate bounded deterministic migrations while
containing migration callbacks. Saves use unique private staged files with sync, cleanup, and
rollback after callback or persistence failure. Settings paths, serialized values, and migration
counts are bounded. Undo/redo depth, transaction size, and descriptions are validated; apply,
revert, description, and source callbacks are panic-contained without advancing or discarding the
history cursor, and an empty transaction no longer destroys redo history. Runtime deep-link schemes
and handler counts are validated and bounded, duplicate routes preserve the first handler, metadata
and dispatch callbacks are contained, and reopen callbacks have a checked panic-safe path. The
current gate is 2,004 passing library tests plus strict standard and `macos-blade` all-target
runtime-shader Clippy; direct Rustfmt and diff checks pass. The older aggregate counts above are
superseded by this note.

Row 31 computed-state note: computed dependency tracking is capped at 4,096 unique entities, with
checked tracker reads/observations reporting overflow and legacy tracking disabling caching after
overflow instead of allocating an unbounded observer set or returning stale cached values.
Recomputation now detects recursive reads before stack exhaustion, contains user computation
panics, resets its in-progress state after failure, and exposes checked borrowed and cloned reads so
callers can recover without unwinding. The current gate is 2,007 passing library tests plus strict
standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks pass.
The older aggregate counts above are superseded by this note.

Row 31 developer-tools note: inspected element trees now validate unique bounded identifiers,
styles, finite geometry, total node count, and depth before recursive queries can run. Overlay,
frame-timeline, job-snapshot, structured-log, and telemetry retention is capped; checked ingestion
rejects malformed text, non-finite geometry/progress/metrics, duplicate jobs, and impossible job
timestamps. Frame and job averages use overflow-safe accumulation, oversized timeline capacities
are clamped or rejected, disabled telemetry clears retained events, and privacy filtering no longer
preserves path filenames or email-like strings. The current gate is 2,010 passing library tests plus
strict standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks
pass. The older aggregate counts above are superseded by this note.

Row 31 golden-image note: tolerance fractions now require finite values in the inclusive zero-to-one
range and malformed public tolerances fail closed in both comparison and report evaluation. RGBA
dimension arithmetic is checked, zero and oversized images are rejected at a 256 MiB boundary, and
solid-reference generation has a fallible API while its legacy wrapper returns an empty buffer
instead of overflowing or attempting an unbounded allocation. The current gate is 2,011 passing
library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; direct
Rustfmt and diff checks pass. The older aggregate counts above are superseded by this note.

Row 31 GPU-runtime note: tracked GPU resources are capped, checked registration rejects byte-count
overflow, and legacy registration fails with the reserved zero identifier instead of corrupting
accounting. Resource identifiers skip live values across wraparound, LRU clocks compact before
overflow, and unknown touches no longer age the cache. Eviction removes all required resources even
when an owner callback panics, with checked APIs surfacing callback failure; memory-pressure
subscribers are likewise isolated so later subscribers still run. Invalid utilization values fail
safe as critical pressure. Headless rendering validates a 256 MiB frame boundary, caps procedural
scene complexity at one million primitives and compute inputs at 64 MiB, and rejects oversized
dimensions before lossy native viewport casts or allocation. The current gate is 2,015 passing
library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; direct
Rustfmt and diff checks pass. The older aggregate counts above are superseded by this note.

Row 31 interpolation note: finite checked scalar interpolation now rejects NaN, infinity, and
overflow while legacy animation paths select a finite endpoint instead of leaking invalid geometry
into layout or shaders. Incompatible inset/outset shadows switch discretely at the halfway point,
preserving the source shadow at animation start instead of snapping immediately to the destination.
Interpolated shadow stacks are capped at 1,024 entries with an explicit checked API. The current
gate is 2,018 passing library tests plus strict standard and `macos-blade` all-target
runtime-shader Clippy; direct Rustfmt and diff checks pass. The older aggregate counts above are
superseded by this note.

Row 31 Lottie note: JSON/dotLottie, file, embedded, in-memory, and HTTP sources are capped at 16 MiB;
file reads use a bounded no-follow handle and successful HTTP bodies stream only to the cap. HTTP
status diagnostics and missing embedded-asset errors no longer expose URLs, response bodies, or
asset paths. Parsed animations require finite positive frame rates/timelines, at most 100,000
frames, bounded nonzero dimensions, and a valid bounded RGBA poster buffer. Render sizes and batch
look-ahead are bounded, extreme elapsed-time arithmetic saturates without `Instant` subtraction
panics, and oversized frame metadata is rejected before native casts or allocation. The current
gate is 2,021 passing library tests plus strict standard and `macos-blade` all-target
runtime-shader Clippy; direct Rustfmt and diff checks pass. The older aggregate counts above are
superseded by this note.

Row 31 scene-graph note: rectangles, spatial entries, viewport transforms, transform handles, and
snap inputs now reject non-finite, negative-size, or overflowing geometry. Frame-budget retention
and spatial/scene collections are capped; frame averages avoid `Duration` accumulation overflow,
and render counters saturate. Scene names, bounds, identifiers, node count, and hierarchy depth have
checked insertion paths; identifier wrap skips live nodes. Reparenting rejects ancestor and corrupt
parent/child cycles, accounts for subtree height, and respects locked nodes. Hit testing and subtree
removal are iterative and visited-set guarded, so even externally corrupted child cycles cannot
overflow the stack or loop forever. Movement is finite/transactional and locked nodes cannot move.
The current gate is 2,025 passing library tests plus strict standard and `macos-blade` all-target
runtime-shader Clippy; direct Rustfmt and diff checks pass. The older aggregate counts above are
superseded by this note.

Row 31 scroll-elasticity note: canonical scroll physics now rejects or resets non-finite offsets,
overscroll, ranges, and deltas; negative scroll ranges no longer reach an invalid `clamp` and panic.
Future or stale animation timestamps use saturating elapsed time, invalid snap-back state terminates
instead of animating forever, and overflow-sized boundary motion still resolves to a bounded rubber
band. Zero-delta application now repairs an offset that became invalid after content bounds changed.
The core `Div` integration regressions pass alongside the helper tests. The current gate is 2,028
passing library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy;
direct Rustfmt and diff checks pass. The older aggregate counts above are superseded by this note.

Row 31 split-pane note: non-closable/pinned tabs now refuse close requests, while checked tab
insertion validates bounded labels, nonzero unique IDs, and per-pane capacity. Split operations
validate the existing tree and incoming pane, reject duplicate pane IDs, and enforce 4,096-pane and
256-level bounds without repeatedly cloning the incoming pane during search. Persisted layouts use
validated deserialization and reject duplicate pane/tab IDs, missing active tabs, empty/unary split
nodes, mismatched/non-finite/non-positive ratios, ratios that do not sum to one, and excessive
count/depth before recursive APIs can run. Ratio normalization uses stable widened accumulation and
malformed vectors cannot panic removal. The current gate is 2,031 passing library tests plus strict
standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks pass.
The older aggregate counts above are superseded by this note.

Row 31 status-bar note: status item IDs, text, tooltips, and total retention now have checked bounds;
invalid legacy additions fail closed without replacing valid items. Deserialization validates every
item, rejects duplicate IDs, and rebuilds the skipped lookup index automatically, while stale index
state cannot panic getters. Removal preserves insertion order, making equal-priority ordering stable
instead of changing through `swap_remove`. A dedicated visible-item query excludes hidden entries
in deterministic priority/insertion order, and status-bar Debug output reports only item count rather
than potentially private branch, path, or tooltip content. The current gate is 2,035 passing library
tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and
diff checks pass. The older aggregate counts above are superseded by this note.

Row 31 theme-runtime note: JSON/TOML strings and files are capped at 1 MiB, unknown schema fields are
rejected, and file loading uses bounded regular-file no-follow handles with content-safe errors.
Theme validation covers normalized finite colors, bounded custom color names/counts, font families,
100–900 font weights, positive font sizes/line height, non-negative spacing/radii, and finite bounded
shadow geometry. Independent spacing/radius tokens remain customizable for partial hot reloads.
Theme paths and retained file watchers are bounded. Application change callbacks and every bridge
subscriber are panic-contained so one faulty consumer cannot block later subscribers, while parse or
callback failure leaves the previously installed theme intact. The current gate is 2,038 passing
library tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; direct
Rustfmt and diff checks pass. The older aggregate counts above are superseded by this note.

Row 31 video-color note: normalized YCbCr, transfer-function, LUT, and grading inputs now handle
NaN and infinities deterministically instead of propagating non-finite shader values or producing
invalid float-to-index conversions. Tone maps explicitly resolve infinite and overflow-scale HDR
light, including malformed extended-Reinhard white points. Bradford adaptation rejects invalid
chromaticities and singular/non-finite intermediates with an identity fallback; non-finite color
temperatures use a bounded neutral fallback. 3D LUT dimensions are capped at 128 per axis, cube-size
arithmetic is checked before comparison or allocation, imported samples must be finite, and legacy
identity construction clamps before multiplying. Malformed CDL parameters fail to their neutral
per-channel values. The current gate is 2,042 passing library tests plus strict standard and
`macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks pass. The older
aggregate counts above are superseded by this note.

Row 31 virtual-data note: selection ranges, select-all operations, and collection diffs now expose
transactional checked APIs with 100,000-entry bounds; legacy infallible calls fail closed rather
than iterating or retaining attacker-sized domains. Selection, diff, visible-range, and table
deserialization rejects unknown or structurally invalid state. Tables cap columns, validate unique
bounded identifiers and labels, require finite ordered width bounds, reject non-finite resizes, and
only accept sorting by an existing sortable column; diagnostics no longer echo caller identifiers.
Tree insertion validates structural depth and total node count, while flattening, visible counting,
and path traversal are iterative, capped, and safe for malformed deep state rather than recursively
overflowing the UI thread stack. The current gate is 2,049 passing library tests plus strict
standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks pass.
The older aggregate counts above are superseded by this note.

Row 31 worker-API note: caller-provided request serializers and response/progress deserializers are
now unwind-contained, so custom Serde implementations cannot escape through synchronous framework
APIs or silently kill a progress-reader thread; progress decoding failures retain the existing
structured error and cancellation behavior. Worker pools expose a checked insertion path, cap
retention at 1,024 handles, and reject zero or duplicate process identifiers, while legacy addition
fails closed. The audited typed IPC layer continues to enforce the 16 MiB frame envelope and exact
message framing. The current gate is 2,051 passing library tests plus strict standard and
`macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks pass. The older
aggregate counts above are superseded by this note.

Row 31 public-documentation note: all six ownership/data-flow examples compile against the current
entity, context, observation, and event APIs. The crate-wide doctest gate exposed and corrected the
README counter example's missing `WindowOptionsBuilder` import. The current documentation gate is
73 passing doctests with 4 explicitly ignored platform-interaction examples; no doctests fail.

Row 31 macOS Core Foundation/native-menu note: focused-window accessibility values now balance
non-null error outputs, verify AX/CFString runtime types before wrapping, bound native strings, and
convert process identifiers without signed truncation. Missing rich-text Objective-C classes return
null instead of panicking. OpenType feature construction keeps CFString/CFNumber owners alive while
native dictionaries consume their pointers, checks every nullable create-rule result, balances
temporary arrays/dictionaries, and bounds feature tags, values, fallback names, counts, and system
fallback enumeration. Null network paths fail offline, zero power assertions are ignored, and
non-finite/overflowing idle seconds cannot panic `Duration` construction. Tray icons/text/menu
trees and geometry are bounded and validated; class lookup and malformed native state fail closed.
The shared cross-platform tray validator and summary walker are iterative, cap 1,024 items and 32
levels, reject invalid/duplicate bounded text without echoing identifiers, and therefore protect all
native backends consistently. The current gate is 2,055 passing library tests plus strict standard
and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff checks pass. The older
aggregate counts above are superseded by this note.

Row 31 remaining macOS utility note: the shared Objective-C string bridge now obtains and caps the
declared UTF-8 byte length before forming a slice, eliminating unchecked C-string scans across
window callbacks. NSRange conversion uses checked addition. Native AppKit/CoreGraphics dimensions,
coordinates, display counts, and refresh rates reject non-finite, negative-size, overflowing, or
capacity-inconsistent values before reaching framework geometry; display enumeration cannot set a
vector length beyond its allocation. OS version/locale and appearance names are bounded, and
unknown appearances no longer print native content. The current gate is 2,056 passing library tests
plus strict standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and diff
checks pass. The older aggregate counts above are superseded by this note.

Row 31 macOS launch/IPC completion note: auto-launch entry points validate bounded bundle-style app
identifiers and fail closed for invalid status queries. macOS socket resolution rejects relative or
oversized `TMPDIR` bases in favor of `/tmp`; the shared checked IPC endpoint API now enforces a
conservative 100-byte Unix-domain socket path limit after platform resolution, so otherwise valid
but unbindable long identifiers fail before listener creation. Every macOS platform source file has
now received an audit edit or explicit checkpoint review. The current gate is 2,058 passing library
tests plus strict standard and `macos-blade` all-target runtime-shader Clippy; direct Rustfmt and
diff checks pass. The older aggregate counts above are superseded by this note.

Row 31 Windows remaining-source note: active-window process IDs and returned buffer lengths, OS
hostname/locale lengths, taskbar rectangle subtraction, logical coordinate scale factors, and tray
icon rectangles are validated before slicing or geometry conversion. Auto-launch identifiers are
bounded and executable commands are Unicode-checked and quoted. Network query errors fail offline;
the monitor rejects invalid windows, handles `GetMessageW` errors correctly, unadvises its COM sink,
and balances successful COM initialization. Blocking WinRT helpers cancel after 30 seconds instead
of spinning forever. Microphone permission probing no longer reports initialization/unknown errors
as granted. Unimplemented Windows capture backends advertise unavailable devices, leave failed
starts idle without retaining callbacks, and enforce pause/resume state. ICO loading now parses and
bounds the container before passing an exact image slice to Win32 instead of giving an unbounded raw
pointer to `LookupIconIdFromDirectoryEx`; tooltip/balloon conversion and tray menus are bounded.
Shared named-pipe resolution enforces the Windows 256-wide-character path limit, and unsafe handle
Send/Sync assumptions are documented. The host gate remains 2,058 passing library tests plus strict
standard and `macos-blade` all-target runtime-shader Clippy. The Windows cross-check was attempted
but stopped in `psm`/`ring` before `kael` because `lib.exe`, MSVC headers, and the target C sysroot
are unavailable; these Windows-only edits therefore remain source-reviewed and Rustfmt/diff-checked,
not target type-checked. The older aggregate counts above are superseded by this note.

Row 31 Linux remaining-source note: AT-SPI action retention and app names, global-hotkey retention
and IDs, XIM commit/preedit strings, Wayland cursor theme sizes/scales/icon names/images, sysfs and
OS metadata reads, runtime/socket paths, and dialog subprocess output are bounded. Cursor selection
handles empty theme entries, checked scaling, dimensions, hotspots, and zero/negative scales without
indexing or division failures. Autostart identifiers/config roots are validated; desktop entries are
UTF-8/size bounded, Exec-escaped, written through unique mode-0600 staged files with sync/cleanup,
and read through bounded no-follow handles. Failed removal is surfaced. Unimplemented PipeWire and
portal capture sessions advertise unavailable sources, retain no callbacks after failed starts, and
enforce valid pause/resume state. Network/power monitors retain their child processes, clean up on
reader-thread spawn failure, and kill plus reap on drop rather than leaving zombies; sysfs probes
are bounded. X11 keycode range math is widened and malformed keymap arrays cannot index out of
bounds. The host gate remains 2,058 passing library tests plus strict standard and `macos-blade`
all-target runtime-shader Clippy. The Linux cross-check was attempted but stopped in GTK/GLib/GIO/
Pango/Cairo sys crates before `kael` because target pkg-config/sysroot configuration is unavailable;
Linux-only edits remain source-reviewed and Rustfmt/diff-checked, not target type-checked. The older
aggregate counts above are superseded by this note.

Row 1 follow-up from the workspace runtime smoke: the code editor now exposes a labeled macOS
text-input accessibility node with read-only/focused state and appropriate focus/set-value actions,
is a real keyboard tab stop, and caps its accessibility value at 65,536 Unicode characters without
splitting a character. The focused regression test, all 323 editor-language library tests, strict
library Clippy, and an entitlement-signed native accessibility inspection passed.

## Per-crate completion gate

- Review the manifest, features, source surface, unsafe code, platform gates,
  error handling, and public types.
- Check finite/bounded input handling, overflow behavior, cancellation and
  resource cleanup where relevant.
- Add focused regression tests for every issue fixed.
- Run the crate's tests and a strict crate-local Clippy target.
- Record the exact result and any intentionally deferred cross-crate work here.
