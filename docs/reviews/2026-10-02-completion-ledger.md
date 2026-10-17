# Full completion ledger

This ledger preserves the remaining scope of the performance/component review.
The goal is complete only after every requirement has current implementation
and runtime evidence at the scope described here. A compile check, a data-only
API, or a documentation update does not prove an interactive/GPU requirement.

The comparison target supplied by the user is [GPUI Kit](https://gpui-kit.com/).
Its published claims are comparison inputs; superiority requires matched runs.

| ID | Required final state | Authoritative proof | Current status |
| --- | --- | --- | --- |
| R1 | Public typed targets, portable fragment registration, reflected layouts/bindings, bounded pipeline/source/target storage, device ownership, budget-aware lifecycle | Public application example executes GPU pass and displays target without readback; malformed/layout/admission/lifetime tests; exact output pixels | Implemented with native examples and Metal/Blade GPU proof; platform completion remains |
| R2 | Native fragment parity on Metal, DirectX 11 and Blade, browser fragment path | Same effect/bindings on each real renderer; transparency, HDR, clipping, sampling and resize goldens | Metal/Blade/browser-on-Metal runtime proof; eight DX11/WARP custom tests passed in CI; fifteen mandatory browser GPU contracts and the complete Chromium/Firefox/WebKit job pass at `dacdea1` |
| R3 | Native compute and GPU render-graph execution with declared imports, barriers/lifetimes, cache-key skipping and budget-pressure eviction | Real compute output and multi-pass DAG pixels; aliasing, stale input, cache, invalid imports and device-loss tests | Six shared graph fixtures pass on Metal, Blade and native DX11/WARP CI |
| R4 | Matched product-shaped performance workloads, startup/interaction/idle/RSS/GPU/frame/churn measurements and environment metadata | Runnable same-contract Kael/GPUI Kit applications; raw repeated measurements; explicit power/wakeup capture and fair comparison gates | All hundred schema-3 hosted captures at `b11ed50` pass the unchanged workload/activity gates and all 800 geometry boundaries at an equally constrained 1100×645, scale 1. Both modes retain five repetitions per engine/contract. Independent source/raw validation and separate later physical navigation/cache diagnostics are linked in the October 17 follow-up. These checkpoint-specific diagnostics establish no overall ranking. Latest-source cross-framework runs, controlled power and comparative presentation remain |
| R5 | Built-in rendering parity for translucent effects, clips, gradients, text, resize and device loss | Pixel/runtime matrix on native Metal, DX11, Blade and browser engines; no silent skip treated as pass | Local Metal/Blade/SwiftShader proof plus DX11/WARP and Linux llvmpipe CI; fresh text positioning, GTK and native populated-frame checks remain |
| R6 | Full logical accessibility for virtualized controls while viewport-only rendering remains | Offscreen-node or active-descendant semantics plus native assistive-technology exploration/actions; stable IDs/focus and large-model mounting tests | Actual macOS Editor Unicode/selection/geometry/reveal/edit/clipboard/composition/guard protocol passed with pasteboard restoration; browser mounted-priority projection passes wide/compact scale runtime. Native Windows 2022/2025, Linux X11 and GTK full Unicode protocol clients pass at their recorded checkpoints; screen-reader exploration and physical IME remain |
| R7 | Idle accessibility actions wake every native window correctly | Platform-thread/lifecycle/action-queue tests plus idle action execution on macOS, Windows and Linux AT adapters | macOS idle/protocol proof; Windows 2022/2025 and the full Linux native matrix pass at `21b9fa7`. Focused Linux jobs at `feb3f8b` and `92d0cd7` pass both X11/GTK4 outline and full Unicode protocols, strict graphs and clipboard bounds. The earlier `b80bbb2` native outline-focus failure remains intermittent and unresolved; diagnostic visibility is not a production lifecycle fix. Later shared cache/control changes require fresh platform execution |
| R8 | Unified memory policy for decoded images, pending work, completed default assets, scratch and atlases | Checked byte/count admission before expensive work; pressure shedding and retained-owner/cancellation tests on each backend | Metal, native Windows DX11/WARP, Linux Blade/GTK4 and mandatory browser atlas pressure/lifetime pixels pass. Large-tree outgoing-model callback retention is reproduced and fixed with weak-reference tests and six matched native runs (66% lower median peak RSS). Snapshot capacity reservation has a separate six-run latency diagnostic with unchanged peak RSS. Unified cross-platform lifecycle acceptance remains |
| R9 | Reusable virtual filesystem explorer with cached sort keys, lazy async loading, context actions and drag/drop | Visible explorer example and UI-driven tests; bounded model/cache/pending state, generations/cancellation and stale-response tests | Actual native keyboard move/reload and fresh pointer readme-to-Archive move pass; disk oracle verifies source removal and destination bytes. Fresh complete glyph pixels and hidden-file toggle pass |
| R10 | Persistent movable docking/tab groups, nested splits, floating panes and structured property inspectors | Runnable workspace; pointer/keyboard rearrangement, float/redock, persistence restore and editing/undo tests | Fresh integrated native pointer tab/edge split movement, float/move/resize/redock, keyboard edge docking, floating/nested save/reset/restore, Unicode inspector Undo/Redo and live preview pixels pass |
| R11 | Rich document and remote-data workflows build on current editors/grids | Real documents exercise selection, IME, undo/redo; remote-data editing/clipboard/frozen-pane/accessibility tests and sustained workloads | Actual native formatting/Undo, Unicode remote edit/sort identity, atomic 2×2 TSV paste/Undo/Redo, frozen-row vertical scrolling and populated pixels pass. Native horizontal keyboard reveal/frozen-column pixels also pass; physical input-source IME, nonzero horizontal wheel delivery and sustained performance remain |
| R12 | Consumer docs, examples, capability truth and platform CI cover the finished APIs | Native/browser consumer builds, working examples, strict lints/docs, CI evidence and no outdated roadmap claims for implemented features | Open |

The shader design's later built-in-blur migration is a separate architecture
effort; it must not remove the existing visual correctness tests. Any scope
interpretation or blocked runtime requirement remains visible here instead of
being deleted from the review.

## Evidence tracks

- `2026-10-02-shader-completion.md`: GPU APIs, backends, pixels, ownership.
- `2026-10-02-memory-accessibility-completion.md`: admission, cancellation, pressure.
- `2026-10-02-components-completion.md`: filesystem and workspace interaction.
- `2026-10-02-gpui-kit-comparison.md`: pinned comparison and measured workloads.

The current acceptance evidence is summarized in
[the October 3 follow-up](2026-10-03-native-quality-followup.md). Historical
checkpoints remain in their evidence tracks; later fixes require fresh CI.

The maintained comparison workflow builds both engines with and without frame
instrumentation and preserves one hundred navigation/editor/data/tree/workspace
runs. The earlier sixty-run CI checkpoint succeeded, but predates the CoreText
fractional-glyph correction and cannot establish the corrected framework's
visual/performance ranking. The later eighty completed runs identify Editor CPU
and tree memory costs, with incomplete viewport/presentation/power evidence.
Docking's constrained-window regression now passes locally with its original
oracles. Fresh complete runs are required after input, lifetime and client fixes.

The unlocked Mac now supplies actual displayed Metal frames, model-only idle
wake-up, native filesystem pointer movement and document/data interactions.
The CoreText fractional raster bounds defect is corrected and complete fresh
application text is visible. Native Editor clipboard and NSTextInputClient
composition acceptance are green. A protocol-driven composition check does
not establish physical input-source behavior or screen-reader exploration.

Windows UIA focus, Linux AT-SPI wire changes, GTK shedding and native renderer
presentation gates require fresh hosted execution. Full shader, resource
pressure, controlled power and cross-platform acceptance requirements remain
visible until their corresponding runtime proofs pass. No compile check or
historical artifact waives these requirements.

The hundred-run [hosted comparison](https://github.com/Augani/kael/actions/runs/37144285572)
completes successfully. Its [preserved diagnostic](2026-10-03-hosted-100-run-diagnostic.json)
revalidates every raw result against the exact checkpoint collector and retains
binary/raw hashes and five repetitions per engine/mode/contract. This predates
schema 3 and cannot satisfy the new geometry/frame-activity gates. Editor and
data CPU costs remain concrete profiling targets; the results establish no
overall superiority, comparative display latency or physical power advantage.

Native `f81adaf` CI passes Windows outline selection and Linux X11 Unicode text.
Windows multiline text omits ValuePattern after duplicate root values were
removed; its provider now constructs the value on request from the retained
TextPattern runs. Read-only and disabled mutation guards remain mandatory.
Linux restored rows require refreshed asynchronous interface discovery, and GTK
failure artifacts now include bounded owned-node and tree-publication traces.
These new changes require fresh hosted execution. The sleeping local display
rejects visible painting/presentation; a separately verified AppKit client-size
correction does not waive that environment requirement.

The later six-run Editor CPU diagnostic and accepted native tree pair are
recorded in the October 3 follow-up with raw/source/binary hashes and explicit
shared-desktop limits. The GTK editor now passes complete Unicode reads,
selection, geometry, Copy, Cut and atomic history. The asynchronous bounded
clipboard correction subsequently passes the full native Paste/edit/guard
protocol at `04799df`; its complete focused job still needs to pass after the
lint graph and X11 startup diagnostics are corrected.
The local display has returned to sleep, so its fresh macOS protocol rerun is
excluded and does not replace earlier complete physical evidence.

The [October 17 follow-up](2026-10-17-benchmark-quality-followup.md) records the
completed `21b9fa7` platform matrix and later focused Linux jobs. Both Windows
versions, Linux and browser jobs pass; hosted macOS drawable presentation still
fails its mandatory gate. Historical environment statements above describe
their original captures. The physical display is awake for the new sizing
validation: all eight navigation boundaries match 1100×760 at scale 2, with
active/churn frame activity on both engines. The complete schema-3 hosted rerun
at `b11ed50` now passes all hundred captures. Its binaries predate the subsequent image
cache, display-link and shared control improvements; those changes require
separate evidence. Following the user's direction, further profiling prioritizes
general desktop workloads rather than additional editor-specific optimization.

The subsequent clean `ac7811f` native navigation diagnostic contains three
alternating pairs, with all schema-3/original gates and all eight boundaries
matching 1100×760 at scale 2. It shows lower idle CPU, similar active CPU, and
higher churn CPU and sampled peak RSS for Kael on this shared Mac. It supplies
neither an overall ranking nor controlled power/comparative presentation proof.
The October 17 follow-up preserves raw and binary hashes, measurement scope,
and separate sampling profiles. Later measured Select reveal, disabled submenu
ownership and cached semantic reparenting fixes pass their focused regressions,
the full core/UI graphs and a fresh 59-test physical Metal capture; those fixes
are absent from both frozen comparison builds.

The next frozen source `9227207` passes 2,162 core tests, all 497 UI tests,
strict native Clippy, 59 required physical Metal GPU checks and 60 actual
completed-and-presented smoke frames. The October 17 follow-up links exact
source/log/image identities. Shared styled-label font preservation, a macOS
BOM attributed-string crash and Select/Combobox action/focus/search invariants
are corrected. Physical screen-reader and input-source IME acceptance remains.
A separate three-pair navigation experiment isolates a bounded 128-handle
CoreText font cache: active/churn CPU medians fall 17.4%/30.5%, sampled peak RSS
rises 0.80 MiB, draw p99 falls 5.0% and CPU submission p99 rises 194→624 µs.
This is a workload-specific tradeoff, with no overall or display-latency ranking.

The next dependency checkpoints (`02e341f`, `d40166f`) replace vulnerable
TLS/QUIC/ZIP releases, route published Kael consumers through the licensed
`kael_rasterlottie` fork, and retain Office name/admission checks. Native fork,
browser compatibility, networking, Office, lint and Wasm consumer evidence is
linked in the October 17 follow-up. The recorded root scan has zero known
vulnerabilities, while strict audit still denies six unmaintained D-Bus warnings;
no new ignore is added. The new 39-package preflight and latest platform matrix
remain required. ZIP dependency compatibility does not close the newly identified
archive expansion, image resize/scratch and retained-image admission gaps.

The next animation checkpoint (`83190dc`, `90743b8`) closes those scoped admission
gaps with finite archive/image policies, a 256 MiB retained-frame batch cap and
adaptive prefetch/cache planning. Zero admission suppresses rendering tasks and
recurring animation wakes; restoring capacity resumes elapsed playback. Genuine
small-allocation regressions pass alongside 134 fork native tests, 15 actual
Chrome cases, 2,178 full core tests and strict native/Wasm consumer checks. The
[animation validation](2026-10-17-animation-memory-validation.json) records the
two opt-in core probes, independent review, raw hashes and explicit remaining
metadata/decoder/model/worker/concurrent-task/global-budget boundaries.

All 39 release archives now build through fresh isolated preflight without a
registry upload. Independent audit matches every included Rust byte (928 files)
to immutable `90743b8`, checks licenses and registry aliases, and records actual
packaged VCS metadata without inferring dirty-state information. The earlier
`ac7811f` matrix finishes with both Windows jobs, browser and Linux runtime jobs
passing; hosted Metal presentation and Linux strict advisory audit still fail.
Those results predate the current fixes. Latest platform execution, controlled
power/comparative presentation and physical IME/screen-reader acceptance remain
open. General desktop forms, data, navigation, workspaces, media and shared
rendering remain the priority; Tree disclosure needs profiling before attribution.

The subsequent [animation task ownership checkpoint](2026-10-17-animation-task-ownership-validation.json)
owns pending element render tasks, cancels obsolete source/size/drop work and
rejects ready batches for stale geometry. Worker-held permits retain admission
until an executing synchronous frame returns; cancellation cannot preempt that
frame. A fixed two-worker limit bounds active element work while ready results
and per-element caches remain additional. All 22 focused regressions and 2,184
native core tests pass, with the two explicit diagnostic probes opt-in; strict
native all-target Clippy and the browser/Lottie Wasm compile also pass. The
record preserves exact source/log hashes and the real geometry RED. A separate
fresh 39-package preflight and archive audit now verify all included Rust bytes
against `b8aff24`; no package is uploaded. Browser ownership-runtime acceptance,
unified configurable memory accounting and cross-platform R8 lifecycle acceptance
remain open; the earlier animation validation record stays immutable.
