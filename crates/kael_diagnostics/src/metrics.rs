//! Metrics and trace-event collection.

use std::{
    cell::Cell,
    collections::{BTreeMap, VecDeque},
    mem,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(not(target_arch = "wasm32"))]
use std::{fs, io::Write, path::Path};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use web_time::Instant;

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);
static GLOBAL_TRACER: OnceLock<Mutex<Option<Tracer>>> = OnceLock::new();
const MAX_TRACE_EVENTS: usize = 100_000;
const MAX_METRICS: usize = 10_000;
const MAX_HISTOGRAM_SAMPLES: usize = 10_000;
const MAX_TOTAL_HISTOGRAM_SAMPLES: usize = 100_000;
const MAX_NAME_BYTES: usize = 512;

thread_local! {
    static TRACE_THREAD_ID: Cell<u64> = const { Cell::new(0) };
}

/// The phase of a trace event in the Chrome Trace Event format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TracePhase {
    /// The event begins.
    #[serde(rename = "B")]
    Begin,
    /// The event ends.
    #[serde(rename = "E")]
    End,
    /// The event happens instantaneously.
    #[serde(rename = "i")]
    Instant,
    /// The event records a complete duration.
    #[serde(rename = "X")]
    Complete,
    /// The event records a counter value.
    #[serde(rename = "C")]
    Counter,
    /// The event begins asynchronously.
    #[serde(rename = "b")]
    AsyncBegin,
    /// The event ends asynchronously.
    #[serde(rename = "e")]
    AsyncEnd,
}

/// The visibility scope of an instantaneous Chrome trace event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceScope {
    /// Visible across the trace.
    #[serde(rename = "g")]
    Global,
    /// Visible within the current process.
    #[serde(rename = "p")]
    Process,
    /// Visible within the current thread.
    #[serde(rename = "t")]
    Thread,
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
    /// Correlation identifier for asynchronous events.
    #[serde(rename = "id", skip_serializing_if = "Option::is_none")]
    pub async_id: Option<String>,
    /// Visibility scope for instantaneous events.
    #[serde(rename = "s", skip_serializing_if = "Option::is_none")]
    pub scope: Option<TraceScope>,
    /// Optional structured arguments.
    #[serde(rename = "args", skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TraceEvent {
    /// Serializes this event into a Chrome Trace JSON object.
    pub fn to_chrome_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string(self).context("failed to serialize trace event")
    }

    fn validate(&self) -> Result<()> {
        let valid = match self.phase {
            TracePhase::Begin | TracePhase::End => {
                self.duration_us.is_none() && self.async_id.is_none() && self.scope.is_none()
            }
            TracePhase::Instant => {
                self.duration_us.is_none() && self.async_id.is_none() && self.scope.is_some()
            }
            TracePhase::Complete => {
                self.duration_us.is_some() && self.async_id.is_none() && self.scope.is_none()
            }
            TracePhase::Counter => {
                self.duration_us.is_none()
                    && self.async_id.is_none()
                    && self.scope.is_none()
                    && self.args.as_ref().is_some_and(|args| !args.is_empty())
            }
            TracePhase::AsyncBegin | TracePhase::AsyncEnd => {
                self.duration_us.is_none()
                    && self
                        .async_id
                        .as_ref()
                        .is_some_and(|identifier| !identifier.is_empty())
                    && self.scope.is_none()
            }
        };
        anyhow::ensure!(
            valid,
            "trace event fields do not match its Chrome trace phase"
        );
        Ok(())
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
                process_id: current_process_id(),
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

    /// Records a begin, end, or thread-scoped instant trace event.
    ///
    /// Complete, counter, and asynchronous events require additional fields;
    /// use their dedicated recording methods instead.
    pub fn record(&self, name: impl Into<String>, category: impl Into<String>, phase: TracePhase) {
        let scope = match phase {
            TracePhase::Begin | TracePhase::End => None,
            TracePhase::Instant => Some(TraceScope::Thread),
            TracePhase::Complete
            | TracePhase::Counter
            | TracePhase::AsyncBegin
            | TracePhase::AsyncEnd => return,
        };
        let _ = self.record_event(name.into(), category.into(), phase, None, None, scope);
    }

    /// Records a complete trace event with its elapsed duration.
    pub fn record_complete(
        &self,
        name: impl Into<String>,
        category: impl Into<String>,
        duration: std::time::Duration,
    ) {
        let _ = self.record_event(
            name.into(),
            category.into(),
            TracePhase::Complete,
            Some(u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)),
            None,
            None,
        );
    }

    /// Records a numeric Chrome trace counter event.
    pub fn record_counter(&self, name: impl Into<String>, category: impl Into<String>, value: f64) {
        let Some(value) = serde_json::Number::from_f64(value) else {
            return;
        };
        let mut args = serde_json::Map::new();
        args.insert("value".to_string(), serde_json::Value::Number(value));
        let _ = self.record_event_with_args(
            name.into(),
            category.into(),
            TracePhase::Counter,
            None,
            None,
            None,
            Some(args),
            true,
        );
    }

    /// Records the beginning of an asynchronous trace event.
    pub fn record_async_begin(
        &self,
        name: impl Into<String>,
        category: impl Into<String>,
        identifier: impl Into<String>,
    ) {
        self.record_async(
            name.into(),
            category.into(),
            identifier.into(),
            TracePhase::AsyncBegin,
        );
    }

    /// Records the end of an asynchronous trace event.
    pub fn record_async_end(
        &self,
        name: impl Into<String>,
        category: impl Into<String>,
        identifier: impl Into<String>,
    ) {
        self.record_async(
            name.into(),
            category.into(),
            identifier.into(),
            TracePhase::AsyncEnd,
        );
    }

    fn record_async(&self, name: String, category: String, identifier: String, phase: TracePhase) {
        let identifier = bounded_string(identifier);
        if identifier.is_empty() {
            return;
        }
        let _ = self.record_event_with_args(
            name,
            category,
            phase,
            None,
            Some(identifier),
            None,
            None,
            true,
        );
    }

    fn record_event(
        &self,
        name: String,
        category: String,
        phase: TracePhase,
        duration_us: Option<u64>,
        async_id: Option<String>,
        scope: Option<TraceScope>,
    ) -> bool {
        self.record_event_with_args(
            name,
            category,
            phase,
            duration_us,
            async_id,
            scope,
            None,
            true,
        )
    }

    fn record_event_with_args(
        &self,
        name: String,
        category: String,
        phase: TracePhase,
        duration_us: Option<u64>,
        async_id: Option<String>,
        scope: Option<TraceScope>,
        args: Option<serde_json::Map<String, serde_json::Value>>,
        require_enabled: bool,
    ) -> bool {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if require_enabled && !inner.enabled {
            return false;
        }

        let event = TraceEvent {
            name: bounded_string(name),
            category: bounded_string(category),
            phase,
            timestamp_us: u64::try_from(inner.started_at.elapsed().as_micros()).unwrap_or(u64::MAX),
            process_id: inner.process_id,
            thread_id: current_thread_id(),
            duration_us,
            async_id,
            scope,
            args,
        };

        push_event(&mut inner, event)
    }

    fn record_scope_end(&self, name: String, category: String) {
        let _ = self.record_event_with_args(
            name,
            category,
            TracePhase::End,
            None,
            None,
            None,
            None,
            false,
        );
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

        let name = bounded_string(name.into());
        let category = bounded_string(category.into());
        let active = self.record_event(
            name.clone(),
            category.clone(),
            TracePhase::Begin,
            None,
            None,
            None,
        );
        let _end = TraceEndGuard {
            tracer: self.clone(),
            name,
            category,
            active,
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
    ///
    /// Browser targets have no native paths and return a typed error; call
    /// [`Self::export_to_chrome_json`] and pass the result to Kael's browser
    /// file-export API instead.
    pub fn write_to_file(&self, path: impl Into<PathBuf>) -> Result<()> {
        let json = self.export_to_chrome_json()?;
        let path = path.into();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (json, path);
            return Err(anyhow::anyhow!(
                "browser traces do not have native file paths; use export_to_chrome_json and Kael's browser file export API"
            ));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create trace directory: {}", parent.display())
            })?;
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
}

struct TraceEndGuard {
    tracer: Tracer,
    name: String,
    category: String,
    active: bool,
}

impl Drop for TraceEndGuard {
    fn drop(&mut self) {
        if self.active {
            self.tracer
                .record_scope_end(self.name.clone(), self.category.clone());
        }
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
    histogram_samples: usize,
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
        let histogram_exists = inner.histograms.contains_key(name);
        if !histogram_exists && metric_count(&inner) >= MAX_METRICS {
            return;
        }
        let total_is_full = inner.histogram_samples >= MAX_TOTAL_HISTOGRAM_SAMPLES;
        if total_is_full && !histogram_exists {
            return;
        }
        let mut sample_count_grew = false;
        {
            let samples = inner.histograms.entry(name.to_string()).or_default();
            if samples.len() >= MAX_HISTOGRAM_SAMPLES {
                samples.pop_front();
            } else if total_is_full {
                if samples.pop_front().is_none() {
                    return;
                }
            } else {
                sample_count_grew = true;
            }
            samples.push_back(value);
        }
        if sample_count_grew {
            inner.histogram_samples += 1;
        }
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
        let name = bounded_string(name.into());
        let tracer = tracer.filter(|tracer| {
            tracer.record_event(
                name.clone(),
                "transaction".to_string(),
                TracePhase::Begin,
                None,
                None,
                None,
            )
        });
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
                tracer.record_scope_end(self.name.clone(), "transaction".to_string());
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
        let operation = bounded_string(operation.into());
        let tracer = tracer.filter(|tracer| {
            tracer.record_event(
                operation.clone(),
                "span".to_string(),
                TracePhase::Begin,
                None,
                None,
                None,
            )
        });
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
                tracer.record_scope_end(self.operation.clone(), "span".to_string());
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

#[cfg(all(not(unix), not(target_arch = "wasm32")))]
fn sync_directory(_directory: &Path) -> Result<()> {
    Ok(())
}

fn current_thread_id() -> u64 {
    TRACE_THREAD_ID.with(|thread_id| {
        let current = thread_id.get();
        if current != 0 {
            current
        } else {
            let next = allocate_thread_id(&NEXT_THREAD_ID);
            thread_id.set(next);
            next
        }
    })
}

#[cfg(target_arch = "wasm32")]
const fn current_process_id() -> u64 {
    // Chrome Trace accepts any stable process-group identifier. Browser wasm
    // has no OS process identity, and the standard-library accessor panics.
    0
}

#[cfg(not(target_arch = "wasm32"))]
fn current_process_id() -> u64 {
    u64::from(std::process::id())
}

fn allocate_thread_id(counter: &AtomicU64) -> u64 {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            Some(current.saturating_add(1))
        })
        .unwrap_or_else(|current| current)
}

fn push_event(inner: &mut TracerInner, event: TraceEvent) -> bool {
    if inner.max_events == 0 {
        return false;
    }
    if inner.events.len() >= inner.max_events {
        inner.events.pop_front();
    }
    inner.events.push_back(event);
    true
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
    use std::sync::atomic::AtomicU64;

    use super::{
        MAX_HISTOGRAM_SAMPLES, MAX_TOTAL_HISTOGRAM_SAMPLES, MetricsRegistry, TraceEvent,
        TracePhase, Tracer, Transaction, allocate_thread_id,
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
    fn tracer_emits_valid_chrome_phase_fields() {
        let tracer = Tracer::new(8);
        tracer.enable();
        tracer.record("instant", "test", TracePhase::Instant);
        tracer.record_complete("complete", "test", std::time::Duration::from_micros(7));
        tracer.record_counter("counter", "test", 3.0);
        tracer.record_async_begin("async", "test", "job-1");
        tracer.record_async_end("async", "test", "job-1");

        let events: Vec<serde_json::Value> =
            serde_json::from_str(&tracer.export_to_chrome_json().unwrap()).unwrap();
        assert_eq!(events[0]["ph"], "i");
        assert_eq!(events[0]["s"], "t");
        assert_eq!(events[1]["ph"], "X");
        assert_eq!(events[1]["dur"], 7);
        assert_eq!(events[2]["ph"], "C");
        assert_eq!(events[2]["args"]["value"], 3.0);
        assert_eq!(events[3]["ph"], "b");
        assert_eq!(events[3]["id"], "job-1");
        assert_eq!(events[4]["ph"], "e");
        assert_eq!(events[4]["id"], "job-1");
    }

    #[test]
    fn invalid_public_trace_events_are_rejected() {
        let event = TraceEvent {
            name: "counter".to_string(),
            category: "test".to_string(),
            phase: TracePhase::Counter,
            timestamp_us: 0,
            process_id: 1,
            thread_id: 1,
            duration_us: None,
            async_id: None,
            scope: None,
            args: None,
        };
        assert!(event.to_chrome_json().is_err());
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
    fn scoped_events_close_when_tracing_is_disabled_mid_scope() {
        let tracer = Tracer::new(8);
        tracer.enable();
        let transaction = Transaction::new("work", Some(tracer.clone()));
        tracer.disable();
        transaction.finish();
        assert_eq!(tracer.events().len(), 2);
        assert_eq!(tracer.events()[1].phase, TracePhase::End);

        tracer.clear();
        tracer.enable();
        let tracer_for_work = tracer.clone();
        tracer.record_duration("work", "test", move || tracer_for_work.disable());
        assert_eq!(tracer.events().len(), 2);
        assert_eq!(tracer.events()[1].phase, TracePhase::End);
    }

    #[test]
    fn zero_capacity_tracer_retains_nothing() {
        let tracer = Tracer::new(0);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);
        assert!(tracer.events().is_empty());
    }

    #[test]
    fn thread_identifiers_never_wrap_to_zero() {
        let counter = AtomicU64::new(u64::MAX);
        assert_eq!(allocate_thread_id(&counter), u64::MAX);
        assert_eq!(allocate_thread_id(&counter), u64::MAX);
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
    fn histograms_share_a_global_sample_budget() {
        let metrics = MetricsRegistry::default();
        let series = MAX_TOTAL_HISTOGRAM_SAMPLES / MAX_HISTOGRAM_SAMPLES;
        for series_index in 0..series {
            for value in 0..MAX_HISTOGRAM_SAMPLES {
                metrics.record_histogram(&format!("series-{series_index}"), value as f64);
            }
        }
        metrics.record_histogram("over-budget", 1.0);

        let snapshot = metrics.snapshot();
        let retained = snapshot.histograms.values().map(Vec::len).sum::<usize>();
        assert_eq!(retained, MAX_TOTAL_HISTOGRAM_SAMPLES);
        assert!(!snapshot.histograms.contains_key("over-budget"));
    }

    #[cfg(not(target_arch = "wasm32"))]
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
