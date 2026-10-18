# Native document text contract

This follow-on to checkpoint `0adb736` connects Kael's complete immutable text documents to native reveal, atomic editing, and actual viewport geometry. The document model is independent of physically painted text and is prepared once per content revision. Large Editor documents are prepared on a coalescing background worker and outgoing final-owned metadata is reclaimed there.

## Coordinates and identity

Portable text endpoints are checked UTF-8 byte offsets. Native UTF-16 and Unicode scalar offsets are converted by the AccessKit provider using the same immutable run metadata. Selectable units are Unicode scalars; CRLF is one line-break unit. Native selection can therefore address scalar boundaries within a grapheme, while keyboard character movement retains the editor's grapheme behavior. Hard lines are split at resolved Unicode bidi embedding transitions and divided into at most 255 selectable units per exported run so AccessKit's `u8` word indices never wrap. Unicode bidi paragraph/line resolution includes the trailing-whitespace reset; the run direction is retained even when its geometry is offscreen. Segments within a hard line retain `previous_on_line`/`next_on_line` links.

Every replacement document has fresh run identities and a fresh transparent container identity. A normalized selection, reveal, clipboard, or partial replacement carries that immutable document identity. The provider checks the current tree when draining; the deferred Div callback checks the current semantic node again; the Editor checks its current prepared content revision and document identity immediately before applying byte endpoints. Hidden, disabled, removed, unadvertised, and stale actions are rejected. Copy and selection remain supported for read-only documents; mutation is rejected.

Synthetic run lookup visits mounted frame nodes and the immutable snapshots' prepared list of text owners. There is no common NodeMap record, listener, or lookup HashMap entry for every document run. The control root retains one container child across caret changes; native providers retain the complete runs.

## Reveal and editing

`ScrollToVisible` with `TextReveal` exposes text without changing directed selection, caret, marked IME range, focus, blink, content, or history. It opens containing collapsed folds, reveals the logical display row, and completes horizontal reveal after that row is shaped. The optional alignment is honored at the supported viewport edge. Revealing the trailing empty EOF run creates a valid final display row without changing the public content-line count.

The pinned Windows UIA `TextRange.ScrollIntoView` and Linux text range provider target a TextRun ID and an edge hint. They do not carry the requested character index. The portable reveal payload therefore describes the complete targeted run, with at most 255 selectable units. Exact character-level horizontal alignment is not claimed for that native request format.

Partial native edits commit one checked range replacement as one undo operation. Copy reads the supplied range without changing the current selection, history, or focus. Cut copies that range and commits one removal; paste reads the actual platform clipboard and commits one replacement. Full `SetValue` replacement is likewise undoable, while application `set_content` remains the explicit history-reset API. Native adapters transport a partial range and replacement together; they do not simulate a selection setter followed by an edit.

`EditorState::set_read_only` controls mutation; `set_disabled` and `Editor::disabled` remove native actions and tab participation, cancel composition/autoscroll/blink, and reject keyboard, pointer-selection, and IME mutations. Complete text remains readable while disabled.

## Geometry and current limits

`AccessibilityTextGeometry` validates a bounded viewport overlay against its exact immutable document. Run bounds use window logical pixels. Character positions are relative to each run; widths match the same selectable units, including the hard line-break marker. The Editor takes these coordinates from the actual shaped layout rather than estimating by font size or character count. Arrays contain at most 255 entries per run. A retained physical glyph index identifies intersecting source runs with binary viewport lookup; actual native cluster lookup is binary in logical source order. Neither path repeatedly scans every preceding glyph for every unit. Horizontal clipping restricts overlays to intersecting run segments; vertical overlays cover only painted rows.

The Editor retains geometry across caret-only redraws. Content, fold mapping, font size/family, viewport, gutter, and scroll changes invalidate the relevant geometry. Native incremental export updates current and departing viewport runs; departed runs lose their geometry while their complete logical text remains available. Offscreen text is not assigned invented rectangles. Reveal mounts the target row before geometry is queried. The IME `bounds_for_range` now returns the requested range's first visible shaped fragment (or `None` if its starting row is offscreen), rather than the entire editor rectangle; the empty EOF caret gets a real one-pixel rectangle.

`PlatformTextSystem::layout_line_geometry` is an optional, source-compatible native capability. It accepts checked UTF-8 byte spans and returns `LineTextGeometry` with actual logical cluster spans, leading/trailing physical caret edges and direction. `WindowTextSystem::line_text_geometry` resolves the exact painted font runs. This opt-in path keeps per-character geometry out of the shared global `LineLayout` cache. The Editor retains only mounted line font descriptors, physical source indices, and the requested native span overlays; a caret-only redraw reuses the prepared Arcs.

CoreText supplies its real primary/secondary bidi caret offsets. Emoji and combining graphemes keep native cluster endpoints; ambiguous scalar offsets inside a grapheme are not interpolated. The Linux cosmic-text backend retains its actual cluster hitboxes and bidi embedding levels. DirectWrite supplies `HitTestTextPosition` cluster metrics; native Windows runtime acceptance is tracked separately. A ligature/grapheme with multiple selectable scalars can therefore give those scalars the same actual cluster rectangle. Logical scalar selection remains complete, while pointer movement respects the available native cluster endpoints. Exported directional runs keep native range/hit algorithms from treating a mixed paragraph as one monotonic left-to-right run. The Editor uses those same metrics for the drawn caret, pointer mapping, disjoint bidi selection fragments and the IME first-fragment rectangle.

Positioned glyphs use a documented y-down baseline displacement. The common paint path adds this displacement to glyph destinations and culling bounds, while decoration baselines stay fixed. CoreText y-up positions are converted once; cosmic-text x/y glyph offsets and DirectWrite baseline/ascender offsets are retained. This fixes painted marks independently of accessibility rectangles. Select All painting visits only the current viewport source rows. Finishing cursor visibility after a new native shape is a one-shot operation, so later user scrolling is preserved.

The browser and external backends that omit the optional native geometry capability retain the previous monotonic left-to-right fallback. Unsupported non-monotonic runs have no fabricated geometry. Native reveal remains run-granular as explained above. Physical IME and screen-reader exploration require separate runtime proof; this source work does not mark R6 complete.

## Verification

The shared native-consumer regressions exercise left-to-right and right-to-left range bounding boxes and point mapping, clearing departing geometry, long-line word movement across segment boundaries, stale/read-only replacement and clipboard guards, and 1,000 offscreen reveals against a 100,000-line document whose common NodeMap has one node. The existing 100,000-line consumer test preserves the complete text with a one-node caret delta.

The Editor regressions exercise a 100,000-line document with at most 27 run overlays and pointer-equal document/geometry reuse across caret redraws; actual shaped IME bounds and EOF caret mapping; folded offscreen reveal preserving selection/composition/focus/history; atomic clipboard and full replacement undo; delayed old-document selection/reveal/edit rejection; disabled input rejection; and native cluster metrics driving caret, pointer, disjoint selection and IME rectangles. Deferred Div callbacks separately test disable/removal before callback execution for explicit listeners, synthetic clicks, and focus. These tests use TestPlatform CPU layout/paint and AccessKit consumer behavior; they do not claim GPU presentation or native AT runtime acceptance. Native clients use the dedicated `editor_accessibility` example and canonical Unicode fixture, with results recorded separately.
Final source validation is recorded in `.artifacts/kael-components-native-text-final-ui-tests.log`: 449 UI tests passed in 15.65 seconds. `.artifacts/kael-native-text-system-followon-tests.log` records 18 text-system tests passing in 0.45 seconds, including actual CoreText shaping of Japanese, emoji, combining marks, Arabic and Hindi, checked requested spans, and actual native-raster sprite destination capture for independent upward/downward glyph offsets with unchanged decoration geometry. This is CPU layout and scene evidence; native AT and GPU presentation acceptance remain separate.

Strict browser compilation is green in `.artifacts/kael-components-native-text-final-browser-check.log`: `cargo clippy --locked --offline -p kael_ui --lib --target wasm32-unknown-unknown --no-default-features --features browser,editor,markdown,tree-sitter-rust -- -D warnings`. This verifies the optional geometry API and richer action payloads retain browser source compatibility; it does not establish native bidi geometry in the browser fallback.

The shared accessibility checkpoint is `.artifacts/kael-native-text-followon-core-tests.log` (72 passing tests). Earlier source checkpoints remain available in `.artifacts/kael-components-followon-ui-tests.log` (448 UI tests before the native bidi extension) and `.artifacts/kael-editor-bidi-geometry-tests.log` (21 Editor tests); the final full UI result above covers the subsequent one-shot cursor and viewport-only selection refinements.

Fresh native GUI QA then exposed a separate raster contract defect: macOS declared glyph bounds without fractional-position antialias padding, but enlarged the bitmap afterward. Strict atlas admission rejected that bitmap and stopped painting the remaining glyphs in the line. The bounds now include this padding before admission, rasterization returns exactly the declared size, and the existing Core Graphics baseline transform is preserved. `.artifacts/kael-native-text-system-padding-final-tests.log` records 20 passing text-system tests in 0.58 seconds, including exact declared/actual bitmap extents at 1×/2× and complete StyledText scene emission/masks for “Launch plan”, “Bold”, and “Source”. The renderer's `.artifacts/kael-metal-native-fractional-glyph-green.log` separately records actual CoreText-to-GPU coverage for 39 glyphs (24 fractional x positions with fractional y), matching every native CPU raster coverage pixel. Full application screenshots and native AT acceptance are still separate requirements.

## Actual macOS Editor protocol acceptance

The owned `editor_accessibility` app was launched through CUA on the unlocked
Mac with `KAEL_NATIVE_TEXT_SELF_TEST=1`. Its driver obtains the actual Window's
AppKit NSView and calls the exported NSAccessibility objects; it creates no
synthetic nodes and performs no direct Editor mutation to simulate a native
action. Editor reads supply the independent byte/content/history oracle.

`.artifacts/native-ui-followon/editor_accessibility.log` records
`NATIVE_TEXT_ACCESSIBILITY_OK platform=macos` followed by
`NATIVE_TEXT_FIXTURE_COMPLETE`. The passing assertions cover the canonical
105,391-byte/66,983-UTF-16-unit document, complete native values/ranges, Unicode
selection reaching foreground byte endpoints, mounted shaped glyph bounds and
screen hit testing, bounded visible ranges, offscreen reveal preserving
selection/content/history, EOF caret mapping, read-only selection and mutation
guards, disabled action rejection/recovery, selected-text partial replacement
as one undo transaction, native Undo/Redo buttons, and queued old-origin
selection/edit rejection after the native Replace button. CUA independently
enumerated the real AppKit window, complete read-only document and intermediate
edited value.

The executed binary SHA-256 is
`5f8fbe54c757c9a66796d9e8715ce3079d8714ea1a7e8ed27484c1d1114b0b0e`,
recorded in `.artifacts/native-ui-followon/binaries.json`. The driver uses only
the owned process's protocol objects and does not acquire or bypass system AX
trust. Its strict example Clippy check is recorded in
`.artifacts/kael-real-editor-macos-driver-check.log`.

The marker explicitly reports `clipboard=false`: this run did not modify the
system clipboard. Actual clipboard Copy/Cut/Paste acceptance, physical IME and
screen-reader exploration remain separate requirements. The protocol run and
CUA object inspection do not replace GPU presentation/viewport pixel evidence
or the CI presentation callback gate. Linux AT-SPI and Windows UIA text clients
still require their own actual native runtime evidence.

## Fresh clipboard and composition acceptance

The October 3 run in `.artifacts/kael-native-editor-clipboard-ime-runtime.log`
passes the original protocol contract and adds actual Copy/Cut/Paste with
all-format pasteboard restoration, repeated Unicode marked preedit, commit,
single-step Undo/Redo, and actual focused Editor verification. It also checks
native candidate-rectangle queries with a real output pointer, an optional
null pointer and an invalid range. The executed binary SHA-256 is
`176fb35f7ffdf4e304ffb32992b5675674a0ad2a2c770efa3ab4440f8d453b10`.

This native acceptance exposed and verified fixes to the AppKit `NSNotFound`
range sentinel and the candidate-rectangle callback's `NSRange *` ABI. The
marker reports `clipboard=true pasteboard_restored=true ime_protocol=true
physical_ime=false`. A protocol composition check does not substitute for a
physical input source or screen-reader session.
