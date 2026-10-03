# Desktop component completion track — 2026-10-02

This track implements the filesystem explorer, persistent workspace/docking and structured property-inspector requirements from `2026-10-02-performance-and-components.md`. It builds visible interactions on existing viewport, input and splitter infrastructure. Other renderer, shader, memory and benchmark tracks remain owned by their separate completion work.

## Filesystem explorer

`FileTreeEntry` stores one shallow entry with a normalized sort key computed when the entry is created or renamed. Directories retain sorted child paths. `FileTreeState` builds reusable shallow `VirtualTreeModel<PathBuf>` snapshots only when listings, expansion or hidden-file visibility change. Redraws clone the snapshot Arc; `VirtualFileTree` mounts the requested viewport range through the existing uniform-list renderer and keeps one logical keyboard focus handle.

The native loader enumerates only an expanded directory, reads metadata on a background worker, and classifies symbolic links without traversing them. `new` accepts a blocking custom loader; `with_async_loader` accepts a Send boxed future for async Rust storage clients. Generation checks reject stale results; dropping a queued task cancels it and a cancellation token lets running sources stop cooperatively. Duplicate/non-child results fail explicitly. Failures render beside the directory and remain retryable through Reload.

The per-directory entry limit defaults to 100,000 and the retained source cache has a 250,000-entry hard budget. Listings that would exceed either limit fail without partial insertion. `evict_directory` releases collapsed subtrees and cancels pending descendants. Bulk refresh/eviction walks removed child paths once, including deletion of an entire large directory; it does not scan the entire cache once per removed file.

Selection, double-click/Enter open, disclosure/Left/Right expansion, Home/End navigation, right-click/Shift-F10 menus, built-in Open/Reload/Pick-up/Move-here and application-specific context actions are wired. Pointer drag/drop and keyboard movement share validation that rejects roots, cycles, leaf/symlink targets and same-parent no-ops. File writes stay application-owned. The runnable `filesystem_explorer` example performs real background rename operations in an isolated demo directory, refuses overwrites, and reloads both parents. Passing an external root makes that example read only.

The legacy `FileTree` also receives borrowed flattening and cached-key sorting at ingestion. Its eager per-row elements remain appropriate only for small trees.

Thirteen filesystem tests passed in the complete UI suite, including real keyboard context-menu dispatch and pointer drag/drop. Adversarial coverage includes stale generations, duplicate/non-child listings, atomic global-cache admission, coalesced visibility changes, bounded pending/prepared listings, cancellation when the controller is released, native symlinks and weak-owner release after eviction/refresh.

Catalog edits, reusable row snapshots, complete logical tree accessibility, and outgoing catalog/model/AX reclamation now run on workers. Only generation-checked prepared Arc swaps and bounded request metadata commit on the foreground thread. Expanded-directory membership uses an immutable shared set plus bounded worker/pending delta batches; foreground branch toggles do not clone a directory-heavy set while a worker retains it. `try_set_expanded` rejects a new distinct change once 4,096 pending changes accumulate, leaves the requested expansion intact and permits retry after preparation. Repeated changes coalesce. Stale listing results retain unpruned requested expansion and preserve newer pending intent; catalog pruning is adopted only for a valid source result. The source pipeline defaults to four in-flight/completed listings and 32 queued paths. A superseded view generation cancels row preparation and one preparation task coalesces later changes.

The latest optimized 100,000-file workload in `.artifacts/kael-filesystem-release-workload.log` passed with 100,001 logical TreeItems and at most seven physically bounded items. Source loading plus worker preparation took 162.529 ms; worker catalog/model/accessibility preparation took 125.870 ms; the foreground prepared-Arc commit took 47.083 µs. TestPlatform window creation plus first draw took 3.236 ms; 20 forced redraws had p50 135.625 µs and p95 149.042 µs; End dispatch plus draw took 462.250 µs. Refresh/reclamation took 68.160 ms overall and a 3 µs foreground commit. This run includes the full surviving accessibility identity catalog and bounded expansion delta changes. Other builds were active: these are diagnostic CPU layout/draw and model measurements on TestPlatform, not native GPU submission, compositor presentation or an uncontended product comparison.

Reproduce with `cargo test -p kael_ui --release --locked --offline --lib hundred_thousand_files_mount_only_viewport_and_bulk_refresh_releases_entries -- --nocapture`. The runnable release explorer accepts `--stress`; native first-paint/presentation and assistive-technology proof remain tracked with the native workflow checks.

## Persistent desktop workspace

`DockLayout` is a versioned serde contract with nested horizontal/vertical splits, stable tab groups, active tabs, floating-group bounds, closed panes and zoom. `DockWorkspaceState` validates all node/pane IDs, geometry, split depth and registry coverage before restore. Invalid snapshots leave the current layout intact. Runtime render callbacks and entities are registered by stable pane ID and are not serialized.

`DockWorkspace` renders real `SplitPane` dividers, active pane content, draggable tabs and whole-group grips, merge/reorder and edge split targets, group zoom/restore, close/reopen, and floating groups that can be moved/resized with the pointer or keyboard and docked again. Ctrl-Shift-arrow docks the active pane against a workspace edge. Floating groups are inside the application workspace window; this API does not create OS windows. Bounds clamp when restoring into a smaller workspace. Inactive pane content is unmounted while its application-owned entities remain in the pane registry.

`DockWorkspaceEvent::LayoutChanged` supports application-owned persistence. The `desktop_workspace` example uses actual background JSON save/restore, combines a real lazy project explorer with property editing, live preview and retained text notes, and demonstrates reset, float, close and reopen workflows. The splitter captures only its three color tokens rather than cloning the whole theme for every nested split.

Native macOS QA exercised actual workspace pointer dragging, floating move/resize/redock, tab rearrangement, bottom nested splits, save/restore and inspector text/width undo/redo. Floating panes remained inside the workspace window.

Six docking tests passed, covering nested pane moves, floating geometry, persisted zoom/layout round trip, atomic invalid-layout rejection, splitter persistence/pruned-state release, and real accessibility Click actions for tab selection, Zoom/Restore, Float/Dock, Close/Reopen. Actual pointer gestures exercise split resizing and floating move/resize; keyboard gestures exercise edge docking and floating geometry. Independent review added near-ID-rollover and depth-32 regressions: mutations reserve identities before detaching a subtree, and an invalid result rolls back atomically rather than producing a layout its own restore rejects.

## Structured property inspector

`PropertyInspectorState` declares stable groups and typed text, number, boolean and choice fields with labels, descriptions and read-only state. It validates schemas and values, rejects mixed-type/invalid-range/invalid-choice edits, and exposes values/events. `set_values` validates a whole batch before applying it. Undo/redo applies whole transactions and retains a bounded history (128 by default).

The visible `PropertyInspector` uses existing Input, NumberInput, Checkbox and Dropdown components, renders validation errors, provides collapsible groups and Undo/Redo, and synchronizes editor contents after programmatic edits/history changes. Ordinary text edits preserve the cursor instead of resetting it on redraw. The workspace example changes a live preview through these actual controls.

Four inspector tests passed, covering atomic typed batches, bounded history, numeric/choice/type rejection, actual rendered Checkbox/choice-menu/Undo actions, numeric accessibility SetValue and Unicode text editing without caret reset. Ten thousand rejected unknown-field IDs retain no unrenderable error-map entries or history, preserving bounded state.

## Document and remote-data workflows

`document_data_workbench` uses a real product-launch Markdown document, the existing rope editor, a worker-parsed rich preview and formatting/Undo/Redo controls. Its fixture contains headings, tasks, tables, code, Japanese text, accented letters and a joined emoji. Preview parsing observes content revisions rather than cursor-only notifications and discards superseded parses. Run `cargo run -p kael_ui --features markdown --example document_data_workbench`.

Editor fixes make selection replacement one undoable operation, honor explicit empty UTF-16 replacement ranges, normalize reversed selections and replace the current marked range when an IME omits its range. Repeated preedit plus composition commit coalesce into one history operation. Cursor movement and deletion use Unicode grapheme boundaries. Four editor document tests passed, including real rendered keyboard selection/edit/clipboard and Japanese/emoji input-protocol composition with undo/redo (`.artifacts/kael-document-editor-tests.log`). Native macOS QA additionally exercised keyboard selection/formatting/undo and Unicode paste/undo in the running document. Physical native IME interaction remains a separate runtime requirement; Unicode paste and input-handler protocol tests do not substitute for it. Native accessibility text selection was found unavailable during QA. The common/native text-run bridge and editor integration now retain complete immutable Unicode document metadata and expose checked byte selections. Documents above 16 KiB prepare on a worker with one coalesced revision job; caret-only updates reuse the document Arc, and superseded metadata reclaims on workers. Until preparation completes, the editor exports a bounded value with BUSY and omits the selection action. Native prepared documents omit a copied root value; text runs supply the full document. `EditorState::set_selection_bytes` validates exact UTF-8 boundaries atomically and preserves selection direction without changing content/history. Two editor bridge tests passed, including complete/reversed Japanese+emoji selection, reused metadata, stale revision rejection, coalescing, worker release and file-load revision invalidation (`.artifacts/kael-document-accessibility-tests.log`). Fresh native selection protocol and screen-reader checks remain separate evidence.

The editor's unfolded document coordinates now map directly in O(1), with no document-sized displayed-line vector in paint, hit testing or navigation. Collapsed folds retain a shared interval index proportional to effective folds; line/row conversion uses binary search, and each paint enumerates only its viewport. Nested/overlapping/clamped folds preserve their existing semantics. A caret request inside a collapsed fold reveals it. Shaped-line storage prunes against actual visible buffer lines, including gaps across collapsed ranges, rather than retaining every line between the viewport's endpoints. Large document syntax preparation clones the Rope, flattens/parses/extracts available fold metadata on the worker, cancels superseded preparation and checks revision/language before commit. Fourteen editor tests passed with the real Rust grammar, including 20 coalesced source revisions, 100 nested/overlap reference models, a rendered 100,000-line document, retained fold-index reuse and folded caret reveal (`.artifacts/kael-editor-index-tests.log`). Twenty unfolded debug TestPlatform draws took 59.569 ms in this diagnostic run; this is CPU evidence, not native presentation.

The core shared shaped-text cache now retains two bounded pools totaling at most 4,096 entries and 8 MiB of charged allocations. Its second-chance clock avoids sorting/temporary eviction lists, oversized layouts bypass admission, and wrapped text cache keys include the line-clamp limit. Pressure clears the shared pool. This is a cache retention budget; document storage, active viewport owners, undo history and GPU atlases have separate lifetimes and are not included in those 8 MiB.

`RemoteSheetState<Q>` retains the existing virtual spreadsheet renderer and adds bounded asynchronous tile jobs, captured query identities, cooperative cancellation and stale-generation rejection. Invalid responses pause fetches until explicit refresh, avoiding a redraw/retry loop. A query change retires positional edits/history on workers; refresh of the same records preserves them. The `Edited` event captures the query and generation at edit time so delayed application writes cannot reinterpret a cell against a newer sort/filter.

The workbench runs a bounded loopback HTTP source with 100,000 logical records, eight named columns, sparse remote edits, actual POST requests, query reversal and frozen first row/two columns. Application writes use the captured record identity, a bounded serialized write queue and visible retry controls. Two real HTTP tests passed (0.23 s): Unicode/tab edits persist across reload and reversed queries, rejected requests preserve source contents, a request cancels while awaiting a network reply and the service remains usable (`.artifacts/kael-document-data-http-tests.log`).

The latest display suite passed 48 tests (22.45 s), including actual Enter/typing/commit, clipboard TSV with quoted tabs, undo/redo, frozen panes, query cancellation/stale replies, bounded mounts and source-failure recovery (`.artifacts/kael-grid-remote-workflow-tests.log`). A sustained 100-iteration query/scroll/edit workload keeps cache/pending/mounted counts bounded and checks deferred write identity. Remote edits require loaded baselines: mixed loaded/unloaded paste rejects atomically, undo after cache refresh emits the original remote value, and query changes clear both undo and redo so they cannot write another record.

Grid semantics retain all column headers and logical dimensions, one active cell and at most 1,024 cached cell previews; they do not allocate a semantic node for every possible cell or row in a million-by-XFD sheet. Headers/cached semantic snapshots prepare and reclaim on workers. Coordinate identities reserve constant-sized checked ranges without allocating nodes and change when record identity changes. Tests cover a million-by-XFD reservation, independent scrolling with one focused grid and preserved active descendant, offscreen cached Focus/reveal/fetch, Japanese SetValue and cancelled queued actions after a query reset. Cell previews cap at 4 KiB with a description directing long-value readers to the complete cell editor. Native editing, clipboard and assistive-technology workflow checks remain open until exercised in the running workbench. Native QA exposed a real scrolling defect: nested horizontal column lists remapped vertical wheel movement into horizontal scrolling; the grid now restricts each list to its own axis. QA also found cached remote baseline text missing from initial presentation until a wheel event. The CPU test-platform regressions now trace the exact loaded baseline text at the StyledText paint stage before any focus/edit/scroll, including delayed RemoteSheet completion, with nonempty measured bounds intersecting its clip. The delayed completion test uses a normal platform frame callback rather than forcing a draw/refresh, so it also exercises observer invalidation and layout reuse. Actual vertical and horizontal wheel dispatch preserves the other axis. This is CPU layout/paint evidence (`.artifacts/kael-grid-paint-wheel-test.log` plus the full UI suite); the initial native presentation issue remains open pending a fresh binary check. Accessibility values alone are not accepted as evidence that native text was presented.

## Lifecycle and validation status

A shared internal keyed model observer connects controller notifications to the containing view only while its component is mounted. Observers use weak ownership and are replaced if a component receives a different controller entity; unmount releases the observation. Embedded registry panes therefore update without requiring per-pane observer plumbing in the application.

The complete UI suite with Markdown and the real Rust grammar passed 441 tests in 13.89 s (`.artifacts/kael-components-frozen-ui-tests.log`). The caret now starts its 500 ms timer only during focused, active, visible paint, cancels it on blur/window deactivation, stops retained hidden editors after their outstanding paint lease expires, and stays visible without a timer under reduced motion. Programmatic selection on an unfocused controller does not start a timer. The focused/blurred/deactivated/retained-hidden lifecycle regression passed in that complete suite. Browser compilation passed against the frozen source (`.artifacts/kael-components-frozen-browser-check.log`), and the updated release filesystem evidence is recorded above. Strict all-target Clippy with Markdown/Rust grammar passed (`.artifacts/kael-components-frozen-clippy.log`). The three native workflow binaries rebuilt successfully (`.artifacts/kael-component-workflow-native-build.log`) with the full document bridge, caret lifecycle, bounded fold mapping and scrolling fixes, ready for fresh runtime QA. Native screen-reader exploration, physical IME and cross-platform runtime accessibility remain explicit separate requirements.

## Matched rendered component workload

`benchmarks/desktop-comparison/src/component_comparison.rs` adds a second native comparison contract, `native-editor-document-v1`, against the released pinned GPUI Kit Editor. Both processes render the same 416,000-byte, 16,001-hard-line Unicode Rust/Markdown fixture (`fnv1a64:694401c508afd37d`) in a 1100×760 window with Menlo 14 px and 21 px line height. Syntax is plain on both sides. Each selection, replacement, routed Undo, routed Redo, distant caret reveal and Home uses a separate paced frame; churn replaces complete documents every 30 ticks. Exact-byte document oracles validate each phase outside the paced samples. The fixture/count/font/command counters and CPU draw/submission scopes are reported explicitly. `--quick` is only a smoke mode and is excluded from rankings. Both engine-plus-frame-timing compile checks and the deterministic fixture/offset test passed. Native runs, release timing and wider table/tree/workspace comparisons are separate remaining evidence; a compile check is not a performance result.

## Fresh native acceptance — October 3

The fractional CoreText bounds correction restores complete native labels and
rich text. `.artifacts/native-ui-idle-wake/filesystem-pointer-move.json` records
an actual pointer drag of `readme` into `Archive` in the isolated filesystem
fixture. Both parents refresh, the source disappears, and an independent disk
oracle verifies the 29-byte destination with SHA-256
`10c34bf9ad84ec7ddbadc2943e3bd8e20a2db348575d3dd57c8fd37886296ab9`.
The hidden-file checkbox and complete native text pixels also pass.

`.artifacts/native-ui-idle-wake/document-data-native-interactions.json` records
real Markdown formatting/Undo, a remote `Café 日本語 👩‍💻` edit, captured record
identity across reversed/normal queries, and a 2×2 Unicode TSV paste with one
Undo/Redo transaction and zero pending writes. A native vertical wheel gesture
changes the viewport while row zero remains frozen. Native horizontal wheel
acceptance remains open; it is not inferred from the passing two-axis CPU
regression.

The real Editor native protocol subsequently passes actual clipboard and
composition with restoration and one-step history. Physical input-source IME
remains separate. Fresh workspace pixels show complete inspector/explorer/Notes
text. Fresh integrated pointer tab movement/nested edge splits, keyboard edge docking,
floating move/resize/redock, floating/nested save/reset/restore, Unicode inspector
Undo/Redo and live preview pixels pass in
`.artifacts/native-ui-idle-wake/workspace-native-interactions.json`. The native OS
dead-key path composes `é` with one-step Undo/Redo. Full details and binary
hashes are in [the follow-up](2026-10-03-native-quality-followup.md).

Fresh integrated native End/Home/Right input reveals the final Notes column and
returns to Score while Title/Owner remain frozen and vertical position stays
unchanged. `.artifacts/native-ui-idle-wake/grid-horizontal-native-keyboard.json`
records binary SHA-256
`b890ae056e9d81015609964bacbffcb2179ad5930d53ba085496f804b1ee49e3`.
The prior horizontal wheel attempts are now diagnosed: the CUA tool delivers
`scrollingDeltaX=0` and `scrollingDeltaY=0` on this host, as preserved in
`grid-horizontal-scroll-diagnostics.log`. They provide no nonzero wheel test.
