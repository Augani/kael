//! Product-level benchmark workloads and harness for GPUI.
//!
//! This module defines standard benchmark scenarios that resemble real
//! products: messaging UI, workspace/editor UI, and media-control dashboard
//! UI. These workloads are used to measure startup, memory, responsiveness,
//! and energy use against Baseline baselines.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::tracer::Tracer;

const MAX_COLLECTOR_SAMPLES: usize = 100_000;
const MAX_HARNESS_RESULTS: usize = 1_024;
const MAX_MEASUREMENTS_PER_RESULT: usize = 4_096;
const MAX_BENCHMARK_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_BENCHMARK_TEXT_CHARS: usize = 512;
const MAX_SAMPLE_INTERACTIONS: usize = 256;
const MAX_THRESHOLD_OVERRIDES: usize = 64;
#[cfg(target_os = "macos")]
const MAX_SYSCTL_STRING_BYTES: usize = 128 * 1024;

// ---------------------------------------------------------------------------
// Benchmark Scenarios
// ---------------------------------------------------------------------------

/// A predefined benchmark scenario resembling a real product workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BenchmarkScenario {
    /// A messaging/chat client UI with lists, avatars, and input.
    Messaging,
    /// A workspace/editor UI with panes, tabs, and file trees.
    Workspace,
    /// A media-control dashboard with previews and device routing.
    MediaControl,
    /// IDE workspace with file tree, tabs, editor, terminal, diagnostics panel.
    Ide,
    /// Chat app with thousands of messages and live typing indicators.
    Chat,
    /// Notion-style document with nested blocks, embeds, and large undo history.
    Document,
    /// Figma-style canvas with thousands of nodes, pan/zoom, selection.
    Canvas,
    /// OBS/video editor with live preview, thumbnails, waveforms, and export.
    VideoEditor,
    /// Data dashboard with large tables, charts, filters, and real-time updates.
    Dashboard,
}

impl BenchmarkScenario {
    /// Human-readable description of the scenario.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Messaging => {
                "Chat interface with conversation list, message bubbles, and composer"
            }
            Self::Workspace => "IDE-like workspace with sidebar, editor tabs, and terminal panel",
            Self::MediaControl => {
                "OBS-style control surface with scene list, preview, and source properties"
            }
            Self::Ide => "Full IDE with file tree, tabs, editor, terminal, and diagnostics panel",
            Self::Chat => {
                "Chat app with thousands of messages, threads, and live typing indicators"
            }
            Self::Document => {
                "Notion-style document with nested blocks, embeds, and large undo history"
            }
            Self::Canvas => "Figma-style canvas with thousands of nodes, pan/zoom, and selection",
            Self::VideoEditor => {
                "Video editor with live preview, timeline, thumbnails, waveforms, and export"
            }
            Self::Dashboard => {
                "Data dashboard with large tables, charts, filters, and real-time updates"
            }
        }
    }

    /// Approximate complexity score (higher = more elements).
    pub fn complexity_score(&self) -> u32 {
        match self {
            Self::Messaging => 500,
            Self::Workspace => 1200,
            Self::MediaControl => 800,
            Self::Ide => 2000,
            Self::Chat => 1500,
            Self::Document => 1000,
            Self::Canvas => 3000,
            Self::VideoEditor => 2500,
            Self::Dashboard => 1800,
        }
    }

    /// Return the comparable workload contract for this scenario.
    pub fn workload_spec(&self) -> BenchmarkWorkloadSpec {
        let required_metrics = match self {
            Self::Messaging | Self::Chat => vec![
                BenchmarkMetric::ColdStart,
                BenchmarkMetric::IdleMemory,
                BenchmarkMetric::InputLatency,
                BenchmarkMetric::ScrollLatency,
                BenchmarkMetric::LongSessionCpu,
            ],
            Self::Workspace | Self::Ide | Self::Document => vec![
                BenchmarkMetric::ColdStart,
                BenchmarkMetric::FirstInteractiveFrame,
                BenchmarkMetric::IdleMemory,
                BenchmarkMetric::InputLatency,
                BenchmarkMetric::FrameTimeP95,
                BenchmarkMetric::MemoryGrowth,
            ],
            Self::Canvas | Self::VideoEditor | Self::MediaControl => vec![
                BenchmarkMetric::ColdStart,
                BenchmarkMetric::IdleMemory,
                BenchmarkMetric::FrameTimeP95,
                BenchmarkMetric::FrameTimeP99,
                BenchmarkMetric::GpuUsage,
                BenchmarkMetric::LongSessionCpu,
            ],
            Self::Dashboard => vec![
                BenchmarkMetric::ColdStart,
                BenchmarkMetric::FirstInteractiveFrame,
                BenchmarkMetric::IdleMemory,
                BenchmarkMetric::FrameTimeP95,
                BenchmarkMetric::LongSessionCpu,
                BenchmarkMetric::WakeupsPerSecond,
            ],
        };

        let required_interactions = match self {
            Self::Messaging | Self::Chat => {
                vec!["scroll history", "send message", "receive update"]
            }
            Self::Workspace | Self::Ide => {
                vec!["open file", "switch tab", "type in editor", "toggle panel"]
            }
            Self::Document => vec!["scroll document", "edit block", "undo edit"],
            Self::Canvas => vec!["pan canvas", "zoom canvas", "select node"],
            Self::VideoEditor => vec!["scrub timeline", "play preview", "select clip"],
            Self::MediaControl => vec!["switch scene", "toggle source", "adjust slider"],
            Self::Dashboard => vec!["sort table", "filter data", "receive live update"],
        }
        .into_iter()
        .map(str::to_string)
        .collect();

        BenchmarkWorkloadSpec {
            scenario: *self,
            description: self.description().to_string(),
            min_complexity_score: self.complexity_score(),
            required_metrics,
            required_interactions,
        }
    }

    /// Returns all defined benchmark scenarios.
    pub fn all() -> &'static [BenchmarkScenario] {
        &[
            Self::Messaging,
            Self::Workspace,
            Self::MediaControl,
            Self::Ide,
            Self::Chat,
            Self::Document,
            Self::Canvas,
            Self::VideoEditor,
            Self::Dashboard,
        ]
    }
}

/// Comparable workload contract for one benchmark scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkWorkloadSpec {
    /// Scenario this contract describes.
    pub scenario: BenchmarkScenario,
    /// Human-readable workload description.
    pub description: String,
    /// Minimum scenario complexity score expected for comparable samples.
    pub min_complexity_score: u32,
    /// Metrics that should be present before comparing against Baseline.
    pub required_metrics: Vec<BenchmarkMetric>,
    /// Interactions both the Kael and Baseline sample should exercise.
    pub required_interactions: Vec<String>,
}

/// Runtime/framework that owns a comparable benchmark sample app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BenchmarkSampleRuntime {
    /// Baseline baseline sample.
    Baseline,
    /// Kael candidate sample.
    Kael,
}

impl BenchmarkSampleRuntime {
    /// Stable lowercase identifier for reports and sample manifests.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Kael => "kael",
        }
    }
}

/// Descriptor for one comparable sample app used in Baseline-vs-Kael evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkSampleApp {
    /// Runtime/framework the sample is implemented with.
    pub runtime: BenchmarkSampleRuntime,
    /// Scenario this sample claims to cover.
    pub scenario: BenchmarkScenario,
    /// Human-readable sample name.
    pub name: String,
    /// Repository-relative path or package identifier for the sample source.
    pub source: String,
    /// Command that builds the sample, if it must be built before measuring.
    pub build_command: Option<String>,
    /// Command that launches the sample for measurement.
    pub run_command: String,
    /// Interactions the harness or tester must exercise in this sample.
    pub interactions: Vec<String>,
}

impl BenchmarkSampleApp {
    /// Start a checked sample descriptor for a runtime and scenario.
    pub fn builder(
        runtime: BenchmarkSampleRuntime,
        scenario: BenchmarkScenario,
        name: impl Into<String>,
    ) -> BenchmarkSampleAppBuilder {
        BenchmarkSampleAppBuilder::new(runtime, scenario, name)
    }

    /// Validate this sample descriptor against the scenario workload contract.
    pub fn validate_against(&self, spec: &BenchmarkWorkloadSpec) -> Vec<BenchmarkEvidenceIssue> {
        let mut issues = Vec::new();

        if self.scenario != spec.scenario {
            issues.push(BenchmarkEvidenceIssue::SampleScenarioMismatch {
                runtime: self.runtime,
                expected: spec.scenario,
                actual: self.scenario,
            });
            return issues;
        }

        validate_benchmark_sample_text(
            self.runtime,
            self.scenario,
            "name",
            &self.name,
            &mut issues,
        );
        validate_benchmark_sample_text(
            self.runtime,
            self.scenario,
            "source",
            &self.source,
            &mut issues,
        );
        validate_benchmark_sample_text(
            self.runtime,
            self.scenario,
            "run_command",
            &self.run_command,
            &mut issues,
        );
        if let Some(command) = &self.build_command {
            validate_benchmark_sample_text(
                self.runtime,
                self.scenario,
                "build_command",
                command,
                &mut issues,
            );
        }

        if self.interactions.is_empty() {
            issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
                runtime: self.runtime,
                scenario: self.scenario,
                field: "interactions".to_string(),
                reason: "sample must declare at least one interaction".to_string(),
            });
        }

        if self.interactions.len() > MAX_SAMPLE_INTERACTIONS {
            issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
                runtime: self.runtime,
                scenario: self.scenario,
                field: "interactions".to_string(),
                reason: format!(
                    "sample cannot declare more than {MAX_SAMPLE_INTERACTIONS} interactions"
                ),
            });
        }

        let mut seen_interactions = std::collections::HashSet::new();
        for interaction in &self.interactions {
            validate_benchmark_sample_text(
                self.runtime,
                self.scenario,
                "interaction",
                interaction,
                &mut issues,
            );
            if !seen_interactions.insert(interaction) {
                issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
                    runtime: self.runtime,
                    scenario: self.scenario,
                    field: "interactions".to_string(),
                    reason: "sample interactions cannot contain duplicates".to_string(),
                });
            }
        }

        for required in &spec.required_interactions {
            if !self
                .interactions
                .iter()
                .any(|interaction| interaction == required)
            {
                issues.push(BenchmarkEvidenceIssue::MissingSampleInteraction {
                    runtime: self.runtime,
                    scenario: self.scenario,
                    interaction: required.clone(),
                });
            }
        }

        issues
    }
}

/// Builder for checked Baseline/Kael benchmark sample descriptors.
#[derive(Debug, Clone)]
pub struct BenchmarkSampleAppBuilder {
    runtime: BenchmarkSampleRuntime,
    scenario: BenchmarkScenario,
    name: String,
    source: Option<String>,
    build_command: Option<String>,
    run_command: Option<String>,
    interactions: Vec<String>,
}

impl BenchmarkSampleAppBuilder {
    /// Create a sample descriptor builder.
    pub fn new(
        runtime: BenchmarkSampleRuntime,
        scenario: BenchmarkScenario,
        name: impl Into<String>,
    ) -> Self {
        Self {
            runtime,
            scenario,
            name: name.into(),
            source: None,
            build_command: None,
            run_command: None,
            interactions: Vec::new(),
        }
    }

    /// Set the repository-relative source path or package identifier.
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Set the build command.
    pub fn build_command(mut self, command: impl Into<String>) -> Self {
        self.build_command = Some(command.into());
        self
    }

    /// Set the command used to launch the measured sample.
    pub fn run_command(mut self, command: impl Into<String>) -> Self {
        self.run_command = Some(command.into());
        self
    }

    /// Add one interaction that the sample exercises.
    pub fn interaction(mut self, interaction: impl Into<String>) -> Self {
        if self.interactions.len() <= MAX_SAMPLE_INTERACTIONS {
            self.interactions.push(interaction.into());
        }
        self
    }

    /// Add several interactions that the sample exercises.
    pub fn interactions(
        mut self,
        interactions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        for interaction in interactions {
            if self.interactions.len() > MAX_SAMPLE_INTERACTIONS {
                break;
            }
            self.interactions.push(interaction.into());
        }
        self
    }

    /// Build the checked sample descriptor.
    pub fn build_checked(self) -> Result<BenchmarkSampleApp, Vec<BenchmarkEvidenceIssue>> {
        let sample = BenchmarkSampleApp {
            runtime: self.runtime,
            scenario: self.scenario,
            name: self.name,
            source: self.source.unwrap_or_default(),
            build_command: self.build_command,
            run_command: self.run_command.unwrap_or_default(),
            interactions: self.interactions,
        };
        let issues = sample.validate_against(&sample.scenario.workload_spec());
        if issues.is_empty() {
            Ok(sample)
        } else {
            Err(issues)
        }
    }
}

/// Pair of comparable Baseline and Kael sample apps for one scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkSamplePair {
    /// Baseline baseline sample descriptor.
    pub baseline: BenchmarkSampleApp,
    /// Kael candidate sample descriptor.
    pub kael: BenchmarkSampleApp,
}

impl BenchmarkSamplePair {
    /// Create a sample pair for comparison evidence.
    pub fn new(baseline: BenchmarkSampleApp, kael: BenchmarkSampleApp) -> Self {
        Self { baseline, kael }
    }

    /// Scenario shared by both samples when they match.
    pub fn scenario(&self) -> BenchmarkScenario {
        self.kael.scenario
    }

    /// Validate runtime, scenario, and required interaction parity.
    pub fn validate(&self) -> Vec<BenchmarkEvidenceIssue> {
        let mut issues = Vec::new();
        let scenario = self.kael.scenario;
        let spec = scenario.workload_spec();

        if self.baseline.runtime != BenchmarkSampleRuntime::Baseline {
            issues.push(BenchmarkEvidenceIssue::SampleRuntimeMismatch {
                expected: BenchmarkSampleRuntime::Baseline,
                actual: self.baseline.runtime,
                scenario: self.baseline.scenario,
            });
        }
        if self.kael.runtime != BenchmarkSampleRuntime::Kael {
            issues.push(BenchmarkEvidenceIssue::SampleRuntimeMismatch {
                expected: BenchmarkSampleRuntime::Kael,
                actual: self.kael.runtime,
                scenario: self.kael.scenario,
            });
        }
        if self.baseline.scenario != self.kael.scenario {
            issues.push(BenchmarkEvidenceIssue::SampleScenarioMismatch {
                runtime: BenchmarkSampleRuntime::Baseline,
                expected: self.kael.scenario,
                actual: self.baseline.scenario,
            });
            return issues;
        }

        issues.extend(self.baseline.validate_against(&spec));
        issues.extend(self.kael.validate_against(&spec));
        issues
    }
}

fn validate_benchmark_sample_text(
    runtime: BenchmarkSampleRuntime,
    scenario: BenchmarkScenario,
    field: &'static str,
    value: &str,
    issues: &mut Vec<BenchmarkEvidenceIssue>,
) {
    if value.trim().is_empty() {
        issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
            runtime,
            scenario,
            field: field.to_string(),
            reason: "value cannot be empty".to_string(),
        });
        return;
    }
    if value != value.trim() {
        issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
            runtime,
            scenario,
            field: field.to_string(),
            reason: "value cannot have leading or trailing whitespace".to_string(),
        });
    }
    if value.chars().any(char::is_control) {
        issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
            runtime,
            scenario,
            field: field.to_string(),
            reason: "value cannot contain control characters".to_string(),
        });
    }
    if value.chars().count() > MAX_BENCHMARK_TEXT_CHARS {
        issues.push(BenchmarkEvidenceIssue::InvalidSampleField {
            runtime,
            scenario,
            field: field.to_string(),
            reason: format!("value cannot be longer than {MAX_BENCHMARK_TEXT_CHARS} characters"),
        });
    }
}

impl BenchmarkWorkloadSpec {
    /// Validate one result against this workload contract.
    pub fn validate_result(&self, result: &BenchmarkResult) -> Vec<BenchmarkEvidenceIssue> {
        let mut issues = Vec::new();

        if result.scenario != self.scenario {
            issues.push(BenchmarkEvidenceIssue::ScenarioMismatch {
                expected: self.scenario,
                actual: result.scenario,
            });
            return issues;
        }

        issues.extend(result.validation_issues());

        for metric in &self.required_metrics {
            if !result
                .measurements
                .iter()
                .any(|measurement| measurement.metric == *metric && measurement.validate().is_ok())
            {
                issues.push(BenchmarkEvidenceIssue::MissingMetric {
                    scenario: self.scenario,
                    metric: *metric,
                    subject: result.subject.clone(),
                });
            }
        }

        issues
    }
}

/// Evidence issue found before comparing benchmark result sets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BenchmarkEvidenceIssue {
    /// A result was recorded under the wrong scenario.
    ScenarioMismatch {
        /// Expected scenario.
        expected: BenchmarkScenario,
        /// Actual scenario.
        actual: BenchmarkScenario,
    },
    /// A required metric is missing from a result.
    MissingMetric {
        /// Scenario being validated.
        scenario: BenchmarkScenario,
        /// Missing metric.
        metric: BenchmarkMetric,
        /// Subject missing the metric.
        subject: String,
    },
    /// A scenario result is missing for one side of an Baseline-vs-Kael comparison.
    MissingResult {
        /// Scenario needing a result.
        scenario: BenchmarkScenario,
        /// Runtime missing the result.
        runtime: BenchmarkSampleRuntime,
    },
    /// More than one result was supplied for one side of a scenario comparison.
    DuplicateResult {
        /// Scenario with duplicate results.
        scenario: BenchmarkScenario,
        /// Runtime that supplied duplicate results.
        runtime: BenchmarkSampleRuntime,
        /// Number of results supplied for the same scenario.
        count: usize,
    },
    /// Baseline and Kael results were captured under different hardware or OS conditions.
    EnvironmentMismatch {
        /// Scenario with mismatched benchmark environments.
        scenario: BenchmarkScenario,
        /// Environment field that differs.
        field: String,
        /// Baseline baseline field value.
        baseline: String,
        /// Kael candidate field value.
        kael: String,
    },
    /// No comparable sample descriptor was supplied for a runtime/scenario.
    MissingSample {
        /// Scenario needing a sample descriptor.
        scenario: BenchmarkScenario,
        /// Runtime missing the sample.
        runtime: BenchmarkSampleRuntime,
    },
    /// A sample descriptor was supplied for the wrong runtime.
    SampleRuntimeMismatch {
        /// Expected runtime.
        expected: BenchmarkSampleRuntime,
        /// Actual runtime.
        actual: BenchmarkSampleRuntime,
        /// Scenario the sample claimed.
        scenario: BenchmarkScenario,
    },
    /// A sample descriptor was supplied for the wrong scenario.
    SampleScenarioMismatch {
        /// Runtime whose sample mismatched.
        runtime: BenchmarkSampleRuntime,
        /// Expected scenario.
        expected: BenchmarkScenario,
        /// Actual scenario.
        actual: BenchmarkScenario,
    },
    /// A comparable sample is missing a required scenario interaction.
    MissingSampleInteraction {
        /// Runtime whose sample is incomplete.
        runtime: BenchmarkSampleRuntime,
        /// Scenario being validated.
        scenario: BenchmarkScenario,
        /// Missing interaction.
        interaction: String,
    },
    /// A sample descriptor has invalid generated metadata.
    InvalidSampleField {
        /// Runtime whose sample is invalid.
        runtime: BenchmarkSampleRuntime,
        /// Scenario being validated.
        scenario: BenchmarkScenario,
        /// Invalid field name.
        field: String,
        /// Validation failure reason.
        reason: String,
    },
    /// A benchmark result contains malformed or non-comparable evidence.
    InvalidResultField {
        /// Scenario whose result is invalid.
        scenario: BenchmarkScenario,
        /// Subject whose result is invalid.
        subject: String,
        /// Invalid field name.
        field: String,
        /// Validation failure reason.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// A single measurement collected during a benchmark run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkMeasurement {
    /// The metric being measured.
    pub metric: BenchmarkMetric,
    /// The measured value.
    pub value: f64,
    /// The unit of the value.
    pub unit: MetricUnit,
    /// When the measurement was taken relative to benchmark start.
    pub elapsed: Duration,
}

impl BenchmarkMeasurement {
    /// Validate that this measurement is finite and uses a compatible unit.
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.value.is_finite(), "measurement value must be finite");
        anyhow::ensure!(
            self.metric == BenchmarkMetric::MemoryGrowth || self.value >= 0.0,
            "measurement value cannot be negative"
        );
        anyhow::ensure!(
            self.metric.accepts_unit(self.unit),
            "measurement unit is incompatible with metric"
        );
        if matches!(
            self.metric,
            BenchmarkMetric::GpuUsage | BenchmarkMetric::AssetCacheHitRate
        ) {
            anyhow::ensure!(
                self.value <= 100.0,
                "percentage measurement cannot exceed 100"
            );
        }
        Ok(())
    }
}

/// Types of benchmark metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BenchmarkMetric {
    /// Time from process launch to first frame rendered.
    ColdStart,
    /// Time from background to foreground with first frame rendered.
    WarmStart,
    /// Time until the UI is interactive after launch.
    FirstInteractiveFrame,
    /// Resident memory at idle.
    IdleMemory,
    /// Input event to frame presentation latency.
    InputLatency,
    /// Median frame time (50th percentile).
    FrameTimeP50,
    /// 95th percentile frame time.
    FrameTimeP95,
    /// 99th percentile frame time.
    FrameTimeP99,
    /// Input-to-present latency during scroll interactions.
    ScrollLatency,
    /// Time to complete a window resize interaction smoothly.
    ResizeSmoothness,
    /// Time to complete a scroll interaction smoothly.
    ScrollSmoothness,
    /// Memory growth over a long session in megabytes.
    MemoryGrowth,
    /// CPU utilization over a long session.
    LongSessionCpu,
    /// GPU utilization percentage.
    GpuUsage,
    /// Energy impact score over a long session.
    LongSessionEnergy,
    /// Idle power consumption score.
    IdlePower,
    /// Thread/timer wakeups per second at idle.
    WakeupsPerSecond,
    /// Asset cache hit rate as a percentage.
    AssetCacheHitRate,
}

/// Units for benchmark measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricUnit {
    /// Time in milliseconds.
    Milliseconds,
    /// Time in microseconds.
    Microseconds,
    /// Memory in megabytes.
    Megabytes,
    /// A percentage value.
    Percent,
    /// Frame rate in frames per second.
    FramesPerSecond,
    /// Wakeups per second.
    WakeupsPerSec,
    /// A dimensionless score.
    Score,
}

impl std::fmt::Display for MetricUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Milliseconds => write!(f, "ms"),
            Self::Microseconds => write!(f, "µs"),
            Self::Megabytes => write!(f, "MB"),
            Self::Percent => write!(f, "%"),
            Self::FramesPerSecond => write!(f, "fps"),
            Self::WakeupsPerSec => write!(f, "wakeups/s"),
            Self::Score => write!(f, "score"),
        }
    }
}

impl BenchmarkMetric {
    /// Whether lower values are better for this metric.
    pub fn lower_is_better(&self) -> bool {
        match self {
            Self::AssetCacheHitRate => false,
            _ => true,
        }
    }

    fn accepts_unit(self, unit: MetricUnit) -> bool {
        match self {
            Self::ColdStart
            | Self::WarmStart
            | Self::FirstInteractiveFrame
            | Self::InputLatency
            | Self::FrameTimeP50
            | Self::FrameTimeP95
            | Self::FrameTimeP99
            | Self::ScrollLatency => unit == MetricUnit::Milliseconds,
            Self::ResizeSmoothness | Self::ScrollSmoothness => {
                matches!(unit, MetricUnit::Milliseconds | MetricUnit::FramesPerSecond)
            }
            Self::IdleMemory | Self::MemoryGrowth => unit == MetricUnit::Megabytes,
            Self::LongSessionCpu | Self::GpuUsage | Self::AssetCacheHitRate => {
                unit == MetricUnit::Percent
            }
            Self::LongSessionEnergy | Self::IdlePower => unit == MetricUnit::Score,
            Self::WakeupsPerSecond => unit == MetricUnit::WakeupsPerSec,
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark Run
// ---------------------------------------------------------------------------

/// The full result of a benchmark run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// The scenario that was benchmarked.
    pub scenario: BenchmarkScenario,
    /// The name of the platform/framework under test.
    pub subject: String,
    /// Individual measurements.
    pub measurements: Vec<BenchmarkMeasurement>,
    /// Start time of the benchmark.
    #[serde(skip, default = "Instant::now")]
    pub started_at: Instant,
    /// Total duration of the benchmark run.
    pub duration: Duration,
    /// Hardware and OS conditions recorded for fair comparison.
    pub environment: BenchmarkEnvironment,
}

impl BenchmarkResult {
    /// Return all malformed or non-comparable fields in this result.
    pub fn validation_issues(&self) -> Vec<BenchmarkEvidenceIssue> {
        let mut issues = Vec::new();
        if self.subject.trim().is_empty()
            || self.subject != self.subject.trim()
            || self.subject.chars().any(char::is_control)
            || self.subject.chars().count() > MAX_BENCHMARK_TEXT_CHARS
        {
            issues.push(BenchmarkEvidenceIssue::InvalidResultField {
                scenario: self.scenario,
                subject: truncate_benchmark_text(&self.subject),
                field: "subject".to_string(),
                reason: "subject must be bounded, trimmed, and free of control characters"
                    .to_string(),
            });
        }
        if self.measurements.len() > MAX_MEASUREMENTS_PER_RESULT {
            issues.push(BenchmarkEvidenceIssue::InvalidResultField {
                scenario: self.scenario,
                subject: truncate_benchmark_text(&self.subject),
                field: "measurements".to_string(),
                reason: format!(
                    "result cannot contain more than {MAX_MEASUREMENTS_PER_RESULT} measurements"
                ),
            });
        }
        let mut seen = std::collections::HashSet::new();
        for measurement in &self.measurements {
            if let Err(error) = measurement.validate() {
                issues.push(BenchmarkEvidenceIssue::InvalidResultField {
                    scenario: self.scenario,
                    subject: truncate_benchmark_text(&self.subject),
                    field: "measurement".to_string(),
                    reason: error.to_string(),
                });
            }
            if !seen.insert((measurement.metric, measurement.unit)) {
                issues.push(BenchmarkEvidenceIssue::InvalidResultField {
                    scenario: self.scenario,
                    subject: truncate_benchmark_text(&self.subject),
                    field: "measurements".to_string(),
                    reason: "duplicate metric/unit measurement".to_string(),
                });
            }
        }
        issues
    }
}

/// Hardware and OS environment recorded during benchmarking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkEnvironment {
    /// Operating system name.
    pub os_name: String,
    /// Operating system version.
    pub os_version: String,
    /// CPU description.
    pub cpu: String,
    /// Total system memory in GB.
    pub memory_gb: u32,
    /// GPU description.
    pub gpu: String,
}

impl BenchmarkEnvironment {
    /// Collect the current environment information.
    pub fn current() -> Self {
        Self {
            os_name: std::env::consts::OS.to_string(),
            os_version: Self::os_version(),
            cpu: Self::cpu_info(),
            memory_gb: Self::system_memory_gb(),
            gpu: String::new(),
        }
    }

    #[cfg(target_os = "macos")]
    fn os_version() -> String {
        unsafe {
            let mut size = 0usize;
            if libc::sysctlbyname(
                c"kern.osproductversion".as_ptr(),
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            ) == 0
                && size > 0
                && size <= MAX_SYSCTL_STRING_BYTES
            {
                let mut buf = vec![0u8; size];
                if libc::sysctlbyname(
                    c"kern.osproductversion".as_ptr(),
                    buf.as_mut_ptr() as *mut _,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                ) == 0
                    && size > 0
                    && size <= buf.len()
                {
                    let value_len = if buf[size - 1] == 0 { size - 1 } else { size };
                    return String::from_utf8_lossy(&buf[..value_len]).to_string();
                }
            }
        }
        String::new()
    }

    #[cfg(not(target_os = "macos"))]
    fn os_version() -> String {
        String::new()
    }

    fn cpu_info() -> String {
        let value = std::env::var("PROCESSOR_IDENTIFIER")
            .or_else(|_| std::env::var("CPU"))
            .unwrap_or_default();
        truncate_benchmark_text(&value)
    }

    #[cfg(target_os = "macos")]
    fn system_memory_gb() -> u32 {
        unsafe {
            let mut mem: u64 = 0;
            let mut size = std::mem::size_of::<u64>();
            if libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                &mut mem as *mut _ as *mut _,
                &mut size,
                std::ptr::null_mut(),
                0,
            ) == 0
            {
                return (mem / (1024 * 1024 * 1024)) as u32;
            }
        }
        0
    }

    #[cfg(target_os = "linux")]
    fn system_memory_gb() -> u32 {
        if let Ok(contents) = std::fs::read_to_string("/proc/meminfo") {
            for line in contents.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb_str) = rest.trim().split_whitespace().next() {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            return (kb / (1024 * 1024)) as u32;
                        }
                    }
                }
            }
        }
        0
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn system_memory_gb() -> u32 {
        0
    }
}

// ---------------------------------------------------------------------------
// Metric Collectors
// ---------------------------------------------------------------------------

/// Trait for benchmark metric collectors.
pub trait MetricCollector: Send {
    /// Start the collector.
    fn start(&mut self);
    /// Stop the collector and return measurements.
    fn stop(&mut self) -> Vec<BenchmarkMeasurement>;
    /// Return a mutable reference to `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

/// Measures time from creation until first collection.
pub struct ColdStartCollector {
    start: Instant,
    stopped: bool,
}

impl ColdStartCollector {
    /// Create a new collector starting now.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            stopped: false,
        }
    }
}

impl Default for ColdStartCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCollector for ColdStartCollector {
    fn start(&mut self) {
        self.start = Instant::now();
        self.stopped = false;
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        if self.stopped {
            return Vec::new();
        }
        self.stopped = true;
        let elapsed = self.start.elapsed();
        vec![BenchmarkMeasurement {
            metric: BenchmarkMetric::ColdStart,
            value: elapsed.as_secs_f64() * 1000.0,
            unit: MetricUnit::Milliseconds,
            elapsed,
        }]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Measures resident memory using platform APIs.
pub struct MemoryCollector {
    sample_time: Instant,
}

impl MemoryCollector {
    /// Create a new memory collector.
    pub fn new() -> Self {
        Self {
            sample_time: Instant::now(),
        }
    }

    /// Read current resident memory in megabytes.
    pub fn resident_mb() -> f64 {
        #[cfg(target_os = "linux")]
        {
            if let Ok(contents) = std::fs::read_to_string("/proc/self/status") {
                for line in contents.lines() {
                    if let Some(rest) = line.strip_prefix("VmRSS:") {
                        if let Some(kb_str) = rest.trim().split_whitespace().next() {
                            if let Ok(kb) = kb_str.parse::<f64>() {
                                return kb / 1024.0;
                            }
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut rusage) } == 0 {
                return rusage.ru_maxrss as f64 / (1024.0 * 1024.0);
            }
        }

        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::ProcessStatus::GetProcessMemoryInfo;
            use windows::Win32::System::Threading::GetCurrentProcess;
            unsafe {
                let mut counters = std::mem::zeroed();
                let process = GetCurrentProcess();
                if GetProcessMemoryInfo(
                    process,
                    &mut counters,
                    std::mem::size_of_val(&counters) as u32,
                )
                .is_ok()
                {
                    return counters.WorkingSetSize as f64 / (1024.0 * 1024.0);
                }
            }
        }

        0.0
    }
}

impl Default for MemoryCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCollector for MemoryCollector {
    fn start(&mut self) {
        self.sample_time = Instant::now();
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        vec![BenchmarkMeasurement {
            metric: BenchmarkMetric::IdleMemory,
            value: Self::resident_mb(),
            unit: MetricUnit::Megabytes,
            elapsed: self.sample_time.elapsed(),
        }]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Measures input-to-frame presentation latency.
pub struct InputLatencyCollector {
    input_time: Option<Instant>,
    latencies: Vec<Duration>,
}

impl InputLatencyCollector {
    /// Create a new latency collector.
    pub fn new() -> Self {
        Self {
            input_time: None,
            latencies: Vec::new(),
        }
    }

    /// Record that an input event occurred.
    pub fn record_input(&mut self) {
        self.input_time = Some(Instant::now());
    }

    /// Record that the frame was presented.
    pub fn record_frame_presented(&mut self) {
        if let Some(input_time) = self.input_time.take() {
            if self.latencies.len() < MAX_COLLECTOR_SAMPLES {
                self.latencies.push(input_time.elapsed());
            }
        }
    }

    /// Average latency in milliseconds.
    pub fn average_ms(&self) -> f64 {
        if self.latencies.is_empty() {
            return 0.0;
        }
        let total_us: u128 = self.latencies.iter().map(|d| d.as_micros()).sum();
        total_us as f64 / self.latencies.len() as f64 / 1000.0
    }
}

impl Default for InputLatencyCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCollector for InputLatencyCollector {
    fn start(&mut self) {
        self.input_time = None;
        self.latencies.clear();
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        vec![BenchmarkMeasurement {
            metric: BenchmarkMetric::InputLatency,
            value: self.average_ms(),
            unit: MetricUnit::Milliseconds,
            elapsed: Duration::default(),
        }]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Measures frame times during resize or scroll interactions.
pub struct SmoothnessCollector {
    frames: Vec<Duration>,
    last_frame: Option<Instant>,
    metric: BenchmarkMetric,
}

impl SmoothnessCollector {
    /// Create a new smoothness collector for the given metric.
    pub fn new(metric: BenchmarkMetric) -> Self {
        Self::try_new(metric).expect("SmoothnessCollector only supports resize or scroll metrics")
    }

    /// Create a collector after validating that the metric measures smoothness.
    pub fn try_new(metric: BenchmarkMetric) -> anyhow::Result<Self> {
        anyhow::ensure!(
            matches!(
                metric,
                BenchmarkMetric::ResizeSmoothness | BenchmarkMetric::ScrollSmoothness
            ),
            "SmoothnessCollector only supports resize or scroll metrics"
        );
        Ok(Self {
            frames: Vec::new(),
            last_frame: None,
            metric,
        })
    }

    /// Record a frame timestamp.
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            if self.frames.len() < MAX_COLLECTOR_SAMPLES {
                self.frames.push(now.duration_since(last));
            }
        }
        self.last_frame = Some(now);
    }

    /// Average frame time in milliseconds.
    pub fn average_frame_time_ms(&self) -> f64 {
        if self.frames.is_empty() {
            return 0.0;
        }
        let total_us: u128 = self.frames.iter().map(|d| d.as_micros()).sum();
        total_us as f64 / self.frames.len() as f64 / 1000.0
    }

    /// Minimum frame time in milliseconds.
    pub fn min_frame_time_ms(&self) -> f64 {
        if self.frames.is_empty() {
            return 0.0;
        }
        self.frames
            .iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .fold(f64::MAX, f64::min)
            .min(f64::MAX)
    }

    /// Maximum frame time in milliseconds.
    pub fn max_frame_time_ms(&self) -> f64 {
        self.frames
            .iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .fold(0.0, f64::max)
    }

    /// Estimated FPS from average frame time.
    pub fn estimated_fps(&self) -> f64 {
        let avg_ms = self.average_frame_time_ms();
        if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 }
    }
}

impl MetricCollector for SmoothnessCollector {
    fn start(&mut self) {
        self.frames.clear();
        self.last_frame = None;
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        vec![
            BenchmarkMeasurement {
                metric: self.metric,
                value: self.average_frame_time_ms(),
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: self.metric,
                value: self.estimated_fps(),
                unit: MetricUnit::FramesPerSecond,
                elapsed: Duration::default(),
            },
        ]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Measures CPU utilization and energy impact over a long session.
pub struct LongSessionCollector {
    start: Instant,
    samples: Vec<CpuSample>,
    last_cpu_time: Duration,
    sampling_interval: Duration,
    last_sample: Instant,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct CpuSample {
    elapsed: Duration,
    cpu_percent: f64,
}

impl LongSessionCollector {
    /// Create a new long session collector with the given sampling interval.
    pub fn new(sampling_interval: Duration) -> Self {
        Self {
            start: Instant::now(),
            samples: Vec::new(),
            last_cpu_time: Duration::default(),
            sampling_interval,
            last_sample: Instant::now(),
        }
    }

    /// Sample current CPU usage. Call periodically.
    pub fn sample(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start);
        let wall_elapsed = now.duration_since(self.last_sample);
        self.last_sample = now;
        let cpu_time = Self::process_cpu_time();
        let delta_cpu = cpu_time.saturating_sub(self.last_cpu_time);
        self.last_cpu_time = cpu_time;

        let sample_elapsed = if wall_elapsed.is_zero() {
            self.sampling_interval
        } else {
            wall_elapsed
        };
        let cpu_percent = if sample_elapsed.as_secs_f64() > 0.0 {
            (delta_cpu.as_secs_f64() / sample_elapsed.as_secs_f64()) * 100.0
        } else {
            0.0
        };

        if self.samples.len() < MAX_COLLECTOR_SAMPLES {
            self.samples.push(CpuSample {
                elapsed,
                cpu_percent: cpu_percent.min(100.0 * num_cpus::get() as f64),
            });
        }
    }

    fn process_cpu_time() -> Duration {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut rusage) } == 0 {
                let duration_from_timeval = |value: libc::timeval| {
                    let seconds = u64::try_from(value.tv_sec).ok()?;
                    let micros = u64::try_from(value.tv_usec).ok()?;
                    if micros >= 1_000_000 {
                        return None;
                    }
                    Duration::from_secs(seconds).checked_add(Duration::from_micros(micros))
                };
                if let (Some(utime), Some(stime)) = (
                    duration_from_timeval(rusage.ru_utime),
                    duration_from_timeval(rusage.ru_stime),
                ) {
                    return utime.checked_add(stime).unwrap_or(Duration::MAX);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            use windows::Win32::System::Threading::GetCurrentProcess;
            use windows::Win32::System::Threading::GetProcessTimes;
            unsafe {
                let mut creation = std::mem::zeroed();
                let mut exit = std::mem::zeroed();
                let mut kernel = std::mem::zeroed();
                let mut user = std::mem::zeroed();
                let process = GetCurrentProcess();
                if GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user)
                    .is_ok()
                {
                    let kernel_us =
                        ((kernel.dwHighDateTime as u64) << 32 | kernel.dwLowDateTime as u64) / 10;
                    let user_us =
                        ((user.dwHighDateTime as u64) << 32 | user.dwLowDateTime as u64) / 10;
                    return Duration::from_micros(kernel_us + user_us);
                }
            }
        }

        Duration::default()
    }

    /// Average CPU percentage across all samples.
    pub fn average_cpu_percent(&self) -> f64 {
        if self.samples.is_empty() {
            return 0.0;
        }
        self.samples.iter().map(|s| s.cpu_percent).sum::<f64>() / self.samples.len() as f64
    }

    /// Estimated energy impact score (0-100, higher = more energy used).
    pub fn energy_score(&self) -> f64 {
        let avg_cpu = self.average_cpu_percent();
        let duration_minutes = self.start.elapsed().as_secs_f64() / 60.0;
        (avg_cpu * duration_minutes / 100.0).min(100.0)
    }
}

impl Default for LongSessionCollector {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}

impl MetricCollector for LongSessionCollector {
    fn start(&mut self) {
        self.start = Instant::now();
        self.samples.clear();
        self.last_cpu_time = Self::process_cpu_time();
        self.last_sample = self.start;
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        vec![
            BenchmarkMeasurement {
                metric: BenchmarkMetric::LongSessionCpu,
                value: self.average_cpu_percent(),
                unit: MetricUnit::Percent,
                elapsed: self.start.elapsed(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::LongSessionEnergy,
                value: self.energy_score(),
                unit: MetricUnit::Score,
                elapsed: self.start.elapsed(),
            },
        ]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Collects frame times and computes percentiles (P50, P95, P99).
pub struct FrameTimeCollector {
    frame_times: Vec<Duration>,
    last_frame: Option<Instant>,
}

impl FrameTimeCollector {
    /// Create a new frame time collector.
    pub fn new() -> Self {
        Self {
            frame_times: Vec::new(),
            last_frame: None,
        }
    }

    /// Record a frame timestamp for percentile computation.
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            if self.frame_times.len() < MAX_COLLECTOR_SAMPLES {
                self.frame_times.push(now.duration_since(last));
            }
        }
        self.last_frame = Some(now);
    }

    fn percentile_ms(&self, p: f64) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self
            .frame_times
            .iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }
}

impl Default for FrameTimeCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCollector for FrameTimeCollector {
    fn start(&mut self) {
        self.frame_times.clear();
        self.last_frame = None;
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        vec![
            BenchmarkMeasurement {
                metric: BenchmarkMetric::FrameTimeP50,
                value: self.percentile_ms(50.0),
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::FrameTimeP95,
                value: self.percentile_ms(95.0),
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::FrameTimeP99,
                value: self.percentile_ms(99.0),
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
        ]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Tracks memory growth over the duration of a benchmark.
pub struct MemoryGrowthCollector {
    start_memory_mb: f64,
}

impl MemoryGrowthCollector {
    /// Create a new memory growth collector.
    pub fn new() -> Self {
        Self {
            start_memory_mb: 0.0,
        }
    }
}

impl Default for MemoryGrowthCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCollector for MemoryGrowthCollector {
    fn start(&mut self) {
        self.start_memory_mb = MemoryCollector::resident_mb();
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        let end_mb = MemoryCollector::resident_mb();
        vec![BenchmarkMeasurement {
            metric: BenchmarkMetric::MemoryGrowth,
            value: end_mb - self.start_memory_mb,
            unit: MetricUnit::Megabytes,
            elapsed: Duration::default(),
        }]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// Tracks asset cache hit rate during a benchmark.
pub struct CacheHitRateCollector {
    hits: u64,
    misses: u64,
}

impl CacheHitRateCollector {
    /// Create a new cache hit rate collector.
    pub fn new() -> Self {
        Self { hits: 0, misses: 0 }
    }

    /// Record a cache hit.
    pub fn record_hit(&mut self) {
        self.make_room();
        self.hits += 1;
    }

    /// Record a cache miss.
    pub fn record_miss(&mut self) {
        self.make_room();
        self.misses += 1;
    }

    /// Compute hit rate as a percentage (0-100).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            return 0.0;
        }
        (self.hits as f64 / total as f64) * 100.0
    }

    fn make_room(&mut self) {
        if self.hits == u64::MAX || self.misses == u64::MAX {
            self.hits /= 2;
            self.misses /= 2;
        }
    }
}

impl Default for CacheHitRateCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricCollector for CacheHitRateCollector {
    fn start(&mut self) {
        self.hits = 0;
        self.misses = 0;
    }

    fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
        vec![BenchmarkMeasurement {
            metric: BenchmarkMetric::AssetCacheHitRate,
            value: self.hit_rate(),
            unit: MetricUnit::Percent,
            elapsed: Duration::default(),
        }]
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

// ---------------------------------------------------------------------------
// Regression Thresholds
// ---------------------------------------------------------------------------

/// Configurable regression thresholds per metric.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionThresholds {
    /// Default threshold percentage for all metrics.
    pub default_percent: f64,
    /// Per-metric threshold overrides.
    pub overrides: std::collections::HashMap<BenchmarkMetric, f64>,
}

impl RegressionThresholds {
    /// Create thresholds with the given default percentage.
    pub fn new(default_percent: f64) -> Self {
        Self {
            default_percent,
            overrides: std::collections::HashMap::new(),
        }
    }

    /// Create thresholds after validating the default percentage.
    pub fn new_checked(default_percent: f64) -> anyhow::Result<Self> {
        let thresholds = Self::new(default_percent);
        thresholds.validate()?;
        Ok(thresholds)
    }

    /// Add a per-metric threshold override.
    pub fn with_override(mut self, metric: BenchmarkMetric, percent: f64) -> Self {
        self.overrides.insert(metric, percent);
        self
    }

    /// Add an override and validate the complete threshold set.
    pub fn with_override_checked(
        mut self,
        metric: BenchmarkMetric,
        percent: f64,
    ) -> anyhow::Result<Self> {
        self.overrides.insert(metric, percent);
        self.validate()?;
        Ok(self)
    }

    /// Validate threshold percentages and the override count.
    pub fn validate(&self) -> anyhow::Result<()> {
        validate_threshold_percent(self.default_percent)?;
        anyhow::ensure!(
            self.overrides.len() <= MAX_THRESHOLD_OVERRIDES,
            "benchmark threshold overrides cannot exceed {MAX_THRESHOLD_OVERRIDES}"
        );
        for percent in self.overrides.values() {
            validate_threshold_percent(*percent)?;
        }
        Ok(())
    }

    /// Get the threshold for a specific metric.
    pub fn threshold_for(&self, metric: BenchmarkMetric) -> f64 {
        let percent = self
            .overrides
            .get(&metric)
            .copied()
            .unwrap_or(self.default_percent);
        if validate_threshold_percent(percent).is_ok() {
            percent
        } else {
            0.0
        }
    }
}

fn validate_threshold_percent(percent: f64) -> anyhow::Result<()> {
    anyhow::ensure!(
        percent.is_finite() && (0.0..=10_000.0).contains(&percent),
        "benchmark regression threshold must be finite and in 0..=10000"
    );
    Ok(())
}

impl Default for RegressionThresholds {
    fn default() -> Self {
        Self::new(10.0)
    }
}

/// Check regressions with per-metric thresholds.
pub fn check_regressions_with_thresholds(
    baseline: &[BenchmarkResult],
    candidate: &[BenchmarkResult],
    thresholds: &RegressionThresholds,
) -> Vec<Regression> {
    let mut regressions = Vec::new();

    for candidate_result in candidate {
        if let Some(baseline_result) = baseline
            .iter()
            .find(|b| b.scenario == candidate_result.scenario)
        {
            for comparison in compare_results(baseline_result, candidate_result) {
                let threshold = thresholds.threshold_for(comparison.metric);
                let is_regression = if comparison.lower_is_better {
                    comparison.percent_change > threshold
                } else {
                    comparison.percent_change < -threshold
                };

                if is_regression {
                    regressions.push(Regression {
                        scenario: candidate_result.scenario,
                        metric: comparison.metric,
                        baseline: comparison.baseline,
                        candidate: comparison.candidate,
                        percent_change: comparison.percent_change,
                        unit: comparison.unit,
                    });
                }
            }
        }
    }

    regressions
}

// ---------------------------------------------------------------------------
// CI Report
// ---------------------------------------------------------------------------

/// A CI-friendly benchmark report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiReport {
    /// Benchmark results from the candidate run.
    pub results: Vec<BenchmarkResult>,
    /// Detected regressions.
    pub regressions: Vec<Regression>,
    /// Whether the run passed all regression checks.
    pub passed: bool,
    /// Path to the attached Chrome Trace file, if any.
    pub trace_file: Option<String>,
}

impl CiReport {
    /// Generate a CI report comparing candidate results against a baseline.
    pub fn generate(
        baseline: &[BenchmarkResult],
        candidate: &[BenchmarkResult],
        thresholds: &RegressionThresholds,
        trace_file: Option<String>,
    ) -> Self {
        let regressions = check_regressions_with_thresholds(baseline, candidate, thresholds);
        let thresholds_valid = thresholds.validate().is_ok();
        Self {
            results: candidate.to_vec(),
            regressions: regressions.clone(),
            passed: thresholds_valid && regressions.is_empty(),
            trace_file,
        }
    }

    /// Serialize the report to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Generate a human-readable summary of the report.
    pub fn summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Benchmark Report: {}\n",
            if self.passed { "PASSED" } else { "FAILED" }
        ));
        out.push_str(&format!("Results: {} scenarios\n", self.results.len()));
        if !self.regressions.is_empty() {
            out.push_str(&format!("Regressions: {}\n", self.regressions.len()));
            for reg in &self.regressions {
                out.push_str(&format!(
                    "  {:?}/{:?}: {:.1}{} -> {:.1}{} ({:+.1}%)\n",
                    reg.scenario,
                    reg.metric,
                    reg.baseline,
                    reg.unit,
                    reg.candidate,
                    reg.unit,
                    reg.percent_change,
                ));
            }
        }
        if let Some(trace) = &self.trace_file {
            out.push_str(&format!("Trace: {}\n", trace));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Baseline Comparison Report
// ---------------------------------------------------------------------------

/// A benchmark comparison where Baseline is the baseline and Kael is the candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineComparisonReport {
    /// Baseline baseline results.
    pub baseline_results: Vec<BenchmarkResult>,
    /// Kael candidate results.
    pub kael_results: Vec<BenchmarkResult>,
    /// Per-metric comparisons for matching scenarios.
    pub comparisons: Vec<BaselineMetricComparison>,
    /// Evidence issues found before comparison.
    pub evidence_issues: Vec<BenchmarkEvidenceIssue>,
    /// Comparable Baseline/Kael sample contracts supplied for this report.
    #[serde(default)]
    pub sample_pairs: Vec<BenchmarkSamplePair>,
    /// Path to the attached Chrome Trace file, if any.
    pub trace_file: Option<String>,
}

impl BaselineComparisonReport {
    /// Generate an Baseline-vs-Kael report from matching scenario result sets.
    pub fn generate(
        baseline_results: &[BenchmarkResult],
        kael_results: &[BenchmarkResult],
        trace_file: Option<String>,
    ) -> Self {
        Self::generate_with_sample_pairs(baseline_results, kael_results, &[], trace_file)
    }

    /// Generate an Baseline-vs-Kael report and validate comparable sample contracts.
    pub fn generate_with_sample_pairs(
        baseline_results: &[BenchmarkResult],
        kael_results: &[BenchmarkResult],
        sample_pairs: &[BenchmarkSamplePair],
        trace_file: Option<String>,
    ) -> Self {
        let mut comparisons = Vec::new();
        let mut evidence_issues = Vec::new();
        evidence_issues.extend(duplicate_result_issues(
            baseline_results,
            BenchmarkSampleRuntime::Baseline,
        ));
        evidence_issues.extend(duplicate_result_issues(
            kael_results,
            BenchmarkSampleRuntime::Kael,
        ));

        for kael_result in kael_results {
            let Some(baseline_result) = baseline_results
                .iter()
                .find(|result| result.scenario == kael_result.scenario)
            else {
                evidence_issues.push(BenchmarkEvidenceIssue::MissingResult {
                    scenario: kael_result.scenario,
                    runtime: BenchmarkSampleRuntime::Baseline,
                });
                continue;
            };

            let spec = kael_result.scenario.workload_spec();
            evidence_issues.extend(spec.validate_result(baseline_result));
            evidence_issues.extend(spec.validate_result(kael_result));
            evidence_issues.extend(environment_mismatch_issues(
                kael_result.scenario,
                &baseline_result.environment,
                &kael_result.environment,
            ));
            if let Some(pair) = sample_pairs
                .iter()
                .find(|pair| pair.scenario() == kael_result.scenario)
            {
                evidence_issues.extend(pair.validate());
            } else {
                evidence_issues.push(BenchmarkEvidenceIssue::MissingSample {
                    scenario: kael_result.scenario,
                    runtime: BenchmarkSampleRuntime::Baseline,
                });
                evidence_issues.push(BenchmarkEvidenceIssue::MissingSample {
                    scenario: kael_result.scenario,
                    runtime: BenchmarkSampleRuntime::Kael,
                });
            }
            comparisons.extend(
                compare_results(baseline_result, kael_result)
                    .into_iter()
                    .map(|comparison| BaselineMetricComparison {
                        scenario: kael_result.scenario,
                        metric: comparison.metric,
                        baseline: comparison.baseline,
                        kael: comparison.candidate,
                        delta: comparison.delta,
                        percent_change: comparison.percent_change,
                        unit: comparison.unit,
                        lower_is_better: comparison.lower_is_better,
                    }),
            );
        }

        for baseline_result in baseline_results {
            if !kael_results
                .iter()
                .any(|result| result.scenario == baseline_result.scenario)
            {
                evidence_issues.push(BenchmarkEvidenceIssue::MissingResult {
                    scenario: baseline_result.scenario,
                    runtime: BenchmarkSampleRuntime::Kael,
                });
            }
        }

        Self {
            baseline_results: baseline_results.to_vec(),
            kael_results: kael_results.to_vec(),
            comparisons,
            evidence_issues,
            sample_pairs: sample_pairs.to_vec(),
            trace_file,
        }
    }

    /// Return comparisons where Kael is better than the Baseline baseline.
    pub fn kael_wins(&self) -> impl Iterator<Item = &BaselineMetricComparison> {
        self.comparisons
            .iter()
            .filter(|comparison| comparison.kael_is_better())
    }

    /// Return comparisons where Baseline is better than Kael.
    pub fn baseline_wins(&self) -> impl Iterator<Item = &BaselineMetricComparison> {
        self.comparisons
            .iter()
            .filter(|comparison| comparison.baseline_is_better())
    }

    /// Serialize the report to JSON.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Generate a human-readable summary of the report.
    pub fn summary(&self) -> String {
        let kael_wins = self.kael_wins().count();
        let baseline_wins = self.baseline_wins().count();
        let ties = self
            .comparisons
            .iter()
            .filter(|comparison| comparison.is_tie())
            .count();

        let mut out = String::new();
        out.push_str("Baseline Comparison Report\n");
        out.push_str(&format!(
            "Compared metrics: {} (Kael wins: {kael_wins}, Baseline wins: {baseline_wins}, ties: {ties})\n",
            self.comparisons.len()
        ));
        if !self.evidence_issues.is_empty() {
            out.push_str(&format!(
                "Evidence issues: {} (do not publish parity claims until resolved)\n",
                self.evidence_issues.len()
            ));
            for issue in &self.evidence_issues {
                out.push_str(&format!("  {issue:?}\n"));
            }
        }

        for comparison in &self.comparisons {
            let winner = if comparison.kael_is_better() {
                "Kael"
            } else if comparison.baseline_is_better() {
                "Baseline"
            } else {
                "Tie"
            };
            out.push_str(&format!(
                "  {:?}/{:?}: Baseline {:.1}{}, Kael {:.1}{} ({:+.1}%) => {}\n",
                comparison.scenario,
                comparison.metric,
                comparison.baseline,
                comparison.unit,
                comparison.kael,
                comparison.unit,
                comparison.percent_change,
                winner,
            ));
        }

        if let Some(trace) = &self.trace_file {
            out.push_str(&format!("Trace: {}\n", trace));
        }

        out
    }
}

fn duplicate_result_issues(
    results: &[BenchmarkResult],
    runtime: BenchmarkSampleRuntime,
) -> Vec<BenchmarkEvidenceIssue> {
    let mut issues = Vec::new();

    for scenario in BenchmarkScenario::all() {
        let count = results
            .iter()
            .filter(|result| result.scenario == *scenario)
            .count();
        if count > 1 {
            issues.push(BenchmarkEvidenceIssue::DuplicateResult {
                scenario: *scenario,
                runtime,
                count,
            });
        }
    }

    issues
}

fn environment_mismatch_issues(
    scenario: BenchmarkScenario,
    baseline: &BenchmarkEnvironment,
    kael: &BenchmarkEnvironment,
) -> Vec<BenchmarkEvidenceIssue> {
    let mut issues = Vec::new();

    push_environment_mismatch(
        &mut issues,
        scenario,
        "os_name",
        &baseline.os_name,
        &kael.os_name,
    );
    push_environment_mismatch(
        &mut issues,
        scenario,
        "os_version",
        &baseline.os_version,
        &kael.os_version,
    );
    push_environment_mismatch(&mut issues, scenario, "cpu", &baseline.cpu, &kael.cpu);
    push_environment_mismatch(
        &mut issues,
        scenario,
        "memory_gb",
        &baseline.memory_gb.to_string(),
        &kael.memory_gb.to_string(),
    );
    push_environment_mismatch(&mut issues, scenario, "gpu", &baseline.gpu, &kael.gpu);

    issues
}

fn push_environment_mismatch(
    issues: &mut Vec<BenchmarkEvidenceIssue>,
    scenario: BenchmarkScenario,
    field: &'static str,
    baseline: &str,
    kael: &str,
) {
    if baseline != kael {
        issues.push(BenchmarkEvidenceIssue::EnvironmentMismatch {
            scenario,
            field: field.to_string(),
            baseline: baseline.to_string(),
            kael: kael.to_string(),
        });
    }
}

/// One Baseline-vs-Kael metric comparison.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineMetricComparison {
    /// Scenario being compared.
    pub scenario: BenchmarkScenario,
    /// Metric being compared.
    pub metric: BenchmarkMetric,
    /// Baseline baseline value.
    pub baseline: f64,
    /// Kael candidate value.
    pub kael: f64,
    /// Absolute difference (`kael - baseline`).
    pub delta: f64,
    /// Percentage change from Baseline to Kael.
    pub percent_change: f64,
    /// Unit of measurement.
    pub unit: MetricUnit,
    /// Whether lower values are better for this metric.
    pub lower_is_better: bool,
}

impl BaselineMetricComparison {
    /// Return true when Kael beats the Baseline baseline.
    pub fn kael_is_better(&self) -> bool {
        if self.is_tie() {
            return false;
        }
        if self.lower_is_better {
            self.kael < self.baseline
        } else {
            self.kael > self.baseline
        }
    }

    /// Return true when Baseline beats Kael.
    pub fn baseline_is_better(&self) -> bool {
        if self.is_tie() {
            return false;
        }
        !self.kael_is_better()
    }

    /// Return true when both values are equal.
    pub fn is_tie(&self) -> bool {
        (self.kael - self.baseline).abs() < f64::EPSILON
    }
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A benchmark harness that runs scenarios and collects measurements.
pub struct BenchmarkHarness {
    results: Vec<BenchmarkResult>,
    tracer: Option<Tracer>,
}

impl BenchmarkHarness {
    /// Create a new harness.
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            tracer: Tracer::global(),
        }
    }

    /// Set a tracer for emitting benchmark phase spans.
    pub fn with_tracer(mut self, tracer: Tracer) -> Self {
        self.tracer = Some(tracer);
        self
    }

    /// Run a benchmark scenario and collect results.
    pub fn run<F>(
        &mut self,
        scenario: BenchmarkScenario,
        subject: impl Into<String>,
        runner: F,
    ) -> BenchmarkResult
    where
        F: FnOnce(&mut Vec<BenchmarkMeasurement>),
    {
        let subject = truncate_benchmark_text(&subject.into());
        let started_at = Instant::now();
        let mut measurements = Vec::new();

        let tracer = self.tracer.clone();
        let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(tracer) = tracer {
                let phase_name = format!("benchmark_start:{:?}", scenario);
                tracer.record_duration(phase_name, "benchmark", || {
                    tracer.record_duration("runner", "benchmark", || {
                        runner(&mut measurements);
                    });
                });
            } else {
                runner(&mut measurements);
            }
        }));
        if let Err(payload) = run_result {
            std::panic::resume_unwind(payload);
        }
        sanitize_measurements(&mut measurements);

        let duration = started_at.elapsed();

        let result = BenchmarkResult {
            scenario,
            subject,
            measurements,
            started_at,
            duration,
            environment: BenchmarkEnvironment::current(),
        };

        self.push_result(result.clone());
        result
    }

    /// Run a benchmark with explicit metric collectors.
    pub fn run_with_collectors(
        &mut self,
        scenario: BenchmarkScenario,
        subject: impl Into<String>,
        collectors: &mut [&mut dyn MetricCollector],
        runner: impl FnOnce(&mut [&mut dyn MetricCollector]),
    ) -> BenchmarkResult {
        let subject = truncate_benchmark_text(&subject.into());
        let started_at = Instant::now();

        for collector in collectors.iter_mut() {
            collector.start();
        }

        let tracer = self.tracer.clone();
        let run_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(tracer) = tracer {
                tracer.record_duration("runner", "benchmark", || {
                    runner(collectors);
                });
            } else {
                runner(collectors);
            }
        }));

        let mut measurements = Vec::new();
        let mut stop_panic = None;
        for collector in collectors.iter_mut() {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| collector.stop())) {
                Ok(collected) => {
                    let remaining = MAX_MEASUREMENTS_PER_RESULT.saturating_sub(measurements.len());
                    measurements.extend(collected.into_iter().take(remaining));
                }
                Err(payload) if stop_panic.is_none() => stop_panic = Some(payload),
                Err(_) => {}
            }
        }
        if let Err(payload) = run_result {
            std::panic::resume_unwind(payload);
        }
        if let Some(payload) = stop_panic {
            std::panic::resume_unwind(payload);
        }
        sanitize_measurements(&mut measurements);

        let duration = started_at.elapsed();

        let result = BenchmarkResult {
            scenario,
            subject,
            measurements,
            started_at,
            duration,
            environment: BenchmarkEnvironment::current(),
        };

        self.push_result(result.clone());
        result
    }

    fn push_result(&mut self, result: BenchmarkResult) {
        if self.results.len() == MAX_HARNESS_RESULTS {
            self.results.remove(0);
        }
        self.results.push(result);
    }

    /// Return all collected results.
    pub fn results(&self) -> &[BenchmarkResult] {
        &self.results
    }

    /// Export results to a JSON string.
    pub fn export_to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.results)
    }

    /// Write the attached tracer's events to a Chrome Trace format file.
    pub fn write_trace_artifact(&self, path: impl Into<std::path::PathBuf>) -> anyhow::Result<()> {
        if let Some(tracer) = &self.tracer {
            tracer.write_to_file(path)?;
        }
        Ok(())
    }

    /// Generate a CI report comparing against a baseline file.
    pub fn generate_ci_report(
        &self,
        baseline: &[BenchmarkResult],
        thresholds: &RegressionThresholds,
        trace_file: Option<String>,
    ) -> CiReport {
        CiReport::generate(baseline, &self.results, thresholds, trace_file)
    }
}

impl Default for BenchmarkHarness {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate_benchmark_text(value: &str) -> String {
    value.chars().take(MAX_BENCHMARK_TEXT_CHARS).collect()
}

fn sanitize_measurements(measurements: &mut Vec<BenchmarkMeasurement>) {
    let mut seen = std::collections::HashSet::new();
    measurements.retain(|measurement| {
        measurement.validate().is_ok() && seen.insert((measurement.metric, measurement.unit))
    });
    measurements.truncate(MAX_MEASUREMENTS_PER_RESULT);
}

// ---------------------------------------------------------------------------
// Comparative Analysis
// ---------------------------------------------------------------------------

/// Compare two benchmark results for the same scenario.
pub fn compare_results(
    baseline: &BenchmarkResult,
    candidate: &BenchmarkResult,
) -> Vec<MetricComparison> {
    if baseline.scenario != candidate.scenario {
        return Vec::new();
    }
    let mut comparisons = Vec::new();

    for baseline_m in &baseline.measurements {
        if baseline_m.validate().is_err() {
            continue;
        }
        if let Some(candidate_m) = candidate.measurements.iter().find(|measurement| {
            measurement.metric == baseline_m.metric
                && measurement.unit == baseline_m.unit
                && measurement.validate().is_ok()
        }) {
            let delta = candidate_m.value - baseline_m.value;
            let percent_change = if baseline_m.value != 0.0 {
                (delta / baseline_m.value) * 100.0
            } else if candidate_m.value > 0.0 {
                100.0
            } else if candidate_m.value < 0.0 {
                -100.0
            } else {
                0.0
            };

            comparisons.push(MetricComparison {
                metric: baseline_m.metric,
                baseline: baseline_m.value,
                candidate: candidate_m.value,
                delta,
                percent_change,
                unit: baseline_m.unit,
                lower_is_better: baseline_m.metric.lower_is_better(),
            });
        }
    }

    comparisons
}

/// Load benchmark results from a JSON string.
pub fn load_results_from_json(json: &str) -> Result<Vec<BenchmarkResult>, serde_json::Error> {
    if json.len() > MAX_BENCHMARK_JSON_BYTES {
        return Err(benchmark_json_error(format!(
            "benchmark JSON cannot exceed {MAX_BENCHMARK_JSON_BYTES} bytes"
        )));
    }
    let results: Vec<BenchmarkResult> = serde_json::from_str(json)?;
    if results.len() > MAX_HARNESS_RESULTS {
        return Err(benchmark_json_error(format!(
            "benchmark JSON cannot contain more than {MAX_HARNESS_RESULTS} results"
        )));
    }
    for result in &results {
        if !result.validation_issues().is_empty() {
            return Err(benchmark_json_error(
                "benchmark result contains invalid data",
            ));
        }
    }
    Ok(results)
}

fn benchmark_json_error(message: impl Into<String>) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

/// Compare a result set against a baseline file and report regressions.
pub fn check_regressions(
    baseline: &[BenchmarkResult],
    candidate: &[BenchmarkResult],
    threshold_percent: f64,
) -> Vec<Regression> {
    let threshold_percent = if validate_threshold_percent(threshold_percent).is_ok() {
        threshold_percent
    } else {
        0.0
    };
    let mut regressions = Vec::new();

    for candidate_result in candidate {
        if let Some(baseline_result) = baseline
            .iter()
            .find(|b| b.scenario == candidate_result.scenario)
        {
            for comparison in compare_results(baseline_result, candidate_result) {
                let is_regression = if comparison.lower_is_better {
                    comparison.percent_change > threshold_percent
                } else {
                    comparison.percent_change < -threshold_percent
                };

                if is_regression {
                    regressions.push(Regression {
                        scenario: candidate_result.scenario,
                        metric: comparison.metric,
                        baseline: comparison.baseline,
                        candidate: comparison.candidate,
                        percent_change: comparison.percent_change,
                        unit: comparison.unit,
                    });
                }
            }
        }
    }

    regressions
}

/// A detected regression between baseline and candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Regression {
    /// The scenario where regression occurred.
    pub scenario: BenchmarkScenario,
    /// The metric that regressed.
    pub metric: BenchmarkMetric,
    /// Baseline value.
    pub baseline: f64,
    /// Candidate value.
    pub candidate: f64,
    /// Percentage change.
    pub percent_change: f64,
    /// Unit of measurement.
    pub unit: MetricUnit,
}

/// A comparison between baseline and candidate for a single metric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricComparison {
    /// The metric being compared.
    pub metric: BenchmarkMetric,
    /// Baseline measurement.
    pub baseline: f64,
    /// Candidate measurement.
    pub candidate: f64,
    /// Absolute difference (candidate - baseline).
    pub delta: f64,
    /// Percentage change.
    pub percent_change: f64,
    /// Unit of measurement.
    pub unit: MetricUnit,
    /// Whether a lower value is better for this metric.
    pub lower_is_better: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn single_measurement_result(
        scenario: BenchmarkScenario,
        value: f64,
        unit: MetricUnit,
    ) -> BenchmarkResult {
        BenchmarkResult {
            scenario,
            subject: "subject".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value,
                unit,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }
    }

    struct StopTrackingCollector {
        stopped: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl MetricCollector for StopTrackingCollector {
        fn start(&mut self) {}

        fn stop(&mut self) -> Vec<BenchmarkMeasurement> {
            self.stopped
                .store(true, std::sync::atomic::Ordering::Release);
            Vec::new()
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_scenario_descriptions() {
        assert!(!BenchmarkScenario::Messaging.description().is_empty());
        assert!(!BenchmarkScenario::Workspace.description().is_empty());
        assert!(!BenchmarkScenario::MediaControl.description().is_empty());
    }

    #[test]
    fn test_harness_run() {
        let mut harness = BenchmarkHarness::new();
        let result = harness.run(BenchmarkScenario::Messaging, "kael", |measurements| {
            measurements.push(BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 120.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::from_secs(1),
            });
        });

        assert_eq!(result.scenario, BenchmarkScenario::Messaging);
        assert_eq!(result.subject, "kael");
        assert_eq!(result.measurements.len(), 1);
        assert_eq!(result.measurements[0].value, 120.0);
    }

    #[test]
    fn test_harness_run_with_collectors() {
        let mut harness = BenchmarkHarness::new();
        let mut cold_start = ColdStartCollector::new();
        let mut memory = MemoryCollector::new();
        let mut collectors: [&mut dyn MetricCollector; 2] = [&mut cold_start, &mut memory];

        let result = harness.run_with_collectors(
            BenchmarkScenario::Messaging,
            "kael",
            &mut collectors,
            |_collectors| {},
        );

        assert_eq!(result.scenario, BenchmarkScenario::Messaging);
        assert!(!result.measurements.is_empty());
    }

    #[test]
    fn benchmark_harness_stops_collectors_when_runner_panics() {
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut collector = StopTrackingCollector {
            stopped: stopped.clone(),
        };
        let mut collectors: [&mut dyn MetricCollector; 1] = [&mut collector];
        let mut harness = BenchmarkHarness::new();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            harness.run_with_collectors(
                BenchmarkScenario::Messaging,
                "kael",
                &mut collectors,
                |_| panic!("benchmark runner failed"),
            );
        }));

        assert!(result.is_err());
        assert!(stopped.load(std::sync::atomic::Ordering::Acquire));
        assert!(harness.results().is_empty());
    }

    #[test]
    fn benchmark_comparisons_reject_mismatches_and_report_zero_baselines() {
        let baseline =
            single_measurement_result(BenchmarkScenario::Messaging, 0.0, MetricUnit::Milliseconds);
        let candidate =
            single_measurement_result(BenchmarkScenario::Messaging, 12.0, MetricUnit::Milliseconds);
        assert_eq!(
            compare_results(&baseline, &candidate)[0].percent_change,
            100.0
        );
        assert_eq!(
            check_regressions(std::slice::from_ref(&baseline), &[candidate], 10.0).len(),
            1
        );

        let wrong_unit =
            single_measurement_result(BenchmarkScenario::Messaging, 12.0, MetricUnit::Megabytes);
        assert!(compare_results(&baseline, &wrong_unit).is_empty());
        let wrong_scenario =
            single_measurement_result(BenchmarkScenario::Workspace, 12.0, MetricUnit::Milliseconds);
        assert!(compare_results(&baseline, &wrong_scenario).is_empty());
    }

    #[test]
    fn test_compare_results() {
        let baseline = BenchmarkResult {
            scenario: BenchmarkScenario::Messaging,
            subject: "baseline".to_string(),
            measurements: vec![
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::ColdStart,
                    value: 500.0,
                    unit: MetricUnit::Milliseconds,
                    elapsed: Duration::from_secs(1),
                },
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::IdleMemory,
                    value: 250.0,
                    unit: MetricUnit::Megabytes,
                    elapsed: Duration::from_secs(2),
                },
            ],
            started_at: Instant::now(),
            duration: Duration::from_secs(3),
            environment: BenchmarkEnvironment::current(),
        };

        let candidate = BenchmarkResult {
            scenario: BenchmarkScenario::Messaging,
            subject: "kael".to_string(),
            measurements: vec![
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::ColdStart,
                    value: 120.0,
                    unit: MetricUnit::Milliseconds,
                    elapsed: Duration::from_secs(1),
                },
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::IdleMemory,
                    value: 80.0,
                    unit: MetricUnit::Megabytes,
                    elapsed: Duration::from_secs(2),
                },
            ],
            started_at: Instant::now(),
            duration: Duration::from_secs(3),
            environment: BenchmarkEnvironment::current(),
        };

        let comparisons = compare_results(&baseline, &candidate);
        assert_eq!(comparisons.len(), 2);

        let cold_start = comparisons
            .iter()
            .find(|c| c.metric == BenchmarkMetric::ColdStart)
            .unwrap();
        assert_eq!(cold_start.delta, -380.0);
        assert_eq!(cold_start.percent_change, -76.0);
        assert!(cold_start.lower_is_better);
    }

    #[test]
    fn test_metric_unit_display() {
        assert_eq!(MetricUnit::Milliseconds.to_string(), "ms");
        assert_eq!(MetricUnit::Megabytes.to_string(), "MB");
        assert_eq!(MetricUnit::Percent.to_string(), "%");
    }

    #[test]
    fn test_cold_start_collector() {
        let mut collector = ColdStartCollector::new();
        std::thread::sleep(Duration::from_millis(10));
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, BenchmarkMetric::ColdStart);
        assert!(measurements[0].value >= 10.0);
    }

    #[test]
    fn test_memory_collector_returns_value() {
        let mut collector = MemoryCollector::new();
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, BenchmarkMetric::IdleMemory);
        assert!(measurements[0].value >= 0.0);
    }

    #[test]
    fn test_input_latency_collector() {
        let mut collector = InputLatencyCollector::new();
        collector.start();
        collector.record_input();
        std::thread::sleep(Duration::from_millis(5));
        collector.record_frame_presented();
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, BenchmarkMetric::InputLatency);
        assert!(measurements[0].value >= 5.0);
    }

    #[test]
    fn test_smoothness_collector() {
        let mut collector = SmoothnessCollector::new(BenchmarkMetric::ScrollSmoothness);
        collector.start();
        for _ in 0..10 {
            std::thread::sleep(Duration::from_millis(16));
            collector.record_frame();
        }
        let measurements = collector.stop();
        assert!(!measurements.is_empty());
    }

    #[test]
    fn benchmark_collectors_bound_samples_and_avoid_overflow() {
        assert!(SmoothnessCollector::try_new(BenchmarkMetric::ColdStart).is_err());
        let collector = SmoothnessCollector::new(BenchmarkMetric::ScrollSmoothness);
        assert_eq!(collector.min_frame_time_ms(), 0.0);

        let mut input = InputLatencyCollector::new();
        input.latencies = vec![Duration::from_millis(1); MAX_COLLECTOR_SAMPLES];
        input.record_input();
        input.record_frame_presented();
        assert_eq!(input.latencies.len(), MAX_COLLECTOR_SAMPLES);

        let mut cache = CacheHitRateCollector {
            hits: u64::MAX,
            misses: u64::MAX,
        };
        cache.record_hit();
        cache.record_miss();
        assert!(cache.hits < u64::MAX);
        assert!(cache.misses < u64::MAX);
        assert!((cache.hit_rate() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_long_session_collector() {
        let mut collector = LongSessionCollector::new(Duration::from_millis(50));
        collector.start();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(50));
            collector.sample();
        }
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 2);
        let cpu = measurements
            .iter()
            .find(|m| m.metric == BenchmarkMetric::LongSessionCpu)
            .unwrap();
        assert!(cpu.value >= 0.0);
    }

    #[test]
    fn test_check_regressions() {
        let baseline = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Messaging,
            subject: "baseline".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let candidate = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Messaging,
            subject: "candidate".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 150.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let regressions = check_regressions(&baseline, &candidate, 10.0);
        assert_eq!(regressions.len(), 1);
        assert_eq!(regressions[0].percent_change, 50.0);
    }

    #[test]
    fn test_all_scenarios_have_descriptions() {
        for scenario in BenchmarkScenario::all() {
            assert!(!scenario.description().is_empty());
            assert!(scenario.complexity_score() > 0);
            let spec = scenario.workload_spec();
            assert_eq!(spec.scenario, *scenario);
            assert!(!spec.required_metrics.is_empty());
            assert!(!spec.required_interactions.is_empty());
        }
    }

    #[test]
    fn test_frame_time_collector() {
        let mut collector = FrameTimeCollector::new();
        collector.start();
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(8));
            collector.record_frame();
        }
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 3);
        let p50 = measurements
            .iter()
            .find(|m| m.metric == BenchmarkMetric::FrameTimeP50)
            .unwrap();
        let p99 = measurements
            .iter()
            .find(|m| m.metric == BenchmarkMetric::FrameTimeP99)
            .unwrap();
        assert!(p50.value > 0.0);
        assert!(p99.value >= p50.value);
    }

    #[test]
    fn test_memory_growth_collector() {
        let mut collector = MemoryGrowthCollector::new();
        collector.start();
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, BenchmarkMetric::MemoryGrowth);
    }

    #[test]
    fn test_cache_hit_rate_collector() {
        let mut collector = CacheHitRateCollector::new();
        collector.start();
        for _ in 0..7 {
            collector.record_hit();
        }
        for _ in 0..3 {
            collector.record_miss();
        }
        let measurements = collector.stop();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0].metric, BenchmarkMetric::AssetCacheHitRate);
        assert!((measurements[0].value - 70.0).abs() < 0.01);
    }

    #[test]
    fn test_regression_thresholds() {
        let thresholds =
            RegressionThresholds::new(10.0).with_override(BenchmarkMetric::ColdStart, 5.0);
        assert_eq!(thresholds.threshold_for(BenchmarkMetric::ColdStart), 5.0);
        assert_eq!(thresholds.threshold_for(BenchmarkMetric::IdleMemory), 10.0);
    }

    #[test]
    fn invalid_regression_thresholds_fail_closed() {
        assert!(RegressionThresholds::new_checked(f64::NAN).is_err());
        assert!(RegressionThresholds::new_checked(-1.0).is_err());
        assert!(
            RegressionThresholds::default()
                .with_override_checked(BenchmarkMetric::ColdStart, f64::INFINITY)
                .is_err()
        );

        let thresholds = RegressionThresholds::new(f64::NAN);
        assert_eq!(thresholds.threshold_for(BenchmarkMetric::ColdStart), 0.0);
        let report = CiReport::generate(&[], &[], &thresholds, None);
        assert!(!report.passed);
    }

    #[test]
    fn test_ci_report_generation() {
        let baseline = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Ide,
            subject: "baseline".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let candidate = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Ide,
            subject: "candidate".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 105.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let thresholds = RegressionThresholds::new(10.0);
        let report = CiReport::generate(&baseline, &candidate, &thresholds, None);
        assert!(report.passed);
        assert!(report.regressions.is_empty());
        assert!(!report.summary().is_empty());
        assert!(report.to_json().is_ok());
    }

    #[test]
    fn test_baseline_comparison_report_classifies_wins() {
        let baseline = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "baseline".to_string(),
            measurements: vec![
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::IdleMemory,
                    value: 300.0,
                    unit: MetricUnit::Megabytes,
                    elapsed: Duration::default(),
                },
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::AssetCacheHitRate,
                    value: 80.0,
                    unit: MetricUnit::Percent,
                    elapsed: Duration::default(),
                },
            ],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let kael = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "kael".to_string(),
            measurements: vec![
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::IdleMemory,
                    value: 120.0,
                    unit: MetricUnit::Megabytes,
                    elapsed: Duration::default(),
                },
                BenchmarkMeasurement {
                    metric: BenchmarkMetric::AssetCacheHitRate,
                    value: 70.0,
                    unit: MetricUnit::Percent,
                    elapsed: Duration::default(),
                },
            ],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let report =
            BaselineComparisonReport::generate(&baseline, &kael, Some("trace.json".into()));

        assert_eq!(report.comparisons.len(), 2);
        assert_eq!(report.kael_wins().count(), 1);
        assert_eq!(report.baseline_wins().count(), 1);
        assert!(report.evidence_issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::MissingSample {
                scenario: BenchmarkScenario::Dashboard,
                ..
            }
        )));
        assert!(report.to_json().is_ok());
        let summary = report.summary();
        assert!(summary.contains("Baseline Comparison Report"));
        assert!(summary.contains("Kael wins: 1"));
        assert!(summary.contains("Baseline wins: 1"));
        assert!(summary.contains("Evidence issues"));
        assert!(summary.contains("trace.json"));
    }

    #[test]
    fn test_baseline_comparison_report_flags_unmatched_scenarios() {
        let baseline = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "baseline".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];
        let kael = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Chat,
            subject: "kael".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 80.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let report = BaselineComparisonReport::generate(&baseline, &kael, None);

        assert!(report.comparisons.is_empty());
        assert!(report.evidence_issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::MissingResult {
                scenario: BenchmarkScenario::Chat,
                runtime: BenchmarkSampleRuntime::Baseline,
            }
        )));
        assert!(report.evidence_issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::MissingResult {
                scenario: BenchmarkScenario::Dashboard,
                runtime: BenchmarkSampleRuntime::Kael,
            }
        )));
    }

    #[test]
    fn test_baseline_comparison_report_flags_duplicate_scenarios() {
        let baseline_result = BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "baseline".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        };
        let baseline = vec![baseline_result.clone(), baseline_result];
        let kael = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "kael".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 80.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];

        let report = BaselineComparisonReport::generate(&baseline, &kael, None);

        assert!(report.evidence_issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::DuplicateResult {
                scenario: BenchmarkScenario::Dashboard,
                runtime: BenchmarkSampleRuntime::Baseline,
                count: 2,
            }
        )));
    }

    #[test]
    fn test_baseline_comparison_report_flags_environment_mismatch() {
        let baseline = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "baseline".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment {
                os_name: "macos".to_string(),
                os_version: "27.0".to_string(),
                cpu: "Apple M4".to_string(),
                memory_gb: 32,
                gpu: "Apple GPU".to_string(),
            },
        }];
        let kael = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "kael".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 80.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment {
                os_name: "linux".to_string(),
                os_version: "6.12".to_string(),
                cpu: "AMD Ryzen".to_string(),
                memory_gb: 64,
                gpu: "Radeon".to_string(),
            },
        }];

        let report = BaselineComparisonReport::generate(&baseline, &kael, None);

        assert!(report.evidence_issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::EnvironmentMismatch {
                scenario: BenchmarkScenario::Dashboard,
                field,
                baseline,
                kael,
            } if field == "os_name" && baseline == "macos" && kael == "linux"
        )));
        assert!(report.evidence_issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::EnvironmentMismatch {
                scenario: BenchmarkScenario::Dashboard,
                field,
                baseline,
                kael,
            } if field == "memory_gb" && baseline == "32" && kael == "64"
        )));
    }

    #[test]
    fn test_benchmark_sample_pair_validates_comparable_interactions() {
        let interactions = BenchmarkScenario::Dashboard
            .workload_spec()
            .required_interactions;
        let baseline = BenchmarkSampleApp::builder(
            BenchmarkSampleRuntime::Baseline,
            BenchmarkScenario::Dashboard,
            "Baseline dashboard sample",
        )
        .source("samples/baseline/dashboard")
        .build_command("npm ci && npm run build")
        .run_command("npm run bench:dashboard")
        .interactions(interactions.clone())
        .build_checked()
        .unwrap();
        let kael = BenchmarkSampleApp::builder(
            BenchmarkSampleRuntime::Kael,
            BenchmarkScenario::Dashboard,
            "Kael dashboard sample",
        )
        .source("samples/kael/dashboard")
        .build_command("cargo build --release -p dashboard_sample")
        .run_command("cargo run --release -p dashboard_sample -- --bench")
        .interactions(interactions)
        .build_checked()
        .unwrap();

        let pair = BenchmarkSamplePair::new(baseline, kael);
        assert!(pair.validate().is_empty());
    }

    #[test]
    fn test_benchmark_sample_builder_rejects_incomplete_generated_contracts() {
        let issues = BenchmarkSampleApp::builder(
            BenchmarkSampleRuntime::Baseline,
            BenchmarkScenario::Dashboard,
            " Dashboard",
        )
        .source("samples/baseline/dashboard")
        .run_command("npm run bench:dashboard")
        .interaction("sort table")
        .build_checked()
        .unwrap_err();

        assert!(issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::InvalidSampleField { field, .. } if field == "name"
        )));
        assert!(issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::MissingSampleInteraction {
                interaction,
                ..
            } if interaction == "filter data"
        )));
    }

    #[test]
    fn test_baseline_comparison_report_accepts_sample_pairs() {
        let measurements = vec![
            BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::FirstInteractiveFrame,
                value: 120.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::IdleMemory,
                value: 220.0,
                unit: MetricUnit::Megabytes,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::FrameTimeP95,
                value: 16.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::LongSessionCpu,
                value: 12.0,
                unit: MetricUnit::Percent,
                elapsed: Duration::default(),
            },
            BenchmarkMeasurement {
                metric: BenchmarkMetric::WakeupsPerSecond,
                value: 25.0,
                unit: MetricUnit::WakeupsPerSec,
                elapsed: Duration::default(),
            },
        ];
        let baseline = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "baseline".to_string(),
            measurements: measurements.clone(),
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];
        let kael = vec![BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "kael".to_string(),
            measurements,
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        }];
        let interactions = BenchmarkScenario::Dashboard
            .workload_spec()
            .required_interactions;
        let pair = BenchmarkSamplePair::new(
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

        let report =
            BaselineComparisonReport::generate_with_sample_pairs(&baseline, &kael, &[pair], None);
        assert!(report.evidence_issues.is_empty());
        assert_eq!(report.sample_pairs.len(), 1);
    }

    #[test]
    fn test_workload_spec_validates_missing_metrics() {
        let spec = BenchmarkScenario::Dashboard.workload_spec();
        let result = BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "kael".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Milliseconds,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        };

        let issues = spec.validate_result(&result);
        assert!(issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::MissingMetric {
                metric: BenchmarkMetric::IdleMemory,
                ..
            }
        )));

        let invalid = BenchmarkResult {
            scenario: BenchmarkScenario::Dashboard,
            subject: "kael".to_string(),
            measurements: vec![BenchmarkMeasurement {
                metric: BenchmarkMetric::ColdStart,
                value: 100.0,
                unit: MetricUnit::Megabytes,
                elapsed: Duration::default(),
            }],
            started_at: Instant::now(),
            duration: Duration::default(),
            environment: BenchmarkEnvironment::current(),
        };
        let issues = spec.validate_result(&invalid);
        assert!(issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::InvalidResultField {
                field,
                reason,
                ..
            } if field == "measurement" && reason.contains("incompatible")
        )));
        assert!(issues.iter().any(|issue| matches!(
            issue,
            BenchmarkEvidenceIssue::MissingMetric {
                metric: BenchmarkMetric::ColdStart,
                ..
            }
        )));
    }

    #[test]
    fn test_metric_lower_is_better() {
        assert!(BenchmarkMetric::ColdStart.lower_is_better());
        assert!(BenchmarkMetric::IdleMemory.lower_is_better());
        assert!(!BenchmarkMetric::AssetCacheHitRate.lower_is_better());
    }

    #[test]
    fn test_load_results_from_json() {
        let json = r#"[{
            "scenario": "Messaging",
            "subject": "kael",
            "measurements": [{
                "metric": "ColdStart",
                "value": 120.0,
                "unit": "Milliseconds",
                "elapsed": {"secs": 1, "nanos": 0}
            }],
            "duration": {"secs": 2, "nanos": 0},
            "environment": {
                "os_name": "linux",
                "os_version": "",
                "cpu": "",
                "memory_gb": 0,
                "gpu": ""
            }
        }]"#;
        let results = load_results_from_json(json).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].scenario, BenchmarkScenario::Messaging);
    }

    #[test]
    fn benchmark_json_loader_rejects_oversized_or_invalid_results() {
        assert!(load_results_from_json(&" ".repeat(MAX_BENCHMARK_JSON_BYTES + 1)).is_err());

        let mut result =
            single_measurement_result(BenchmarkScenario::Messaging, 1.0, MetricUnit::Milliseconds);
        result.subject = "\ninvalid".to_string();
        assert!(load_results_from_json(&serde_json::to_string(&vec![result]).unwrap()).is_err());
    }
}
