# Matched Kael / GPUI Kit comparison — b091

The [completed run](https://github.com/Augani/kael/actions/runs/39110574440) passes all 100 original schema-3 captures, 800 equal actual client-geometry boundaries, 50 pairs and 20 independently rehashed uploaded executables. This workflow-dispatch run checks out `b0916c21212eca223cace9bccafbc321baa22615` directly. GPUI Kit is 0.7.3 at `67265a36e260ffe03c5aaebdcd8d654dbf892eab`, using GPUI 0.3.7.

General desktop workloads lead below; Editor remains a supplemental record in the [complete result](2026-10-18-b091-gpui073-hosted-comparison.json). All captures use actual 1100 × 645 client viewports at scale 1, with five alternating repetitions per engine in each of two separate timing modes.

## Active CPU and sampled memory

These are timing-disabled medians. CPU is active-phase process CPU as a percentage of one CPU, not per-operation throughput. RSS is the median stable-sample peak, excluding startup and oracle validation; it is not a lifetime peak or Rust/GPU byte accounting.

| Workload | Active CPU Kael / GPUI Kit | Sampled RSS MiB Kael / GPUI Kit |
| --- | ---: | ---: |
| Navigation/detail (100,000 items) | 8.76% / 11.62% | 91.73 / 78.92 |
| Table (100,000 × 8) | 24.65% / 17.25% | 203.81 / 136.12 |
| Tree (100,025 nodes) | 46.97% / 11.71% | 970.28 / 654.28 |
| Dock workspace (12 panes) | 15.03% / 16.06% | 80.67 / 80.20 |

Kael has lower navigation and docking active CPU in this cohort. Table/tree CPU and RSS remain substantial gaps. Navigation RSS is also higher, and docking RSS is close. Full logical accessibility work, component architecture and unequal operation totals remain explicit; these results do not establish an overall framework winner.

## CPU draw and submission tails

The separate timing-enabled runs report the median of five per-run p99 values in microseconds. Draw and platform submission measure CPU scopes, not GPU completion or compositor presentation. Timing modes are separate captures and do not isolate instrumentation overhead.

| Workload | Draw CPU p99 Kael / GPUI Kit, µs | Submission CPU p99 Kael / GPUI Kit, µs |
| --- | ---: | ---: |
| Navigation/detail (100,000 items) | 7112 / 11436 | 732 / 561 |
| Table (100,000 × 8) | 15832 / 18065 | 545 / 512 |
| Tree (100,025 nodes) | 5349 / 7484 | 425 / 502 |
| Dock workspace (12 panes) | 16026 / 13583 | 579 / 507 |

## Energy and source limits

All 10,922 process samples report RUSAGE_INFO_V6. Both OS energy fields are zero throughout, and all 400 phase capability/delta summaries correctly remain unknown/null. No V4 fallback occurs in these captures. The maintained collector gate separately passes 32 Python tests, three strict C builds and two synthetic C executions, including fallback handling. Zero counters do not mean zero energy; this run supports no watts, calibrated power, core-class interpretation or energy ranking.

The hosted environment is macOS 26.6.2 on an Apple M1 virtual machine with three logical CPUs and 7 GiB RAM. The source snapshots, locks and 20 actual Mach-O files are independently hashed. The restored Cargo target cache has no observed fresh GPUI dependency compile line or complete producer attestation, so cached dependencies are not described as freshly compiled or hermetically reproduced.

The [raw artifact](https://github.com/Augani/kael/actions/runs/39110574440/artifacts/12132209126) is 123,416,241 bytes with SHA256 `c6601b55ffa3d437853901b49d820bac5fe9fd1e18f9692de231a2b27a8e793f`; all 278 extracted files are rehashed. Original validators, raw operations, both timing modes, Editor supplement, environment, executable identities and source disclosures are preserved in the complete machine record and artifact. No sample is discarded or original workload/geometry gate relaxed.

The separate [b091 platform cohort](2026-10-18-platform-b091-validation.json) remains failed. That PR job checks a distinct merge commit with the same Git tree; its native presentation and hardware-browser failures do not become passes because this comparison succeeds. Later source, including the tree label fix at `e8344d3`, requires separate acceptance. No comparative presentation, controlled power or current-source result is inferred.
