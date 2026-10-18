# Kael performance and component review — 2026-10-02

Kael's retained rendering, native platform layer, optional batteries, and broad
component library provide a strong starting point. This review prioritizes
measurable scaling problems and visual correctness over adding more nominal
features. It does not establish a cross-framework ranking.

The user's comparison target is [GPUI Kit](https://gpui-kit.com/). The
[completion ledger](2026-10-02-completion-ledger.md) preserves the entire
remaining scope; this review's initial validation below is historical evidence,
not a claim that the newer APIs and every platform are finished.
The [matched comparison](2026-10-02-gpui-kit-comparison.md) includes ten native
runs and their limitations. Kael has lower idle wakeups and active CPU in that
navigation/detail workload, while physical footprint and p99 draw latency need
further work. Component, GPU completion, and controlled power comparisons remain.

The [October 3 native quality follow-up](2026-10-03-native-quality-followup.md)
records corrected fractional glyph rasterization, model-only idle wake-up,
actual clipboard/composition acceptance and fresh filesystem/document/workspace
pixels. Five maintained benchmark contracts now cover the application-shaped
surfaces. Historical comparisons remain qualified until corrected-source runs
and the remaining platform runtime gates pass.

The earlier October 3 hosted checkpoint preserves all one hundred full navigation,
Editor, data, tree and workspace runs in both instrumentation modes. Its
[diagnostic summary](2026-10-03-hosted-100-run-diagnostic.json) revalidates each
original workload oracle and retains raw hashes and environment metadata.
These schema-2 captures lack actual constrained viewport and comparative
presentation/power proof. Schema 3 adds equal actual client geometry and
active/churn frame requirements. The continuation also replaces prefix-scanned
UTF-16 conversion, releases outgoing tree models held by painted callbacks and
reserves accessibility snapshot storage. Current measurements and native
platform acceptance stay in the October 3 follow-up and completion ledger.

The [October 17 follow-up](2026-10-17-benchmark-quality-followup.md) records the
client-size mismatch correction and fresh matched native navigation diagnostics,
alongside subsequent image-cache, display-link, Select, menu, data-table and
cached accessibility improvements. General desktop workloads and shared controls
are the ongoing priority; historical editor results remain scoped evidence.
The full sizing-corrected hosted suite now completes all hundred schema-3
captures. A later isolated native text-cache experiment reduces navigation CPU
with a small RSS increase and worse CPU submission tails; it establishes no
overall framework ranking. Shared styled labels, BOM handling and form-control
focus/actions have fresh complete core/UI and physical Metal correctness proof.
The later [hundred-capture rerun](2026-10-17-hosted-e219-comparison.json) at
`e219439` also preserves and independently rehashes every measured application
executable. Data/tree CPU and RSS remain concrete gaps; its historical competitor
pin is distinguished from the newer 0.7.3 validation candidate.

The next [animation memory checkpoint](2026-10-17-animation-memory-validation.json)
bounds archive expansion, image decoding/resize buffers and retained frame batches,
and adapts prefetch to the admitted byte capacity. It passes native and actual
Chrome regressions, 2,178 core tests and the complete 39-package archive preflight.
These scoped limits leave global memory accounting and overlapping tasks open.
Later [tree stage measurements](2026-10-17-tree-stage-optimization-validation.json)
reduce expanded model construction and logical comparison work while retaining
full accessibility; complete preparation remains expensive. The subsequent
[idle animation cache](2026-10-17-animation-idle-cache-validation.json) bounds
process-wide retained decoded-image payload and entry count, with active/model/
scratch/output budgets still separate. A [general desktop data investigation](2026-10-17-compact-data-storage-validation.json)
shows 51.61 MiB less requested Rust payload for opt-in compact domain records in
the same grid. Public string contracts and canonical benchmark fixtures remain
unchanged; these scoped diagnostics do not establish native RSS or an overall
framework ranking. Combined validation passes 2,194 core tests, all 501 UI tests
and fresh builds of all 39 package archives at the recorded checkpoint.

The later [shared-control checkpoint](2026-10-17-shared-control-validation.json)
fixes bounded table layouts, variable-list origins, independent wheel axes,
Dropdown session/focus/pointer handling and native scrollbar action handlers.
The general records example has fresh native selection, read-only paste and
paging proof. Combined validation at `d766b5d` passes 2,198 core tests, all 537
UI tests and another isolated 39-package preflight, with every included Rust
file independently matched to Git. A separate [output ownership regression](2026-10-17-animation-output-ownership-validation.json)
removes a completed animation canvas copy: a real 16×8 frame requests 512 bytes
in one allocation rather than 1,024 in two, with identical pixels and balanced
release. Primary fork validation passes 137 native tests and 16 actual Chrome
cases. Native OS action-client limitations, unified memory accounting and fresh
latest-competitor/platform execution remain explicit open scopes.

The [exact-tag local runtime checkpoint](2026-10-18-gpui073-local-runtime-validation.json)
then freezes all twenty Kael/GPUI Kit 0.7.3 release executables and verifies their
source and compiler inventories. All twenty native attempts pass with equal
1100×760 logical client sizes at scale 2. Four navigation runs use the original
duration; sixteen component cases are quick correctness smokes. At that
checkpoint the full hosted comparison remained separate and unaudited; these
local checks establish no comparative performance, presentation or power result.

The [October 18 sizing/animation checkpoint](2026-10-18-track-and-animation-validation.json)
extends table scrollbars across their actual parent width and prevents Choker
integer overflow at extreme finite effect values. Native track clicks, keyboard
steps, independent scrolling and window zoom pass. The later
[asset lifecycle checkpoint](2026-10-18-asset-lifecycle-validation.json) preserves
ready drawing results under zero completed-cache retention, retires unmounted
owners and avoids redundant ready-hit notifications. Its combined primary checks
pass 2,216 core tests and all 538 UI tests; active output ownership remains
separate from completed-cache budgets.

The [completed platform run at `0d15aff`](2026-10-18-platform-0d15-final.json)
passes Windows, Linux renderer and browser jobs, while hosted Mac presentation
and three denied dependency advisories remain failed. Browser capture and
software-rendering limits remain explicit. These captures predate the later
nineteen-file follow-up; same-job independent Apple controls now improve future
diagnostics without replacing the required Kael presentation gate.

The [current physical Metal check](2026-10-18-native-desktop-quality-validation.json)
passes the same nineteen-file source: 58 distinct GPU tests, correct client
sizing and resize, idle model wake, and 58 frames with positive presentation
timestamps. The complete readback passes visual inspection. This local proof
leaves the hosted presentation failure and fresh packaging separate.

The [package preflight at `b279795`](2026-10-18-package-source-validation.json)
builds all 39 extracted archives and independently matches all 929 included Rust
files to Git. Required licenses, dependency aliases and fork provenance pass;
no crates are uploaded.

The [completed GPUI Kit 0.7.3 comparison](2026-10-18-gpui073-hosted-comparison.md)
then independently validates all 100 hosted captures at the earlier `0d15aff`
source, all twenty executable identities and 800 equal client-size boundaries.
Navigation and workspace use less active CPU in that cohort; table/tree CPU and
RSS remain concrete gaps. Hosted viewports are equally constrained to 1100×645,
scale 1. Zero-only energy counters provide no power result; later source,
physical presentation and overall superiority remain separate.

The subsequent [asset-admission checkpoint](2026-10-18-asset-admission-validation.json)
limits built-in animation loading to two concurrent I/O/parse/poster operations
and reserves checked asset initialization before application callbacks. Reproduced
reentrant oversubscription and duplicate-load failures are fixed, with explicit
panic cleanup waking waiting views. The combined nine-path source passes 15
checks, including 2,232 core tests, all 538 UI tests, native animation 140 plus
the separate public loader policy, strict lint, Wasm and consumer builds.
The same batch's [CPU Choker fast path](2026-10-18-choker-optimization-validation.json)
avoids one 4N-byte RGBA scratch copy in each of 52 eligible tiny allocator cases,
with exact pixels and actual Chrome17 acceptance. Global byte accounting,
fresh packages, platform CI and later-source comparative measurements remain
separate from this headless checkpoint.

The same committed `4bb5a44` source subsequently passes
[fresh packaging](2026-10-18-admission-package-source-validation.json): all 39
extracted package builds succeed, and all 930 included Rust files (23,517,476
bytes) match Git, including the public loader-policy regression. All 3,014 combined
validation inputs remain unchanged. No crates are uploaded. This closes the
fresh-package follow-up for that batch; current platform and later-source
comparative measurements remain open.


The [immutable tree-record checkpoint](2026-10-18-tree-shared-record-validation.json)
shares unchanged accessibility payloads across adjacent snapshots while preserving
full logical nodes, native identities and owned public value contracts. In the
100,025-row headless workload, five fixed alternating CPU pairs reduce warm whole
pipeline CPU by 22.02–34.68%; separate allocator runs reduce preparation requested
bytes by 90.19–91.26% and two-generation overlap payload by 43.90–44.28%. All 66
predeclared cost gates pass. Initial/replacement allocation counts rise about 50%
and one replacement median is 7.58% slower; these tradeoffs remain explicit.
Unfiltered native suites pass 2,241 core tests (the same two opt-in ignores) and
all 560 UI tests, with strict lint, production Wasm, supplemental joint Wasm test
compilation and formatting. Existing optional-Lottie test portability and
standalone UI Wasm test-feature limitations are recorded separately. This is
requested-payload and headless CPU evidence; latest-source native RSS, platform
acceptance and the matched GPUI Kit comparison remain separate.

## Component coverage and gaps

The library already contains most common controls, so component count alone is
a poor quality measure. The useful expansion is in reusable application models
and demanding interaction paths:

| Surface | Existing foundation | Next quality target |
| --- | --- | --- |
| Large navigation | Trees, file trees, virtual lists, menus, tabs | Worker-prepared filesystem snapshots, bounded lazy loading and full logical accessibility; native workflow validation remains |
| Workspace layout | App shell, sidebar, resizable split panes, tabs | Persistent movable docking groups and floating panes now have interaction regressions; native workflow validation remains |
| Data applications | Data table, editable data grid, virtual spreadsheet | Real remote-data, editing, clipboard, frozen-pane, and accessibility workloads; build on existing grids |
| Forms and settings | Inputs, Select, Combobox, menus, sliders | Native actions, live search/selection, focus ownership and app/window deactivation |
| Documents | Rich text, Markdown, HTML, PDF/Office and text editing | Import/export fidelity, selection, accessibility and document workflows |
| Visual tools | Retained 2D scene, gradients, shadows, blur, charts, media controls | Portable custom fragment passes and GPU-backed targets, plus visual parity across devices |

## Implemented improvements

### Rendering and visual correctness

Metal now allocates path, MSAA, and cached-subtree scratch targets only when a
scene uses them. Each target retains its own capacity for reuse; a resize that
does not use that feature no longer increases its allocation. Zero-size
teardown and first-use rendering are covered by pixel and allocation tests.

A native probe on this Apple M2 Pro measured the previously eager 3840 × 2160
textures as 33,947,648 bytes for paths, 135,790,592 bytes for 4× MSAA, and
33,947,648 bytes for the subtree cache: 203,685,888 bytes (194.25 MiB) in total.
Quad-only rendering now leaves these targets absent. This is a saving in those
specific scratch allocations, not a measurement of total application RSS.

Metal and DirectX ordinary and path-sprite pipelines now use source-over alpha.
Two half-opacity layers previously produced alpha 255; Metal pixel tests
reproduced that failure and now require the correct alpha of approximately 191.
Blade and browser blend-state paths were inspected for the same error.

Backdrop blur now binds its fragment parameters correctly on Metal and preserves
premultiplied intermediate color. Metal, DirectX, and Blade shader math no longer
applies captured alpha twice. Captures retain their framebuffer coordinates, so
sampling now uses those coordinates directly and clamps to the captured texel
centers. Native red/blue-step and viewport-edge tests reproduced the distorted
sampling and verify the corrected Gaussian output, alongside translucent tint
and saturation tests. Blade output also follows the surface alpha convention.
The continuation replaces the browser tint fallback with a bounded GPU Gaussian
blur. Chrome on Metal passed its eight blur fixtures. A fractional-coordinate
sampling correction also passed the unchanged eight blur fixtures on forced
SwiftShader, with all fifteen mandatory browser GPU contracts passing locally;
the fresh hosted browser run remains tracked separately.

WGSL checks now validate shader semantics and layouts beyond parsing, including
on Metal/DirectX test hosts. Blade assigns resource bindings at pipeline creation;
the source-level validator deliberately leaves binding checks to that stage.

Metal offscreen exports now reject oversized readbacks before GPU targets,
path scratch, command work, or instance buffers are allocated. The existing
image/readback limits remain intact.

### Resource planning and GPU budgets

Transient allocation planning preserves deterministic first-fit slot IDs and
inclusive resource lifetimes while replacing unbounded slot scans with a small
bounded scan plus per-class interval heaps. Planning takes O(R log R) time and
O(R) temporary storage for R used transient resources. Tests compare exact
assignments against a simple independent oracle, including allocation classes,
buffer/texture kinds, imports, unused resources, and overlapping lifetimes.
The stress case uses all 100,000 permitted resources simultaneously.

The GPU memory manager now uses a resource hash index and ordered LRU index.
Touches, releases, and each eviction take O(log N) time; registration no longer
scans every previous resource. Identifier/clock rollover, callback panics, byte
overflow, and the full 65,536-resource limit are covered. A full release/pressure
drain also releases the resource bookkeeping table.
Checked requests larger than the whole budget now fail before evicting existing
resources, so an impossible allocation cannot unnecessarily empty the cache.
The indexes add metadata compared with the former vector. The resource-count
ceiling bounds that bookkeeping and full drains release it; GPU byte accounting
continues to cover registered payloads, rather than CPU index allocations.

The maintained allocator benchmark measured these nine-sample medians on this
M2 Pro (16 GiB, macOS 27.2, Rust 1.97.1, optimized workspace release profile):

| 30,000-resource planning workload | Before | After |
| --- | ---: | ---: |
| All lifetimes overlap | 280.899 ms | 0.429 ms |
| Sequential, one allocation class | 0.136 ms | 0.129 ms |
| Sequential, 256 classes | 2.696 ms | 0.518 ms |

These timings exclude graph construction, compilation, GPU allocations, and
rendering. They demonstrate a planning improvement, not an application-wide
speedup. Resource-planning and GPU-bookkeeping benchmarks are now included in
the scheduled cross-platform performance workflow and uploaded with its results.

An identical optimized standalone harness, using the old and new manager source,
measured the following nine-sample medians. These are CPU bookkeeping costs with
one-byte logical weights and empty callbacks, excluding driver allocation:

| 30,000-resource GPU budget workload | Before | After |
| --- | ---: | ---: |
| Register | 180.583 ms | 3.727 ms |
| Touch all | 183.458 ms | 4.560 ms |
| Evict all | 714.994 ms | 1.016 ms |

The maintained Cargo release benchmark independently measured 2.913 ms for
registration, 3.282 ms for touches, and 1.194 ms for eviction. Different compiler
contexts affect absolute timings; compare the standalone results to each other.
The [measurement data](2026-10-02-benchmarks.json) preserves scope and sample counts.

The maintained CPU planning and budget benchmarks have also executed on GitHub's
macOS, Linux and Windows runners at commit `0adb736`. Their
[raw numerical results](2026-10-02-cross-platform-bookkeeping-results.json)
establish portable execution. Different runner hardware and incomplete hardware
metadata prevent a cross-platform ranking; these workloads exclude driver
allocations, native applications, presentation and power.

### Cache memory and pressure

Image LRU completion now prunes completed loads on the next frame, including
when no new unique image is requested. Pending loads do not evict decoded
images that fit the capacity. Existing-entry accesses preserve the currently
requested image while pruning other completed entries. Tests cover ready-load
bursts, callback cleanup after clearing, released image references, and recency
rollover. Pending loads remain outside the decoded-entry limit.

The continuation adds separate byte/count admission for decoded images and
pending work, a four-permit decode gate before I/O, bounded completed default
assets, and cancellation on pressure or loss of the last waiting view. These
new limits address pending work separately from the initial decoded-entry LRU.
The [memory evidence](2026-10-02-memory-accessibility-completion.md) records the
new implementation and current test scope.

`MemoryCache::with_byte_budget` and `CacheManager::with_memory_byte_budget`
add optional payload-byte ceilings without changing existing `CacheConfig`
literals. Oversized values do not enter memory; manager replacements invalidate
stale memory after the disk accepts new content. Budgets cover retained payloads,
with keys, metadata, temporary serialization, and caller-owned clones excluded.
Entry caps still bound metadata independently.

The media frame cache now bounds both bytes and entry count (4,096 entries by
default, configurable with `with_entry_limit`), rejects empty frames, and
supports immediate budget changes. An indexed slot arena removes repeated
full-map scans during eviction and avoids repeated key hashing on hits. Slots
are reused, pressure changes compact metadata, and clearing releases storage.
Mixed-operation tests compare cache behavior and link invariants against an
independent LRU model.

An identical optimized old/new source harness measured bulk eviction of 30,000
one-byte frames at 1,127.901 ms before and 0.884 ms after (three-sample medians).
Insertion changed from 2.546 to 2.632 ms and reverse-order hits from 0.438 to
0.538 ms; the small hit overhead accompanies the new metadata bound and LRU
maintenance. The maintained release example measured 4,096/16,384/65,536 evictions
at 0.122/0.505/2.601 ms respectively (seven-sample medians). These probes exclude
decoding, GPU work, and application frame time.

### Components for large desktop workspaces

`TreeList` now builds shallow row payloads, borrows filtered nodes, uses a
one-pass ancestor index, and avoids unused row allocations. Shared node models
can be passed as `Arc<[TreeNode<T>]>`. Matching no longer recounts label prefixes
for every substring, and Unicode lowercase expansion maps highlights back to
the original label.

The new `VirtualTreeList`, `VirtualTreeModel`, and `VirtualTreeState` provide a
scalable file/project explorer. Immutable snapshots are reused across redraws;
only viewport rows mount. One keyboard focus handle tracks the active logical
ID through insertion, filtering, and collapse. Navigation skips disabled rows,
scrolls active rows into view, and supports selection and expansion callbacks.
Mounted rows expose hierarchy and wired accessibility actions. The initial
implementation represented only mounted rows. The continuation adds immutable
logical accessibility snapshots, stable semantic IDs and active descendants,
with worker-prepared metadata for large filesystem models. Native assistive
technology exploration is still required before closing the parity requirement.

The runnable `virtual_tree` example contains 100,000 files and 25 directories.
Regression tests verify viewport mounting, End/Home navigation, tab traversal,
logical focus retention, duplicate visible ID rejection, filtering, and mounted
accessibility behavior. Ordinary `TreeList` remains appropriate for smaller
trees; it still mounts all displayed rows.

Native manual inspection exercised the complete 100,025-row example: jump to
last row, Home/End, collapse/expand, and selection all update the displayed model
and focused row. That initial native inspection used the mounted-row
accessibility implementation; it does not validate the newer logical snapshots.

Manual AX-only button activation also exposed an idle macOS window defect:
AccessKit queued actions without requesting a frame to drain them. The provider
now wakes once per batch, and the window schedules a forced foreground frame
using weak ownership. Tests cover batch preservation, unlocked/reentrant wakeups,
callback cleanup, and Button dispatch without pointer input.
The rebuilt native example was then verified twice using only the accessibility
button action: it immediately scrolled to and focused logical row 100,024,
without a coordinate click or another input event.

## Highest-priority remaining work

| Priority | Work | Quality bar |
| --- | --- | --- |
| P0 | Public render targets and portable custom fragment passes | Validated WGSL, reflected bindings/layout, bounded pipelines, device ownership, budget-aware targets, GPU-to-UI sampling without readback, native pixel tests |
| P0 | Product-shaped performance evidence | Matched startup/interaction workloads, idle CPU/wakeups/power, RSS/GPU bytes, frame percentiles, sustained churn, complete environment metadata |
| P1 | Rendering parity across hardware | Translucent effects, clips, gradients, text, resize and device-loss coverage on Metal, DirectX, Vulkan/Blade, and browsers |
| P1 | Virtual accessibility depth | An explicit offscreen-node/active-descendant model with native screen-reader tests; preserve viewport-only rendering |
| P1 | Idle accessibility actions on other native platforms | Audit action-queue wakeup paths with each platform's threading model and real assistive technology |
| P1 | Unified memory-pressure policy | Byte budgets for decoded images and pending work, an eviction policy for the default `App::fetch_asset` completed-task cache, scratch-target shedding, and consistent atlas policy beyond Metal |
| P1 | Reusable filesystem explorer models | Adapt `FileTree` to shallow/virtual models and cached sort keys, with lazy asynchronous directory population, context actions, and drag/drop |
| P2 | Additional desktop workflows | Persistent docking/workspace models, structured property inspectors, and richer document interactions around real app workloads; extend existing split panes, grids, and editor primitives |

## Custom shader status and staging

The optional `custom-shaders` feature now provides typed render-target handles,
WGSL fragment registration, reflected bindings and direct GPU texture display.
Metal's seven custom-renderer regressions and Blade's five regressions have run
on this host. Windows CI has executed eight Direct3D 11 custom-renderer tests and
six graph contracts using WARP, without skips. Linux CI has executed five custom,
six graph and eight retained-scene contracts with llvmpipe. These software devices
exercise the actual native backends; additional physical-device coverage remains.
Six browser fragment/context-loss regressions have run in real Chrome on this
host. GTK4/GSK has
an explicit unsupported custom-rendering path until its native context/texture
interop is implemented. `runtime_shaders` continues to control built-in Metal
source compilation independently of the public API.

`GpuRenderGraph` executes declared fragment/compute DAGs, preserves exported lifetimes,
checks imports/bindings before allocation, compares exact input/output revisions
and reuses compatible dead transient slots. Six shared native graph contracts
have executed on both Metal and Blade. They verify buffer → image → fragment
pixels, alias residency, cache invalidation, import validation, pressure release
and invalidated-output recovery. Native compute supports bounded storage buffers
and RGBA8/RGBA16F storage images. The same six DirectX contracts passed in native
Windows CI using WARP. macOS CI, fresh populated application frames and the
remaining accessibility/native interaction checks remain tracked in the
completion ledger.

The updated [render-target/shader design](../design/0001-render-targets-and-custom-shaders.md)
defines the implementation boundaries and exit criteria. The non-media
`custom_shader` example uses a reusable GPU target and direct UI sampling.
Backend parity, native compute and general graph execution retain their full
runtime quality gates.

Uniform data must obey the reflected WGSL offsets, alignment, and strides;
`#[repr(C)]` alone is insufficient. The standard defines
[uniform-address-space layout constraints](https://www.w3.org/TR/WGSL/#address-space-layout-constraints).
Browser compute requires a separate capability decision: Khronos retired the
WebGL compute proposal in favor of
[WebGPU](https://registry.khronos.org/webgl/specs/latest/2.0-compute/).

## Reproduction

```bash
cargo bench --locked -p kael_render_graph --bench transient_allocation
cargo bench --locked -p kael --bench gpu_budget
cargo run --locked -p kael_media_engines --release --example frame_cache_pressure
cargo run --locked -p kael_ui --example virtual_tree
```

## Initial validation, before the continuation

| Check | Result |
| --- | --- |
| Unit tests, default features, five affected crates | 2,884 passed: core 2,056; UI 396; cache 59; media 251; render graph 122 |
| Native Metal offscreen regressions | 18 passed on the real device, included in core results; no device skips |
| All-target Clippy, five affected crates | Passed with `-D warnings` |
| Browser component library | `wasm32-unknown-unknown` compile passed with `browser` and no default features |
| macOS Blade library | Compile passed with `font-kit,macos-blade` and no default features |
| WGSL semantic and primitive-layout validation | Passed, included in core results |
| Native virtual-tree demo | Built; pointer, keyboard, and repeated AX-only activation verified |
| Formatting and whitespace | Workspace formatting and `git diff --check` passed |
| Documentation | 46 pages verified; no broken local links or duplicate IDs |

The initial unit/lint commands were:

```bash
cargo test --locked --offline -p kael -p kael_ui -p kael_cache \
  -p kael_media_engines -p kael_render_graph --lib --features kael/runtime_shaders
cargo clippy --locked --offline -p kael -p kael_ui -p kael_cache \
  -p kael_media_engines -p kael_render_graph --all-targets \
  --features kael/runtime_shaders -- -D warnings
```

Local build logs and probes are retained in `.artifacts/kael-review-*`,
`.artifacts/kael-renderer-*`, and `.artifacts/kael-metal-scratch-allocation-probe.*`.
The changes are validated on this macOS host; a browser compile check is distinct
from browser runtime evidence, and DirectX/Vulkan runtime parity still needs
the maintained platform CI and hardware checks.

The [modal and clear-control checkpoint](2026-10-18-modal-and-clear-validation.json)
adds scoped Tab traversal for Dialog/Sheet, correct empty and cached semantic
focus, and named native clear-button Click actions. A real hide/show regression
first reproduces a stale cursor-style range panic; cached paint now rebuilds
after a missed paint, then resumes reuse. Twenty focused cases pass alongside
unfiltered default-parallel core 2,236 passes (the same two opt-in ignores), all
555 UI tests, strict all-target lint, Wasm compilation and formatting. All 1,015
selected compiled inputs in the primary checkout match the isolated tested
graph. Four updated guides pass documentation checks. This is headless
correctness proof; fresh native, package and later-source performance checks
remain separate.

The [Grid value/publication checkpoint](2026-10-18-grid-value-and-publication-validation.json)
shares one mounted-cell value between painting and accessibility, and avoids
notifications or redraws when outdated snapshot work is discarded. The candidate
passes all nineteen focused cases; the baseline reproduces three fixed regressions.
The maintained five regressions pass with all 565 UI tests, strict UI lint,
production and test Wasm compilation and formatting. The earlier frame-test
failure remains archived: its helper also drained a valid latest publication;
a separately named case now checks those two intervals independently. This is
headless behavior evidence, with no new CPU/RSS or native-presentation claim.

The [static-image admission checkpoint](2026-10-18-static-image-working-validation.json)
shares an App-wide byte reservation across built-in static decoders, covering
native pixels and RGBA conversion. All 2,251 enabled core tests and 565 UI tests
pass, with strict lint, Wasm compilation and formatting. Five fixed alternating
CPU pairs pass all 80 predeclared checks; tiny WebP calling-thread CPU increases
5.56% for the typed common-shim route and 3.61% for the public resource route.
The 1,280 MiB accounting capacity preserves permitted high-bit-depth images.
Codec-private buffers, encoded input, animations, synchronous constructors and
retained output ownership remain separate; no RSS saving or unified R8 closure
is claimed. Current native presentation and fresh packaging remain pending.

The [immutable platform cohort at `b411223`](2026-10-18-platform-b411-validation.json)
passes Windows 2022/2025, Linux runtime and browser jobs. Hosted Mac GPU tests
pass, while native presentation and three independent Apple controls produce
zero positive timestamps. The same-source local Metal check records a positive
frame. Three denied unmaintained-dependency advisories also remain failed.
This cohort predates the subsequent Grid and image changes.

The image/Grid source at `10db90b` also passes the original local Metal gates:
59 GPU test executions covering 58 distinct tests, plus 59 separate positive
presentation frames. Initial 720×460 sizing at scale 2, the programmatic resize,
1,250 ms idle model wake and the complete deterministic readback pass. The PNG
is byte-identical to the earlier scene. Independent audit keeps these GPU and
presentation counts separate; current packaging and hosted acceptance remain open.

The [completed b411 comparison](2026-10-18-b411-gpui073-hosted-comparison.md)
passes 100 original captures, 800 equal actual 1100×645 at scale 1 boundaries, 50 pairs and
20 executable identities. Four general desktop workloads lead the CPU/RSS and
separate CPU timing tables. Navigation active CPU is 9.95% / 12.64%
(Kael/GPUI Kit); table and tree CPU/RSS remain higher, while docking is close.
Operation totals and component/accessibility work differ, and later Grid/image
changes are excluded. No overall winner, power or presentation claim is made.

The [committed ce39eed package proof](2026-10-18-image-grid-package-validation.json)
builds all 39 extracted archives and independently matches all 931 included Rust
files to immutable source, with no upload. Local image/Grid Metal proof remains
a separate checkpoint. The [native wheel attempt](2026-10-18-native-wheel-validation.json)
stops on a child-process-group ownership guard before any event; later cleanup
recovery is recorded separately and native wheel acceptance remains open.

The [completed e205 comparison](2026-10-18-e205-gpui073-hosted-comparison.md)
validates all 100 original captures, 800 equal actual client-size boundaries,
50 pairs and 20 executable identities at frozen `e2054aa1`. Four general desktop
workloads lead; editor coverage remains supplemental. Navigation active CPU is
11.88% / 16.58% (Kael/GPUI Kit 0.7.3); table and tree CPU/RSS remain higher.
Tree active CPU is 45.30% / 13.90% and stable sampled peak RSS is
932.86 / 654.52 MiB. Unequal operation counts and accessibility/preparation work
prevent an equal-work or single-cause interpretation. The restored competitor
build cache is explicit; no fresh dependency compilation, current-HEAD ranking,
power or presentation advantage follows.

The separate [e205 platform record](2026-10-18-platform-e205-validation.json)
remains failed overall. Windows 2022/2025, Linux renderer and browser jobs pass
their scoped software-device/runtime contracts. Hosted Mac runs 59 GPU tests
covering 58 names, but its 20 native completion callbacks have zero positive
presentation timestamps; three independent Apple controls also have zero in
180 callbacks. The Mac PR merge checkout shares the exact e205 Git tree, while
its commit identity remains distinct. Three denied dependency advisories remain
failed. Local Metal and browser proof do not waive those hosted gates.

Both [idle text-wrapper experiments](2026-10-18-idle-text-wrapper-experiment.json)
are rejected at the unchanged over-capacity churn threshold. Lower warm CPU or
retained requested bytes do not establish acceptance. The pool is not adopted;
whole-cache byte policy, active ownership and overlapping/scratch allocations
remain separate memory work. The later [tree catalog reserve](2026-10-18-tree-catalog-reserve-validation.json)
passes all 58 fixed factory cost gates and all 566 UI tests, strict UI lint,
production/test Wasm and formatting, then is adopted. A large partially collapsed
factory uses 44→29 allocation events and about 15% less CPU; the fully expanded
case is 4.67% slower, and early duplicate rejection temporarily requests about
2.13 MiB more. These tradeoffs are explicit. The first short empty-input cohort
remains inconclusive; a separately specified longer cohort satisfies the same
20 ms floor. Full logical accessibility and arbitrary public setter behavior stay
intact. This does not establish application RSS or current cross-framework gains.

The additive [macOS energy collector](2026-10-18-macos-energy-collector-validation.json)
is adopted after isolated synthetic/own-process validation and maintained 32 Python,
three strict C builds and two synthetic fallback checks. CI now gates comparisons
on those tests. V4 unsupported-flavor fallback, nullable capability and monotonic
deltas remain explicit; the completed e205 cohort still uses V4 and its original
schema-3 gates. No watts, core-class interpretation or application power advantage
is established. Table/tree preparation cost, unified memory accounting, denied
advisories, hosted native presentation and native wheel/IME delivery remain open.

The [tree accessibility label fix](2026-10-18-tree-accessibility-label-validation.json)
is adopted: relabeling an unchanged model or displaying a stale worker preparation
updates the accessible root name while preserving row identities. Two behavioral
regressions, all 568 headless UI tests, strict UI lint, production and test Wasm
type checks, and formatting pass. Native screen-reader exploration remains open.

The [compact text-wrapper experiments](2026-10-18-compact-text-wrapper-experiments.json)
preserve another rejection. The bitmap candidate passes all 100 original cost
checks and reduces requested retained bytes from 676,872 to 308,232, but a separate
1,024-key cohort costs 4.20 times baseline CPU. Its 512-entry eviction policy is
not adopted; the original thresholds and failed measurements are retained.

The later [b091 platform cohort](2026-10-18-platform-b091-validation.json) also
fails overall. Windows, Linux renderer and browser jobs pass their scoped checks;
Linux still denies three advisories. Mac presentation timestamps remain zero,
and its distinct hardware-browser timing test exceeds both p95 and p99 budgets.
The merge checkout has the b091 Git tree with a different commit identity. This
cohort precedes the label fix and does not establish current platform acceptance.

The [completed b091 comparison](2026-10-18-b091-gpui073-hosted-comparison.md)
passes all 100 captures, 800 equal actual geometry boundaries, 50 pairs and 20
executable identities against GPUI Kit 0.7.3. Timing-disabled Kael/Kit active CPU
medians are 8.76%/11.62% for navigation, 24.65%/17.25% for tables,
46.97%/11.71% for trees and 15.03%/16.06% for docking. Table/tree RSS remains
substantially higher. Unequal work and sampled-memory limits stay explicit.
All 10,922 samples use V6 but return zero energy fields; capability and deltas
remain unknown/null. No power or overall ranking follows. This direct b091
checkout precedes the label fix; the separate failed platform job uses a distinct
merge commit with the same tree.

The [40b24 platform cohort](2026-10-18-platform-40b24-validation.json) remains
failed overall. Its actual PR merge has the exact workflow-head tree. Windows,
Linux renderer and browser jobs pass within their recorded scopes; the focused
GTK4 job and Windows 2022 workspace compile/lint were skipped. Mac records 44 GPU
completions, 42 drawable callbacks and zero positive timestamps; three independent
Apple controls each record 60 callbacks and zero positive timestamps. Linux still
denies the same three dependency advisories. The separate Metal-backed browser
budget passes at both viewports this run under unchanged limits; no repeatable
improvement or cause is established. Later font-source platform acceptance remains
separate.

The [shared font-arrival fix](2026-10-18-font-arrival-validation.json) is adopted.
A real CoreText fixture reproduces a successfully registered font remaining stuck
on fallback, then passes with the fix while established IDs, cached layouts and
held wrappers remain valid. Eight shared regressions, all 2,260 enabled Core tests
and 568 UI tests, strict Core/UI lints, production/test Wasm typechecks and
formatting pass. Three fixture files are present in the package listing; extracted
archive builds remain separate. Platform-specific font caches, browser held-width
revisions, native presentation and comparative performance remain open.

The [font-source package preflight](2026-10-18-font-package-validation.json) verifies one complete set of 39 extracted archives at `21b3932`: 931 Rust files (23,718,715 bytes), exact font fixture/license bytes, 47 internal dependency aliases and 53 required license/notice checks. The underlying build succeeds once. Its original wrapper STOP is preserved because Cargo’s `Verifying` labels name source origins; a separate supplement checks those 39 origins and 39 extracted `Compiling` paths, then completes only the three remaining postchecks. Both 3,182-input maps stay unchanged and known-owned staging cleanup passes; no crate is uploaded.

The [21b platform cohort](2026-10-18-platform-21b-validation.json) remains failed overall. Its distinct PR merge has the exact `21b3932` tree. Scoped Windows, Linux runtime and browser jobs pass, while Windows 2022 workspace compile/lint and the focused GTK4 job are skipped. Mac records 46 GPU completions, 44 drawable callbacks and zero positive presentation timestamps; three independent Apple controls each record 60 callbacks and zero positive timestamps. The separate Metal-class browser budgets pass both viewports, without an attributed improvement or native-presentation acceptance. Linux still denies the same three advisories. Earlier 40b24 platform history and the b091 GPUI Kit comparison remain separate frozen-source evidence.

The [earlier native wheel attempt](2026-10-18-native-wheel-21b-validation.json) also fails. One fixed horizontal event reaches the fixture’s AppKit monitor with delta `[-80, 0]` and no assigned window; no Core wheel or Grid offset trace follows. S0 is populated, but S1 does not settle, so no vertical or extra event is posted. Owned process cleanup passes; prior-foreground restoration remains unconfirmed and is not forced over an unrelated app. The evidence does not identify a routing, decoder or Grid cause. The user explicitly defers IME testing; IME, nonzero horizontal wheel delivery, presentation, denied-advisory resolution and all other required acceptance scopes remain open.

The [immutable tree catalog
experiment](2026-10-18-tree-immutable-catalog-experiment.json) passes its separate
correctness checks, including six focused cases, all 574 UI tests and three mutation
oracles, but remains unadopted. Its pre-font 3,177-input cost cohort is INCONCLUSIVE:
1,624 of 1,628 numeric checks pass; four legacy requested-byte limits fail, and two
warm-model-wide CPU quality rows exceed the 10% MAD limit at about 10.92%. All numeric
CPU ratio gates and duration floors pass; those observations establish neither an
accepted CPU improvement nor an RSS result. Source-path-dependent Icon default-ID
formatting is an investigation hypothesis, not an allocation-site trace or an adjusted
pass. No threshold, sample or workload was changed, and this experiment does not replace
the frozen b091 application comparison.

The [V10 ordinary-session wheel record](2026-10-18-native-wheel-session-validation.json)
retains its original wrapper **FAIL**: both post-return CGEvent flags are `0x20000000`,
while send-intent flags and received AppKit modifier flags are zero; the bit’s named
meaning and cause remain unknown. The new session route is distinct from the historical
V8/V9 failed PID-addressed attempts. Recorded owned-window AppKit and Core/Grid traces
show the two fixed H(-80,0)/V(0,-56) events. After the wrapper stopped, an independent
artifact audit checked the later stable IDs/actions/values, exact geometry, 12
translated cell-pixel matches, full frozen-column/header-row regions and three 1400×840
retained GPU PNGs. Prior foreground is confirmed before fixture finish and afterward
with one activation; normal fixture/helper cleanup and group quiescence pass. Of 79
independently evaluated predicate groups, only the two outgoing post-flag predicates
fail; this is not retroactive wrapper acceptance. The evidence is scoped to the recorded
`21b3932` pre-Icon native source, with no physical mouse, compositor, cost or later-HEAD
claim. IME remains explicitly deferred.

The adopted [Icon default identity
optimization](2026-10-18-icon-lazy-default-id-validation.json) defers default Name
creation until the interactive path while preserving explicit IDs. The same seven
identity and mounted-action cases pass for both implementations, together with all 575
UI tests, strict UI Clippy, production and joint-test Wasm typechecks and formatting.
With precreated owned input, the fixed release control drops from 82 requested bytes and
three allocation events to zero; the 1,024-constructor bundle saves 83,968 requested
bytes and 3,072 events. Rendering 64 public named-string icons across 16 draws saves the
same counts. Icon size remains 1,352 bytes with alignment 8 on the measured host. Input
retirement is accounted separately; these requested-layout counts establish no CPU, RSS,
native pixel or application-wide memory result. The exact validated source and seven
regressions are installed in both checkouts. All 35 validation groups are terminal; the
two earlier test-fixture compile failures remain distinct from the three intentional
baseline allocation controls.
