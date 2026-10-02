# Reproducible desktop comparison

This application pins GPUI Kit 0.7.0 at
`3467e647600290343885b500bd7464057e334d18` and builds Kael from the local source.
Both binaries compile the same navigation/detail scene, shared 100,000-row
model, 1100×760 window, font, row height, selection sequence, scroll positions,
idle intervals, and sustained model replacements. Both initialize their default
component libraries. It is a comparison of this concrete application contract;
Tree, docking, and browser workloads need their own contracts. Two additional
binaries compare the shipped editors and data controls using the contracts below.

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
The scheduled/manual performance workflow retains all sixty runs as artifacts
(three contracts, two instrumentation modes, five repetitions per engine);
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
