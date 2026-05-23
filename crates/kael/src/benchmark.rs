//! Product-level benchmark workloads and harness for GPUI.
//!
//! This module defines standard benchmark scenarios that resemble real
//! products: messaging UI, workspace/editor UI, and media-control dashboard
//! UI. These workloads are used to measure startup, memory, responsiveness,
//! and energy use against Electron baselines.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::tracer::Tracer;

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
            {
                let mut buf = vec![0u8; size];
                if libc::sysctlbyname(
                    c"kern.osproductversion".as_ptr(),
                    buf.as_mut_ptr() as *mut _,
                    &mut size,
                    std::ptr::null_mut(),
                    0,
                ) == 0
                {
                    return String::from_utf8_lossy(&buf[..buf.len().saturating_sub(1)])
                        .to_string();
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
        std::env::var("PROCESSOR_IDENTIFIER")
            .or_else(|_| std::env::var("CPU"))
            .unwrap_or_default()
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
            self.latencies.push(input_time.elapsed());
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
        assert!(
            metric == BenchmarkMetric::ResizeSmoothness
                || metric == BenchmarkMetric::ScrollSmoothness,
            "SmoothnessCollector only supports resize or scroll metrics"
        );
        Self {
            frames: Vec::new(),
            last_frame: None,
            metric,
        }
    }

    /// Record a frame timestamp.
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            self.frames.push(now.duration_since(last));
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
        }
    }

    /// Sample current CPU usage. Call periodically.
    pub fn sample(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.start);
        let cpu_time = Self::process_cpu_time();
        let delta_cpu = cpu_time.saturating_sub(self.last_cpu_time);
        self.last_cpu_time = cpu_time;

        let cpu_percent = if self.sampling_interval.as_secs_f64() > 0.0 {
            (delta_cpu.as_secs_f64() / self.sampling_interval.as_secs_f64()) * 100.0
        } else {
            0.0
        };

        self.samples.push(CpuSample {
            elapsed,
            cpu_percent: cpu_percent.min(100.0 * num_cpus::get() as f64),
        });
    }

    fn process_cpu_time() -> Duration {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
            if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut rusage) } == 0 {
                let utime = Duration::from_secs(rusage.ru_utime.tv_sec as u64)
                    + Duration::from_micros(rusage.ru_utime.tv_usec as u64);
                let stime = Duration::from_secs(rusage.ru_stime.tv_sec as u64)
                    + Duration::from_micros(rusage.ru_stime.tv_usec as u64);
                return utime + stime;
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
            self.frame_times.push(now.duration_since(last));
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
        self.hits += 1;
    }

    /// Record a cache miss.
    pub fn record_miss(&mut self) {
        self.misses += 1;
    }

    /// Compute hit rate as a percentage (0-100).
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        (self.hits as f64 / total as f64) * 100.0
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

    /// Add a per-metric threshold override.
    pub fn with_override(mut self, metric: BenchmarkMetric, percent: f64) -> Self {
        self.overrides.insert(metric, percent);
        self
    }

    /// Get the threshold for a specific metric.
    pub fn threshold_for(&self, metric: BenchmarkMetric) -> f64 {
        self.overrides
            .get(&metric)
            .copied()
            .unwrap_or(self.default_percent)
    }
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
        Self {
            results: candidate.to_vec(),
            regressions: regressions.clone(),
            passed: regressions.is_empty(),
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
        let subject = subject.into();
        let started_at = Instant::now();
        let mut measurements = Vec::new();

        let tracer = self.tracer.clone();
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

        let duration = started_at.elapsed();

        let result = BenchmarkResult {
            scenario,
            subject,
            measurements,
            started_at,
            duration,
            environment: BenchmarkEnvironment::current(),
        };

        self.results.push(result.clone());
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
        let subject = subject.into();
        let started_at = Instant::now();

        for collector in collectors.iter_mut() {
            collector.start();
        }

        let tracer = self.tracer.clone();
        if let Some(tracer) = tracer {
            tracer.record_duration("runner", "benchmark", || {
                runner(collectors);
            });
        } else {
            runner(collectors);
        }

        let mut measurements = Vec::new();
        for collector in collectors.iter_mut() {
            measurements.extend(collector.stop());
        }

        let duration = started_at.elapsed();

        let result = BenchmarkResult {
            scenario,
            subject,
            measurements,
            started_at,
            duration,
            environment: BenchmarkEnvironment::current(),
        };

        self.results.push(result.clone());
        result
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

// ---------------------------------------------------------------------------
// Comparative Analysis
// ---------------------------------------------------------------------------

/// Compare two benchmark results for the same scenario.
pub fn compare_results(
    baseline: &BenchmarkResult,
    candidate: &BenchmarkResult,
) -> Vec<MetricComparison> {
    let mut comparisons = Vec::new();

    for baseline_m in &baseline.measurements {
        if let Some(candidate_m) = candidate
            .measurements
            .iter()
            .find(|m| m.metric == baseline_m.metric)
        {
            let delta = candidate_m.value - baseline_m.value;
            let percent_change = if baseline_m.value != 0.0 {
                (delta / baseline_m.value) * 100.0
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
    serde_json::from_str(json)
}

/// Compare a result set against a baseline file and report regressions.
pub fn check_regressions(
    baseline: &[BenchmarkResult],
    candidate: &[BenchmarkResult],
    threshold_percent: f64,
) -> Vec<Regression> {
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
    fn test_compare_results() {
        let baseline = BenchmarkResult {
            scenario: BenchmarkScenario::Messaging,
            subject: "electron".to_string(),
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
}
