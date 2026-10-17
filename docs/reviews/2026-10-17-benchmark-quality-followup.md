# Benchmark and native quality evidence follow-up

Reviewed on 2026-10-17. The historical matrix and focused runs were executed
on 2026-10-03; fresh October 17 results are distinguished below. These establish
the stated checkpoint results; they do not support an overall Kael/GPUI Kit
performance ranking or a fully passing platform matrix.

The focused Linux checkpoints retain both successful Unicode coverage and the
intermittent outline-focus failure:

| Checkpoint and CI run | X11 outline | GTK4 outline | X11 Unicode Editor | GTK4 Unicode Editor | Strict graphs and clipboard bounds |
| --- | --- | --- | --- | --- | --- |
| `feb3f8b`, [37159120165](https://github.com/Augani/kael/actions/runs/37159120165/job/111308669935) | PASS | PASS | PASS | PASS | PASS |
| `b80bbb2`, [37157978853](https://github.com/Augani/kael/actions/runs/37157978853/job/111305329512) | FAIL: native focus | FAIL: native focus | PASS | PASS | PASS |

At `feb3f8b`, both outline clients verify 100,025 logical nodes, 25 project
roots, 4,000 children per project, idle selection, native focus, disclosure and
stable service/path identity. Both Editor clients verify the complete
105,391-byte fixture: 63,390 Unicode scalars, 66,983 UTF-16 units and 1,001
logical lines. Their success markers include selection, geometry, hit testing,
word navigation, EOF, reveal, clipboard, editing, atomic Undo, read-only,
disabled and replacement behavior. The selected GTK core graph and native UI
graph pass separate Clippy checks with `-D warnings`. Both clipboard-stream
tests pass: exact byte-limit preservation across chunks and rejection of one
byte over the limit. Preserved evidence is in
`.artifacts/kael-ci-feb3f8b-focused-linux.log` and
`.artifacts/ci-feb3f8b-focused-linux/`.

At `b80bbb2`, both full Unicode protocols pass, but both outline clients fail
the unchanged native `FOCUSED` assertion after the application records selection
of row 100024. Their diagnostic states report `focusable=False selected=False`.
The application traces retain host focus and an active descendant. The later
passing run does not resolve this intermittency: `feb3f8b` adds diagnostic
visibility, rather than a demonstrated production cache-lifecycle correction.
A retained libatspi wrapper versus current native-object lifecycle remains an
investigation, with no wire-state fallback accepted in place of native focus.
The failing client logs and application traces remain in
`.artifacts/ci-b80bbb2-focused-linux/`.

The full [`21b9fa7` run, 37157644901](https://github.com/Augani/kael/actions/runs/37157644901)
finishes with the following results:

| Job | Result and scope |
| --- | --- |
| [Windows 2022](https://github.com/Augani/kael/actions/runs/37157644901/job/111304313677) | PASS: Direct3D and generated-project runtime proofs, WebView, native UIA protocols and verified MSI. |
| [Windows 2025 / VS2026](https://github.com/Augani/kael/actions/runs/37157644901/job/111304313658) | PASS: complete Windows compile/lint graph and native runtime/UIA/WebView/MSI checks. |
| [Linux native quality](https://github.com/Augani/kael/actions/runs/37157644901/job/111304313704) | PASS. |
| [Linux renderer / WebView runtime](https://github.com/Augani/kael/actions/runs/37157644901/job/111304313587) | PASS: Blade, generated projects, X11/GTK4 outline and Unicode protocols, GTK4 atlas contracts, and Wayland/XWayland WebViews. |
| [Browser](https://github.com/Augani/kael/actions/runs/37157644901/job/111304313685) | PASS: the required WebAssembly / WebGL2 browser job. |
| [macOS native quality](https://github.com/Augani/kael/actions/runs/37157644901/job/111304313647) | FAIL: mandatory Metal drawable-presentation gate. |

The macOS failure is specific and remains a failure. The Metal runtime log
records `presented_time=0` for frames 0–29 and ends with
`NATIVE_RENDERER_SMOKE_FAIL: no drawable presentation callback within two seconds`.
Native workspace verification, Blade checking, WKWebView, macOS outline/text
protocols and the Metal-backed browser performance step pass in that job; those
passes do not substitute for presentation acceptance. The final platform
readiness job therefore also fails. CPU submission or GPU completion is not
counted as a drawable presentation.

The schema-3 matched desktop comparison at
[`04799df`, run 37156492270](https://github.com/Augani/kael/actions/runs/37156492270/job/111300825061)
is rejected at its first navigation pair. Actual phase-boundary client
viewports are 1024×642 for Kael and 1024×645 for GPUI Kit, both at scale 1.
The collector raises `native viewport or display scale changed between captures`.
The two raw captures and process samples remain in
`.artifacts/ci-04799df-schema3-comparison/native-navigation-detail/`, and the
rejection remains in `measurement.log`. Their timing, memory and wakeup counters
are rejected comparison data; no aggregate ranking is derived from them.

The same artifact contains separate compact workspace smoke runs. Both engines
pass their four state/content/persistence phase oracles in both instrumentation
modes, but every run reports `quick=true`. Their actual viewports also differ:
Kael is 880×640 and GPUI Kit is 880×641 at scale 1. These are compact regression
checks, not accepted matched measurements; they do not repair or complete the
rejected navigation collection.

The sizing/timing correction at `b11ed50` requests the displayed client size
through each engine's public resize API before measured phases. Startup, warmup,
resize and validation durations are excluded from comparative CPU duration
arrays. First render/submission remain diagnostic startup values because their
initial geometry is not paired.

The [local native sizing validation](2026-10-17-native-sizing-validation.json)
passes one complete navigation capture per engine. All eight boundaries match
1100×760 at scale 2; all measured activity and original workload gates pass.
Kael has 684 active and 663 churn renders/draws/submissions; GPUI Kit has 677
and 649. Frozen binary identities, raw hashes, phase activation and geometry
are preserved. These binaries were built at `b11ed50`; later collection checkout
metadata is explicitly distinguished from their build source. One pair on a
shared desktop establishes no overall performance or power ranking.

The [full hosted rerun](https://github.com/Augani/kael/actions/runs/39057746656)
also uses `b11ed50` and retains the complete five-contract, two-mode,
five-repetition-per-engine suite. The complete job now passes all hundred
captures. Independent raw-result revalidation and contract-specific results
are recorded below. The binaries predate the subsequent framework changes;
this run verifies the sizing correction at its own checkpoint.

The [independently revalidated hundred-run diagnostic](2026-10-17-hosted-desktop-comparison.json)
checks the exact unmodified `b11ed50` collector, all original workload and
activity gates, all 800 phase geometry boundaries and all 50 cross-engine pairs.
Every actual viewport is 1100×645 at scale 1: the virtual runner constrains the
requested height equally. All ten environments identify the same clean source,
and all 2,864 eligible file hashes match the frozen Git blobs. The artifact
preserves raw logs/reports and helper executables; application executable bytes
were omitted by its frozen upload paths, so their 20 SHA identities are checked
for collector-recorded consistency rather than independently rehashed.

The following general-desktop medians use five captures per engine with optional
frame timing disabled. Each cell gives **Kael / GPUI Kit**:

| Contract | Active process CPU | Churn process CPU | Stable sampled peak RSS, MiB |
| --- | ---: | ---: | ---: |
| Navigation/detail | 14.52% / 13.71% | 10.51% / 10.54% | 92.45 / 78.86 |
| Data table | 30.07% / 23.48% | 25.41% / 17.39% | 204.98 / 136.42 |
| Virtual tree | 61.25% / 14.58% | 30.54% / 9.27% | 1,094.73 / 650.89 |
| Docking workspace | 17.48% / 18.93% | 11.28% / 11.93% | 81.80 / 79.92 |

At this checkpoint, tree and data CPU/RSS remain the clearest general-desktop
profiling targets. Workspace active/churn CPU is lower for Kael in this mode,
with slightly higher sampled RSS. Kael idle-after CPU is lower in all four
contracts. The tree contract includes Kael's complete logical accessibility
projection versus the competitor's mounted rows; this implementation difference
stays visible rather than dropping semantics to equalize cost. The report also
retains the existing document contract and all instrumented results, including
CPU draw/submission tails. Modes remain separate; no pooled result is used.

This is one three-core, 7 GiB Apple M1 virtual runner with an empty display
inventory. Accepted foreground requests and active repaint checks establish
neither monitor presentation nor visible smoothness. Stable sampled RSS excludes
startup and oracle validation; whole-phase CPU is not normalized per operation.
No comparative GPU/compositor completion or controlled power result is available.
The later shared controls and native font cache are absent from these binaries.

A fresh [focused Linux run at `92d0cd7`](https://github.com/Augani/kael/actions/runs/39057690389)
passes both X11/GTK4 outline and full Unicode protocols, separate strict graphs,
and both clipboard byte-bound tests on October 17. The earlier intermittent
focus failures remain preserved; this checkpoint adds diagnostics rather than
proving a production lifecycle fix for that earlier failure.

Subsequent implementation work prioritizes shared desktop needs:

- `639fc6b`: image caches within their entry/byte limits skip eviction worklist
  allocation and sorting. All 17 focused headless tests pass, including exact
  limits, ready completion promotion, protected LRU eviction and real HTTP
  cancellation. A deterministic test performs 2,048 lookups with 65 completed
  entries and zero eviction worklists. This does not establish an application
  RSS or CPU percentage improvement.
- `24d4efe`: dense bar sparklines fit their canvas, finite extreme/constant
  series produce finite coordinates, and constant-value accessibility summaries
  report their actual extrema. Three focused tests pass. The range is shared
  between semantics and painting instead of scanning the data twice.
- `e392ba2`: macOS windows retain and resume their stopped display link rather
  than recreating it on every idle transition. Four ordinary lifecycle tests
  and six test-platform window tests pass, including 32 model-only wake cycles.
  The explicitly executed native probe passes one test with zero ignored:
  128 stop/start cycles use one native object, including one other-display
  retarget, and finish stopped. The existing final-release safety workaround
  remains.
- `06c3f39`: Select handles advertised native Click/Expand/Collapse and option
  actions, maintains one focused control with a valid active descendant, and
  rejects duplicate or stale selection commits. Deferred popup nodes retain
  their semantic parent and survive cached scene replay with their handlers.
  Both view and element caches use stable scalar allocator scopes; fresh
  siblings cannot overwrite replayed anonymous IDs. Retired anonymous paths are
  pruned after tree rebuild. Eight Select tests, two Deferred tests, one role
  classification test and the cache matrix pass. The matrix covers both cache
  APIs, direct/deferred content, both sibling orders, independent native
  callbacks and 128 scope replacements retaining exactly four live anonymous
  entries. The action tests use headless window dispatch and native-tree export;
  they do not establish screen-reader exploration.
- `c6d7258`: virtual DataTable queries admit at most eight pending pages by
  default, with configurable admission and separate completed-cache limits.
  Request planning also stops scanning the logical range when capacity fills;
  cached-page LRU touches retain their ascending order. All 21 focused tests
  pass, including 10,000 disjoint scroll ranges, a 100,000-page range with bounded
  planning visits, generation-safe stale settlement rejection and mounted
  completion/failure callbacks admitting the current viewport without a backlog.
  Application-owned tasks from older queries are not cancelled by the table;
  the API and consumer docs state that limitation explicitly.
- `417b791`: PopoverMenu items handle their advertised native Click action.
  Mouse-down timing and persistent menus' repeated actions are preserved.
  Enabled items share an activation claim with Escape/backdrop dismissal for
  the same rendered menu, so queued requests cannot activate it twice or act
  after those dismissals. Six focused tests pass, including disabled/inert
  items, delayed-unmount dismissal, handler retirement and owning-view release.
  Real missing-handler and dismissal callback failures are recorded before
  their fixes. The claim covers one rendered menu; a host that continues
  rendering a closing animation owns its closing/disabled state across frames.
- `02b2246` and `8c0de53`: nonempty in-memory appends reapply active sorting.
  The existing permutation preserves selection, active cell edits and row-menu
  identity without a second row-sized allocation. Unsaved input entities and
  content survive both appended and header-triggered sorts; still-painted menu
  callbacks resolve the current row before acting. Empty in-memory appends
  retain their completion notification and load-more reset without sorting;
  virtual appends remain inert. All 28 DataTable tests pass, including seven
  append/completion/identity regressions. Independent review caught and corrected
  the intermediate empty-completion and transient row-index regressions before
  branch publication.

The fresh [physical Metal validation](2026-10-17-native-metal-validation.json)
passes all 59 programmable-renderer, graph, offscreen scene, atlas and identity
tests with zero failed or ignored tests. The visible native smoke starts at
720×460 logical client size, scale 2, resumes from idle after a model-only notification,
and records 59 GPU-completed frames with actual compositor presentation
timestamps. The final scene has 6,067 text-probe pixels. Log, image, executable
and source-file hashes are preserved; the core cache files were frozen but
uncommitted during capture and are identified individually. Other correctness
builds could run concurrently, so this establishes correctness rather than
comparative latency, CPU or power performance. Hosted macOS presentation remains
a separate failing requirement.

The complete headless core graph passes 2,147 tests with zero failures and one
ignored opt-in native display-link probe. That probe is explicitly executed
separately with one pass and zero ignored tests, as recorded above.
The [shared desktop validation](2026-10-17-shared-desktop-validation.json) at
`8c0de53` also records all 471 UI tests passing, strict native all-target Clippy
with `-D warnings`, 21 collector regressions, source hashes and raw test logs.
The core/native source hashes match their earlier frozen captures. The 46-page
book passes its build and local-link/asset verification; workflow lint and Rust
formatting pass. The modified Kael benchmark consumer graph compiles with and
without frame timing. These checks do not replace fresh platform CI.

The editor-specific ASCII bidi candidate remains unapplied. Subsequent shared
cache, display-link and component changes are absent from the frozen benchmark
binaries and require their own validation.

The subsequent shared-control fixes preserve the failures that prompted them:

- `8d3f530`: Select reveals the highlighted option using measured child bounds,
  including wrapped group headings, inherited typography and all three control
  sizes. Opening at option 79, batched keyboard navigation, narrowing/widening
  search, stable semantic IDs and wheel scrolling pass nine focused tests.
  Two measured-geometry tests fail before the fix. The shared ScrollHandle now
  resolves requests after publishing current viewport metrics, preserves an
  unresolved request through empty content, and clears stale child bounds.
  Two scroll tests pass, including first paint, resize, padding and explicit
  bottom-request precedence. Ordinary redraws preserve wheel offsets.
- `60141bf`: cached view and element semantics rebase external root parents
  during cache hits, including deferred content. Internal parent links, action
  handlers and snapshot storage remain intact. Scalar allocator scopes preserve
  anonymous IDs through a subsequent fresh repaint; no-op paths reuse storage,
  and changed shared buffers use copy-on-write. Four independent reparenting
  variants fail before the fix; six focused tests pass afterward, including
  root transitions, retained old snapshots and bounded retired metadata.
- `9bae63b`: disabled submenus reject pointer/native activation and hide their
  previously expanded children. Enabled submenus retain their stored disclosure
  state. The submenu's semantic owner is its MenuItem, with unchanged row-sized
  bounds and independent child keyboard handling. Ten focused menu tests pass;
  two disabled-state cases fail before the fix. A separate hierarchy assertion
  reproduces the former sibling relation before ownership is corrected.

The complete second core graph passes 2,153 tests with zero failures and the
same opt-in native probe ignored. All 476 UI tests pass. Strict native all-target
Clippy passes after replacing one equivalent test assertion with `contains`.
These graphs overlap the focused results and are not unique aggregate counts.
Headless event dispatch and exported native trees do not establish screen-reader
exploration. These commits have separate source identities from the running
`ac7811f` platform matrix.

The [second physical Metal capture](2026-10-17-control-native-validation.json)
identifies the committed core source after these changes and again passes all
59 required GPU tests with zero skips. The visible window verifies its requested
720×460 logical client size, resumes from a model-only idle notification, and
records an actual completed-and-presented frame. Its final readback retains
6,067 text-probe pixels. Concurrent correctness builds were allowed; this is
correctness evidence and does not compare presentation latency or frame counts.

A [three-pair native navigation diagnostic](2026-10-17-current-navigation-diagnostic.json)
uses a clean frozen `ac7811f` worktree and the unchanged pinned competitor. All
six captures pass the original workload and schema-3 gates, with all eight
phase boundaries at 1100×760 logical pixels and scale 2. Owned compiler, test and
native-window workloads were paused during collection; other desktop activity
was uncontrolled and the recorded load averages are retained.

| Median diagnostic | Kael | GPUI Kit |
| --- | ---: | ---: |
| Idle-before process CPU | 0.050% | 0.911% |
| Active process CPU | 25.736% | 25.489% |
| Idle-after process CPU | 0.010% | 0.812% |
| Churn process CPU | 31.932% | 30.121% |
| Stable sampled peak RSS | 118.16 MiB | 110.08 MiB |
| Per-run draw p99, median | 5,684 µs | 5,453 µs |

This one navigation contract shows lower idle CPU, similar active CPU, and
higher churn CPU and sampled peak RSS for Kael. Time-paced runs have slightly
different operation/frame counts; CPU percentages are whole-phase diagnostics,
not normalized per-operation results. Draw spans measure CPU-side wall time,
not thread CPU time or display latency. Startup geometry is unpaired, and no
comparative compositor or controlled watts/joules measurement is established.
The three subsequent control/cache commits are absent from these binaries.
Separate sampling captures, excluded from this table, identify general UI text
shaping during navigation churn as a profiling lead rather than an established
optimization or a reason to expand editor-specific work.

The next source-frozen batch addresses text and form behavior shared by desktop
applications:

- `5811652`: adjacent styled label runs retain distinct font descriptors even
  when their colors match. The previous last-font comparison compared an ID
  with itself and could collapse regular, bold, italic and other font changes.
  Three independent construction/native shaping regressions fail before the
  fix and pass afterward, including wrapped text with a newline.
- `3e8a2d8`: a run beginning with a byte-order mark preserves its UTF-16 length
  when constructing the native string. The former UTF-8 constructor could strip
  that mark, misalign subsequent attributed ranges and crash CoreText even with
  font reuse disabled. Explicit UTF-16 construction is confined to this prefix
  case. Leading, interior and split marks, NUL and multibyte boundaries pass;
  nonpainting marks retain the existing empty-output behavior.
- `4da2a9c`: native text shaping retains at most 128 sized CoreText font handles,
  keyed by effective font identity after optical substitution and exact size
  bits. Invalid sizes retain their existing factory path. Eviction and owner
  teardown release the handles; text and layouts are not retained. The cap is
  a handle count, not a native-byte budget. In the 48-case script/font/size
  fixture, 24 supported cases retain exact glyphs, positions, indices, widths
  and metrics; 24 baseline-unsupported cases retain their existing `None` result.
- `a72e29b`: Combobox executes its advertised native actions and opens from the
  keyboard while closed. One semantic/focus owner retains stable original-index
  option IDs. Current query, disabled state and live selection guard stale
  actions; single selection closes before callbacks. Measured option reveal,
  clear actions, pointer/wheel boundaries and active-window Tab dismissal pass
  14 focused tests. Closed rendering avoids scanning all options.
- `9227207`: Select clears search consistently on every close path and restores
  a valid highlight when filtering becomes empty. Live query, disabled and
  loading state guard queued actions; loading retains focus and busy semantics
  while blocking mutation. Active-window Tab dismissal needs weak deferred
  cleanup so the next ordinary render retires popup semantics. All 18 focused
  tests pass, including closed rendering of 100,000 options without filtering.

The corrected focus fixtures explicitly deactivate and reactivate their test
window before asserting active-window behavior. Earlier inactive-window failures
are retained as fixture errors. Test-support effect flushing draws dirty windows,
so these tests do not isolate the production frame clock. Select dismissal on
OS-window/app deactivation remains unproved. Combobox non-ASCII/IME query entry,
its fully mounted filtered popup and the separate Dropdown remain follow-ups.

A [source-isolated native font-cache diagnostic](2026-10-17-native-font-cache-diagnostic.json)
contains three alternating navigation pairs. Both builds include the BOM fix;
their only source difference is the bounded font cache. All six captures pass
the original workload/activity gates and all eight geometry boundaries at
1100×760, scale 2. Owned builds, tests and native workloads were paused throughout
collection. The initial driver mistakenly invoked the two-engine geometry
validator for a Kael/Kael pair and stopped after one pair; the corrected driver
retains and revalidates that pair and collects the remaining four captures.
Both driver versions, the failure and environment notes are preserved.

| Median navigation diagnostic | Before font cache | With font cache |
| --- | ---: | ---: |
| Active process CPU | 25.602% | 21.148% |
| Churn process CPU | 32.208% | 22.377% |
| Stable sampled peak RSS | 115.00 MiB | 115.80 MiB |
| Per-run draw p99, median | 5,586 µs | 5,309 µs |
| Per-run CPU submission p99, median | 194 µs | 624 µs |

This workload shows 17.4% lower active CPU, 30.5% lower churn CPU and 5.0% lower
CPU-side draw p99, with 0.80 MiB higher stable sampled peak RSS. All runs,
including the unusually low first cached churn sample, remain in the medians.
CPU submission tails worsen in all three pairs: median per-run p50/p95/p99
changes from 76/119/194 to 105/488/624 µs. The CPU reduction therefore does not
establish improved end-to-end latency.
The separate mixed-script `.SystemUIFont` API diagnostic removes repeated
factory calls but demonstrates no timing gain (43.054 versus 43.405 ms medians).
The navigation app uses `.AppleSystemUIFont` and mostly ASCII labels. These
results support retaining the bounded cache for this app workload, not a general
text speedup. They establish no GPUI Kit ranking, per-operation normalized result,
comparative presentation or controlled power claim. The styled-label and final
form fixes are deliberately excluded from these isolated binaries.

The [final text/control validation](2026-10-17-text-control-native-validation.json)
identifies source `9227207` and individual file hashes. The complete core graph
passes 2,162 tests with zero failures; two opt-in native diagnostics are ignored
there and have separately recorded explicit passes. All 497 UI tests pass, and
strict native all-target Clippy passes. Fresh physical Metal validation passes
all 59 required GPU tests with zero skips. The visible smoke starts at 720×460
logical pixels, scale 2, wakes from a model-only idle notification and records
60 GPU-completed frames with compositor presentation timestamps. Its final
readback retains 6,067 text-probe pixels and the earlier scene fingerprint.
Concurrent correctness builds were allowed; these are correctness checks and
do not compare latency or power. Fresh platform execution remains separate.

The `ac7811f` [platform run](https://github.com/Augani/kael/actions/runs/39061136764)
retains the mandatory hosted Metal failure. Its raw macOS job log records
completed GPU work and the successful model-only wake, but presentation callbacks
still have `presented_time=0`. The final window is visible and active at 723×460,
scale 1; the gate fails with `no drawable presentation callback within two
seconds`. Physical-Mac passes do not waive this hosted requirement. Linux native
quality passes its workspace, backend, package and documentation checks, then
fails strict dependency audit on newly published advisories. Both Windows 2022
and Windows 2025/VS2026, browser and Linux renderer/WebView jobs pass at this
completed checkpoint. Later text,
forms and dependency fixes are absent from those CI binaries.

The [dependency validation](2026-10-17-dependency-quality-validation.json)
separately identifies `02e341f` and `d40166f`. Rustls 0.23.46 and Quinn-proto
0.11.18 replace vulnerable root lockfile releases; the comparison lock retains
its already newer Quinn-proto 0.11.19 and updates Rustls. Office and the optional
dotLottie loader both resolve ZIP 9.0.5. Office preserves strict UTF-8/ambiguous
name checks while adapting ZIP's fallible name getter.

The licensed `kael_rasterlottie` fork keeps all 40 upstream implementation files
unchanged at this dependency-only checkpoint. It preserves attribution, original
hashes and a complete reconstruction patch, adds four native/browser compatibility
tests, and explicitly adopts Kael's existing lint policy. The original upstream
style policy produces the same 169 library diagnostics before and after the
ZIP-only change; it is not claimed as passing. Kael depends on the named fork
through a direct path/version/package alias, so registry consumers receive the
same dependency after publication. A root-only patch would not provide that.
No crate has been published.

All 116 fork native tests/doctests and four actual Chrome WebAssembly cases pass
with zero ignored tests. Eight HTTP and 84 networking tests pass, including local
WebSocket behavior. All six Office package tests pass. Strict native all-target
Clippy with Lottie enabled and the Kael/Office Wasm consumer checks pass.
The isolated fork archive verifies at 119,421 compressed bytes with licenses,
provenance and fixtures; the complete 39-package preflight remains separate.
The checked-in reconstruction verifier also rejects tampering, oversized files,
links, special files and unsafe paths. Its 16 negative probes use copies.

At advisory database `999ec89d`, the new root scan reports zero vulnerabilities.
Strict audit still fails on six unmaintained warnings: three D-Bus advisories
across two dependency families. No ignore has been added. Their replacement
requires coordinated portal, tray, keyring, notifications and native accessibility
dependencies; changing one direct dependency would leave the other families.
ZIP compatibility does not establish decompressed archive/image memory admission;
that is the next independent implementation slice.

General desktop performance remains the priority: forms, navigation, tables,
trees, workspaces, media and shared rendering. The current Tree source inspection
narrows the next profiling target. Selection and ordinary scrolling reuse the
model and accessibility snapshot; disclosure reconstructs all 100,025 model
nodes and the semantic catalog. Snapshot delta construction then enumerates both
full catalogs, even when one 4,000-row branch changes. This is a code-level lead,
not an allocation or CPU attribution established by a profiler. A useful change
must preserve complete logical accessibility and stable identities, share the
unchanged catalog, and bound pending work and reclamation. Prebuilding benchmark
variants or dropping offscreen accessibility would change the comparison contract.

The subsequent [animation memory validation](2026-10-17-animation-memory-validation.json)
records the independent admission implementation. The maintained fork bounds
compressed archives before construction, expanded selected entries while reading,
and image decode, conversion, Triangle resize scratch/weights and retained decoded
pixels. Defaults preserve lazy unused image decoding. Cache hits incur no new
reservation, and failed decodes roll their reservation back. Duplicate ZIP names
and inconsistent local/central names are rejected through the library's strict
read option; Office receives the same consistency protection. Stored, deflated,
ZIP64 and commented archives retain positive controls.

The fork passes 134 native tests/doctests and 15 actual Chrome/Wasm cases locally,
with zero ignored tests. The new mandatory CI verifier executes these suites and
requires named admission and pixel regressions and rejects empty, filtered or
skipped runtime results. The pinned fork reconstructs exactly with eight reviewed
upstream Rust changes; the verifier rejects 11 additional provenance-corruption
probes. Original standalone tests, fixtures, licenses and all other original
sources retain their upstream hashes.

Kael separately caps a render batch's retained output at 256 MiB, using the
rasterizer's actual fitted/rounded canvas before preparation or rendering.
Previously, the independent per-frame and frame-count limits admitted up to
64 GiB in principle; that amount was never allocated in the tests. Tiny fixtures
prove exact inclusive limits and rejection before preparation/pixel calls. The
element adapts its configured prefetch and cache count to the admitted batch
size, preserving playback when one frame fits but the default five-frame request
does not. Loop advancement and cache bounds are exercised through the caller's
real planner with eight-byte batches.

These limits do not establish a global memory ceiling. ZIP constructor metadata,
parsed models/base64 strings, decoder-private allocations, allocator overhead,
posters, renderer scratch, prepared animations and overlapping detached tasks
remain additional. Worker budgets are independent. Existing requested-rectangle
validation also remains in force when the fitted output would be smaller.

At the final animation source `90743b8`, all 2,178 core tests pass with the two
explicit native diagnostic probes left opt-in. Strict native all-target Clippy,
the Kael browser/Lottie and Office Wasm consumer checks, formatting, workflow
lint and 46-page documentation verification pass. All eight Office tests pass;
the two new ambiguity regressions first failed against the ZIP-only baseline.
No shader or comparative latency result is inferred from these CPU raster tests.

The complete 39-package preflight now builds every extracted archive in a fresh
staging registry and verifies the publication contents and 10 MiB limit. No
crate is uploaded. Its first attempt exposed Cargo's mixed publish-setting
registry requirement; the final command explicitly selects crates.io while
retaining the fork's publish restriction. An independent archive audit verifies
all 928 included Rust files, totaling 23,222,165 bytes, against immutable
`90743b8` Git blobs. The 39 compressed archives total 6,019,103 bytes. The
packaged VCS IDs span `83190dc` and `90743b8`, and all dirty fields are absent;
those metadata fields do not establish a clean packaging checkout. Actual byte
comparison supplies the source identity. Future benchmark uploads also retain
the measured executables for independent rehashing; the historical 100-capture
artifact and its recorded binary-byte limitation remain unchanged.
