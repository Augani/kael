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
five-repetition-per-engine suite. Its first ten instrumented navigation runs
pass the strict geometry/activity and original workload gates; collection
continues with the uninstrumented builds and remaining contracts. The final
outcome and raw revalidation must be recorded separately; neither the local
pair nor that first completed group establishes all hundred runs.

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

The shared-control review also identifies follow-up work outside the fixes
above. Select's keyboard reveal assumes 28px rows despite variable text/padding
and group-header heights; acceptance must compare actual row and viewport
bounds, including initial offscreen selection. Cached semantic replay also
retains an old external parent if an outer element changes its explicit
accessibility ID without invalidating the cached view. Stable Select ownership
does not exercise that case. A disabled navigation submenu can still attach a
pointer activation handler. These findings remain open; existing focused or
full-suite passes do not establish their correctness.
