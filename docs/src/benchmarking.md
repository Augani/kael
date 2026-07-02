# Benchmarking Kael Against Electron

Do not claim Kael is lighter than Electron from architecture alone. Measure the
same workload on the same machine, then compare the results.

Kael exposes product-shaped benchmark scenarios and metrics through
`kael::benchmark`:

```rust
use kael::{
    BenchmarkHarness, BenchmarkMeasurement, BenchmarkMetric, BenchmarkScenario,
    ElectronComparisonReport, MetricUnit,
};
use std::time::Duration;

let electron_results = load_electron_baseline()?;

let mut harness = BenchmarkHarness::new();
harness.run(BenchmarkScenario::Dashboard, "kael", |measurements| {
    run_dashboard_workload();
    measurements.push(BenchmarkMeasurement {
        metric: BenchmarkMetric::IdleMemory,
        value: measure_idle_memory_mb(),
        unit: MetricUnit::Megabytes,
        elapsed: Duration::from_secs(5),
    });
});

let report = ElectronComparisonReport::generate(
    &electron_results,
    harness.results(),
    Some("trace.json".into()),
);

println!("{}", report.summary());
```

Use the same scenario name for both result sets so metrics line up. Each
scenario exposes a workload contract:

```rust
let spec = BenchmarkScenario::Dashboard.workload_spec();
println!("required metrics: {:?}", spec.required_metrics);
println!("required interactions: {:?}", spec.required_interactions);

let issues = spec.validate_result(&kael_result);
assert!(issues.is_empty(), "missing benchmark evidence: {issues:?}");
```

Available scenarios include chat, IDE/workspace, document, canvas/design tool,
video editor, dashboard, messaging, and media-control workloads.

Important metrics for Electron-replacement claims:

| Claim | Metrics |
| --- | --- |
| Starts faster | `ColdStart`, `WarmStart`, `FirstInteractiveFrame` |
| Uses less memory | `IdleMemory`, `MemoryGrowth` |
| Stays idle | `LongSessionCpu`, `IdlePower`, `WakeupsPerSecond` |
| Feels responsive | `InputLatency`, `ScrollLatency`, frame-time percentiles |
| Handles real UI load | Scenario coverage plus `AssetCacheHitRate` |

`ElectronComparisonReport` classifies each shared metric as a Kael win,
Electron win, or tie while preserving the raw numbers. Lower-is-better metrics
such as memory and latency are handled differently from higher-is-better metrics
such as cache hit rate. The report also records `evidence_issues` when either
side is missing required metrics for the scenario; do not publish parity claims
until those are resolved.

For CI, keep using `CiReport` and `RegressionThresholds` to compare a Kael
candidate against a previous Kael baseline. Use `ElectronComparisonReport` for
product/positioning evidence against an Electron sample app.
