# GPUI Kit comparison and quality gates

The user identified [GPUI Kit](https://gpui-kit.com/) as Kael's competition on
2026-10-02. The comparison source is pinned to GPUI Kit 0.7.0, commit
[`3467e647600290343885b500bd7464057e334d18`](https://github.com/longbridge/gpui-kit/tree/3467e647600290343885b500bd7464057e334d18).
Its facade uses the published GPUI 0.3.7 packages. Published capabilities and
observed source are useful review inputs; they do not prove performance rankings.

## What sets the competitive bar

GPUI Kit combines styled components with unstyled behavioral primitives and an
application shell. Kael needs consistent composition, keyboard behavior, theming
and examples across those layers. Adding component names is insufficient. See
the [component documentation](https://gpui-kit.com/component/) and
[base primitives](https://gpui-kit.com/base/).

Its [docking system](https://gpui-kit.com/component/dock/) makes persistent,
rearrangeable workspaces a concrete parity requirement. The benchmark for Kael
is a working workspace with nested splits, tabs, floating panes, restoration,
pointer interaction, and keyboard access.

Its [editor](https://gpui-kit.com/component/editor/) and
[data table](https://gpui-kit.com/component/data-table/) set useful targets for
large documents and data applications. Kael's existing controls must support
real selection, editing, IME, undo, clipboard, scrolling and accessibility under
sustained workloads. Advertised large-model capacity needs measured interaction
and memory costs, rather than an empty construction example.

The [full completion ledger](2026-10-02-completion-ledger.md) retains these
requirements alongside portable GPU effects, bounded resource ownership and
native platform verification. This comparison does not remove requirements from
the original review.

## Maintained matched application

[`benchmarks/desktop-comparison`](../../benchmarks/desktop-comparison/README.md)
builds two native binaries from the same application source. Both initialize
their component library and render the same core virtual navigation/detail
scene: 100,000 owned row labels, a 1100×760 window, identical typography and row
height, deterministic selection/scrolling, two idle intervals, and repeated
100,000-label replacements. Engine-specific component workflows will need
separate matched contracts; this initial contract does not compare the editors,
trees, tables or docking implementations.

The maintained `native-editor-document-v1` contract now uses the two shipped
editors with the same 416,000-byte, 16,001-line Unicode Rust/Markdown document,
Menlo 14 px/21 px typography and 1100×760 window. Selection, replacement,
routed undo/redo, caret scrolling and document churn receive separate paced
updates. Each phase validates the full document against an exact-byte oracle.
Both engine builds compile; native execution and repeated results are still
pending. The collector rejects altered fixtures, smoke durations and missing
operations. This implementation is not a measured editor advantage.

The `native-data-table-v1` contract also compiles both shipped controls against
the same 100,000-row, eight-column Unicode fixture. It requires two-axis scrolling,
one fixed column, selection/reveal, query reversal and replacement of two retained
immutable snapshots. Boundary checks verify all 800,000 ordered model cells and
eight actual control values. Explicit validation phases exclude oracle CPU and
frames from measured phases. Both component contracts use public dark themes.
Table header geometry and tile/delegate differences are recorded. Native
populated-frame checks and repeated measurements remain pending; first window
submission alone does not prove that deferred table values are visible.

Both binaries use the same compiler and release optimization profile. The runner
alternates engine order and saves raw application logs, process samples, per-run
percentiles, revision/environment metadata and binary hashes. Missing phases,
missing instrumentation, failed applications and invalid contracts reject a run.
At least five runs per engine are required for the maintained comparison.

Measurements distinguish root-render entry, first CPU frame submission, CPU draw,
CPU submission, pacing intervals, RSS, physical footprint, process wakeups and
Metal device allocation counters. CPU submission does not prove GPU completion
or compositor presentation. Device allocations do not equal process RSS. Both
binaries in the first diagnostic dataset enable their optional timing
instrumentation. The maintained workflow now builds both engines with and
without framework frame timing and retains five alternating repetitions per
engine, mode and contract. Both modes retain common application callback and
process counters. No overhead result is claimed until these runs finish.
The three contracts produce sixty retained runs in the maintained workflow.

macOS process CPU counters are Mach absolute ticks. The collector preserves the
raw values and converts them using `mach_timebase_info`, with widened arithmetic.
This follows [Apple's XNU measurement tests](https://github.com/apple-oss-distributions/xnu/blob/main/tests/recount/recount_perf_tests.c).
Billed/serviced energy fields remain raw OS counters; they are not converted to
watts or treated as physical energy measurements.

## Gates before claiming an advantage

1. Preserve all repetitions and cold-start outliers, and publish the application
   contract with the source and build configuration.
2. Compare idle wakeups and sustained memory as well as median frame duration;
   report p95/p99 and startup separately.
3. Record display refresh, scale, power source, system activity and thermal
   conditions. An active shared desktop is a diagnostic environment, not a
   controlled ranking laboratory.
4. Verify GPU completion/presentation with platform tooling, then repeat native
   workloads on Windows and Linux. Compilation alone does not satisfy this gate.
5. Exercise matched editor, data, workspace and filesystem workflows, including
   accessible offscreen exploration and real pointer/keyboard interaction.
6. Require pixels, lifecycle/pressure tests and working public examples for GPU
   extension claims across the supported rendering backends.

## First diagnostic results

All ten native application runs completed on this Apple M2 Pro, 16 GiB,
macOS 27.2 desktop with Rust 1.97.1 and AC power. The table reports medians of
five per-run measurements for each engine; draw percentiles are calculated per
run before taking their median.

| Measurement | Kael | GPUI Kit |
| --- | ---: | ---: |
| First CPU frame submission | 457.7 ms | 436.2 ms |
| CPU draw p95 | 5.458 ms | 5.163 ms |
| CPU draw p99 | 5.760 ms | 5.535 ms |
| Active scrolling CPU, one core = 100% | 19.78% | 22.03% |
| Idle-after CPU | 0.009% | 0.563% |
| Idle-after interrupt wakeups | 0.87/s | 120.77/s |
| Active RSS | 118.55 MiB | 112.00 MiB |
| Active peak physical footprint | 107.13 MiB | 91.77 MiB |
| Churn peak physical footprint | 121.86 MiB | 91.95 MiB |

Kael's wakeups fell when interaction stopped. GPUI Kit continued waking near
the display rate. Kael used less CPU during active scrolling, while its draw
tail, startup median and physical footprint were worse in this contract. Those
are optimization targets, not evidence that Kael already wins every dimension.
The first submissions retain cold outliers: 1,109.8 ms for Kael and 798.6 ms for
GPUI Kit. Individual GPU allocation samples and all per-run measurements are in
the [machine-readable results](2026-10-02-gpui-kit-benchmarks.json).

Raw process/frame/application captures are retained under
`.artifacts/competition/native-navigation-detail/`. The original collector
mislabeled Mach ticks as nanoseconds. Its untouched captures are separately
retained under `native-navigation-detail-v1-raw/`; corrected CPU results use this
machine's timebase ratio, 125/3, following Apple's API and tests. Other counters
and application timings are unchanged. The maintained collector now records
both units and its timebase explicitly.

Compilers and GPU tests were paused during collection, but other desktop
applications remained active and the initial one-minute load average was 8.01.
Display refresh/scale, GPU completion and thermal traces were not captured.
Energy fields are raw counters and provide no reliable power comparison here.
The executable digests identify the measured binaries; the workspace was still
under development. A repeat against finalized source, controlled desktop and
uninstrumented builds is required before publishing a ranking.
