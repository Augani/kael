# Kael and GPUI Kit 0.7.3: completed e205 hosted comparison

The [completed run](https://github.com/Augani/kael/actions/runs/39102329053) contains 100 accepted captures: five contracts, two instrumentation modes, two frameworks and five alternating repetitions. Kael uses frozen commit `e2054aa17c8661fffd89c49e3d8433a5a7ba6008`; GPUI Kit uses released 0.7.3 at `67265a36e260ffe03c5aaebdcd8d654dbf892eab`. Workflow head and actual checkout are identical, at Git tree `1a9cc45049509e7624b13304b52441a72cd5dbcb`. Later Kael and competitor changes are excluded. The [machine-readable record](2026-10-18-e205-gpui073-hosted-comparison.json) preserves exact source, raw-artifact and executable identities, all five contracts, and both independent audit references. The [b411 comparison](2026-10-18-b411-gpui073-hosted-comparison.md) and [older 0d15 comparison](2026-10-18-gpui073-hosted-comparison.md) remain separate historical cohorts.

Both frameworks have the same actual **1100×645 client viewport at scale 1** through all 800 phase boundaries and 50 cross-framework pairs. The virtual runner constrains the nominal 1100×760 request equally. These are actual recorded boundaries, not proof of a 1100×760 viewport.

## General desktop workloads

These are medians of five captures per framework with optional frame timing disabled. Each paired value gives **Kael / GPUI Kit**; the idle column gives before/after for each framework. Process CPU uses 100% for one core. RSS is the maximum sampled value within each capture’s original stable windows, followed by the median across five captures; it is not lifetime peak, retained heap or GPU residency.

| Contract | Active CPU | Churn CPU | Idle before / after CPU, Kael / GPUI Kit | Stable sampled peak RSS, MiB |
|---|---:|---:|---:|---:|
| Navigation/detail | 11.88% / 16.58% | 8.97% / 11.84% | 0.98/0.01% / 1.57/0.84% | 92.11 / 79.25 |
| Data table | 26.19% / 19.92% | 20.69% / 13.10% | 1.02/0.01% / 1.59/1.19% | 204.03 / 136.30 |
| Virtual tree | 45.30% / 13.90% | 27.70% / 9.24% | 0.98/0.02% / 1.47/1.04% | 932.86 / 654.52 |
| Dock workspace | 15.25% / 14.09% | 9.83% / 10.76% | 1.07/0.01% / 1.62/1.04% | 80.61 / 80.19 |

The table and tree measurements remain concrete CPU and memory optimization targets. Tree active CPU is 45.30% / 13.90% and sampled peak RSS is 932.86 / 654.52 MiB in this cohort. The tree contracts have different accessibility/preparation work and unequal recorded operation totals; these values do not isolate a single cause. RSS alone does not establish a leak. Full logical accessibility remains a requirement for future Kael tree work.

## Separate CPU timing diagnostics

Timing-enabled executables are separate captures. Each capture computes its original discrete p99, then the five p99 values are summarized by their median. These are CPU-side wall-clock spans, not GPU/compositor completion, presentation latency or visible smoothness.

| Contract | Draw p99 µs, Kael / GPUI Kit | Submission p99 µs, Kael / GPUI Kit |
|---|---:|---:|
| Navigation/detail | 9,669 / 14,690 | 723 / 619 |
| Data table | 17,258 / 22,483 | 617 / 625 |
| Virtual tree | 5,012 / 8,177 | 649 / 604 |
| Dock workspace | 18,583 / 14,311 | 799 / 2,114 |

## Actual operation counts and component differences

These are the five actual recorded totals in repetition order, including model replacement. Counts differ between frameworks and runs; these are not equal-work or per-operation cost estimates. Navigation/detail has no operation counter. The machine record retains every operation type and each render/frame count.

| Contract | Timing | Kael totals | GPUI Kit totals |
|---|---|---|---|
| Data table | disabled | 389, 329, 342, 356, 390 | 394, 358, 401, 370, 365 |
| Data table | enabled | 334, 376, 407, 371, 383 | 386, 381, 374, 364, 373 |
| Virtual tree | disabled | 262, 274, 255, 256, 237 | 308, 315, 324, 336, 321 |
| Virtual tree | enabled | 275, 251, 286, 239, 279 | 332, 305, 322, 302, 317 |
| Dock workspace | disabled | 398, 360, 339, 370, 405 | 367, 333, 331, 384, 223 |
| Dock workspace | enabled | 333, 386, 372, 396, 393 | 343, 344, 357, 357, 368 |

Kael tables retain a bounded 16×512-cell tile cache; GPUI Kit’s delegate reads immutable data directly. Native leaf headers are 32px / 28px. Kael prepares complete logical tree accessibility metadata, while this pinned GPUI Kit Tree exposes mounted rows; expansion APIs differ. Dock tab bars are 34px / 30px, and persistence/model/layout rules differ. These disclosed contracts do not establish pixel or feature parity. Native editing, physical input, floating panes and edge docks are excluded where declared.

The retained 20 editor captures also pass and remain supplemental historical coverage in the machine record. They are excluded from these four general desktop tables. Timing-enabled resource rows are retained separately. The two instrumentation modes are separate workloads; their difference is not a controlled measurement of instrumentation overhead.

## Evidence and limits

Independent validation verifies artifact 12128659135 against GitHub digest `e146ecfe9a3bc80bfb9a3b6eac452960038c76e92604f472aa7ce5bd34f49003`, every extracted byte in 278 files, all 20 uploaded arm64 Mach-O executables, and all 2,948 eligible hosted source hashes against immutable e205 Git blobs. It reapplies every original workload, activity and geometry gate, then recomputes phase metrics and quantiles from 10,833 process samples, including 9,280 original stable samples. The actual current hosted transcript preserves all 21 original collector tests passing. The complete 100-capture audit passes on its first invocation; an independent second review recomputes the raw results and public table projections.

The main audit is pinned by SHA `af61147aede0d74627b01999e72e6a9e7f41bb9d9d1bd04fb256783e0a4a8453`, and the independent final peer by `4eb21a71d44d825ca0e2b0a5686d05645bab99797bfa6d511507462c1e8a419c`. The companion record binds raw reports, source/compiler identities, full operation disclosures and all evidence digests. The first preparation attempt expected verbose test names; it is preserved alongside the exact current 21-dot test transcript and full-log extraction offsets.

All four locked release builds completed successfully with no default features and separate engine/timing features. A target cache was restored. This run logs no fresh GPUI Kit dependency compilation; fresh dependency compilation and cache-object provenance are not attested. The first supplemental expectation of that line is preserved. Its reviewed successor qualifies the evidence while retaining exact source pins, successful build commands, executable identities and every original capture gate.

This is one hosted Mac with an Apple M1 virtual CPU, three CPUs, 7 GiB RAM, macOS 26.6.2 and Rust 1.97.1 / LLVM 22.1.6. Its display inventory is empty. Accepted activation requests and mandatory repaint activity do not independently prove foreground focus or physical presentation. Instructions, cycles and both raw energy counters are zero throughout; cumulative CPU/wakeup counters have no observed decreases. No power or energy-efficiency ranking is supported.

Startup geometry is unpaired, so startup timings are excluded. These fixed-source observations identify optimization priorities; they do not establish current-HEAD performance, an overall framework winner, physical-device/AT/IME behavior, or complete application parity. The successful comparison workflow is separate from broader Platform Validation results.
