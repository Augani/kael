# Native quality follow-up — 2026-10-03

This records the unlocked Mac's fresh acceptance work after the earlier
`0adb736`/`bb891c7` checkpoints. The full requirements remain in the
[completion ledger](2026-10-02-completion-ledger.md); the evidence below does
not establish a cross-framework ranking.

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
identities, split topology, active content and persistence. Fifteen collector
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
