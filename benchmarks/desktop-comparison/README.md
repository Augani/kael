# Reproducible desktop comparison

This application pins GPUI Kit 0.7.0 at
`3467e647600290343885b500bd7464057e334d18` and builds Kael from the local source.
Both binaries compile the same navigation/detail scene, shared 100,000-row
model, 1100×760 window, font, row height, selection sequence, scroll positions,
idle intervals, and sustained model replacements. Both initialize their default
component libraries. It is a comparison of this concrete application contract;
Four additional binaries compare the shipped editors, data controls, virtual
trees and persistent docking using the contracts below. Browser applications
require separate contracts.

Use the same compiler and release profile for both binaries:

```sh
cargo build --manifest-path benchmarks/desktop-comparison/Cargo.toml --locked --release --no-default-features --features gpui-kit-engine,frame-timing
cp benchmarks/desktop-comparison/target/release/kael-desktop-comparison .artifacts/competition/binaries/gpui-kit
cargo build --manifest-path benchmarks/desktop-comparison/Cargo.toml --locked --release --no-default-features --features kael-engine,frame-timing
cp benchmarks/desktop-comparison/target/release/kael-desktop-comparison .artifacts/competition/binaries/kael
python3 benchmarks/desktop-comparison/measure.py --kael .artifacts/competition/binaries/kael --gpui-kit .artifacts/competition/binaries/gpui-kit --output .artifacts/competition/native-navigation-detail --repetitions 5
```

Create the output directories before copying binaries. Run on an otherwise idle
native desktop with the same display, refresh rate, scaling, power source, and
thermal conditions. Do not collect results during compilation or other tests.
The runner alternates engine order and retains every raw application log,
process sample, frame duration, environment record, and binary digest. A failed
application, missing phase, or missing measurement rejects a run.
The environment record captures display properties, power/thermal reports and
a checkout fingerprint that includes untracked source files. Record build-time
source separately when the checkout changes between compilation and collection.
The scheduled/manual performance workflow retains all one hundred runs as artifacts
(five contracts, two instrumentation modes, five repetitions per engine);
hosted-runner measurements remain diagnostic unless their desktop conditions
satisfy the same comparison contract.

`first_render_us` measures entry into the first root render callback;
`first_submission_us` includes native platform startup and the first CPU-side
frame submission. CPU draw covers `Window::draw`, while CPU submission covers
the native renderer call. Render callback intervals include pacing. None of
these claims GPU completion or the time a compositor shows a frame. Both builds
include their optional frame instrumentation; inspect its overhead separately
before asserting an advantage in uninstrumented applications.

To measure framework instrumentation overhead, build each engine again with
its engine feature alone, omit `frame-timing`, and save separately named binaries.
Pass those binaries to the collector with `--without-frame-timing`. The collector
requires the reported mode to match and rejects any unexpected framework frame
records. Both modes retain the same application callback and process counters;
the disabled mode does not report CPU frame duration or first submission.
The performance workflow preserves five alternating runs per engine in each mode.

## Native editor document contract

`component_comparison` renders each framework's shipped editor in a 1100×760
window using Menlo at 14 px with a 21 px line height and plain syntax. The same
416,000-byte Unicode Rust/Markdown fixture contains 16,001 logical lines. Its
stable exact-byte fingerprint is `fnv1a64:694401c508afd37d`.
Both component libraries use their public dark theme.

After a one-second warmup, both engines run five seconds idle, twelve seconds
editing, five seconds idle, and twelve seconds editing with document replacement.
Selection, replacement, routed undo, routed redo, caret scrolling and returning
to the start each receive a separate paced update. During churn, both replace
the complete document every thirty updates. Idle phases blur the editor while
keeping the same document visible. Exact-byte document checks run at phase
boundaries, outside the paced operation samples; a mismatch fails the application.

The build commands above also produce `target/release/component_comparison`.
Copy it after each engine build to separately named binaries, then collect:

```sh
python3 benchmarks/desktop-comparison/measure.py \
  --kael .artifacts/competition/binaries/kael-editor \
  --gpui-kit .artifacts/competition/binaries/gpui-kit-editor \
  --contract native-editor-document-v1 \
  --output .artifacts/competition/native-editor-document --repetitions 5
```

Repeat with the two editor binaries built without `frame-timing` and pass
`--without-frame-timing`. The collector requires the same fixture, typography,
window, all seven operations and four successful exact-byte checks. `--quick`
and `--sections N` are local smoke options for the application; the collector
rejects these altered workloads as measured comparison evidence.

## Native data table contract

`data_comparison` renders Kael's `VirtualSheetGrid` and GPUI Kit's `DataTable`
in the same 1100×760 window. The table occupies 780×704 px, with eight 112 px
columns, 28 px rows and one fixed column. Both use Menlo at 13 px with a 20 px
line height, 12 px header text and their public dark theme. The shared outer
header height is 32 px; GPUI Kit's inner header remains 28 px while Kael's is
32 px. The explicit geometry requires both horizontal and vertical scrolling.

Both engines retain two immutable 100,000-row datasets with the same Unicode
cell values. Dataset zero contains 12,166,666 bytes of cell text and has the
fingerprint `fnv1a64:4ea6da623acce6b5`. The active phase separately exercises
selection/reveal, vertical scrolling, horizontal scrolling, reversing the query,
Home and End. Churn swaps the two prebuilt models every thirty updates; it
measures snapshot replacement and invalidation, without allocating a new model.
Cell editing is outside this contract. Kael loads bounded local tiles; GPUI
Kit's delegate reads the immutable dataset directly.

At each phase boundary, outside measured work, both engines verify every
ordered model cell, query identity, native selection and eight values obtained
from the actual control. A nonempty viewport must stay within 64 rows and eight
scrollable columns. Validation markers exclude these checks and their frames
from process and frame measurements. First submission measures the first window
frame; it does not establish that deferred table values have reached the pixels.
Verify populated baseline screenshots separately before publishing comparisons.

The build commands also produce `target/release/data_comparison`. Copy it after
each engine build and collect both instrumentation modes:

```sh
python3 benchmarks/desktop-comparison/measure.py \
  --kael .artifacts/competition/binaries/kael-data \
  --gpui-kit .artifacts/competition/binaries/gpui-kit-data \
  --contract native-data-table-v1 \
  --output .artifacts/competition/native-data-table --repetitions 5
```

The collector requires the fixed fixture, typography, geometry, every operation
and all four successful control/model oracles. It rejects `--quick` and altered
`--rows N` smoke workloads. Repeat with the binaries built without `frame-timing`
and pass `--without-frame-timing`.

## Native virtual tree contract

`tree_comparison` renders Kael's `VirtualTreeList` and GPUI Kit's `Tree` in a
780×704 px area, with Menlo 13 px/20 px typography, 28 px rows and 14 px indent.
The shared 100,025-node fixture has 25 roots, each with 4,000 Unicode file labels;
two immutable datasets retain 6,100,975 label/identity bytes each. Their hashes
are `fnv1a64:0fbc5b81edc32f9b` and `fnv1a64:1bfc4b56d6a0a5eb`.

Paced selection/reveal, vertical scroll, root collapse/expand, Home and End run
against each shipped control. Churn replaces the model every thirty updates.
Kael uses a controlled immutable model rebuild for expansion; GPUI Kit uses its
public routed expansion actions. Wrapper/index rebuilding stays inside measured
updates. Kael exports complete logical semantics; the pinned competitor exposes
mounted rows. These implementation differences are reported explicitly.

At each of the four phase boundaries, the oracle checks every visible native
control ID, label and depth, the exact projection hash, stable selection and a
physically mounted reveal target. Mounts must stay within 64 rows. The independent
Python collector reconstructs the projection hash and selected/revealed indices.
Collect separately named engine binaries using `--contract native-virtual-tree-v1`;
repeat both instrumentation modes. `--quick` and altered `--children` are rejected
as comparison measurements.

## Native persistent workspace contract

`workspace_comparison` renders `DockWorkspace` and GPUI Kit's `DockArea+DockSkin`
with twelve panes, each containing 32 identical Unicode lines, in the same
1100×704 px area. It begins with three tab groups and two nested horizontal/vertical
splits at 0.5 ratios. Each dataset contains 22,560 bytes, with hashes
`fnv1a64:8575d5bda29bda91` and `fnv1a64:42c3940aeb810ba1`.

Paced public operations select/move tabs, split/merge panes, resize the root split,
zoom/unzoom a group and serialize/restore actual native JSON. Churn alternates
pane content every 32 updates. Native pane reconstruction and persistence costs
remain in measured updates. Each phase verifies all twelve IDs, topology, active
Unicode content, split ratios and complete JSON restoration outside timing.

The engines retain their native chrome: Kael's minimum tab strip is 34 px and
the pinned DockSkin bar is 30 px. Binary/n-ary same-axis splits are normalized
for semantic comparison. Requested resize tolerance is 0.005 and persistence
float tolerance is 0.01 px. GPUI Kit does not serialize zoom, so persistence runs
after unzoom. This contract excludes floating panes and edge docks. Collect with
`--contract native-dock-workspace-v1`, in both instrumentation modes; `--quick`
is smoke evidence only. `--quick-compact` exercises an 880×640 px native window
and is likewise rejected as a measured workload. Initial split sizes and later
resize operations use the actual measured native container, because AppKit can
constrain the requested window to the display. Both engines normalize the
initial 0.5 root ratio after the one-second warmup and settle for 40 ms before
the first measured phase. The original geometry and persistence tolerances
remain unchanged. CI executes the compact regression for both engines and both
instrumentation modes before collecting full runs.

On macOS, the shared adapter uses the SDK's `proc_pid_rusage(RUSAGE_INFO_V4)` for
CPU, resident bytes, physical footprint, idle/interrupt wakeups, I/O, and raw
billed/serviced energy counters. Energy fields are retained as OS counters; no
conversion to watts is assumed. Metal's actual device allocation counter is
sampled at phase boundaries for both engines. Linux and Windows need native
process/power adapters before their measurement contracts can be completed.

The collector retains at most 4,096 application frame samples per run and 300
Kael frame/submission records in the window. Process sampling runs externally
at 250 ms. Idle intervals issue no periodic UI updates. Navigation churn
replaces 100,000 owned labels every 30 updates; its cost includes application
allocation and framework invalidation. Data-table churn swaps retained snapshots
instead. Preserve startup/cold outliers and
report per-run percentiles and distributions before combining results.
