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

At that admission checkpoint, ZIP constructor metadata, parsed models/base64
strings, decoder-private allocations, allocator overhead, posters, renderer
scratch, prepared animations and overlapping detached tasks remained additional.
Worker budgets were independent, without a global memory ceiling. Existing
requested-rectangle validation remains in force when the fitted output would be
smaller.

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

The later [animation task ownership validation](2026-10-17-animation-task-ownership-validation.json)
replaces detached element render tasks with owned tasks and immediate cancellation
signals. Source replacement, resizing, failed/loading sources, zero drawable area
and element-state drop retire obsolete requests. A lane retained across source
and size changes prevents replacement work from overlapping its old worker;
two shared worker slots limit active element rendering across windows. Both
permits stay inside the worker until synchronous work returns. Cancellation is
checked before preparation and between/after frames, discarding partial output
and observed-cancelled prepared assets. A frame already executing cannot be
preempted. Ready results do not hold worker slots, so other waiting elements can
progress while the UI remains unblocked.

A genuine regression first shows a completed old-size batch resetting the current
cache geometry. All 22 focused tests now pass, including actual one-pixel renders,
weak-reference lifetime checks and controlled executor cancellation during a
background poll. Full native core validation passes 2,184 tests with only the two
explicit diagnostic probes opt-in. Strict native Kael/UI all-target Clippy and
the browser/Lottie Wasm consumer compile pass. These are exact-source results;
the Wasm check does not execute browser ownership tests or extend the earlier
release-archive preflight to the new source.

The fixed two-worker default trades aggregate throughput for bounded active work.
Its two 256 MiB output admissions allow up to 512 MiB of active batch output;
ready results, per-element caches, posters, prepared assets, source/model storage,
renderer scratch and allocator overhead remain additional. Completed preparation
can enter the bounded weak-source LRU if cancellation races after the final
check. This slice closes the detached element-task ownership gap without claiming
a unified global memory policy, hard RSS ceiling or completed R8 acceptance.

The [independent presentation control](2026-10-17-native-presentation-environment-validation.json)
runs Apple MTKView and two direct CAMetalLayer paths on a separate diagnostic
branch. All three hosted controls complete 60 GPU frames and receive 60 drawable
callbacks, but every presentation timestamp is zero. The virtual display is
active and awake before and after the bounded wake assertion; its lock state is
unavailable. The physical Mac separately remains locked, and waking its displays
also leaves all three controls without positive timestamps. Display sleep alone
does not explain these results. A Kael-specific defect or working compositor
fix is not established. The actual Kael presentation gate remains unchanged;
this diagnostic supplies neither runtime acceptance nor comparative latency.

The latest platform run independently completes the mandatory dependency
[animation admission suites](2026-10-17-hosted-animation-admission-validation.json):
134 tests each on Linux and macOS, plus 15 actual Chrome cases, with no failures
or ignored tests. Their raw logs and artifact identities are preserved at
`e219439`. These fork tests do not exercise the later element-owned cancellation
changes or establish that the still-running complete platform matrix passes.

The ownership checkpoint subsequently passes a separate fresh 39-package
preflight. Independent archive audit matches all 928 included Rust files
(23,241,370 bytes) to immutable `b8aff24`, verifies licenses, fork payload and
registry aliases, and records 6,022,866 compressed archive bytes. The ownership
validation record links this separate receipt. Earlier verified archives are
preserved before replacement. No package was uploaded; later tree and
notification changes require their own validation.

The [notification wire validation](2026-10-17-notification-wire-validation.json)
records a separate Linux interoperability correction at `c01cb86`. Body clicks
advertise the protocol's `default` key and map back to Kael's stable public
identifier. Private keys preserve custom IDs including `default`, `__closed` and
`__default`; only exact advertised keys dispatch. This follows the
[freedesktop notification specification](https://specifications.freedesktop.org/notification/latest/basic-design.html).
The exact production Zbus 5.18 backend passes 22 tests over an isolated private
bus on macOS, with every registry dependency matched to the root lock. Linux
all-target compilation and strict Clippy pass; native Linux desktop activation
remains a distinct runtime requirement. A separate Zbus 6 feasibility port also
passes its own wire tests, but no production dependency migration or advisory
ignore is introduced. Existing one-thread-per-actionable-notification behavior
and the six denied unmaintained dependency warnings remain.

The [tree stage validation](2026-10-17-tree-stage-optimization-validation.json)
compares frozen `e219439` and `6b2ae11` release binaries over the same
100,025-item Unicode hierarchy. Three process pairs alternate revision order in
one interval without owned compilers, tests or native windows; OS background
work remains uncontrolled. All 64 full hierarchy, identity, focus and unchanged
snapshot scroll checks pass. This is a headless framework API diagnostic, not a
native application or GPUI performance result. The export stage includes its own
delta computation and must not be added to the separately measured delta stage.

| Calling-thread CPU median | Operation | Before (ms) | After (ms) | Change |
| --- | --- | ---: | ---: | ---: |
| Model construction | Collapse | 15.561 | 16.058 | +3.2% |
| Model construction | Expand | 16.796 | 8.377 | −50.1% |
| Logical node comparison | Collapse | 39.812 | 33.782 | −15.1% |
| Logical node comparison | Expand | 39.814 | 32.866 | −17.5% |
| Full accessibility preparation | Collapse | 37.883 | 38.816 | +2.5% |
| Full accessibility preparation | Expand | 35.634 | 37.787 | +6.0% |
| AccessKit export, including comparison | Collapse | 41.524 | 34.457 | −17.0% |
| AccessKit export, including comparison | Expand | 45.885 | 38.358 | −16.4% |

The node comparison removes 4,128,792 requested Rust allocation bytes per call.
Fully expanded model construction removes 8,650,828 requested bytes and 18
allocation events; collapsed construction requests exactly the same bytes.
These are separate instrumented allocation captures, with their clocks excluded
from the CPU comparison. They measure requested Rust payload, excluding native
allocation, allocator overhead and application RSS. Full accessibility
preparation still requests approximately 101–102 MB, with approximately
262–264 MB of process-live requested payload while models coexist. It remains a
substantial optimization target. The small collapse construction increase has
mixed direction across process pairs; the unchanged preparation stage is not
reported as an improvement.

An earlier generic-setter fast path passed semantic tests but made collapse
construction 78.7% slower in release measurements. That candidate and its raw
samples remain preserved; the final change restores the generic setter and
skips a redundant catalog only when constructor metadata proves complete source
visibility. Snapshot comparison retains full offscreen semantics and sorted
effective changes while limiting temporary candidates to overlays and masks;
changed monolithic snapshots still require linear traversal. All 2,188 tests of
the unchanged core source and all 501 tests of the final UI source pass, with
strict native lint, browser compilation and formatting checks. Later native
application, platform and memory-policy acceptance remain required.

The preserved [later hosted checkpoint](2026-10-17-platform-e219-checkpoint.json)
at `e219439` passes the full Linux renderer/WebView runtime job, browser job and
Windows 2022 native/WebView/MSI job. Windows 2025 remains active at that capture.
macOS fails the unchanged presentation gate: 43 actual drawable callbacks have
zero timestamps. Linux quality fails strict audit on the six unmaintained D-Bus
warnings. The three operating-system microbenchmark jobs pass; the matched GPUI
application job finishes compilation and constrained docking verification and
starts collecting alternating measurements. These results predate the later
local ownership, tree and notification changes. No gate is waived or overall
workflow success inferred. Local commits are held while those complete runs
remain active so their frozen comparisons can finish.

A final fresh preflight also verifies all 39 packages after the tree and
notification changes. Independent archive audit matches all 928 included Rust
files (23,268,073 bytes) against immutable `6b2ae11`, with 6,028,236 compressed
archive bytes. The tree validation record links the preflight, exact archive
identities, licenses, fork payload and registry-alias checks. The earlier
ownership archive cohort is preserved before replacement. No package is
uploaded. Completed diagnostic worktrees are archived recoverably; their source
inputs, binaries and raw captures remain in the primary workspace artifacts.

The [Secret Service migration](2026-10-17-secret-service5-migration-validation.json)
updates both existing credential consumers to released 5.2.0 and removes the
duplicate Zbus 4 dependency family. The consumer Rust call bodies and crypto
features remain unchanged. Eleven isolated private-bus tests exercise actual
encrypted Secret Service calls, binary/Unicode replacement, matching labels,
locked/unlocked selection, prompt outcomes, malformed IVs and error propagation.
Both session and system addresses point to task-owned dummy services; these
tests do not access a human keyring. Full default Linux consumer graphs pass
all-target cross-compilation and strict Clippy. Native portable secret tests
pass eight cases; the two existing real-login-keychain tests remain opt-in.

At the same clean advisory database revision, known vulnerabilities remain zero
and denied unmaintained package copies fall from six to three. Strict audit
still fails on the remaining Zbus 5 family. No advisory ignore is added, and
Linux desktop/keyring runtime remains separate from private-bus protocol and
cross-compilation proof.

The [idle animation cache checkpoint](2026-10-17-animation-idle-cache-validation.json)
caps retained prepared-animation decoded RGBA8 payload at 64 MiB/four entries
per thread and 256 MiB/16 entries process-wide. Active checkout releases idle
quota; lazy decoding is reweighed before reinsertion. Oversize or pressured
preparations still render valid pixels and bypass retention. Reservations stay
owned until prepared resources drop, without dropping them under the shared
budget lock or a thread-local borrow. Source identity, local LRU, cancellation
and reentrant rendering retain their existing behavior.

All 28 focused core cases and 136 native fork tests pass, alongside a separate
no-images getter test and 16 actual Chrome fork cases. A non-test executable
using the exact production global/TLS implementation fills 16 idle entries
across four held threads; a fifth renders without retaining another entry, and
thread exit returns quota to zero. The Chrome tests exercise fork admission
and image/getter behavior, not core browser cache ownership. Small-budget
regressions test byte admission and cancellation without huge allocations.
These limits count decoded image payload and entries. Active renders, models,
encoded assets, path/timeline caches, scratch, ready outputs, posters and
allocator overhead remain additional; this is not a unified R8 memory ceiling.

A separate twelve-process cost control retains the synchronization tradeoff.
Two million warm cache operations per process add approximately 24–26 ns per
hit/reinsert in the two tiny fixtures. Actual four-render ranges overlap; the
experiment establishes no stable application CPU, RSS or latency change.
Its pinned baseline, final implementation, pixels and raw samples remain
separate from whole-application benchmarks.

The [compact data storage diagnostic](2026-10-17-compact-data-storage-validation.json)
first reproduces 54,400,000 fewer requested Rust bytes for two 100,000×8 domain
datasets when their short values are inline. The same opt-in domain storage is
then exercised through actual Kael `RemoteSheet`/`VirtualSheetGrid` controls.
With 16 cached tiles and settled accessibility, retained requested payload is
107,235,620 bytes for current shared-string records and 53,114,148 bytes for
compact records, a difference of 54,121,472 bytes (51.61 MiB). Both modes keep
the same 216 mounted cells, 1,162 accessibility snapshot nodes, full logical
dimensions, one focused descendant, 85 source calls and 42,752 materialized
cells. The difference persists through 64 additional tile requests.

The grid probe checks every fixture cell, ordered fingerprints, long Unicode
Arc sharing, static headers, a deferred write keyed by original business ID,
query-history reset, cancellation and poisoned stale-response rejection.
Strong dataset/control ownership releases after executor drain; both runs
retain the same documented 2,559,632 bytes of reporting/runtime ownership.
This is headless requested-payload evidence, without native RSS, CPU, physical
scrolling or framework ranking claims. All fixture values fit the pinned
23-byte inline capacity; long-value ownership controls do not establish a
universal saving. `SharedString`, generic `ArcCow` and canonical benchmark
models remain unchanged. A separate actual empty-default probe finds no
per-value heap allocation, so that proposed lead is rejected as well.

Combined validation at `738695f` passes 2,194 core tests with the same two
diagnostic probes opt-in, all 501 UI tests, strict native all-target Clippy,
browser/Lottie Wasm compilation and formatting. Another fresh isolated
39-package preflight passes without uploading crates. Independent audit
matches all 928 included Rust files (23,290,129 bytes) to immutable source;
compressed archives total 6,032,988 bytes. Earlier archive cohorts remain
preserved. These checks include the credential migration and idle-cache limit,
but do not replace remaining native platform acceptance.

The separate [benchmark consumer validation](2026-10-17-benchmark-consumer-validation.json)
repairs the active comparison lock for Secret Service 5.2 while preserving
existing common package versions, sources and checksums. All five applications
compile with `--locked` for both engines and both frame-timing modes: twenty
actual executable targets across four profiles. Cargo checks supply neither
release measurement binaries nor new comparative results; those remain a
separate build and collection step.

The later [physical presentation recovery check](2026-10-17-native-presentation-recovery-validation.json)
now records actual positive Metal timestamps on the physical M2 Pro. Apple
MTKView and both direct-layer controls each complete 60 frames with 59 positive
presentation timestamps; their initial zero-time frames remain recorded.
At `95ebb78`, Kael passes all 59 mandatory GPU tests with zero ignored cases
and records 59 unique GPU-completed frames with positive compositor timestamps.
These correctness checks do not measure comparative latency or controlled power.
The console and displays are active; the optional lock-status key is absent,
so that key is explicitly unknown rather than evidence of an unlocked session.

Native inspection also exposes clipping in the small renderer-smoke window.
The example's flex/grid minimum-size correction at `738695f` keeps all six
distinct 274×138 tile interiors and the chart inside the captured 1446×920
framebuffer. Its separate smoke passes with one actual positive presented
frame. The renderer/library sources are unchanged between these checkpoints;
the one-frame capture is not reported as another 59-frame run. Earlier locked
physical and zero-timestamp hosted controls remain preserved as failures.

The [final `e219439` platform result](2026-10-17-platform-e219-final.json)
completes both Windows native/WebView/MSI jobs, browser and Linux runtime
successfully. Hosted macOS still fails mandatory presentation and Linux quality
still fails strict audit on six denied warnings at that older source; the
aggregate platform job fails. The local credential migration and physical
presentation recovery do not turn that historical hosted failure into a pass.
Fresh hosted execution of the later implementation remains required.

The second [complete hosted comparison](2026-10-17-hosted-e219-comparison.json)
at `e219439` now passes all hundred captures, all original workload/activity
gates, all 800 geometry boundaries and all 50 cross-engine pairs. This upload
includes the actual twenty application executables. Their independently
rehashed bytes match every capture; all 2,944 eligible source hashes match the
frozen source, and raw/aggregate/phase/quantile metrics are recomputed. The
earlier `b11ed50` artifact and its missing-executable limitation remain unchanged.

The following general-desktop medians again use five captures per engine with
optional frame timing disabled. Each cell gives **Kael / GPUI Kit**:

| Contract | Active process CPU | Churn process CPU | Stable sampled peak RSS, MiB |
| --- | ---: | ---: | ---: |
| Navigation/detail | 10.24% / 14.85% | 6.26% / 9.55% | 91.69 / 79.28 |
| Data table | 26.72% / 20.47% | 20.91% / 14.30% | 203.73 / 136.42 |
| Virtual tree | 52.92% / 14.75% | 27.89% / 10.66% | 832.20 / 654.20 |
| Docking workspace | 18.89% / 17.35% | 9.10% / 10.27% | 81.03 / 80.22 |

Navigation CPU is lower for Kael in this recorded mode, while data/tree CPU
and RSS remain substantial targets. Workspace active CPU and RSS are slightly
higher, with lower churn CPU. The full record retains all instrumented results
and the existing document contract; these modes are not pooled. CPU is measured
over phases, without normalization per operation. The tree comparison retains
Kael's complete logical accessibility versus the competitor's mounted rows.
All actual windows are equally constrained to 1100×645 at scale 1 on the same
three-core, 7 GiB M1 virtual runner with no display inventory. This establishes
neither comparative presentation, controlled power nor an overall ranking.

That comparison uses the frozen package-version 0.7.0 snapshot at `3467e647`,
which is distinct from the official v0.7.0 tag. The current
[GPUI Kit site](https://gpui-kit.com/) recommends 0.7.3, whose
[release notes](https://gpui-kit.com/releases/) describe GPUI pre 0.3.9 and
additional rebuild/table improvements. A separately resolved exact-tag
candidate preserves the original workload sources and checks; its compilation
and runtime acceptance remain separate from these older results.

The [compact records example checkpoint](2026-10-17-compact-records-example-checkpoint.json)
adds a native-only read-only consumer at `a3add69`, without the editor feature.
Its two identity/long-Arc tests, strict example lint and formatting pass.
Actual native pointer/keyboard selection reports stored IDs before and after
reverse ordering; paging, refresh and a rejected dummy paste are recorded.
Native inspection also preserves two failures: short variable lists shift away
from the leading edge, and the fixed table body overlaps the footer in its
constrained parent. These visual defects require correction before acceptance.
Another fresh 39-package preflight verifies all 928 included Rust files against
`a3add69` (23,290,129 bytes; 6,033,157 compressed archive bytes), with no upload.
Example source is excluded from release archives and has its own compile proof.

The later [shared-control checkpoint](2026-10-17-shared-control-validation.json)
corrects those initial leading-edge/footer defects. Variable lists clamp their
active axis before positioning, and `DataTable.fill_parent(true)` fits a bounded
parent while retaining existing default sizing. Header, skeleton, cached-row
and vertical-body scrolling use their intended axes. Dropdown now implements
its declared disclosure actions with one focus owner, live disabled/session
checks, paired key handling and measured popup pointer bounds. Scrollbars
register Focus/Increment/Decrement/numeric SetValue handlers, use live ranges,
preserve the other axis and retire actions when a keyed handle/axis changes.

At `d766b5d`, all 2,198 core tests and 537 UI tests pass, together with strict
native all-target lint, Wasm, two records tests/build, all twenty benchmark
compile targets, formatting and docs. The same two core diagnostics remain
opt-in in the broad suite. A separate actual display-link invocation passes
128 stop/resume cycles with one retained native object; it performs no
comparative CPU, power or presentation measurement. Another isolated
39-package preflight builds every archive. Independent audit matches all 929
included Rust files (23,392,950 bytes) to Git; compressed archives total
6,052,424 bytes. All earlier cohorts and failed attempts remain preserved, and
no crates are uploaded.

The exact current records binary has fresh native stored-ID selection before
and after reversal, read-only paste rejection with clipboard restoration,
vertical paging and refresh proof. A separate consumer changes only the initial
window request to 900×600 logical pixels. Its actual track click moves the
horizontal range from zero to 198; Left decrements by 40 to 158, and vertical
scrolling preserves that offset with header/cell alignment and footer clearance.
These are native pointer/keyboard controls. The available client does not expose
secondary scrollbar actions and its string setter is not numeric-NSNumber
acceptance proof. Nonzero native horizontal wheel delivery and current native
resize acceptance remain unproven. At that earlier checkpoint, the fixed 160-pixel default scrollbar track
remained a separate quality target; its sizing proposal had not changed that
frozen source.

The [output ownership checkpoint](2026-10-17-animation-output-ownership-validation.json)
removes the completed animation canvas copy. A real 16×8 frame requests 512
bytes in one allocation rather than 1,024 bytes in two, preserving exact pixels
and balanced release. Actual primary fork validation passes 137 native tests,
strict all-target lint and 16 actual Chrome cases. Model, layer/effect/image
scratch and active/ready ownership remain additional; this is not native RSS
or a unified memory limit. The current exact GPUI Kit 0.7.3 pin compiles all five
unchanged workloads in both instrumentation modes. Release-byte freezing,
fresh hosted collection and complete latest-source platform acceptance remain
separate from this compilation and the historical hundred-capture results.

The next [exact-tag release/runtime checkpoint](2026-10-18-gpui073-local-runtime-validation.json)
freezes all twenty optimized executables against GPUI Kit 0.7.3 at `67265a36`.
Actual Cargo compiler inventories, features, binary bytes and 3,009 Kael source
files are independently checked against the frozen Git source. All 3,662
competitor files match its immutable revision. The original source archive
contains only the 38-file `crates/kit` projection because Git applied its working
directory prefix. It remains untouched; an additional complete repository
archive is independently verified against all 3,662 files.

On the unlocked physical Mac, all twenty executables pass their first native
oracle attempt, with 160 actual geometry boundaries and ten engine/mode pairs
matching 1100×760 logical pixels at scale 2. Four navigation runs use the full
original duration. Sixteen component runs use custom quick smokes with the
original fixtures and operations; these do not establish full-duration acceptance
or comparative performance. All ninety raw files and before/after executable
hashes are independently verified. No CPU/RSS, presentation, power or overall
ranking follows. The separate full hundred-capture hosted collection at
`0d15aff` remains running and unaudited at this checkpoint. These frozen
executables predate the subsequent track-width and animation arithmetic fixes.

The [next sizing/animation checkpoint](2026-10-18-track-and-animation-validation.json)
adopts the reviewed width fix: `ScrollBar` forwards styling to its existing base
style and the table requests full horizontal width. Default primitive sizes
remain unchanged. Mounted regressions measure changing actual track widths,
stable native identities and independent offsets. In a real constrained records
window, a click beyond the former track reaches 198, Left moves back to 158,
and vertical scrolling preserves 158. An exposed native zoom action grows the
outer capture from 1800×1264 to 3456×2168 with bounded body/header/footer layout;
this does not establish arbitrary edge dragging or numeric native SetValue.

The same checkpoint fixes signed Choker radius/coordinate overflow while
preserving finite/no-op thresholds and the clipped alpha-only kernel. The tiny
oracle moves from 39 exact passes plus six actual debug panics to 45 exact
passes without panics. The byte-identical isolated fork passes 140 native and
17 actual Chrome tests. Provenance reconstructs all 81 files from 74 originals
with ten reviewed original Rust paths and the unchanged 128 KiB patch cap.
Combined primary validation passes 2,199 core tests and all 538 UI tests before
the later asset changes. Worst-case kernel work and scratch remain separate.

The [active asset handoff](2026-10-18-asset-lifecycle-validation.json) then gives
mounted drawing consumers completed-result ownership independently of the LRU.
Zero/below-output retention no longer loses a delivered result or reloads it on
every draw. Pending publication stays weak; explicit removal invalidates ready
owners even after LRU eviction, and generations prevent stale delivery. Actual
cached-render skipping, separate namespaces, 16 retirement cycles, reentrant
hooks and critical pressure pass. Review also reproduced and corrected an
unnecessary synchronous-ready notification; the final test observes zero owner
notifications and no clean-frame rerender. Lottie charges its source and decoded
poster. All 54 focused lifecycle tests and the broader nineteen-file primary
batch pass: 2,216 core tests, all 538 UI tests, strict all-target lint, Wasm,
example tests/build, twenty benchmark compile targets, parser tests, provenance,
formatting and docs. The two existing core opt-in diagnostics remain separate.

Active ready results, including errors, survive ordinary LRU/clear until their
frame state retires or `remove_asset` requests a reload. Their payload and
metadata remain additional to completed-cache budgets. Global memory accounting,
loader concurrency, native OS action-client limits and newer-source packaging/
platform acceptance are not closed by this checkpoint.

The [final `0d15aff` platform audit](2026-10-18-platform-0d15-final.json)
independently rehashes all ten archives and 232 extracted files. Both Windows
jobs, Linux renderer/WebView and the three-browser job pass; the overall run
correctly fails hosted positive presentation and three unmaintained D-Bus
advisories. Mac callbacks arrive with zero presentation times. Browser WebKit
capture supplies lifecycle/bounds proof but zero decoded frames; capture pickers
are not automated and software-fallback latency budgets are not enforced.
These results predate the nineteen-file follow-up. Its new independent Apple
controls run after the original required gate and retain bounded diagnostics;
physical validation supplies 59 positive timestamps in each control variant.
A control pass never turns a failed Kael presentation gate green.

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

The [completed released-0.7.3 hosted comparison](2026-10-18-gpui073-hosted-comparison.md)
subsequently passes independent audit of all 100 captures at `0d15aff`, all
twenty executable identities, 800 geometry boundaries and 50 framework pairs.
The actual equal client area is 1100×645 at scale 1; local scale-2 geometry is
separate. General desktop CPU/RSS and separate CPU timing summaries retain mixed
results, substantial table/tree gaps, native component differences and per-run
values. Energy/instruction/cycle counters are all zero and support no power
ranking. These binaries predate the nineteen-file `b279795` batch.
