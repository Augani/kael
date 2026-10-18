# Kael and GPUI Kit 0.7.3: completed b411 hosted comparison

The [completed run](https://github.com/Augani/kael/actions/runs/39096072548) contains 100 accepted captures: five contracts, two instrumentation modes, two frameworks and five alternating repetitions. Kael uses frozen commit `b411223c7d6e846c9f35a6f04a53d8bf4a73593a`; GPUI Kit uses released 0.7.3 at `67265a36e260ffe03c5aaebdcd8d654dbf892eab`. Later Kael and competitor changes are excluded. The [machine-readable record](2026-10-18-b411-gpui073-hosted-comparison.json) preserves exact source, raw-artifact and executable identities, all five contracts, and the complete audit references. The [older 0d15 comparison](2026-10-18-gpui073-hosted-comparison.md) remains a separate historical cohort.

Both frameworks have the same actual **1100×645 client viewport at scale 1** through all 800 phase boundaries and 50 cross-framework pairs. The virtual runner constrains the nominal 1100×760 request equally. These are actual recorded boundaries, not proof of a 1100×760 viewport.

## General desktop workloads

These are medians of five captures per framework with optional frame timing disabled. Each cell gives **Kael / GPUI Kit**. Process CPU uses 100% for one core. RSS is the maximum sampled value within each capture’s original stable windows, followed by the median across five captures; it is not lifetime peak, retained heap or GPU residency.

| Contract | Active CPU | Churn CPU | Stable sampled peak RSS, MiB |
|---|---:|---:|---:|
| Navigation/detail | 9.95% / 12.64% | 7.17% / 9.66% | 91.95 / 79.23 |
| Data table | 26.48% / 19.31% | 20.66% / 15.98% | 204.02 / 136.16 |
| Virtual tree | 48.33% / 14.03% | 27.25% / 9.66% | 920.30 / 654.28 |
| Dock workspace | 17.08% / 16.26% | 10.95% / 11.88% | 80.91 / 80.44 |

The table and tree measurements remain concrete CPU and memory optimization targets. Tree active CPU is 48.33% / 14.03% and sampled peak RSS is 920.30 / 654.28 MiB in this cohort. The tree contracts also have different accessibility/preparation work and unequal recorded operation totals; these values do not isolate a single cause. RSS alone does not establish a leak.

## Separate CPU timing diagnostics

Timing-enabled executables are separate captures. Each capture computes its original discrete p99, then the five p99 values are summarized by their median. These are CPU-side wall-clock spans, not GPU/compositor completion, presentation latency or visible smoothness.

| Contract | Draw p99 µs, Kael / GPUI Kit | Submission p99 µs, Kael / GPUI Kit |
|---|---:|---:|
| Navigation/detail | 9,070 / 12,783 | 713 / 692 |
| Data table | 17,025 / 21,149 | 744 / 789 |
| Virtual tree | 8,006 / 8,388 | 596 / 466 |
| Dock workspace | 19,239 / 18,367 | 588 / 658 |

## Actual operation counts and component differences

The following are the five actual recorded totals in repetition order, including model replacement. Counts differ between engines and runs; these are not equal-work or per-operation cost estimates. Navigation/detail has no operation counter. The machine record retains every operation type and all timing-enabled counts as well.

| Contract, timing disabled | Kael totals | GPUI Kit totals |
|---|---|---|
| Data table | 387, 341, 378, 384, 362 | 404, 369, 362, 365, 396 |
| Virtual tree | 244, 258, 266, 237, 286 | 339, 331, 331, 331, 334 |
| Dock workspace | 410, 395, 373, 367, 397 | 343, 364, 364, 377, 364 |

Kael tables retain a bounded 16×512-cell tile cache; GPUI Kit’s delegate reads immutable data directly. Native leaf headers are 32px / 28px. Kael prepares complete logical tree accessibility metadata, while this pinned GPUI Kit Tree exposes mounted rows; expansion APIs differ. Dock tab bars are 34px / 30px, and persistence/model/layout rules differ. These disclosed contracts do not establish pixel or feature parity. Native editing, physical input, floating panes and edge docks are excluded where declared. Full logical accessibility remains a requirement for future Kael tree work.

The retained 20 editor captures also pass and remain supplemental historical coverage in the machine record. They are excluded from these four general desktop tables. The two instrumentation modes are separate workloads; their difference is not a controlled measurement of instrumentation overhead.

## Evidence and limits

Independent validation verifies the official artifact digest, every extracted byte in all 278 files, all 20 actual uploaded arm64 Mach-O executables, and all 2,947 eligible hosted source hashes against immutable b411 Git blobs. It reapplies every original workload, activity and geometry gate, then recomputes phase metrics and quantiles from 10,830 process samples, including 9,283 original stable samples. The actual current hosted transcript preserves all 21 original collector tests passing. The original full 100-capture audit passes on its first invocation. A separate supplemental compiler-format authoring error is preserved; corrected V2 checks the complete compiler metadata without changing a benchmark or original gate.

This is one hosted Mac with an Apple M1 virtual CPU, three cores, 7 GiB RAM, macOS 26.6.2 and matched Rust 1.97.1 / LLVM 22.1.6. The four builds use locked release profiles, no default features, and separate engine/timing features. Its display inventory is empty. Accepted activation requests and mandatory repaint activity do not independently prove foreground focus or physical presentation. All energy, instruction and cycle counters are zero; cumulative CPU/wakeup counters have no observed decreases. No power or energy-efficiency ranking is supported.

Startup geometry is unpaired, so startup timings are excluded. These fixed-source observations identify optimization priorities; they do not establish current-HEAD performance, an overall framework winner, physical-device/AT/IME behavior, or complete application parity.
