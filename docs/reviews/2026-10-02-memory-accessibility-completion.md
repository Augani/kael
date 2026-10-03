# Memory pressure and accessibility completion evidence

This track implements requirements R6/R7/R8 in the full completion ledger. The
requirements remain open until native platform runtime evidence covers the
items below. Compilation and headless tests prove their stated scopes only.

## Implemented memory policy

- `App::fetch_asset_checked` admits new shared asset work before calling
  `Asset::load`. Its default pending cap is 64, configurable to 1..=4096. Hits
  retain single-flight behavior even at the limit. Window asset helpers use
  this checked API; legacy `fetch_asset` retains explicit unbounded admission.
- Completed default assets use deterministic LRU retention with defaults of
  256 entries and 64 MiB. Custom heap-backed assets report retained bytes through
  `Asset::cache_bytes`; built-in image loaders count every decoded frame.
  Generation identity prevents an old completion from accounting for a new
  request with the same key. Tick rollover preserves recency.
- Completed asset eviction releases native image residency, contains owner
  hook panics, and preserves external shared outputs. Critical pressure clears
  completed default assets and releases cache ownership of unfinished shared
  tasks before notifying subscribers. Caller-owned shared tasks remain valid.
  Window asset helpers register lightweight view wake tokens with the App;
  they do not retain independent task owners that would prevent cancellation.
  Callback failure does
  not prevent other owners from participating.
- `LruImageCache` has independent completed-entry, decoded-byte, and pending
  limits. Defaults are the requested entry count, 64 MiB and eight pending loads.
  `ImageCacheLimits`, `with_limits`, `set_limits`, and `lru_with_limits` expose
  configuration. Completed errors count toward the entry cap. Cache completion
  observers enforce limits without waiting for a new paint request.
- Declared static raster, GIF/WebP and SVG dimensions are checked against the
  requested decoded-byte budget before allocating pixel storage. Codec limits
  remain enabled. Animation admission additionally checks cumulative retained
  frame bytes. Source bytes, dimensions and frame counts retain native hard
  bounds of 64 MiB, 16,384 pixels per dimension and 10,000 frames respectively.
- A shared four-permit semaphore bounds built-in image I/O/decode concurrency
  across all caches. The permit is acquired before HTTP, file or embedded-source
  loading. Per-cache pending caps include jobs waiting for this permit.
- Cache completion observers are retained tasks, rather than detached owners.
  Image cache removal marks every pending task aborted before dropping the
  first active permit, preventing queued siblings from starting during clearing.
  Removing, clearing, dropping an image cache, or critical pressure cancels its
  owned work. Deferred request views wake when capacity becomes available.
  `RetainAllImageCache` retains explicit unbounded completed retention between
  clear/release/critical-pressure events, with eight pending loads.
- Every native window forwards `shed_memory` to its renderer. Pressure dispatch
  defers the call until the active window is back in `App::windows`, then requests
  redraw. X11/Wayland also forward atlas-budget configuration. GTK4 sheds unused
  atlas/GDK texture residency to the configured atlas budget or zero, protecting
  textures referenced by the current presentation.

These policies bound cache ownership and expensive work. They are not a process
RSS hard limit: caller-owned decoded handles, custom loaders, explicit retain-all
working sets and codec temporary allocations remain distinct owners. Removing an
App cache entry cancels a shared pending load only when its final external owner
releases it. A caller-owned synchronous `Image::to_image_data` decode is outside
the background image-job semaphore.

## Core validation

`cargo test --locked --offline -p kael --lib --features runtime_shaders` passed
with **2,071 passed, zero failed, zero ignored**. The local complete log is
`.artifacts/kael-memory-core-tests.log`.

Focused asset-cache and image-cache suites also passed (nine and fifteen tests respectively).

The passing tests include:

- Real pending HTTP futures with drop signals: before-work per-cache admission,
  the global four-job limit, remove/clear/critical-pressure cancellation and
  entity-release cancellation; queued permit waiters do not start HTTP work.
- Shared App load cancellation with and without an external `Shared` owner.
- Checked App pending admission before invoking a custom loader, single-flight
  hits at capacity, completed byte-LRU retention and preserved shared outputs.
- Old completion generations, cache clear/reload, byte budgets across animation
  frames, zero stale restoration and recency rollover.
- Static PNG/SVG declared-size rejection, cumulative animation byte rejection,
  and content-safe cache diagnostics.
- Panicking custom accounting/eviction hooks: no permanently pending/unaccounted
  outputs, continued pressure processing and retained external output validity.

GTK4 has a regression for current-presentation protection, unused tile shedding,
full drain after presentation release and subsequent insertion. Its platform
module is not built or executed by the macOS core test above.

DirectX scratch storage is now split into independent lazy path/MSAA, backdrop
blur and cached-subtree groups. Plain scenes and resize allocate none of these
groups; each effect initializes its own group, and pressure releases all groups.
Grown instance buffers shrink to their initial capacities while built-in
compiled pipelines remain reusable. Pressure first clears context references,
then releases resources/caches and submits driver retirement using `Flush`;
[Direct3D 11 defers GPU object destruction](https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-id3d11devicecontext-flush).
Live public render-target handles remain valid. Dynamic custom uniform/display
buffers use `WRITE_DISCARD` storage renaming and are retained only by the bounded
pipeline cache. Dead targets, custom pipelines and samplers are shed together.

Mandatory WARP regression sources cover native scratch laziness, exact pixels
before/after pressure, live-target validity, resize and subsequent buffer growth.
Other WARP tests exercise format storage, binding validation, direct composition
and device ownership. WARP validates correctness/lifecycle, not physical-GPU
performance. The Windows custom-shader test compilation passed in
`.artifacts/kael-dx-custom-testcheck.log`. Their Windows runtime execution is still
required.

## Checked atlas admission and retirement (R8)

`Window::set_atlas_admission_limits` validates an explicit hard descriptor before
mutation. Defaults bound atlas residency to 256 MiB, 65,536 live/deferred tiles
and 1,024 pages. Soft `set_atlas_byte_budget` remains an independent LRU policy;
`None` disables only soft eviction. Configuring a larger legacy soft budget can
raise the default hard byte ceiling, but never overrides an explicit descriptor.

Framework glyph/emoji, SVG, icon, image and cached-surface paths declare checked
dimensions before their build closures. Packed native/browser backends reserve
space before raster work and roll it back on error, no output, malformed bytes
or mismatched dimensions. Counts include deferred allocations. Page admission
runs before native driver texture creation and before browser CPU page allocation;
page bytes include the browser's CPU backing plus GPU mirror. GTK reserves the
source bytes plus one RGBA CPU/GPU variant, admits each additional color variant
before conversion and bounds variant counts. Blade tracks the exact sizes of its
actual upload buffers along with texture pages, reserves staging before raster,
and reuses/trims buffers only after their completion fences succeed.

Metal no longer calls `replaceRegion` while a page may be sampled. Apple's
[texture-write contract](https://developer.apple.com/documentation/metal/mtltexture/replace(region:mipmaplevel:withbytes:bytesperrow:))
requires synchronization across the entire texture, including disjoint packed
regions. Metal now submits immutable buffer-to-texture blits on the scene's
command queue. Uploads use chunks of at most 8 MiB, at most two pending batches,
at most 1,024 chunks and a 256 MiB staging ceiling. Admission counts the
simultaneous raster payload, destination textures and upload buffers before
raster work. Actual driver allocation sizes replace descriptor estimates before
the build closure runs; pending textures remain charged even after their atlas
page slot is released. A 64 MiB image is split into eight chunks and stays within
the default combined budget. Completed chunks may be reused or shed; a command
error or 10-second completion deadline retains queued resources until teardown
and stops retirement and progress wakes. Capacity rejection records a bounded
final refresh even when completion races the end-of-paint query, so the existing
Window refresh path retries missing rasters without input. Permanent admission
failures with no queued work do not request refreshes.

The common policy stamps tile identities directly from retained scenes, including
cached-subtree snapshots, avoiding re-rasterization just to refresh recency.
LRU ties use texture/tile identity for deterministic eviction. A live cached
surface remains pinned by its owner. Explicit removal unlinks a cache key while
retaining its region until four successful scene frames have passed. Replayed
uses extend retention. The clock has backend-specific progress guarantees: Metal
advances after a successful submission through its three-drawable layer, with
four retained frames covering the upcoming prepaint; it does not wait for GPU
completion on each displayed frame. Offscreen Metal scene captures complete the
same queue synchronously without advancing this clock. DirectX and WebGL uploads
are ordered in the scene command stream; GTK presentations retain immutable
texture references. Blade waits for the preceding scene fence before advancement.
Blade timeout/error paths freeze retirement; uncompleted
uploads/resources survive pressure. A bounded four-frame window refresh drains
removed regions even when frame-skip would otherwise suppress an unchanged scene;
ongoing retained uses do not force endless animation. Browser mirrors are deleted
before new uploads and after pressure, and context-loss freezes retirement until
restoration. New browser pages also obey the queried driver texture-size limit.

Evidence is intentionally separated:

- `.artifacts/kael-atlas-native-tests.log`: 240 platform tests passed, including
  shared checked accounting, overflow, clock rollover and replay/retirement cases.
- `.artifacts/kael-atlas-metal-tests.log`: seven actual Metal atlas tests passed,
  including rejection before raster, reservation rollback, resident page/count
  admission, retained native texture pixels, retirement and replay under pressure.
- `.artifacts/kael-metal-atlas-ordered-red.log`: the queued-read control failed
  with the legacy CPU write: an earlier GPU read observed the later `[29; 4]`
  pixels instead of `[17; 4]`. The ordered upload suite in
  `.artifacts/kael-metal-atlas-ordered-green.log` passed 12 tests with none
  ignored on the final source, covering old/new pixel ordering,
  two-batch and one-batch byte-capacity recovery, post-completion retry races,
  actual combined peak admission and panic rollback, 64 MiB chunk boundary
  pixels, and deadline retention without endless wakes.
- `.artifacts/kael-renderer-checkpoint-metal-clippy.log`: strict final native
  Metal library and test Clippy passed with warnings denied, including staged
  upload ownership and retry-latch code.
- `.artifacts/kael-atlas-blade-tests.log`: 19 actual Blade GPU cases passed,
  including exact staging allocation admission/rollback and existing scratch,
  graph, readback/fence failure and resource-lifecycle regressions.
- `.artifacts/kael-atlas-native-clippy.log`: strict native and macOS Blade library
  Clippy passed with warnings denied.
- `.artifacts/kael-atlas-windows-check.log`: Windows tests cross-compile passed;
  actual WARP runtime remains the Windows CI gate.
- `.artifacts/kael-programmable-webgl2-ci/webgl2-atlas-replay.log`: the actual
  browser GPU atlas test passed (one test, none ignored), including retained
  replay/pressure pixels, mirror retirement and exact reupload.
- `.artifacts/kael-atlas-gtk-cross-check.log`: real GTK library discovery failed
  locally because `gtk4.pc` is absent. The separate
  `.artifacts/kael-atlas-gtk-metadata-check.log` typechecks GTK source/tests using
  `DOCS_RS=1` to skip native library discovery/linking. It is compile evidence
  only. Linux CI must execute the named admission and replacement-retirement
  tests, then the actual GTK/Wayland presentation proof.

## Accessibility implementation and evidence

The existing macOS queue wakes the foreground accessibility callback and uses
weak/lifecycle-aware references. Linux now uses a coalesced, bounded wake channel
from the AT-SPI adapter thread to a retained foreground task. Windows uses an
empty-to-nonempty queue wake and a posted window message, preserving its platform
thread model. Queue tests cover wake coalescing, order, bounded work and closed
receivers/windows.

Root's Windows cross compile passed with custom shaders:
`.artifacts/kael-windows-ax-shader-check.log`. The initial Linux cross check
stopped before compiling Kael because `x86_64-linux-gnu-gcc` was unavailable;
`.artifacts/kael-linux-ax-check.log` records that limitation. Linux and Windows
platform queue tests and actual idle assistive-technology actions have not run
on this macOS host.

### Retained logical virtual-tree accessibility (R6)

`AccessibilitySnapshot` validates an immutable logical hierarchy.
`AccessibilityNodeMap` shares its node/label/child storage across frames and
adds bounded current-frame overlays. The exporter skips unchanged shared
snapshots and produces incremental AccessKit records, including hidden/revealed
subtrees and removed/reinserted logical nodes. macOS/Linux providers retain cheap
latest/previous trees and defer full conversion until native activation.

Virtual trees expose every displayed logical row while mounting only the
viewport. Stable semantic IDs follow surviving model IDs; the current model's
ID map replaces the old map rather than accumulating retired identities.
Container focus uses the active descendant even while that row is offscreen.
One retained subtree action handler resolves against the current model and
reveals/selects requested rows; stale removed or disabled targets are ignored.

`VirtualTreeModel::prepare_accessibility` prepares labels, the logical hierarchy
and semantic action maps on the model worker. The foreground adopts their Arcs
without rebuilding 100,000 labels or maps. Model lookup storage is shared with
the semantic preparation, and outgoing prepared storage is reclaimed on the
background executor. FileTree and the native virtual-tree example use this
worker route. Generic small/non-Send models retain a documented foreground
fallback; large callers should prepare before publishing.

Native inspection caught automatic UniformList wrappers and duplicate visual
text inside the logical tree. The virtual tree now uses explicit wrapper-free
list painting and hides text already named by its semantic row. A regression
checks pointer-identical 100,000-element root child storage after paint, plus
absence of duplicate List/ListItem nodes.

Focused validation passed:

- `.artifacts/kael-text-selection-tests.log`: 65 core accessibility tests,
  including retained AccessKit-consumer add/remove, hide/reveal, geometry-only
  delta and 100,000-node storage/focus semantics.
- `.artifacts/kael-logical-ax-tree-tests.log`: nine virtual-tree tests including
  worker-prepared first-paint adoption, bounded mounted rows, offscreen actions,
  independent scroll focus, stable catalog IDs across disclosure/filtering/deletion, and queued actions
  after replacement.
- `.artifacts/kael-native-outline-adapter-tests.log`: three macOS adapter
  regressions for complete hierarchical row enumeration, disclosure depth and
  action capabilities, and 100,001 rows without invented geometry.

Native macOS inspection confirmed upstream AccessKit exposes only top-level
outline rows and omits disclosure properties/actions. The licensed Kael adapter
in `vendor/accesskit_macos` supplies complete disclosed-row enumeration,
disclosure parent/level/state and supported expand/collapse dispatch. Native
mutation entrypoints reject disabled/hidden/collapsed retained objects.
[Patch provenance and Apple API references](../../vendor/accesskit_macos/PATCHES.md)
record exact upstream 0.26.3 source/commit, licenses, hashes and complete diff.
It ships as `kael_accesskit_macos` 0.4.1 through Kael's direct target-specific
path/version dependency; external consumers need no workspace override. The
release script orders this package before Kael. No publication occurred.

Actual macOS evidence in `.artifacts/kael-native-outline-protocol-tests.log`:

- Three adapter hierarchy/capability regressions and the harness-free
  `native_outline` test passed. Actual Objective-C getters enumerate 100,025
  rows, 4,000 disclosed children, level/parent relationships and offscreen focus;
  native press/focus/scroll, collapse/expand, rejected collapsed actions and
  released-context actions are checked. One full debug row enumeration took
  743 ms in the final capability-guard run; this is on-demand native AT
  enumeration cost, not steady foreground paint.
- Native CUA inspection of the production virtual-tree window exercised idle
  project collapse/expand and offscreen project selection/reveal. Physical
  geometry remains viewport only. CUA does not expose arbitrary outline getters;
  the actual protocol harness supplies the complete row/disclosure evidence.
- `native_text_selection` uses real NSAccessibility getters/setters for complete
  multiline values, UTF-16 counts/ranges, reversed selection, emoji/Chinese
  setter payloads and released-context rejection. Core metadata retains compact
  per-line UTF-8 lengths/checkpoints and one transparent document container, so
  a 100,000-line caret update exports one node with one child ID. Complete text
  runs are sent once per content revision, and old run identities are rejected.
- External standalone path-consumer metadata resolves only the named adapter
  package, without a root patch. Its actual packaged archive compiled successfully
  (224 KiB source / 47 KiB compressed). Strict adapter Clippy and the final
  native protocol tests also passed; the complete patch record is frozen.

Collapsed production models preserve IDs for current catalog members while
snapshot/action indices remain displayed only. Removed catalog members retire;
re-insertion receives a fresh ID. Retained native-object pointer continuity is
proved by the harness's retained logical-tree path, while removed production
rows promise stable semantic identity on re-expansion rather than ObjC pointers.
Native Windows/UIA and Linux/AT-SPI runtime evidence remains a separate gate.
Editor native CUA selection against the final integrated component remains open.

Raw node-context normalization rejects disabled or hidden nodes for every action,
including stale advertised capabilities. Normalized text selections retain the
originating immutable document ID; the Editor validates it again when its
deferred listener runs, preventing an old native range from selecting replacement
text with coincidentally valid offsets. The 66-case core accessibility suite and
two Editor selection regressions passed in
`.artifacts/kael-native-action-guards-green.log` and
`.artifacts/kael-editor-stale-document-selection-tests.log`.

Read-only Linux adapter review found remaining protocol gaps: upstream
`accesskit_atspi_common` 0.19.1 advertises Click only, without Expand/Collapse
actions or expandable/expanded state mapping, and `accesskit_unix` 0.22.1 exposes
no EditableText interface. Complete text and selection use exported runs without
requiring a root Value, but native TextRange scroll actions target exported run
IDs and still require owning-document routing. Text geometry, word navigation
and native Linux/Windows editor protocol tests remain open. A licensed named
Linux adapter fork and native CI evidence are required before closing this scope;
the shared consumer tests do not substitute for AT-SPI or UIA runtime proof.

## Native text/disclosure follow-on after checkpoint 0adb736

The named `kael_accesskit_atspi_common` and `kael_accesskit_unix` packages now
provide capability-derived native disclosure states/actions and an opt-in atomic
EditableText transport. Insert/delete/copy/cut/paste capture one originating
document range; the common queue and foreground Editor validate its immutable
document identity again. Read-only text permits Copy and selection. Disabled,
hidden, collapsed and released objects reject mutations. Existing adapter
constructors retain their unsupported-operation behavior.

The checkpoint Linux artifact contained repeated libatspi cache-signal signature
errors. The Unix fork now sends AddAccessible's single struct argument rather
than flattening its fields, and a real zbus serializer regression checks both
AddAccessible and RemoveAccessible envelopes. The hierarchy regression in the
component suite independently confirms 25 project roots with 4,000 document
children each through the actual Window/AccessKit full and incremental exports.
This establishes two source-level defects and invariants; a fresh actual Linux
AT-SPI run is still required to prove the cache/disclosure behavior end to end.

The Mac fork supplies native visible ranges, visible-range reveal, exact
UTF-16 string/attributed-string reads, shaped glyph bounds/hit testing and
optional atomic selected-text replacement/clipboard actions. Reveals preserve
selection and carry the original run ID. AccessKit's current reveal protocol
addresses a run, so this does not claim arbitrary character-level reveal
precision. Setter ranges that overflow, split a surrogate or require rounding
an atomic character are rejected. Detached views return empty screen geometry.

Focused evidence is retained in `.artifacts/kael-atspi-fork-tests.log` (13
translation tests), `.artifacts/kael-atspi-cache-envelope-tests.log` (actual zbus
wire signatures), `.artifacts/kael-macos-native-text-geometry.log` (three adapter
unit tests plus both real Objective-C outline/text harnesses), and
`.artifacts/kael-macos-text-origin-provider-tests.log` (origin revalidation and
closed-provider release). The native Mac harness enumerates 100,025 rows and
checks real hidden-window screen conversion, UTF-16 text, geometry, selection,
reveal, edits and guards. It does not substitute for the real Editor example's
presentation, clipboard and undo acceptance.

The subsequent real Editor app acceptance is recorded in
`.artifacts/native-ui-followon/editor_accessibility.log` with
`NATIVE_TEXT_ACCESSIBILITY_OK platform=macos` and clean fixture completion.
The owned Window's native protocol objects passed full Unicode text, selection,
shaped bounds/hit testing, visible ranges, reveal, EOF, read-only/disabled guards,
atomic replacement/Undo/Redo and delayed old-origin rejection. The dedicated
native-text evidence document retains the exact executed binary hash and scope.
Clipboard, physical IME and presentation pixel acceptance remain separate.

All named adapters use path/version/package dependencies and retain upstream
source hashes, licenses, exact upstream manifests and complete patch records.
The standalone external consumer resolves their aliases without root patch
overrides. `cargo package --list` checks archive membership; no registry package
was published. Cargo's reserved `Cargo.toml.orig` filename is excluded, with its
unchanged upstream bytes shipped as `UPSTREAM-Cargo.toml`.

The Linux tree client retains exact 25-by-4,000 hierarchy bounds and compares
D-Bus service/path identity across collapse/re-expansion. The dedicated
`native-text-accessibility-atspi.py` client requires the actual Unicode fixture's
full text, scalar offsets/EOF, word navigation, visible/offscreen glyph geometry,
selection-preserving reveal, clipboard, atomic edit/undo/redo, read-only/disabled
guards and a clean fixture completion. GNOME's independent D-Bus XML/C contract
confirms InsertText length is UTF-8 bytes, whereas positions are character
offsets. The client also checks the actual clipboard rather than relying on
CopyText's C/GI success boolean for its void D-Bus reply.

## Required remaining evidence

- [x] Checked decoded-byte and pending admission before built-in image work.
- [x] Completed default asset eviction and retained-owner semantics.
- [x] Actual cache-owned pending cancellation, release and stale-result tests.
- [x] Common pressure dispatch and all native window forwarding hooks.
- [x] All-backend checked atlas admission, replay tracking, deferred retirement
  and pressure hooks with native Metal/Blade proof and cross-checks.
- [x] GTK4 shedding/admission implementation and regression source.
- [ ] GTK4 native test/runtime execution and presentation-safety proof.
- [x] Metal scratch/atlas pressure runtime, continued live-target validity and
  redraw/reallocation proof after shedding.
- [ ] DirectX 11 scratch/atlas pressure parity, in-flight retirement and real
  pressure/redraw/lifetime runtime proof.
- [x] Blade scratch/atlas pressure parity, in-flight retirement and real
  pressure/redraw/lifetime runtime proof.
- [ ] Linux/Windows native queue tests, close/reopen lifecycle and actual idle
  action execution using AT-SPI/UI Automation.
- [ ] Native macOS idle action/close lifecycle verification against the final
  integrated tree; previous queue tests remain intact.
- [x] Common retained logical snapshots, stable offscreen tree IDs, incremental
  export and accessible focus across scrolling/model replacement (R6).
- [x] Worker-prepared virtual-tree semantic metadata and bounded first-paint
  adoption; actual FileTree first-paint timings remain tracked separately.
- [x] Native macOS complete outline protocol getters/disclosure/offscreen actions
  plus production Window idle collapse/expand/selection routing (R6).
- [x] Native macOS text-selection protocol and common Unicode/revision validation.
- [x] Final packaged adapter verification and external consumer resolution.
- [ ] Native Editor CUA selection against the integrated component.
- [ ] Bounded logical grid headers/dimensions/active descendant and offscreen
  action fetching, plus native Linux/Windows exploration/actions (R6).

No unchecked item is waived by this document or by the passing core suite.

The final local Metal/Blade atlas identity checkpoints include actual
pressure/replay/reupload and scratch recovery pixel cases. They verify retained
live targets while shed cache storage is recreated; checked texture and packed
tile identities reject retired scenes, foreign atlases and delayed releases.
`.artifacts/kael-atlas-identity-metal-final.log` records 59 executions across
58 unique tests (including the shared identity models), and
`.artifacts/kael-atlas-identity-blade-final.log` records 29 executions. These
are real M2 Pro Metal and Blade-on-Metal fixtures; DirectX, Linux/GTK and browser
backend acceptance remains tied to fresh hosted results.

The refreshed macOS provider run in
`.artifacts/kael-final-followon-native-macos-provider-tests.log` passes three
portable outline cases and actual full 100,025-row NSAccessibility getters,
4,000 disclosed children, offscreen actions, stable native objects and released
lifecycle. Its actual Unicode text provider fixture also passes geometry/hit
testing, reveal, partial editing/clipboard and detached/read-only/disabled guards.
