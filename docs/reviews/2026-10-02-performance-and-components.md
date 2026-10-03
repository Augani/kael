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

## Component coverage and gaps

The library already contains most common controls, so component count alone is
a poor quality measure. The useful expansion is in reusable application models
and demanding interaction paths:

| Surface | Existing foundation | Next quality target |
| --- | --- | --- |
| Large navigation | Trees, file trees, virtual lists, menus, tabs | Worker-prepared filesystem snapshots, bounded lazy loading and full logical accessibility; native workflow validation remains |
| Workspace layout | App shell, sidebar, resizable split panes, tabs | Persistent movable docking groups and floating panes now have interaction regressions; native workflow validation remains |
| Data applications | Data table, editable data grid, virtual spreadsheet | Real remote-data, editing, clipboard, frozen-pane, and accessibility workloads; build on existing grids |
| Documents | Code editor, rich-text display, Markdown and HTML rendering | Verify full editing/selection/IME/undo workflows using product documents |
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
