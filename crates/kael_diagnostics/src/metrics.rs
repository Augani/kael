//! Metrics and trace-event collection.

use std::{
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    mem,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);
static GLOBAL_TRACER: OnceLock<Mutex<Option<Tracer>>> = OnceLock::new();
const MAX_TRACE_EVENTS: usize = 100_000;
const MAX_METRICS: usize = 10_000;
const MAX_HISTOGRAM_SAMPLES: usize = 10_000;
const MAX_NAME_BYTES: usize = 512;

thread_local! {
    static TRACE_THREAD_ID: Cell<u64> = const { Cell::new(0) };
}

/// The phase of a trace event in the Chrome Trace Event format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TracePhase {
    /// The event begins.
    Begin,
    /// The event ends.
    End,
    /// The event happens instantaneously.
    Instant,
    /// The event records a counter value.
    Counter,
    /// The event begins asynchronously.
    AsyncBegin,
    /// The event ends asynchronously.
    AsyncEnd,
}

/// A single trace event compatible with the Chrome Trace Event format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    /// The event name.
    pub name: String,
    /// Event category for grouping.
    #[serde(rename = "cat")]
    pub category: String,
    /// The event phase.
    #[serde(rename = "ph")]
    pub phase: TracePhase,
    /// Timestamp in microseconds.
    #[serde(rename = "ts")]
    pub timestamp_us: u64,
    /// Process identifier.
    #[serde(rename = "pid")]
    pub process_id: u64,
    /// Thread identifier.
    #[serde(rename = "tid")]
    pub thread_id: u64,
    /// Optional duration for complete events.
    #[serde(rename = "dur", skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,
    /// Optional structured arguments.
    #[serde(rename = "args", skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TraceEvent {
    /// Serializes this event into a Chrome Trace JSON object.
    pub fn to_chrome_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize trace event")
    }
}

/// A tracer that retains trace events in an in-memory ring buffer.
#[derive(Debug, Clone)]
pub struct Tracer {
    inner: Arc<Mutex<TracerInner>>,
}

#[derive(Debug)]
struct TracerInner {
    enabled: bool,
    events: VecDeque<TraceEvent>,
    max_events: usize,
    process_id: u64,
    started_at: Instant,
}

impl Tracer {
    /// Creates a tracer with the given retention capacity.
    pub fn new(max_events: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TracerInner {
                enabled: false,
                events: VecDeque::new(),
                max_events: max_events.min(MAX_TRACE_EVENTS),
                process_id: std::process::id() as u64,
                started_at: Instant::now(),
            })),
        }
    }

    /// Returns whether tracing is enabled.
    pub fn is_enabled(&self) -> bool {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled
    }

    /// Enables tracing.
    pub fn enable(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled = true;
    }

    /// Disables tracing.
    pub fn disable(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled = false;
    }

    /// Clears all retained events.
    pub fn clear(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.clear();
    }

    /// Records a trace event.
    pub fn record(&self, name: impl Into<String>, category: impl Into<String>, phase: TracePhase) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !inner.enabled {
            return;
        }

        let event = TraceEvent {
            name: bounded_string(name.into()),
            category: bounded_string(category.into()),
            phase,
            timestamp_us: u64::try_from(inner.started_at.elapsed().as_micros()).unwrap_or(u64::MAX),
            process_id: inner.process_id,
            thread_id: current_thread_id(),
            duration_us: None,
            args: None,
        };

        push_event(&mut inner, event);
    }

    /// Records begin and end trace events around the provided closure.
    pub fn record_duration<R>(
        &self,
        name: impl Into<String>,
        category: impl Into<String>,
        f: impl FnOnce() -> R,
    ) -> R {
        if !self.is_enabled() {
            return f();
        }

        let name = name.into();
        let category = category.into();
        self.record(name.clone(), category.clone(), TracePhase::Begin);
        let _end = TraceEndGuard {
            tracer: self.clone(),
            name,
            category,
        };
        f()
    }

    /// Returns a snapshot of retained events.
    pub fn events(&self) -> Vec<TraceEvent> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.iter().cloned().collect()
    }

    /// Installs this tracer as the process-global tracer.
    pub fn install_global(&self) -> Option<Tracer> {
        let mut slot = global_tracer_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mem::replace(&mut *slot, Some(self.clone()))
    }

    /// Returns the current process-global tracer.
    pub fn global() -> Option<Tracer> {
        global_tracer_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Clears the current process-global tracer.
    pub fn clear_global() -> Option<Tracer> {
        global_tracer_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    /// Exports retained events as Chrome Trace JSON.
    pub fn export_to_chrome_json(&self) -> Result<String> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let events: Vec<&TraceEvent> = inner.events.iter().collect();
        serde_json::to_string_pretty(&events).context("failed to export trace events")
    }

    /// Writes retained events to disk as Chrome Trace JSON.
    pub fn write_to_file(&self, path: impl Into<PathBuf>) -> Result<()> {
        let json = self.export_to_chrome_json()?;
        let path = path.into();
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create trace directory: {}", parent.display()))?;
        let mut file = tempfile::Builder::new()
            .prefix(".kael-trace-")
            .tempfile_in(parent)
            .with_context(|| {
                format!(
                    "failed to create temporary trace file in {}",
                    parent.display()
                )
            })?;
        file.as_file_mut()
            .write_all(json.as_bytes())
            .with_context(|| format!("failed to write trace file for {}", path.display()))?;
        file.as_file()
            .sync_all()
            .with_context(|| format!("failed to sync trace file for {}", path.display()))?;
        file.persist(&path).map_err(|error| {
            anyhow::Error::new(error.error)
                .context(format!("failed to finalize trace file: {}", path.display()))
        })?;
        sync_directory(parent)?;
        Ok(())
    }
}

struct TraceEndGuard {
    tracer: Tracer,
    name: String,
    category: String,
}

impl Drop for TraceEndGuard {
    fn drop(&mut self) {
        self.tracer
            .record(self.name.clone(), self.category.clone(), TracePhase::End);
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new(10_000)
    }
}

/// A snapshot of the metrics registry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// The latest value for each gauge metric.
    pub gauges: BTreeMap<String, f64>,
    /// The accumulated value for each counter metric.
    pub counters: BTreeMap<String, i64>,
    /// The recorded samples for each histogram metric.
    pub histograms: BTreeMap<String, Vec<f64>>,
}

/// An in-memory metrics registry.
#[derive(Debug, Clone, Default)]
pub struct MetricsRegistry {
    inner: Arc<Mutex<MetricsState>>,
}

#[derive(Debug, Default)]
struct MetricsState {
    gauges: BTreeMap<String, f64>,
    counters: BTreeMap<String, i64>,
    histograms: BTreeMap<String, VecDeque<f64>>,
}

impl MetricsRegistry {
    /// Records a gauge value.
    pub fn record_gauge(&self, name: &str, value: f64) {
        if !valid_metric(name, value) {
            return;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.gauges.contains_key(name) || metric_count(&inner) < MAX_METRICS {
            inner.gauges.insert(name.to_string(), value);
        }
    }

    /// Increments a counter by `delta`.
    pub fn record_counter(&self, name: &str, delta: i64) {
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.counters.contains_key(name) || metric_count(&inner) < MAX_METRICS {
            let counter = inner.counters.entry(name.to_string()).or_default();
            *counter = counter.saturating_add(delta);
        }
    }

    /// Appends a histogram sample.
    pub fn record_histogram(&self, name: &str, value: f64) {
        if !valid_metric(name, value) {
            return;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !inner.histograms.contains_key(name) && metric_count(&inner) >= MAX_METRICS {
            return;
        }
        let samples = inner.histograms.entry(name.to_string()).or_default();
        if samples.len() >= MAX_HISTOGRAM_SAMPLES {
            samples.pop_front();
        }
        samples.push_back(value);
    }

    /// Returns a clone of the current metrics snapshot.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        MetricsSnapshot {
            gauges: inner.gauges.clone(),
            counters: inner.counters.clone(),
            histograms: inner
                .histograms
                .iter()
                .map(|(name, samples)| (name.clone(), samples.iter().copied().collect()))
                .collect(),
        }
    }
}

/// A transaction-style tracing helper.
pub struct Transaction {
    tracer: Option<Tracer>,
    name: String,
    finished: bool,
}

impl Transaction {
    /// Creates a transaction using the given tracer.
    pub fn new(name: impl Into<String>, tracer: Option<Tracer>) -> Self {
        let name = name.into();
        if let Some(tracer) = tracer.as_ref() {
            tracer.record(name.clone(), "transaction", TracePhase::Begin);
        }
        Self {
            tracer,
            name,
            finished: false,
        }
    }

    /// Starts a child span.
    pub fn start_span(&self, operation: &str) -> Span {
        Span::new(operation.to_string(), self.tracer.clone())
    }

    /// Finishes the transaction. Dropping it also finishes it automatically.
    pub fn finish(mut self) {
        self.finish_once();
    }

    fn finish_once(&mut self) {
        if !self.finished {
            if let Some(tracer) = self.tracer.as_ref() {
                tracer.record(self.name.clone(), "transaction", TracePhase::End);
            }
            self.finished = true;
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        self.finish_once();
    }
}

/// A span created from a transaction.
pub struct Span {
    tracer: Option<Tracer>,
    operation: String,
    finished: bool,
}

impl Span {
    /// Creates a span using the given tracer.
    pub fn new(operation: impl Into<String>, tracer: Option<Tracer>) -> Self {
        let operation = operation.into();
        if let Some(tracer) = tracer.as_ref() {
            tracer.record(operation.clone(), "span", TracePhase::Begin);
        }
        Self {
            tracer,
            operation,
            finished: false,
        }
    }

    /// Finishes the span. Dropping it also finishes it automatically.
    pub fn finish(mut self) {
        self.finish_once();
    }

    fn finish_once(&mut self) {
        if !self.finished {
            if let Some(tracer) = self.tracer.as_ref() {
                tracer.record(self.operation.clone(), "span", TracePhase::End);
            }
            self.finished = true;
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        self.finish_once();
    }
}

fn global_tracer_slot() -> &'static Mutex<Option<Tracer>> {
    GLOBAL_TRACER.get_or_init(|| Mutex::new(None))
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<()> {
    fs::File::open(directory)
        .with_context(|| format!("failed to open trace directory: {}", directory.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync trace directory: {}", directory.display()))
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<()> {
    Ok(())
}

fn current_thread_id() -> u64 {
    TRACE_THREAD_ID.with(|thread_id| {
        let current = thread_id.get();
        if current != 0 {
            current
        } else {
            let next = NEXT_THREAD_ID.fetch_add(1, Ordering::Relaxed);
            thread_id.set(next);
            next
        }
    })
}

fn push_event(inner: &mut TracerInner, event: TraceEvent) {
    if inner.max_events == 0 {
        return;
    }
    if inner.events.len() >= inner.max_events {
        inner.events.pop_front();
    }
    inner.events.push_back(event);
}

fn bounded_string(mut value: String) -> String {
    if value.len() <= MAX_NAME_BYTES {
        return value;
    }
    let mut boundary = MAX_NAME_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

fn valid_metric(name: &str, value: f64) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME_BYTES && value.is_finite()
}

fn metric_count(snapshot: &MetricsState) -> usize {
    snapshot
        .gauges
        .len()
        .saturating_add(snapshot.counters.len())
        .saturating_add(snapshot.histograms.len())
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_HISTOGRAM_SAMPLES, MetricsRegistry, TraceEvent, TracePhase, Tracer, Transaction,
    };

    #[test]
    fn tracer_records_events_when_enabled() {
        let tracer = Tracer::new(8);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);

        let events = tracer.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "event");
    }

    #[test]
    fn tracer_exports_valid_trace_json() {
        let tracer = Tracer::new(8);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);

        let json = tracer.export_to_chrome_json().unwrap();
        let events: Vec<TraceEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn metrics_registry_tracks_latest_and_accumulated_values() {
        let metrics = MetricsRegistry::default();
        metrics.record_gauge("memory", 42.0);
        metrics.record_counter("requests", 1);
        metrics.record_counter("requests", 2);
        metrics.record_histogram("latency", 12.5);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.gauges["memory"], 42.0);
        assert_eq!(snapshot.counters["requests"], 3);
        assert_eq!(snapshot.histograms["latency"], vec![12.5]);
    }

    #[test]
    fn transactions_emit_begin_and_end_events() {
        let tracer = Tracer::new(8);
        tracer.enable();
        let transaction = Transaction::new("load", Some(tracer.clone()));
        let span = transaction.start_span("sql");
        span.finish();
        transaction.finish();

        let events = tracer.events();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].phase, TracePhase::Begin);
        assert_eq!(events[3].phase, TracePhase::End);
    }

    #[test]
    fn dropped_transactions_and_spans_emit_end_events_once() {
        let tracer = Tracer::new(8);
        tracer.enable();
        {
            let transaction = Transaction::new("load", Some(tracer.clone()));
            let _span = transaction.start_span("sql");
        }

        let events = tracer.events();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].phase, TracePhase::Begin);
        assert_eq!(events[1].phase, TracePhase::Begin);
        assert_eq!(events[2].phase, TracePhase::End);
        assert_eq!(events[3].phase, TracePhase::End);
    }

    #[test]
    fn record_duration_emits_end_when_the_closure_panics() {
        let tracer = Tracer::new(8);
        tracer.enable();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tracer.record_duration("work", "test", || panic!("stop"));
        }));

        assert!(result.is_err());
        let events = tracer.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].phase, TracePhase::Begin);
        assert_eq!(events[1].phase, TracePhase::End);
    }

    #[test]
    fn zero_capacity_tracer_retains_nothing() {
        let tracer = Tracer::new(0);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);
        assert!(tracer.events().is_empty());
    }

    #[test]
    fn metrics_reject_non_finite_values_and_saturate_counters() {
        let metrics = MetricsRegistry::default();
        metrics.record_gauge("bad", f64::NAN);
        metrics.record_histogram("bad", f64::INFINITY);
        metrics.record_counter("requests", i64::MAX);
        metrics.record_counter("requests", 1);
        let snapshot = metrics.snapshot();
        assert!(!snapshot.gauges.contains_key("bad"));
        assert!(!snapshot.histograms.contains_key("bad"));
        assert_eq!(snapshot.counters["requests"], i64::MAX);
    }

    #[test]
    fn histograms_retain_the_newest_bounded_samples() {
        let metrics = MetricsRegistry::default();
        for value in 0..=MAX_HISTOGRAM_SAMPLES {
            metrics.record_histogram("latency", value as f64);
        }

        let snapshot = metrics.snapshot();
        let samples = &snapshot.histograms["latency"];
        assert_eq!(samples.len(), MAX_HISTOGRAM_SAMPLES);
        assert_eq!(samples[0], 1.0);
        assert_eq!(
            samples[MAX_HISTOGRAM_SAMPLES - 1],
            MAX_HISTOGRAM_SAMPLES as f64
        );
    }

    #[test]
    fn trace_export_replaces_existing_file_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("trace.json");
        std::fs::write(&path, "old").unwrap();
        let tracer = Tracer::new(8);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);

        tracer.write_to_file(&path).unwrap();

        let events: Vec<TraceEvent> =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(events.len(), 1);
    }
}
