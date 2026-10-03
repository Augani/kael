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
