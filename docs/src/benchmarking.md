# Benchmarking Evidence

Do not claim Kael is lighter than a baseline from architecture alone. Measure the
same workload on the same machine, then compare the results.

Kael exposes product-shaped benchmark scenarios and metrics through
`kael::benchmark`:

```rust
use kael::{
    BenchmarkHarness, BenchmarkMeasurement, BenchmarkMetric, BenchmarkSampleApp,
    BenchmarkSamplePair, BenchmarkSampleRuntime, BenchmarkScenario, BaselineComparisonReport,
    MetricUnit,
};
use std::time::Duration;

let baseline_results = load_baseline_results()?;

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

let interactions = BenchmarkScenario::Dashboard
    .workload_spec()
    .required_interactions;
let sample_pair = BenchmarkSamplePair::new(
    BenchmarkSampleApp::builder(
        BenchmarkSampleRuntime::Baseline,
        BenchmarkScenario::Dashboard,
        "Baseline dashboard sample",
    )
    .source("samples/baseline/dashboard")
    .run_command("npm run bench:dashboard")
    .interactions(interactions.clone())
    .build_checked()
    .unwrap(),
    BenchmarkSampleApp::builder(
        BenchmarkSampleRuntime::Kael,
        BenchmarkScenario::Dashboard,
        "Kael dashboard sample",
    )
    .source("samples/kael/dashboard")
    .run_command("cargo run --release -p dashboard_sample -- --bench")
    .interactions(interactions)
    .build_checked()
    .unwrap(),
);

let report = BaselineComparisonReport::generate_with_sample_pairs(
    &baseline_results,
    harness.results(),
    &[sample_pair],
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

Important metrics for resource and performance claims:

| Claim | Metrics |
| --- | --- |
| Starts faster | `ColdStart`, `WarmStart`, `FirstInteractiveFrame` |
| Uses less memory | `IdleMemory`, `MemoryGrowth` |
| Stays idle | `LongSessionCpu`, `IdlePower`, `WakeupsPerSecond` |
| Feels responsive | `InputLatency`, `ScrollLatency`, frame-time percentiles |
| Handles real UI load | Scenario coverage plus `AssetCacheHitRate` |

`BaselineComparisonReport` classifies each shared metric as a Kael win,
baseline win, or tie while preserving the raw numbers. Lower-is-better metrics
such as memory and latency are handled differently from higher-is-better metrics
such as cache hit rate. The report also records `evidence_issues` when either
side is missing a counterpart result for the same scenario, when either side is
missing required metrics, when either side supplies duplicate results for the
same scenario, when baseline and Kael results were captured under different
hardware or OS conditions, when a compared scenario lacks matching
baseline/Kael sample descriptors, or when a sample omits required interactions;
do not make parity claims until those are resolved.

For CI, keep using `CiReport` and `RegressionThresholds` to compare a Kael
candidate against a previous Kael baseline. Use `BaselineComparisonReport` for
product/positioning evidence against a baseline sample app.

## Browser artifact size

Release packaging runs pinned Binaryen 132 at `-O3`, then
`scripts/verify-browser-artifact-budget.sh` rejects the maintained retained-scene
proof above 12 MiB raw Wasm, 5 MiB gzip Wasm, or 100 KiB JavaScript glue. These
are regression ceilings, not a promise that every application has the same
size: enabled features, fonts, codecs, and product assets remain app-owned.
The workspace release profile also uses fat LTO and one codegen unit; measure
clean release-build time separately from runtime and transfer performance.

## Dynamic SceneGraph probe

Large games and whiteboards have a different hot path from retained interface
layout: objects move repeatedly while the scene population stays stable. Run the
maintained release probe with:

```bash
cargo run --release -p kael --example scene_graph_move_query_probe
```

The probe builds and indexes exactly 100,000 nodes, then performs 10,000
cross-cell `move_node` plus point-query operations. It requires:

- zero full spatial-index rebuilds during bounds-only movement;
- exactly 10,000 incremental spatial updates;
- no more than two candidates in any representative point query; and
- the complete move/query phase to finish within two seconds.

The two-second ceiling is a deliberately generous regression budget for shared
CI hosts, not a frame-time or cross-framework performance claim. Preserve the
raw elapsed time and environment when publishing results. Correctness tests also
cover z-order retention, moves between normal and oversized spatial lanes, and
the full-rebuild fallback when cached entry metadata is unavailable.
