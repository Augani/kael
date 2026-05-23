//! Metrics and trace-event collection.

use std::{
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    fs,
    io::Write,
    mem,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, atomic::{AtomicU64, Ordering}},
    time::Instant,
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);
static GLOBAL_TRACER: OnceLock<Mutex<Option<Tracer>>> = OnceLock::new();

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
                events: VecDeque::with_capacity(max_events.min(10_000)),
                max_events,
                process_id: std::process::id() as u64,
                started_at: Instant::now(),
            })),
        }
    }

    /// Returns whether tracing is enabled.
    pub fn is_enabled(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled
    }

    /// Enables tracing.
    pub fn enable(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled = true;
    }

    /// Disables tracing.
    pub fn disable(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled = false;
    }

    /// Clears all retained events.
    pub fn clear(&self) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.clear();
    }

    /// Records a trace event.
    pub fn record(&self, name: impl Into<String>, category: impl Into<String>, phase: TracePhase) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !inner.enabled {
            return;
        }

        let event = TraceEvent {
            name: name.into(),
            category: category.into(),
            phase,
            timestamp_us: inner.started_at.elapsed().as_micros() as u64,
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
        let result = f();
        self.record(name, category, TracePhase::End);
        result
    }

    /// Returns a snapshot of retained events.
    pub fn events(&self) -> Vec<TraceEvent> {
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.iter().cloned().collect()
    }

    /// Installs this tracer as the process-global tracer.
    pub fn install_global(&self) -> Option<Tracer> {
        let mut slot = global_tracer_slot().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let events: Vec<&TraceEvent> = inner.events.iter().collect();
        serde_json::to_string_pretty(&events).context("failed to export trace events")
    }

    /// Writes retained events to disk as Chrome Trace JSON.
    pub fn write_to_file(&self, path: impl Into<PathBuf>) -> Result<()> {
        let json = self.export_to_chrome_json()?;
        let path = path.into();
        let mut file = fs::File::create(&path)
            .with_context(|| format!("failed to create trace file: {}", path.display()))?;
        file.write_all(json.as_bytes())
            .with_context(|| format!("failed to write trace file: {}", path.display()))?;
        Ok(())
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
    inner: Arc<Mutex<MetricsSnapshot>>,
}

impl MetricsRegistry {
    /// Records a gauge value.
    pub fn record_gauge(&self, name: &str, value: f64) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.gauges.insert(name.to_string(), value);
    }

    /// Increments a counter by `delta`.
    pub fn record_counter(&self, name: &str, delta: i64) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *inner.counters.entry(name.to_string()).or_default() += delta;
    }

    /// Appends a histogram sample.
    pub fn record_histogram(&self, name: &str, value: f64) {
        let mut inner = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.histograms.entry(name.to_string()).or_default().push(value);
    }

    /// Returns a clone of the current metrics snapshot.
    pub fn snapshot(&self) -> MetricsSnapshot {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }
}

/// A transaction-style tracing helper.
pub struct Transaction {
    tracer: Option<Tracer>,
    name: String,
}

impl Transaction {
    /// Creates a transaction using the given tracer.
    pub fn new(name: impl Into<String>, tracer: Option<Tracer>) -> Self {
        let name = name.into();
        if let Some(tracer) = tracer.as_ref() {
            tracer.record(name.clone(), "transaction", TracePhase::Begin);
        }
        Self { tracer, name }
    }

    /// Starts a child span.
    pub fn start_span(&self, operation: &str) -> Span {
        Span::new(operation.to_string(), self.tracer.clone())
    }

    /// Finishes the transaction.
    pub fn finish(self) {
        if let Some(tracer) = self.tracer {
            tracer.record(self.name, "transaction", TracePhase::End);
        }
    }
}

/// A span created from a transaction.
pub struct Span {
    tracer: Option<Tracer>,
    operation: String,
}

impl Span {
    /// Creates a span using the given tracer.
    pub fn new(operation: impl Into<String>, tracer: Option<Tracer>) -> Self {
        let operation = operation.into();
        if let Some(tracer) = tracer.as_ref() {
            tracer.record(operation.clone(), "span", TracePhase::Begin);
        }
        Self { tracer, operation }
    }

    /// Finishes the span.
    pub fn finish(self) {
        if let Some(tracer) = self.tracer {
            tracer.record(self.operation, "span", TracePhase::End);
        }
    }
}

fn global_tracer_slot() -> &'static Mutex<Option<Tracer>> {
    GLOBAL_TRACER.get_or_init(|| Mutex::new(None))
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
    if inner.events.len() >= inner.max_events {
        inner.events.pop_front();
    }
    inner.events.push_back(event);
}

#[cfg(test)]
mod tests {
    use super::{MetricsRegistry, TraceEvent, TracePhase, Tracer, Transaction};

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
}