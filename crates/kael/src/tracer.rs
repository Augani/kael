use std::{
    cell::Cell,
    collections::VecDeque,
    fs,
    io::Write,
    mem,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};

static NEXT_THREAD_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_EXPORT_ID: AtomicU64 = AtomicU64::new(1);
static GLOBAL_TRACER: OnceLock<Mutex<Option<Tracer>>> = OnceLock::new();
const MAX_TRACE_EVENTS: usize = 100_000;
const MAX_TRACE_NAME_BYTES: usize = 512;

thread_local! {
    static TRACE_THREAD_ID: Cell<u64> = const { Cell::new(0) };
}

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// The phase of a trace event in the Chrome Trace Event format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TracePhase {
    /// The event begins (duration event start).
    Begin,
    /// The event ends (duration event end).
    End,
    /// An instantaneous event.
    Instant,
    /// A counter event.
    Counter,
    /// An async event start.
    AsyncBegin,
    /// An async event end.
    AsyncEnd,
}

/// A single trace event compatible with the Chrome Trace Event format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceEvent {
    /// The name of the event, displayed in the trace viewer.
    pub name: String,
    /// Event category for grouping.
    #[serde(rename = "cat")]
    pub category: String,
    /// The phase of the event.
    #[serde(rename = "ph")]
    pub phase: TracePhase,
    /// Timestamp in microseconds.
    #[serde(rename = "ts")]
    pub timestamp_us: u64,
    /// Process ID.
    #[serde(rename = "pid")]
    pub process_id: u64,
    /// Thread ID.
    #[serde(rename = "tid")]
    pub thread_id: u64,
    /// Optional duration in microseconds (for complete events).
    #[serde(rename = "dur", skip_serializing_if = "Option::is_none")]
    pub duration_us: Option<u64>,
    /// Optional arguments (key-value data).
    #[serde(rename = "args", skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Map<String, serde_json::Value>>,
}

impl TraceEvent {
    /// Export this event to a JSON object string compatible with Chrome Trace
    /// Event format.
    pub fn to_chrome_json(&self) -> Result<String> {
        serde_json::to_string(self).context("failed to serialize trace event")
    }
}

/// A tracer that collects trace events and can export them in Chrome Trace
/// Event format.
///
/// The tracer can be enabled and disabled at runtime to control overhead.
/// Collected events are stored in an in-memory ring buffer with a configurable
/// capacity.
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
    /// Create a new tracer with the given maximum number of retained events.
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

    /// Returns whether tracing is currently enabled.
    pub fn is_enabled(&self) -> bool {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled
    }

    /// Enable tracing.
    pub fn enable(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled = true;
    }

    /// Disable tracing.
    pub fn disable(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.enabled = false;
    }

    /// Clear all collected events.
    pub fn clear(&self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.clear();
    }

    /// Record a trace event.
    ///
    /// If tracing is disabled, the event is discarded.
    pub fn record(&self, name: impl Into<String>, category: impl Into<String>, phase: TracePhase) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !inner.enabled {
            return;
        }

        let timestamp_us =
            u64::try_from(inner.started_at.elapsed().as_micros()).unwrap_or(u64::MAX);

        let event = TraceEvent {
            name: bounded_trace_text(name.into()),
            category: bounded_trace_text(category.into()),
            phase,
            timestamp_us,
            process_id: inner.process_id,
            thread_id: current_thread_id(),
            duration_us: None,
            args: None,
        };

        push_event(&mut inner, event);
    }

    /// Record a duration event by measuring the time taken by `f`.
    ///
    /// The begin and end events are recorded around the execution of the closure.
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

    /// Returns a snapshot of the currently retained trace events.
    pub fn events(&self) -> Vec<TraceEvent> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inner.events.iter().cloned().collect()
    }

    /// Install this tracer as the process-wide instrumentation sink used by
    /// GPUI's built-in frame, layout, and input probes.
    pub fn install_global(&self) -> Option<Tracer> {
        let mut slot = global_tracer_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mem::replace(&mut *slot, Some(self.clone()))
    }

    /// Returns the currently installed global tracer, if any.
    pub fn global() -> Option<Tracer> {
        global_tracer_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Clear the process-wide global tracer registration.
    pub fn clear_global() -> Option<Tracer> {
        global_tracer_slot()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    /// Export all collected events to a Chrome Trace Event format JSON string.
    pub fn export_to_chrome_json(&self) -> Result<String> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let events: Vec<&TraceEvent> = inner.events.iter().collect();
        serde_json::to_string_pretty(&events).context("failed to export trace events")
    }

    /// Write the collected events to a file in Chrome Trace Event format.
    pub fn write_to_file(&self, path: impl Into<PathBuf>) -> Result<()> {
        let json = self.export_to_chrome_json()?;
        let path = path.into();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create trace directory: {}", parent.display())
            })?;
        }
        let sequence = NEXT_EXPORT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| anyhow::anyhow!("trace export identifier space exhausted"))?;
        let temp = path.with_extension(format!("tmp.{}.{}", std::process::id(), sequence));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("failed to create trace file: {}", temp.display()))?;
        file.write_all(json.as_bytes())
            .with_context(|| format!("failed to write trace file: {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync trace file: {}", temp.display()))?;
        fs::rename(&temp, &path).with_context(|| {
            format!(
                "failed to finalize trace file from {} to {}",
                temp.display(),
                path.display()
            )
        })?;
        Ok(())
    }
}

impl Default for Tracer {
    fn default() -> Self {
        Self::new(10_000)
    }
}

pub(crate) fn trace_global_duration<R>(
    name: &'static str,
    category: &'static str,
    f: impl FnOnce() -> R,
) -> R {
    if let Some(tracer) = Tracer::global() {
        tracer.record_duration(name, category, f)
    } else {
        f()
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
    if inner.max_events == 0 {
        return;
    }
    if inner.events.len() >= inner.max_events {
        inner.events.pop_front();
    }
    inner.events.push_back(event);
}

fn bounded_trace_text(mut value: String) -> String {
    if value.len() <= MAX_TRACE_NAME_BYTES {
        return value;
    }
    let mut boundary = MAX_TRACE_NAME_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracer_enable_disable() {
        let tracer = Tracer::new(100);
        assert!(!tracer.is_enabled());
        tracer.enable();
        assert!(tracer.is_enabled());
        tracer.disable();
        assert!(!tracer.is_enabled());
    }

    #[test]
    fn test_tracer_drops_events_when_disabled() {
        let tracer = Tracer::new(100);
        tracer.record("event", "test", TracePhase::Instant);
        let json = tracer.export_to_chrome_json().unwrap();
        let events: Vec<TraceEvent> = serde_json::from_str(&json).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_tracer_retains_events_when_enabled() {
        let tracer = Tracer::new(100);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);
        let json = tracer.export_to_chrome_json().unwrap();
        let events: Vec<TraceEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "event");
        assert_eq!(events[0].category, "test");
        assert_eq!(events[0].phase, TracePhase::Instant);
    }

    #[test]
    fn test_tracer_respects_max_events() {
        let tracer = Tracer::new(5);
        tracer.enable();
        for i in 0..10 {
            tracer.record(format!("event{}", i), "test", TracePhase::Instant);
        }
        let json = tracer.export_to_chrome_json().unwrap();
        let events: Vec<TraceEvent> = serde_json::from_str(&json).unwrap();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn zero_capacity_tracer_retains_nothing() {
        let tracer = Tracer::new(0);
        tracer.enable();
        tracer.record("event", "test", TracePhase::Instant);
        assert!(tracer.events().is_empty());
    }

    #[test]
    fn test_tracer_timestamps_are_monotonic() {
        let tracer = Tracer::new(100);
        tracer.enable();
        tracer.record("event1", "test", TracePhase::Instant);
        tracer.record("event2", "test", TracePhase::Instant);

        let events = tracer.events();
        assert_eq!(events.len(), 2);
        assert!(events[1].timestamp_us >= events[0].timestamp_us);
    }

    #[test]
    fn test_tracer_uses_stable_thread_id_on_same_thread() {
        let tracer = Tracer::new(100);
        tracer.enable();
        tracer.record("event1", "test", TracePhase::Instant);
        tracer.record("event2", "test", TracePhase::Begin);

        let events = tracer.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].thread_id, events[1].thread_id);
    }

    #[test]
    fn test_global_tracer_records_instrumented_events() {
        let tracer = Tracer::new(100);
        tracer.enable();
        tracer.install_global();

        trace_global_duration("instrumented", "test", || {});

        let events = tracer.events();
        Tracer::clear_global();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].name, "instrumented");
        assert_eq!(events[1].name, "instrumented");
        assert_eq!(events[0].phase, TracePhase::Begin);
        assert_eq!(events[1].phase, TracePhase::End);
    }

    #[test]
    fn test_trace_event_chrome_json_has_required_fields() {
        let event = TraceEvent {
            name: "frame_render".to_string(),
            category: "gpu".to_string(),
            phase: TracePhase::Begin,
            timestamp_us: 12345678,
            process_id: 42,
            thread_id: 7,
            duration_us: Some(100),
            args: None,
        };

        let json = event.to_chrome_json().unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object().unwrap();

        assert!(obj.contains_key("name"));
        assert!(obj.contains_key("cat"));
        assert!(obj.contains_key("ph"));
        assert!(obj.contains_key("ts"));
        assert!(obj.contains_key("pid"));
        assert!(obj.contains_key("tid"));
    }
}
