use kael::{
    BenchmarkHarness, BenchmarkMetric, BenchmarkResult, BenchmarkScenario, ColdStartCollector,
    InputLatencyCollector, LongSessionCollector, MemoryCollector, MetricCollector,
    SmoothnessCollector, check_regressions, compare_results, load_results_from_json,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const BASELINE_PATH: &str = "benchmarks/baselines/gpui-messaging-baseline.json";
const OUTPUT_PATH: &str = "benchmark-results.json";
const REGRESSION_THRESHOLD: f64 = 15.0;

fn main() {
    println!("GPUI Performance Benchmark Harness");
    println!("==================================\n");

    let args: Vec<String> = std::env::args().collect();
    let compare_baseline = args.contains(&"--compare".to_string());
    let output_path = args
        .windows(2)
        .find(|w| w[0] == "--output")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| OUTPUT_PATH.to_string());

    let mut harness = BenchmarkHarness::new();

    run_cold_start_benchmark(&mut harness);
    run_memory_benchmark(&mut harness);
    run_input_latency_benchmark(&mut harness);
    run_scroll_smoothness_benchmark(&mut harness);
    run_resize_smoothness_benchmark(&mut harness);
    run_long_session_benchmark(&mut harness);

    let json = harness
        .export_to_json()
        .expect("failed to serialize results");
    std::fs::write(&output_path, &json).expect("failed to write results");
    println!("\nResults written to: {}\n", output_path);

    print_summary(harness.results());

    if compare_baseline {
        compare_against_baseline(harness.results(), BASELINE_PATH);
    }
}

fn run_cold_start_benchmark(harness: &mut BenchmarkHarness) {
    println!("[Scenario] Cold Start — Messaging");

    let mut cold_start = ColdStartCollector::new();
    let mut memory = MemoryCollector::new();
    let mut collectors: [&mut dyn MetricCollector; 2] = [&mut cold_start, &mut memory];

    let result = harness.run_with_collectors(
        BenchmarkScenario::Messaging,
        "gpui",
        &mut collectors,
        |_collectors| {
            simulate_workload(BenchmarkScenario::Messaging);
        },
    );

    print_result(&result);
}

fn run_memory_benchmark(harness: &mut BenchmarkHarness) {
    println!("[Scenario] Memory — Workspace");

    let mut memory = MemoryCollector::new();
    let mut collectors: [&mut dyn MetricCollector; 1] = [&mut memory];

    let result = harness.run_with_collectors(
        BenchmarkScenario::Workspace,
        "gpui",
        &mut collectors,
        |_collectors| {
            simulate_workload(BenchmarkScenario::Workspace);
        },
    );

    print_result(&result);
}

fn run_input_latency_benchmark(harness: &mut Harness) {
    println!("[Scenario] Input Latency — MediaControl");

    let mut latency = InputLatencyCollector::new();
    let mut collectors: [&mut dyn MetricCollector; 1] = [&mut latency];

    let result = harness.run_with_collectors(
        BenchmarkScenario::MediaControl,
        "gpui",
        &mut collectors,
        |collectors| {
            let latency = collectors[0]
                .as_any_mut()
                .downcast_mut::<InputLatencyCollector>()
                .unwrap();
            for _ in 0..60 {
                latency.record_input();
                simulate_frame(Duration::from_millis(16));
                latency.record_frame_presented();
            }
        },
    );

    print_result(&result);
}

fn run_scroll_smoothness_benchmark(harness: &mut Harness) {
    println!("[Scenario] Scroll Smoothness — Messaging");

    let mut smoothness = SmoothnessCollector::new(BenchmarkMetric::ScrollSmoothness);
    let mut collectors: [&mut dyn MetricCollector; 1] = [&mut smoothness];

    let result = harness.run_with_collectors(
        BenchmarkScenario::Messaging,
        "gpui",
        &mut collectors,
        |collectors| {
            let smoothness = collectors[0]
                .as_any_mut()
                .downcast_mut::<SmoothnessCollector>()
                .unwrap();
            for _ in 0..120 {
                simulate_frame(Duration::from_millis(16));
                smoothness.record_frame();
            }
        },
    );

    print_result(&result);
}

fn run_resize_smoothness_benchmark(harness: &mut Harness) {
    println!("[Scenario] Resize Smoothness — Workspace");

    let mut smoothness = SmoothnessCollector::new(BenchmarkMetric::ResizeSmoothness);
    let mut collectors: [&mut dyn MetricCollector; 1] = [&mut smoothness];

    let result = harness.run_with_collectors(
        BenchmarkScenario::Workspace,
        "gpui",
        &mut collectors,
        |collectors| {
            let smoothness = collectors[0]
                .as_any_mut()
                .downcast_mut::<SmoothnessCollector>()
                .unwrap();
            for _ in 0..90 {
                simulate_frame(Duration::from_millis(16));
                smoothness.record_frame();
            }
        },
    );

    print_result(&result);
}

fn run_long_session_benchmark(harness: &mut Harness) {
    println!("[Scenario] Long Session — MediaControl");

    let mut session = LongSessionCollector::new(Duration::from_millis(100));
    let mut collectors: [&mut dyn MetricCollector; 1] = [&mut session];

    let result = harness.run_with_collectors(
        BenchmarkScenario::MediaControl,
        "gpui",
        &mut collectors,
        |collectors| {
            let session = collectors[0]
                .as_any_mut()
                .downcast_mut::<LongSessionCollector>()
                .unwrap();
            for _ in 0..50 {
                simulate_frame(Duration::from_millis(100));
                session.sample();
            }
        },
    );

    print_result(&result);
}

fn simulate_workload(scenario: BenchmarkScenario) {
    let complexity = scenario.complexity_score();
    let iterations = complexity.max(1);
    let mut accumulator = 0u64;
    for i in 0..iterations {
        accumulator = accumulator.wrapping_add(i as u64);
    }
    std::hint::black_box(accumulator);
}

fn simulate_frame(target_duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < target_duration {
        std::hint::black_box(0u64);
    }
}

fn print_result(result: &BenchmarkResult) {
    println!("  Subject: {}", result.subject);
    println!(
        "  Duration: {:.2} ms",
        result.duration.as_secs_f64() * 1000.0
    );
    for m in &result.measurements {
        println!(
            "  {:>24}: {:>10.2} {}",
            format!("{:?}", m.metric),
            m.value,
            m.unit
        );
    }
    println!();
}

fn print_summary(results: &[BenchmarkResult]) {
    println!("Summary");
    println!("-------");
    for result in results {
        let metric_count = result.measurements.len();
        println!(
            "  {:?}: {} metric{}",
            result.scenario,
            metric_count,
            if metric_count == 1 { "" } else { "s" }
        );
    }
    println!();
}

fn compare_against_baseline(results: &[BenchmarkResult], baseline_path: &str) {
    let baseline_path = Path::new(baseline_path);
    if !baseline_path.exists() {
        println!("Baseline not found at: {}", baseline_path.display());
        println!("Run with --generate-baseline to create one from current results.");
        return;
    }

    println!("Comparing against baseline: {}\n", baseline_path.display());

    let baseline_json = std::fs::read_to_string(baseline_path).expect("failed to read baseline");
    let baseline = load_results_from_json(&baseline_json).expect("failed to parse baseline JSON");

    for result in results {
        if let Some(base) = baseline.iter().find(|b| b.scenario == result.scenario) {
            let comparisons = compare_results(base, result);
            if comparisons.is_empty() {
                continue;
            }
            println!("  {:?}:", result.scenario);
            for comp in &comparisons {
                let indicator = if comp.lower_is_better {
                    if comp.percent_change < 0.0 {
                        "✓"
                    } else {
                        "✗"
                    }
                } else {
                    if comp.percent_change > 0.0 {
                        "✓"
                    } else {
                        "✗"
                    }
                };
                println!(
                    "    {:>24}: {:>+8.2}% ({:.2} → {:.2} {}) {}",
                    format!("{:?}", comp.metric),
                    comp.percent_change,
                    comp.baseline,
                    comp.candidate,
                    comp.unit,
                    indicator
                );
            }
        }
    }

    let regressions = check_regressions(&baseline, results, REGRESSION_THRESHOLD);
    if !regressions.is_empty() {
        println!(
            "\nRegressions detected (threshold: {}%):",
            REGRESSION_THRESHOLD
        );
        for reg in &regressions {
            println!(
                "  {:?} / {:?}: {:.2}% ({:.2} → {:.2} {})",
                reg.scenario, reg.metric, reg.percent_change, reg.baseline, reg.candidate, reg.unit
            );
        }
        std::process::exit(1);
    } else {
        println!("\nNo regressions detected.");
    }
}

type Harness = BenchmarkHarness;
