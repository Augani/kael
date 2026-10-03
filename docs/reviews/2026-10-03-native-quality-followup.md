# Native quality follow-up — 2026-10-03

This records the unlocked Mac's fresh acceptance work after the earlier
`0adb736`/`bb891c7` checkpoints. The full requirements remain in the
[completion ledger](2026-10-02-completion-ledger.md); the evidence below does
not establish a cross-framework ranking.

## Snapshot construction and comparison geometry

Native profiling identified repeated growth of the accessibility snapshot map.
Construction now reserves the iterator's guaranteed lower node count and the
visited set's exact capacity, retaining the existing identities and malformed
tree handling. Three full before and three full after runs of the 100,025-node
tree pass every phase oracle using the same validation-paint wait and native
selection export. Median operation p95 changes from 78.1 to 70.6 ms and p99 from
83.4 to 74.8 ms. Median active CPU changes from 68.88% to 67.18%; median peak RSS
changes from 1,139.4 to 1,141.5 MiB. This is a latency diagnostic with roughly
unchanged peak memory, not a general CPU or cross-framework advantage. The
phases are time-paced, operation counts differ and the desktop is shared.

[The six-run diagnostic](2026-10-03-tree-snapshot-reservation-diagnostic.json)
retains individual metrics, operation counts, binary/raw-log hashes, collector
source hashes and limitations. Three earlier captures failed the original
40 ms mounted-row validation and are excluded. Both measured variants include
the same bounded fresh-paint correction, so that harness change is not a
confounder in this comparison.

All five benchmark binaries now record actual native client viewport and scale
at each phase begin/end. Schema 3 requires eight ordered, finite, positive
samples, rejects resize/scale changes and requires equality between engines and
across repetitions. Equally constrained windows are accepted; nominal 1100×760
requests alone cannot satisfy the gate. Twenty-one collector regressions pass,
including the new fault cases reproduced against the old validator, and strict
all-target Clippy passes for both engine graphs with frame timing enabled and
disabled. Active/churn phases also require repainting beyond their initial
phase frame, with native CPU draw/submission counts required in instrumented
builds. Four fixed counter records exclude startup and validation work; paused
windows cannot pass using startup instrumentation alone.

[Rejected native captures](2026-10-03-native-geometry-rejections.json) identify
the sleeping physical display, binary/raw-log hashes and incomplete painting.
The navigation/editor captures also expose Kael client height 761 versus GPUI's
760 at the same 1100 width and scale 2. The paired geometry gate rejects this
one-point difference. Reasserting the requested size with AppKit's native
`setContentSize:` after titlebar/layer configuration corrects the startup drift;
the native renderer fixture now explicitly verifies its initial client viewport.
The before trace records 720×461 and the corrected trace records 720×460 with
`NATIVE_WINDOW_CONTENT_SIZE_OK`. The display remains asleep and later model
wakeup/presentation still fail, so complete visible-window and paired geometry
acceptance remain pending. No failed capture enters a performance result.

The UI library's complete 448-test native/editor suite and all 98 core
accessibility tests pass after the native selection and host-focus corrections.
Strict native all-target Clippy passes, and fresh isolated package preflight
verifies all 38 extracted archives after these changes, with no registry upload. A fresh local renderer trace records GPU
completion but no submitted model-only wakeup on its current native window.
The added polling trace identifies AppKit occlusion state 8192, without its
`Visible` bit (2), and no display link; the drawable callback reports presentation
time zero. Occluded windows deliberately stop their frame clocks. This run fails
the visible-window acceptance requirement and does not establish an idle-wakeup
regression or a presentation pass. Opt-in diagnostics distinguish actual drawable
callbacks (including dropped presentation time zero), command completion and
native display-link/occlusion state; normal applications do not enable them.

## Correctness fixes found through native use

CoreText declared glyph bounds without fractional antialias padding and then
returned a larger raster. Checked atlas admission rejected that raster and
stopped the remainder of a text line. Bounds now include the padding before
admission, with no relaxation of memory limits. Twenty native text-system tests
and a 39-glyph CoreText-to-Metal pixel comparison pass. Fresh filesystem,
workspace and document/data screenshots show complete text.

Model notifications could arrive after an idle window stopped requesting
frames. `App` now coalesces affected windows and resumes their native polling
after effects are flushed. A reentrant leased window remains pending until its
next safe update. CPU regressions cover idle-to-active-to-idle behavior;
the real renderer fixture waits 1,250 ms and then calls only `cx.notify()`.
Its new revision is submitted without a resize or input event. The displayed
Metal run records 55 GPU-completed/presented frames with ordered timestamps.

AppKit's omitted replacement range uses
[`NSNotFound`](https://developer.apple.com/documentation/foundation/nsnotfound-4qp9h),
which is the maximum signed integer. Treating only the maximum unsigned integer
as invalid misrouted composition replacements. The conversion now recognizes
the native sentinel and retains checked range addition. The actual
`firstRectForCharacterRange:actualRange:` callback also needed an `NSRange *`
argument, rather than an Objective-C object argument. Runtime calls now pass
with a valid output pointer, an optional null pointer, and an invalid range.
These are owned-process native protocol checks.

The browser's bounded mirror previously spent its 4,096-node capacity on
offscreen headers before reaching visible grid cells. Its projection now
admits focus, mounted controls and their valid ancestor paths before sampling
logical descendants. Queue/path/output storage remains bounded. Independent
regressions cover 16,384 headers, active descendants and hidden ancestors.
The actual wide and compact Chromium scale suite passes with separate limits
for physical mounts and retained semantics. This run uses SwiftShader and
does not establish hardware performance.

## Actual application acceptance

| Check | Executed evidence | Scope |
| --- | --- | --- |
| Real Editor protocol | `.artifacts/kael-native-editor-clipboard-ime-runtime.log` and `.artifacts/native-ui-idle-wake/editor-clipboard-composition.json` | Full 105,391-byte / 66,983-UTF-16 document, native selection, geometry, hit test, visible ranges, reveal, EOF, one-operation editing/Undo/Redo, stale-origin and read-only/disabled guards |
| Clipboard and composition | Same Editor run; binary SHA-256 `176fb35f7ffdf4e304ffb32992b5675674a0ad2a2c770efa3ab4440f8d453b10` | Actual native Copy/Cut/Paste, all-format pasteboard restoration, repeated Unicode marked text and commit, native candidate rect and actual focused Editor; `physical_ime=false` remains explicit |
| Native renderer | `.artifacts/native-ui-idle-wake/native_renderer_smoke.log` | Real M2 Pro Metal submission/completion/display, model-only idle wake and populated framebuffer export; no comparative power claim |
| Filesystem pointer move | `.artifacts/native-ui-idle-wake/filesystem-pointer-move.json`; binary SHA-256 `71b3e6b27ab0d0214f032d38fe58b7d8cc5235dfb978b598cd6fb70dd92bef84` | CUA pointer drag of `readme` into `Archive`, both parents refreshed, independent source/destination bytes, hidden-file toggle and complete native pixels in the isolated fixture |
| Document/data interactions | `.artifacts/native-ui-idle-wake/document-data-native-interactions.json` | Native Markdown formatting/Undo, Unicode remote edit and record identity across sort/reload, atomic 2×2 TSV paste/Undo/Redo, frozen-row vertical scrolling and populated pixels |
| Native horizontal grid | `.artifacts/native-ui-idle-wake/grid-horizontal-native-keyboard.json`; binary SHA-256 `b890ae056e9d81015609964bacbffcb2179ad5930d53ba085496f804b1ee49e3` | Actual OS End/Home/Right reveals Notes then Score with unchanged vertical position and frozen Title/Owner pixels. CUA horizontal-wheel attempts deliver native zero deltas; no wheel proof is inferred |
| Browser scale | `.artifacts/kael-browser-suite-projection-runtime.log` and `target/browser-suite-smoke-logs/http.log` | Wide/compact application routes, 1M×16,384 spreadsheet, 250k blocks, 10k slides and 100k shapes with bounded mounted/semantic nodes |
| Workspace | `.artifacts/native-ui-idle-wake/workspace-native-interactions.json`; binary SHA-256 `d3544394d47c2c30baf41b1ae41c488b8cf5d0b93f03c25f850cb3bbb2b5afd3` | Actual pointer float move/resize, tab movement between groups and nested edge split, keyboard edge docking, save/reset/restore for floating and nested layouts, redock, Unicode inspector edits/Undo/Redo and live preview pixels |
| Packaging | `.artifacts/kael-final-followon-publish-preflight.log` | All 38 package archives verified; no registry upload |

The OS keyboard path also passes an Alt-E dead-key preedit followed by E,
producing `é`, with one-step Undo/Redo in retained Notes. This verifies native
keyboard composition on the current input source; Japanese candidate selection
and screen-reader sessions remain separate.

The clipboard guard captures every pasteboard item/type before writing. It
restores only if the change counter still identifies its own latest write, so
an intervening external clipboard change is preserved. Contents are not logged.
Native text calls run outside a leased `WindowHandle::update`, matching AppKit's
synchronous reentry into the input handler.

## Comparison and outstanding acceptance

The benchmark suite now has navigation, Editor, data table, virtual tree and
persistent docking contracts. Both engines execute the two new native quick
fixtures. The independent collector verifies exact Unicode fixture hashes,
selected/revealed tree indices, complete node counts, actual native pane
identities, split topology, active content and persistence. Twenty-one collector
regression tests and 3,016 combined core/UI/cache/media/resource-planning library
tests pass. Strict native all-target and browser library/example Clippy, docs,
workflow lint and all four adapter patch/hash checks pass. The maintained workflow retains five repetitions per
engine, instrumentation mode and contract: one hundred runs.

The historical sixty-run hosted checkpoint predates the glyph correction.
Current-source repeated comparison, controlled power/wakeup evidence and
comparative presentation measurements remain required. Native physical IME,
screen-reader exploration remain separate acceptance requirements. Horizontal
remote-grid keyboard reveal and populated/frozen-column pixels now pass. The
wheel tool delivers zero native deltas on this host, so native nonzero horizontal
wheel delivery remains unverified. Fresh actual workspace move/resize/redock, pointer tab/split movement, nested
persistence and inspector Undo/Redo evidence now passes on the integrated source.

Windows UIA focus, Linux AT-SPI wire behavior, GTK shedding, all-backend resource
pressure/lifetime checks and native renderer gates require fresh hosted runs.
Local compilation and old successful GPU fixtures cannot replace those results.

## Hosted follow-up and startup corrections

The `dacdea1` [platform run](https://github.com/Augani/kael/actions/runs/37134833027)
executes the corrected glyph/atlas source. Its mandatory WebGL2 shader and
context-loss pixels, Linux Blade retained scenes, GTK4 atlas admission/retirement,
Wayland/XWayland embedded WebViews, Windows 2022 Direct3D retained scenes and
WebView, macOS outline/text protocols and Metal-backed Chromium performance
steps pass. The complete run is not green: Unix runtime discovery, desktop UIA
focus and the hosted native Metal presentation callback still fail.
The full Chromium/Firefox/WebKit browser job subsequently completes
successfully, including the large application suite and consumer graph checks.

Workspace-wide `--all-features` selected both mutually exclusive runtimes of
the maintained Unix adapter. The quality/docs matrix now excludes that package
as an all-features root and checks its default and Tokio configurations
separately, including the Windows verification script. The corrected workspace
Clippy graph passes locally. Fork archives exclude the tracked upstream
`.cargo_vcs_info.json` copies: Cargo reserves that name for its generated Kael
package metadata. Original provenance remains available in the source checkout
and upstream hash records; all four complete adapter patches reconstruct exactly.

The next source revision installs the Unix Accessible/Cache interfaces before
publishing the application, yields between bounded registration batches, and
refreshes libatspi's existing cache object when its text capabilities change.
A regression reproduces the missing capability refresh, then passes on addition
and removal of text runs; all twelve common-provider and six Unix-provider
tests pass. Opt-in registration counts and client stages remain in failure
artifacts. These changes still require the external Linux runtime run.

Mac startup now establishes its activation policy before invoking callbacks
that show/activate the first window. The same native smoke launched directly
from the terminal passes with sixty actual GPU-completed/displayed frames,
the model-only idle wake, complete glyph pixels and framebuffer export
(`.artifacts/kael-native-startup-policy-smoke.log`). Its local pass does not
replace the outstanding hosted presentation gate. Accepted Windows accessibility
focus now requests host activation before assigning keyboard focus, and the
external client reports foreground PID/owned-node focus when desktop discovery
fails; the full Windows cross-target Clippy graph passes.

## Indexed input and accessibility lifetime correction

Editor UTF-16 conversions now use Ropey's indexed byte/character/code-unit
operations, rather than repeatedly looking up every preceding character.
Surrogate-interior editing offsets retain the original forward adjustment.
String-oracle regressions cover every byte/code-unit offset, overflow-sized
requests, combining marks, CRLF, supplementary scalars and document replacement.
All 22 Editor tests pass. Native AppKit candidate queries now return the actual
first contiguous logical fragment and its adjusted range, preserving complete
clusters, first-line coverage and zero-width insertion points. The owned native
protocol reproduces the old multiline failure, then passes with real marked
text, a combining-mark request and an empty caret range. This follows
[Apple's actual-range contract](https://developer.apple.com/documentation/appkit/nstextinputclient/firstrect(forcharacterrange:actualrange:)).
The green protocol log is `.artifacts/kael-ime-actual-range-native-green.log`;
its executable SHA-256 is
`c10e9699189761ff5724b92e942810d1a5d17517f5cd48c26d028ddb80eb07ce`.
Physical input-source candidate selection remains separate.

A subsequent complete native Editor run passes on the current callback/input
source, including clipboard restoration
(`.artifacts/kael-painted-callback-native-editor-runtime.log`, executable
SHA-256 `b7a51de8767ed05a8b678c8562d400c2fb8b1e52ebf19a7ca6aae840232b39c0`).

The common AT-SPI provider caches filtered child indices per queried parent.
Preparing every child's native cache entry previously counted preceding
siblings again, making a wide parent quadratic. Reorder, removal, hidden-node,
focus-only and host-focus changes invalidate the index and release its outgoing
storage. Fourteen common-provider tests pass; the complete patch reconstructs
all fifteen modified upstream files exactly. Native Linux clients explicitly
call `Atspi.Text.get_text`, because GI otherwise resolves the no-argument
Accessible method with the same name. A same-PID GTK application root can fail
discovery without suppressing the separate AccessKit root.

The actual Windows 2022 desktop check now passes global focused-element identity
at `ecf874d`, but its next operation incorrectly requests Invoke on a stateful
TreeItem. The client now exercises that node's actual SelectionItem pattern and
calls Select. The unchanged full hierarchy/collapse/expand/focus gates and the
new selection call still require a fresh Windows runtime run.

An owned 100,025-node native tree run reproduces severe model retention:
3,005.77 MiB peak RSS, with 2,537.97 MiB still resident near exit. A read-only
live heap inspection finds twenty-five separate 51,527,680-byte allocations
and millions of small live allocations; this is live storage, not merely
allocator reservation. Mounted row callbacks were retained whenever their
offscreen logical node survived, keeping entire outgoing models and semantic
snapshots alive. Painted handlers now expire with their frame overlay and
release captures when the current subtree handler already supplies the action.
Explicit persistent handlers retain their prior lifecycle. The current logical
subtree still handles offscreen actions. A weak-reference regression fails
before the fix and passes after it; all eleven virtual-tree and 97 core
accessibility tests pass.

[The matched memory diagnostic](2026-10-03-tree-callback-memory-diagnostic.json)
preserves three complete release runs per version on this M2 Pro. Median peak
RSS falls from 3,352.06 to 1,136.86 MiB (66.1%); median idle-after RSS falls from
2,772.31 to 1,107.39 MiB. The maintained collector validates all four full
phases and the same complete logical accessibility contract. Both versions use
frame instrumentation. Median active CPU is 71.46% before and 69.35% after;
no general CPU improvement is claimed. The desktop remains shared and active,
so these are process-memory diagnostics rather than a controlled overall
ranking. Raw captures remain in `.artifacts/tree-callback-memory-matched/`.

## Latest hosted results and comparison repair

The `ecf874d` [platform run](https://github.com/Augani/kael/actions/runs/37137770933)
passes the complete Chromium/Firefox/WebKit job, native macOS outline/text
protocols, Linux Blade/GTK4 GPU pressure and retained-scene tests, Wayland/XWayland
WebViews, and Windows 2022 Direct3D/WebView proofs. Linux workspace Clippy,
tests, both Unix runtime configurations, format and docs pass. Dependency audit
finds [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html)
and a yanked ChaCha20 release. The root lockfile now uses patched Rustls
0.23.45, WebPKI 0.103.15 and ChaCha20 0.10.2; the unchanged strict audit script
passes locally. No advisory or yank exclusion was added. The isolated benchmark
lockfile already contains the patched Rustls/ChaCha20 versions.

Hosted Metal submission and the model-only idle wake pass, but the virtual Mac
still supplies no drawable-presentation callback within two seconds. The
recorded window is visible and later active. The actual local M2 Pro supplies
sixty GPU-completed/presented frames. The hosted presentation requirement remains
open; neither CPU submission nor a virtual host is treated as display proof.

The [latest performance run](https://github.com/Augani/kael/actions/runs/37134841337)
preserves eighty completed native navigation/editor/data/tree runs: five
repetitions per engine, contract and instrumentation mode. Docking aborts on
the first competitor idle oracle after one completed Kael docking run. Its
fixed 550 px first pane incorrectly assumes an unconstrained 1,100 px native
window. An actual compact native competitor run reproduces a 0.625 ratio where
the oracle expects 0.5. Initial and resized splits now use measured native
container sizes. Both engines pass normal and 880×640 compact native fixtures
with the same geometry, pane-identity, Unicode-content and persistence checks.
The workflow runs the compact regression before collecting full runs; quick
fixtures remain rejected as performance evidence.

[The retained diagnostic summary](2026-10-03-hosted-comparison-diagnostic.json)
records the eighty completed runs, executable/archive hashes and limitations.
In uninstrumented runs, median active Editor CPU is 35.76% for Kael versus
9.17% for GPUI Kit. The tree is 69.29% versus 15.99%, with 1,641.28 MiB versus
603.50 MiB median active RSS. These identify work to investigate; they predate
the input/callback fixes, and full versus mounted-only tree semantics differ.
Actual constrained viewport equality, comparative GPU completion/presentation,
physical power and controlled thermal/display conditions are not established.

An owned native System Trace capture of the local navigation fixture completes
all phases with nominal recorded thermal state. Its executable SHA-256 is
`29b9a1ce72f7012e4eacca01cbad73a3f7938ff99bb4d04b35911c6303492b4f`;
the trace, thermal table and native workload log remain under
`.artifacts/kael-system-trace-probe*`. This establishes capture availability,
not a comparative power measurement. The installed Power Profiler rejects
macOS recording, and ordinary-user `powermetrics` exits because it requires
superuser privileges. No privilege change or physical-power claim is inferred.

## Current archive and source verification

All 448 UI library tests, 97 core accessibility tests, fourteen common AT-SPI
tests and six tests in each Unix runtime configuration pass. Strict native
all-target Clippy and both benchmark engine graphs pass, as do fifteen collector
regressions, format, workflow lint and the docs build. All four adapter patches
reconstruct their upstream changes exactly.

The first current archive preflight exposes Cargo reusing an older cached
local-registry core archive at version 0.4.1. The extracted new core contains
the new input/geometry APIs, while the reused cached source lacks them. Release
verification now gives both Cargo output directories fresh staging and retains
the upstream download cache. The UI also requires core 0.4.1. All 38 archives
then compile and pass the size/content checks
(`.artifacts/kael-isolated-package-registry-preflight.log`). Verified archives
are copied to the normal output directory only after the complete set succeeds;
no crate is uploaded. Native CI retains dependency caches when a later runtime
protocol fails, allowing subsequent corrected-source runs to reuse compiled
dependencies without weakening those runtime gates.

An owned native Time Profiler capture of the corrected tree binary completes
the full workload. Accessibility snapshot construction and hashing dominate
its sampled stacks, including repeated map growth. The trace and parsed summary
remain at `.artifacts/kael-tree-current-time-profiler.trace` and
`.artifacts/kael-tree-current-time-profiler-summary.json`. Compilation was active
elsewhere during capture, and inclusive stack weights overlap; these samples
identify the next implementation target rather than establish a CPU ranking.

## Second hosted checkpoint and native state corrections

At `2c5cd60`, the complete hosted Linux quality job and Chromium/Firefox/WebKit
job pass, including the unchanged security audit. The Windows 2022 native
hierarchy/disclosure/identity/global-focus checks pass before selection fails:
unselected TreeItems omit the explicit false selection value required to
advertise UIA SelectionItem. Core export now distinguishes unselected tree/tab
items and interactive list rows from nonselectable controls. Its regression
covers false/true/false transitions and preserves absent selection metadata for
ordinary buttons and static list items. All 98 accessibility-related core tests
and strict native all-target Clippy pass locally.

The Linux native tree client now explores all 100,025 rows, restores stable
D-Bus identities across disclosure and dispatches idle selection to the
foreground model. Its final focus-state check fails because the Kael provider
never forwarded native host activation to AccessKit. X11, Wayland and GTK4 now
forward activation and deactivation, suppressing duplicate updates so ordinary
frames do not clear the filtered child index. The owned Xvfb fixture explicitly
establishes real keyboard focus on its PID-checked window; no window manager is
present in that session. An inactive host is not represented as focused.

The X11 Editor reaches complete text discovery and foreground Unicode selection
before another GI collision: `get_selection` resolves the Accessible interface
getter rather than the Text range query. The client now calls every Text and
EditableText method through its explicit GI interface. GTK4 Editor discovery
still fails at this checkpoint. Discovery now refreshes the desktop cache,
handles root-level query failures independently and records service/PID details
on failure. These changes require a fresh native Linux run; a proposed discovery
fix is not counted as acceptance.

Native tree profiling exposes a separate validation race: the fixed 40 ms
settle can inspect mounted rows from two navigation updates earlier, even
without compilation activity. Three failed captures are retained and excluded
from measurements. Validation now requests a fresh frame and waits, for at most
two seconds, until that frame mounts the original requested row. The complete
hierarchy/Unicode/selection/reveal oracles are unchanged, and validation remains
outside measured phases. An owned complete release pilot then passes all four
full workload phases with 100,025 logical nodes.

## Complete hosted diagnostic and next native checks

The [hundred-run workflow](https://github.com/Augani/kael/actions/runs/37144285572)
at `2c5cd60` completes all five contracts in both engines and instrumentation
modes with five repetitions each. Every saved result revalidates against the
exact schema-2 checkpoint collector. The [full diagnostic](2026-10-03-hosted-100-run-diagnostic.json)
retains raw/binary hashes, environment metadata and phase medians. These runs
predate actual native viewport recording and per-phase frame counts. They
cannot satisfy schema 3 or support an overall comparison, physical power or
display-latency claim. Editor and data-table CPU work remain profiling priorities.

At `f81adaf`, Windows 2022 passes the complete 100,025-node UIA outline client,
including idle host focus, disclosure identities and SelectionItem selection.
The Linux X11 Unicode editor passes selection, geometry/hit-testing, full text,
EOF/reveal, clipboard, edits, atomic Undo and read-only/disabled guards. Windows
then fails its multiline ValuePattern query. The maintained provider now offers
that pattern and constructs its full Unicode value on demand from retained
TextPattern runs, avoiding a duplicate document value in every tree update.
The original mutation guards remain. Microsoft's current
[Value provider guidance](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-implementingvalue)
requires this interface for editable multiline text. Portable query tests and
exact upstream patch/hash reconstruction pass; actual Windows verification of
this correction remains pending.

Both Linux outline clients expose a race while rediscovering a restored leaf's
Action interface after collapse/expand. The client now refreshes the proxy cache
and waits within its existing bound for actual Click support before dispatching
its single native action. GTK Editor discovery remains unresolved; bounded
owned-node diagnostics and opt-in semantic publication counts will distinguish
an absent native hierarchy from stale client discovery. Fresh runtime CI is
required for both changes. No proposed client correction counts as a pass.

## Awake physical Metal acceptance at `01e1cad`

After the physical display wakes, the complete native renderer script passes:
7 fragment/compute tests, 6 graph tests and 46 additional resource/atlas/lifetime
checks, exact initial 720×460 client size, model-only idle wake and 59 actual
GPU-completed/displayed frames with ordered timestamps. Fresh framebuffer
pixels show complete text and retained scenes. The preserved evidence is
`.artifacts/native-size-awake-proof/`, including full logs, GPU test records,
framebuffer pixels, source checkpoint and binary/log hashes. This physical
acceptance does not establish comparative latency or controlled power.

The hosted `f81adaf` macOS trace distinguishes a different failure: native
occlusion includes the Visible bit, the display link runs and model-only idle
wake reaches submission, but every drawable callback reports presentation time
zero. The physical display's successful timestamps do not substitute for that
hosted gate. The cause of the hosted missing display timestamps remains open.

Fresh isolated package verification at `01e1cad` passes all 38 archives after
the multiline provider, client-size and collector changes. All four maintained
adapter patches reconstruct exactly from verified upstream source hashes.
No crate is uploaded.

## Native viewport/activity checks and Editor CPU target

At `01e1cad`, [schema-3 native captures](2026-10-03-native-viewport-activity-validation.json)
pass all original oracles for navigation, Editor, data and docking in both
engines. All eight runs have stable, equal 1100×760 client viewports at scale 2
and active/churn repaint/draw/submission activity. These single repetitions on
a shared desktop validate the new gates; they establish no performance ranking.
Kael's full tree capture also passes. GPUI's tree aborts during churn because
target 100024 remains absent from its mounted rows after fresh validation
painting. That entire pair is excluded and the original failure is preserved;
extra failure-only scroll-state diagnostics do not relax the oracle.

Current Windows 2022 native UIA text and outline checks both pass, including
full Unicode multiline values and read-only/disabled guards. Linux X11 and
GTK4 both pass complete outline idle focus/disclosure/selection; X11 also
passes the full Unicode Editor protocol. GTK Editor still publishes only two
semantic nodes and exposes an empty container. Bounded fixture render/viewport
traces and a manual focused Linux accessibility mode will investigate this
failure while pull requests retain the complete mandatory platform matrix.

An owned native Editor Time Profiler captures 17,345 CPU samples; 41 have no
backtrace. Accessibility document construction appears in 13,981 ms (about 81%)
of inclusive sampled CPU weight, primarily on workers. Grapheme iteration and
bidi classification dominate its leaf stacks. Startup, validation and cleanup
are included, inclusive weights overlap and this is not a comparison or a
phase-calibrated CPU total. Raw trace, workload, exports and summary remain in
`.artifacts/kael-editor-schema3-time-profiler*` with the binary hash recorded.
An ASCII word-segmentation path now preserves the existing Unicode policy,
including CRLF graphemes and vertical-tab whitespace. Independent Unicode
reference checks cover every ASCII byte pair and longer generated sequences;
all 100 core accessibility tests and strict native Clippy pass. Full paired
before/after Editor runs are pending before claiming a performance improvement.

The first ASCII before/after sequence yields three accepted captures and then
rejects a fourth when churn has zero renders/draws/submissions. Its raw logs
remain in `.artifacts/editor-ascii-word-policy-matched`; this incomplete sequence
establishes no performance improvement. The new macOS controller uses an
AppKit helper to activate only its owned child at active/churn begin, with a
shared-parent PID check and recorded acceptance. Both frozen variants use that
same controller in a separate full sequence; native painting is still required.

The focused GTK trace shows repeated 0×0 viewports. Early realization reads the
Fixed widget before allocation, overwrites requested bounds with zero and also
clears the scene's size request. The backend now preserves the last valid size
through that transient stage and observes actual Fixed allocation after native
painting, without adding an idle polling clock. Full GTK protocol verification
of this correction remains pending.

## Subsequent native diagnostics

The complete [six-run ASCII word-policy diagnostic](2026-10-03-editor-ascii-word-policy-diagnostic.json)
now passes all workload, viewport and per-phase frame checks: three before and
three after runs using the same owned-child foreground controller. Median
active process CPU changes from 68.88% to 63.68%, and churn CPU from 71.37% to
67.43%. Median peak RSS changes from 117,850,112 to 116,080,640 bytes, and draw
p99 from 2,479 to 2,446 microseconds. These are local diagnostics with roughly
unchanged memory and draw cost. The workload is time-paced, operation counts
differ and the shared desktop is uncontrolled; this does not establish a
general framework ranking or a controlled power advantage. The earlier
incomplete sequence remains excluded in full.

A separate [fresh native tree pair](2026-10-03-native-tree-foreground-validation.json)
passes the original full-model, selection, disclosure and mounted-reveal
oracles in both engines. Both use the same phase foreground controller and
stable 1100×760 client viewports at scale 2. Active/churn render, draw and
submission counts are 317/292 for Kael and 649/642 for GPUI Kit. Those counts
demonstrate phase activity; one repetition does not establish comparative
performance. The earlier failed GPUI reveal pair remains preserved and
excluded. Its diagnostic binary adds failure-only logging without changing
behavior or weakening the oracle. All five contracts now have accepted native
geometry/activity pairs; fresh repeated schema-3 hosted execution remains.

Focused [Linux native execution at `2694c4b`](https://github.com/Augani/kael/actions/runs/37155884997)
passes both full 100,025-node X11/GTK outline clients and the complete X11
Unicode editor protocol. GTK now has a valid 1280×800 allocation and exposes
the complete Unicode text. Its read-only/disabled guards, range geometry,
selection, Copy, Cut and atomic Cut Undo/Redo pass; Paste fails with an
unchanged native scalar count (63,390 versus the required 63,404), with Busy
false. This narrows the remaining failure to clipboard consumption rather
than initial semantic discovery. The earlier `921a4ce` GTK outline focus
failure remains recorded; the later passing focus trace identifies the
focused semantic root and active descendant.

The GTK reader synchronously drained the stream returned by an asynchronous
clipboard open. GDK can supply a pipe whose local producer still needs that
same main context to serialize content. The reader now asynchronously opens
and drains bounded chunks, with a three-second cancellation deadline. Exact
byte-limit and one-byte-over-limit cases cover chunk boundaries. A fresh
focused job checks these tests, strict native lints and all four unchanged
native protocol clients before this is counted as a runtime correction.

The fresh macOS Unicode fixture after the ASCII change is not accepted: the
physical display is asleep, the fixture remains inactive and a later native
Copy does not complete. An owned-child foreground request is rejected. The
earlier complete AppKit proof remains a historical checkpoint; no failed
rerun replaces it or waives fresh physical acceptance.
