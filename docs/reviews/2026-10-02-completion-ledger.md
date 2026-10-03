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
| R4 | Matched product-shaped performance workloads, startup/interaction/idle/RSS/GPU/frame/churn measurements and environment metadata | Runnable same-contract Kael/GPUI Kit applications; raw repeated measurements; explicit power/wakeup capture and fair comparison gates | Five maintained contracts now cover navigation, Unicode editor, table, 100,025-node tree and persistent docking; both new native engine fixtures pass. Historical sixty-run CI succeeded; current hundred-run comparison, controlled power and presentation comparison remain |
| R5 | Built-in rendering parity for translucent effects, clips, gradients, text, resize and device loss | Pixel/runtime matrix on native Metal, DX11, Blade and browser engines; no silent skip treated as pass | Local Metal/Blade/SwiftShader proof plus DX11/WARP and Linux llvmpipe CI; fresh text positioning, GTK and native populated-frame checks remain |
| R6 | Full logical accessibility for virtualized controls while viewport-only rendering remains | Offscreen-node or active-descendant semantics plus native assistive-technology exploration/actions; stable IDs/focus and large-model mounting tests | Actual macOS Editor Unicode/selection/geometry/reveal/edit/clipboard/composition/guard protocol passed with pasteboard restoration; browser mounted-priority projection passes wide/compact scale runtime. Native Windows/Linux, screen-reader exploration and physical IME remain |
| R7 | Idle accessibility actions wake every native window correctly | Platform-thread/lifecycle/action-queue tests plus idle action execution on macOS, Windows and Linux AT adapters | macOS idle/protocol proof; Windows/Linux/GTK source and clients implemented, native CI pending |
| R8 | Unified memory policy for decoded images, pending work, completed default assets, scratch and atlases | Checked byte/count admission before expensive work; pressure shedding and retained-owner/cancellation tests on each backend | In progress |
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
visual/performance ranking. Fresh full runs are required.

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
