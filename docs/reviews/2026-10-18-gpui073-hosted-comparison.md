# Kael and GPUI Kit 0.7.3: completed hosted comparison

The [completed run](https://github.com/Augani/kael/actions/runs/39079038200)
contains 100 accepted captures: five contracts, two instrumentation modes, two
frameworks and five alternating repetitions. Kael uses frozen commit
`0d15aff4288e8e98a54032e6ed149e080ea13104`; GPUI Kit uses released 0.7.3 at
`67265a36e260ffe03c5aaebdcd8d654dbf892eab`. Later Kael changes are excluded.
The [machine-readable record](2026-10-18-gpui073-hosted-comparison.json) preserves
the source, executable identities, per-run values and raw evidence references.

Both frameworks have the same actual **1100×645 client viewport at scale 1**
through all 800 phase boundaries and 50 cross-framework pairs. The virtual
runner constrains the nominal 1100×760 request equally. Separate local native
checks at 1100×760 and scale 2 remain
[correctness evidence](2026-10-18-gpui073-local-runtime-validation.json).

## General desktop workloads

These are medians of five captures per framework with optional frame timing
disabled. Each cell gives **Kael / GPUI Kit**. Process CPU uses 100% for one core.
RSS is the maximum sampled value within the original stable measurement windows
of each capture, followed by the median across captures; it is not an exact
lifetime high-water mark or retained heap measurement.

| Contract | Active process CPU | Churn process CPU | Stable sampled peak RSS, MiB |
| --- | ---: | ---: | ---: |
| Navigation/detail | 8.60% / 13.07% | 5.54% / 6.54% | 91.5 / 78.7 |
| Data table | 26.06% / 18.32% | 18.54% / 13.06% | 203.4 / 135.7 |
| Virtual tree | 57.31% / 12.50% | 32.55% / 8.69% | 1,038.7 / 653.9 |
| Dock workspace | 15.34% / 16.93% | 9.16% / 10.09% | 80.3 / 80.1 |

Navigation and workspace use less active process CPU in these captures. Tables
and trees remain substantial CPU and memory gaps. Kael tree sampled peaks also
vary from 828.5 to 1,069.8 MiB, versus 647.2 to 654.1 MiB for GPUI Kit.
That spread warrants preparation and retirement measurements; RSS alone does
not establish a leak. Workspace RSS is effectively similar at this precision.

The retained editor contract is secondary to the general desktop priorities.
Its active/churn CPU is 29.73%/19.81% for Kael and 7.82%/11.04% for GPUI Kit;
sampled peak RSS is 86.2/95.1 MiB. All five contracts remain in the record.

## Separate CPU timing diagnostics

The following medians use the separate timing-enabled executables. Each capture
computes its own original discrete p99, then the five p99 values are summarized
by their median. These CPU-side wall-clock spans include scheduling effects and
exclude GPU/compositor completion. They are not presentation latency or visible
smoothness measurements.

| Contract | CPU draw p99, ms, Kael / GPUI Kit | CPU submission p99, ms, Kael / GPUI Kit |
| --- | ---: | ---: |
| Navigation/detail | 7.155 / 11.487 | 0.537 / 0.390 |
| Data table | 15.527 / 18.351 | 0.704 / 0.509 |
| Virtual tree | 4.623 / 7.522 | 0.464 / 0.482 |
| Dock workspace | 16.120 / 14.477 | 0.501 / 0.459 |

The modes are separate captures with different operation counts; their
difference is not a controlled measurement of instrumentation overhead.

## Evidence and limits

Independent audits rehash all 278 uploaded files, including all 20 actual
application executables, match 2,946 eligible hosted source fingerprints to the
frozen Git archive, and reapply every original workload/activity gate. They
recompute all phase metrics and timing quantiles from 10,885 process samples,
including 9,333 original stable samples. Raw failures from earlier checkpoints
remain separate; no samples or gates are removed to obtain these results.

This is one shared hosted macOS runner with an Apple M1 virtual CPU, three cores
and 7 GiB RAM. Its display inventory is empty. Accepted activation requests and
required repaint activity do not independently prove foreground focus or
physical presentation. Energy, instruction and cycle counters are all zero
throughout this cohort and cannot support a power or energy-efficiency ranking.
CPU and wakeup counters have no observed decreases.

The fixed phase schedules do not guarantee equal operation counts. Component
differences remain disclosed: Kael tables use a bounded tile cache; tree logical
accessibility coverage differs; dock chrome and data-header details differ.
Passing workload and geometry gates does not establish pixel-identical controls
or complete feature parity. Startup geometry is unpaired and startup timings
are excluded. These results identify concrete optimization targets without
establishing an overall framework ranking.
