# Frozen 2622 hosted desktop comparison

The completed comparison at `2622dbf77bd614482c0f1fe0a69966b9bca1c60d` passes all 100 original capture gates: 10 workload/mode groups, 800 actual boundaries at 1100×645 pixels and scale 1, 50 paired captures and 20 preserved executable identities. GPUI Kit is pinned to 0.7.3 at `67265a36…`. This cohort predates later catalog, chart and IconButton changes.

Values below are medians of five captures per engine. CPU percentages use 100% for one CPU core. RSS is the median of each run’s stable sampled peak, expressed in MiB; it excludes startup/oracle validation and is not retained heap or GPU accounting.

**Frame timing disabled**

| Workload | Active CPU Kael / GPUI | Churn CPU Kael / GPUI | Sampled peak RSS MiB Kael / GPUI |
|---|---:|---:|---:|
| Navigation/detail | 9.88% / 13.08% | 7.31% / 10.02% | 91.94 / 79.11 |
| Data table | 26.59% / 19.37% | 20.75% / 14.50% | 204.22 / 136.17 |
| Tree | 39.97% / 16.23% | 25.04% / 10.50% | 979.28 / 654.52 |
| Dock workspace | 15.07% / 16.87% | 10.88% / 9.55% | 80.69 / 80.45 |

**Frame timing enabled**

| Workload | Active CPU Kael / GPUI | Churn CPU Kael / GPUI | Sampled peak RSS MiB Kael / GPUI |
|---|---:|---:|---:|
| Navigation/detail | 9.55% / 13.19% | 9.03% / 12.34% | 91.72 / 84.17 |
| Data table | 26.56% / 19.96% | 20.58% / 13.29% | 204.72 / 141.62 |
| Tree | 46.84% / 14.79% | 26.56% / 11.18% | 982.17 / 659.83 |
| Dock workspace | 18.50% / 18.53% | 13.90% / 14.20% | 81.45 / 86.06 |

Actual operation totals differ between captures/engines and remain in the JSON tables; these numbers are descriptive, without an overall winner or per-operation normalization. Timing modes are separate runs, not a controlled estimate of instrumentation overhead.

The source tree is independently reconstructed from the immutable tar and equals the captured primary GitCommit tree. Uploaded binary digests match all captures. Both GPUI dependency mode builds contain actual 0.7.3 compilation lines, while complete hermetic dependency/cache provenance is not established. All four builds retain the `block v0.1.6` future-compatibility warning.

The hosted runner identifies as Apple M1 (Virtual) and reports an empty display inventory. CPU draw/submission spans do not prove GPU completion or compositor presentation. All 10,818 samples use the V6 API, but energy/penergy and legacy instruction/cycle/energy fields are zero-only; per-field hardware capability and phase energy deltas stay unknown/null. No physical power, watts, comparative energy or presentation claim follows. The 32 Python/3 strict C/2 synthetic C collector controls passed separately.

Editor data remains supplemental in the JSON alongside idle wakeups, complete operation counts, per-run ranges and each workload’s semantic differences. No physical-host or later-source performance result is implied.
