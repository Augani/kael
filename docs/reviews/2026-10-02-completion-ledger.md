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
| R4 | Matched product-shaped performance workloads, startup/interaction/idle/RSS/GPU/frame/churn measurements and environment metadata | Runnable same-contract Kael/GPUI Kit applications; raw repeated measurements; explicit power/wakeup capture and fair comparison gates | The [b411 comparison against GPUI Kit 0.7.3](2026-10-18-b411-gpui073-hosted-comparison.md) passes 100 original captures, 800 equal actual 1100×645 at scale 1 boundaries, 50 pairs and 20 independently rehashed executables. Four general desktop CPU/RSS and separate timing tables lead; table/tree gaps, unequal operation totals and component semantics remain explicit. Later-source measurements, power, comparative presentation and an overall ranking remain open |
| R5 | Built-in rendering parity for translucent effects, clips, gradients, text, resize and device loss | Pixel/runtime matrix on native Metal, DX11, Blade and browser engines; no silent skip treated as pass | Local Metal/Blade/SwiftShader proof plus DX11/WARP and Linux llvmpipe CI; fresh text positioning, GTK and native populated-frame checks remain |
| R6 | Full logical accessibility for virtualized controls while viewport-only rendering remains | Offscreen-node or active-descendant semantics plus native assistive-technology exploration/actions; stable IDs/focus and large-model mounting tests | Actual macOS Editor Unicode/selection/geometry/reveal/edit/clipboard/composition/guard protocol passed with pasteboard restoration; browser mounted-priority projection passes wide/compact scale runtime. Native Windows 2022/2025, Linux X11 and GTK full Unicode protocol clients pass at their recorded checkpoints; screen-reader exploration and physical IME remain |
| R7 | Idle accessibility actions wake every native window correctly | Platform-thread/lifecycle/action-queue tests plus idle action execution on macOS, Windows and Linux AT adapters | macOS idle/protocol proof; Windows 2022/2025 and the full Linux native matrix pass at `21b9fa7`. Focused Linux jobs at `feb3f8b` and `92d0cd7` pass both X11/GTK4 outline and full Unicode protocols, strict graphs and clipboard bounds. The earlier `b80bbb2` native outline-focus failure remains intermittent and unresolved; diagnostic visibility is not a production lifecycle fix. Later shared cache/control changes require fresh platform execution |
| R8 | Unified memory policy for decoded images, pending work, completed default assets, scratch and atlases | Checked byte/count admission before expensive work; pressure shedding and retained-owner/cancellation tests on each backend | The [static-image working admission](2026-10-18-static-image-working-validation.json) covers built-in native pixels and RGBA conversion, passes 2,251 enabled core/all 565 UI tests and 80 fixed CPU gates, and has current local native proof. The 1,280 MiB accounting domain preserves permitted high-bit-depth images; codec scratch, encoded input, animations, retained outputs and active/model bytes remain separate. Unified pressure and cross-platform lifecycle acceptance remain open |
| R9 | Reusable virtual filesystem explorer with cached sort keys, lazy async loading, context actions and drag/drop | Visible explorer example and UI-driven tests; bounded model/cache/pending state, generations/cancellation and stale-response tests | Actual native keyboard move/reload and fresh pointer readme-to-Archive move pass; disk oracle verifies source removal and destination bytes. Fresh complete glyph pixels and hidden-file toggle pass |
| R10 | Persistent movable docking/tab groups, nested splits, floating panes and structured property inspectors | Runnable workspace; pointer/keyboard rearrangement, float/redock, persistence restore and editing/undo tests | Fresh integrated native pointer tab/edge split movement, float/move/resize/redock, keyboard edge docking, floating/nested save/reset/restore, Unicode inspector Undo/Redo and live preview pixels pass |
| R11 | Rich document and remote-data workflows build on current editors/grids | Real documents exercise selection, IME, undo/redo; remote-data editing/clipboard/frozen-pane/accessibility tests and sustained workloads | Actual native formatting/Undo, Unicode remote edit/sort identity, atomic 2×2 TSV paste/Undo/Redo, frozen-row vertical scrolling and populated pixels pass. Native horizontal keyboard reveal/frozen-column pixels also pass; physical input-source IME, nonzero horizontal wheel delivery and sustained performance remain |
| R12 | Consumer docs, examples, capability truth and platform CI cover the finished APIs | Native/browser consumer builds, working examples, strict lints/docs, CI evidence and no outdated roadmap claims for implemented features | The [fresh 39-package checkpoint](2026-10-18-image-grid-package-validation.json) binds committed ce39eed source and all 931 included Rust files; all 39 extracted builds and independent archive audit pass. The image/Grid source has separate local Metal proof. The [b411 platform cohort](2026-10-18-platform-b411-validation.json) passes Windows 2022/2025, Linux runtime and browser jobs; hosted native presentation, three strict advisories, later-source platform execution and physical workflows remain open |

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

The later [tree stage checkpoint](2026-10-17-tree-stage-optimization-validation.json)
preserves the full 100,025-item logical hierarchy. Release process pairs show
fully expanded model construction using 50.1% less calling-thread CPU and
logical comparison using 15.1–17.5% less, with separate reductions in requested
Rust allocation. Collapse construction rises 3.2% with mixed pair direction;
full accessibility preparation remains costly and does not improve. A prior
candidate with a 78.7% collapse regression is rejected and retained as evidence.
The exact final sources pass 2,188 core tests, all 501 UI tests, strict native
lint and browser compilation. These headless API stages do not close R4 native
application comparisons, R6 assistive-technology acceptance or R8 unified memory
policy. General desktop data, navigation, forms, workspaces, media and platform
services continue to guide the work.

The final local `6b2ae11` sources, including the separate Linux notification
wire correction, pass another fresh 39-package preflight. Independent audit
matches all 928 included Rust files to immutable source and verifies licenses,
fork contents and registry aliases without uploading packages. The old
`e219439` hosted checkpoint separately passes browser, Linux runtime and Windows
2022 jobs; Metal presentation and strict dependency audit fail, and Windows 2025
and the matched application comparison remain active at capture. New local
commits await those frozen runs before the next push and fresh platform cycle.

The later [credential migration](2026-10-17-secret-service5-migration-validation.json)
removes the old Zbus 4 family without changing credential consumer call bodies.
Eleven actual encrypted private-bus protocol cases and Linux cross-compilation/
strict Clippy pass; no human keyring is accessed. Known vulnerabilities remain
zero, while denied unmaintained package copies fall from six to three at the
same database revision. Strict audit still fails and no ignore is added.

The [idle prepared-animation cache](2026-10-17-animation-idle-cache-validation.json)
now admits decoded RGBA8 payload/count per thread and process-wide, including
lazy growth and resource-owned quota release. All 28 focused core tests, 136
native fork tests and 16 Chrome fork cases pass. Separate exact production
global/TLS and small-budget race/lifetime probes verify admission; browser core
ownership runtime and unified active/model/scratch/output budgets remain open.
Warm cache synchronization adds approximately 24–26 ns per hit/reinsert in two
tiny fixtures; no stable application performance claim follows.

The [general desktop data diagnostic](2026-10-17-compact-data-storage-validation.json)
keeps the public string API and canonical benchmark models unchanged. Opt-in
compact domain records reduce settled requested Rust payload by 54,121,472 bytes
in the same real grid with unchanged caches, mounting, semantics and source-call
counts. Source identity, cancellation, stale replies, long Unicode ownership
and strong dataset release pass independent checks. This is headless allocation
evidence, not native RSS or application performance. A suspected empty-default
allocation yields no finding and no patch.

Combined `738695f` validation passes 2,194 core tests, all 501 UI tests, strict
native lint, browser/Lottie compilation and another fresh 39-package preflight.
All 928 included Rust files match immutable source. The separately repaired
benchmark lock at `738ef10` preserves pinned common dependencies and compiles
all five applications for both engines and both timing modes. Fresh runtime
comparisons and the remaining R1–R12 acceptance scopes remain required.

Fresh [physical presentation evidence](2026-10-17-native-presentation-recovery-validation.json)
at `95ebb78` supplies 59 positive completed/presented Kael frames and all 59
mandatory GPU tests, with separate positive Apple controls. The renderer smoke's
compact layout correction at `738695f` has a fresh complete six-tile capture and
one positive presented frame. Console/display readiness is observed; the absent
lock key remains unknown. These are correctness results, without comparative
latency or power claims. The [final old-source platform run](2026-10-17-platform-e219-final.json)
passes both Windows jobs, browser and Linux runtime while hosted presentation
and strict advisory audit fail; the overall matrix is still not green.

The second [hundred-capture hosted comparison](2026-10-17-hosted-e219-comparison.json)
at `e219439` completes all original gates and 800 equal geometry boundaries.
Unlike the earlier upload, all twenty measured application executables are
preserved and independently rehashed against every capture. Per-contract,
per-mode results keep data/tree CPU/RSS gaps visible alongside lower Kael
navigation CPU. The host has no display inventory; power, comparative
presentation and overall superiority remain unproved. Its pinned 0.7.0 package
snapshot is historical; a new exact-tag 0.7.3 candidate is a separate validation.

The [native-only records example](2026-10-17-compact-records-example-checkpoint.json)
passes compile, two identity/ownership tests, strict lint and several actual
native interactions at `a3add69`. Its first screenshot exposes short-list
placement and constrained table-body defects; visual acceptance remains open.
A fresh 39-package preflight verifies all 928 included Rust files against that
source, separately from the example's excluded source and compile evidence.

The [shared-control correction](2026-10-17-shared-control-validation.json) now
fixes the records example's leading-edge and constrained-footer defects, as well
as independent table wheel axes, Dropdown actions/focus/session ownership and
scrollbar handlers with live ranges and stable bound identities. At `d766b5d`,
all 2,198 core tests and 537 UI tests pass; the same two core diagnostics remain
opt-in in that broad invocation. Strict all-target native lint, Wasm, the two
example tests/build, all twenty GPUI Kit 0.7.3/Kael benchmark compile targets,
formatting and docs pass with frozen source. Fresh packaging builds all 39
archives and independently checks 929 Rust files (23,392,950 bytes) against Git;
compressed archives total 6,052,424 bytes, with no registry upload. An explicit
native display-link probe separately passes 128 pause/resume cycles while
retaining one native object, with no multi-monitor or power claim.

The later [animation output ownership checkpoint](2026-10-17-animation-output-ownership-validation.json)
transfers the completed main pixmap into its returned frame. A real 16×8 frame
uses one 512-byte allocation with 512 requested/peak-live payload bytes, versus
two allocations and 1,024 bytes before; pixels and post-drop release match.
Adopted source passes 137 native fork tests, strict lint and 16 actual Chrome
cases. Layer/image/effect scratch, models, ready/cache ownership and concurrent
callers remain additional. This is requested Rust payload evidence, without
native RSS, application-performance or unified R8 closure. The existing three
strictly denied Zbus-family advisories remain open without new ignores.

The subsequent [GPUI Kit 0.7.3 release/runtime checkpoint](2026-10-18-gpui073-local-runtime-validation.json)
independently verifies twenty actual release binaries, compiler inventories,
3,009 Kael source files and all 3,662 competitor files against their immutable
Git revisions. A preserved 38-file package archive has an additional verified
full-repository supplement. All twenty native oracle attempts pass, including
160 equal actual geometry boundaries at 1100×760 logical pixels, scale 2.
Four navigation cases use the full original duration; sixteen component cases
are custom quick smokes. The full hundred-capture hosted collection remains
running and unaudited at this record. No performance, presentation, power or
overall ranking is inferred, and later implementation changes need separate
validation.

The [October 18 track/animation correction](2026-10-18-track-and-animation-validation.json)
adds actual full-parent table scrollbar sizing, preserves primitive defaults and
fixes finite Choker arithmetic overflow. Real pointer/keyboard scrolling and
native zoom growth pass. The later [asset lifecycle change](2026-10-18-asset-lifecycle-validation.json)
delivers ready drawing results independently of completed LRU retention, keeps
pending consumers weak and retires unmounted metadata. Cached-view reuse,
namespaces, pressure, removal/generations, reentrant hooks and zero ready-hit
notifications have 54 unique focused passes. Broader primary validation passes
2,216 core and all 538 UI tests plus strict native lint, Wasm, examples, twenty
benchmark compile targets, parser/provenance, formatting and docs. The two
existing opt-in core probes remain separate. Active/error ownership and global
memory accounting remain explicit R8 boundaries; fresh packaging/native/platform
proof remains separate from this headless combined pass.

The [completed `0d15aff` platform run](2026-10-18-platform-0d15-final.json)
passes Windows, Linux runtime and browser jobs but retains Mac positive-display
and strict three-advisory failures. All ten archives and 232 extracted files are
rehash-verified. WebKit supplies no decoded capture frames, trusted capture
pickers are not automated, and browser latency budgets are disabled in software
fallback. Same-job independent Apple controls are added for later diagnosis
without changing the required Kael gate. Neither the old platform run nor its
later diagnostics establish current complete R12 acceptance.

The [current physical Metal check](2026-10-18-native-desktop-quality-validation.json)
then passes on the same nineteen-file source: 59 GPU invocations cover 58 distinct
tests, and the window reports 58 completed frames with positive presentation
timestamps. Requested client sizing, resize, a 1,250 ms idle model wake and the
complete six-tile readback pass. An initial recorder expected seven GPU groups
instead of six; its failed receipt remains preserved, and the continuation reused
the successful GPU logs before building and running the window once. The older
hosted Mac cohort has the same 59/58 distinction and still fails presentation.
Fresh packaging and hosted acceptance remain separate.

The [current nineteen-file package preflight](2026-10-18-package-source-validation.json)
then builds all 39 extracted archives at `b279795`. Independent audit matches all
929 included Rust files (23,464,432 bytes) to that immutable Git source; compressed
archives total 6,067,354 bytes. License/notice bytes, normalized dependency
aliases, ZIP 9 and fork provenance pass. All archived VCS IDs name `b279795`;
the missing dirty flag remains unknown metadata. No crates are uploaded, and
hosted presentation, dependency policy and platform acceptance stay separate.

The later [checked/animation loading checkpoint](2026-10-18-asset-admission-validation.json)
fixes two real reentry regressions before source cloning or loader callbacks can
publish an unfinished task. Seven new tests pass; removing only the panic wake
produces the expected generation0-versus1 failure before effect flushing.
Two global built-in animation-loader permits cover I/O through poster creation,
with separate playback admission and unchanged synchronous constructors. The
current primary passes all 2,232 core and 538 UI tests with default parallelism;
the same two opt-in native probes remain separate. Native fork140 plus public
policy1, strict lint, Wasm, examples, twenty benchmark compile targets,
provenance, parser tests, formatting and docs pass with all 3,014 source inputs
unchanged. Private icons-enabled core2233 is an explicitly different feature
scope. Active outputs and queued/model/scratch bytes remain additional; fresh
package and current platform/comparison acceptance are separate.

The [fresh admission-source package preflight](2026-10-18-admission-package-source-validation.json)
then builds all 39 extracted archives at `4bb5a44`. All 930 included Rust files
(23,517,476 bytes), including the maintained public loader-policy test, match
that immutable Git commit. Compressed archives total 6,077,870 bytes. License,
manifest-alias, fork provenance and existing size policies pass; all 3,014 combined
validation inputs still match. Archived VCS IDs name `4bb5a44`, while absent dirty
flags remain unknown metadata. No crates are uploaded. Current hosted platform,
physical workflows and later-source performance remain separate acceptance work.

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

The separate [Lottie test portability fix](2026-10-18-lottie-test-portability-validation.json)
resolves the additional Wasm test compilation failure by matching the existing
production clock type. All eight affected native tests, strict core lint and
combined core/UI Wasm test compilation with Lottie pass. Production code and
test assertions are unchanged; this adds no browser-runtime or performance claim.

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
is claimed. Later local native presentation and committed-source packaging pass at their separately recorded checkpoints; hosted acceptance remains open.

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
presentation counts separate. The later ce39eed package checkpoint passes all 39 extracted builds and931 included Rust-file comparisons; hosted acceptance remains open.

The [b411 hosted comparison](2026-10-18-b411-gpui073-hosted-comparison.md)
retains all 100 accepted captures with equal actual 1100×645 at scale 1 geometry. Four
general desktop workloads lead: Kael navigation active CPU is 9.95% versus 12.64%;
table and tree CPU/RSS remain higher, and docking is close. Unequal operation
counts, full logical accessibility work and native component differences remain
explicit; no overall winner or current-source performance is claimed.

The [native wheel attempt](2026-10-18-native-wheel-validation.json) posts no
scroll events: the ownership guard refuses a child in a separate process group.
Its failed cleanup remains preserved; a later exact-PID/group absence check and
complete source rehash permit safe resumption. R11 wheel delivery stays open.
