/// Developer tools for observability, diagnostics, and runtime inspection.
///
/// Provides element tree inspection, layout overlay visualization,
/// frame timeline profiling, job queue monitoring, structured logging,
/// and privacy-aware telemetry collection.
use std::collections::{HashMap, HashSet};

use anyhow::Result;

const MAX_INSPECTED_ELEMENTS: usize = 10_000;
const MAX_INSPECTED_DEPTH: usize = 256;
const MAX_INSPECTED_STYLES: usize = 256;
const MAX_DEV_TEXT_BYTES: usize = 4_096;
const MAX_OVERLAYS: usize = 10_000;
const MAX_FRAME_TIMELINE_CAPACITY: usize = 100_000;
const MAX_JOB_SNAPSHOTS: usize = 10_000;
const MAX_LOG_ENTRIES: usize = 10_000;
const MAX_TELEMETRY_EVENTS: usize = 10_000;

/// A snapshot of an element in the UI tree for developer inspection.
#[derive(Debug, Clone)]
pub struct InspectedElement {
    /// Unique identifier for this element.
    pub id: String,
    /// The type name of this element (e.g., "Button", "Div").
    pub element_type: String,
    /// Layout bounds as (x, y, width, height), if computed.
    pub bounds: Option<(f32, f32, f32, f32)>,
    /// Style properties applied to this element.
    pub styles: HashMap<String, String>,
    /// Child elements in the tree.
    pub children: Vec<InspectedElement>,
}

/// Builds and queries element trees for developer inspection.
#[derive(Debug, Default)]
pub struct ElementInspector {
    root: Option<InspectedElement>,
}

impl ElementInspector {
    /// Creates a new empty inspector.
    pub fn new() -> Self {
        Self { root: None }
    }

    /// Sets the root of the inspected element tree.
    pub fn build_tree(&mut self, root: InspectedElement) {
        let _ = self.build_tree_checked(root);
    }

    /// Sets the root after validating tree depth, size, geometry, and text.
    pub fn build_tree_checked(&mut self, root: InspectedElement) -> Result<()> {
        validate_inspected_tree(&root)?;
        self.root = Some(root);
        Ok(())
    }

    /// Returns a reference to the root element, if any.
    pub fn root(&self) -> Option<&InspectedElement> {
        self.root.as_ref()
    }

    /// Finds an element by its id, searching the entire tree.
    pub fn find_by_id(&self, id: &str) -> Option<&InspectedElement> {
        self.root.as_ref().and_then(|r| find_by_id_recursive(r, id))
    }

    /// Finds all elements matching the given type name.
    pub fn find_by_type(&self, element_type: &str) -> Vec<&InspectedElement> {
        let mut results = Vec::new();
        if let Some(root) = &self.root {
            find_by_type_recursive(root, element_type, &mut results);
        }
        results
    }

    /// Returns the maximum depth of the element tree.
    pub fn depth(&self) -> usize {
        self.root.as_ref().map_or(0, tree_depth)
    }

    /// Returns the total number of elements in the tree.
    pub fn count(&self) -> usize {
        self.root.as_ref().map_or(0, tree_count)
    }
}

fn validate_inspected_tree(root: &InspectedElement) -> Result<()> {
    let mut stack = vec![(root, 1_usize)];
    let mut count = 0_usize;
    let mut ids = HashSet::new();
    while let Some((element, depth)) = stack.pop() {
        count += 1;
        anyhow::ensure!(
            count <= MAX_INSPECTED_ELEMENTS,
            "inspected tree cannot exceed {MAX_INSPECTED_ELEMENTS} elements"
        );
        anyhow::ensure!(
            depth <= MAX_INSPECTED_DEPTH,
            "inspected tree cannot exceed depth {MAX_INSPECTED_DEPTH}"
        );
        validate_dev_text(&element.id, "element id")?;
        anyhow::ensure!(
            ids.insert(&element.id),
            "inspected element ids must be unique"
        );
        validate_dev_text(&element.element_type, "element type")?;
        anyhow::ensure!(
            element.styles.len() <= MAX_INSPECTED_STYLES,
            "inspected element cannot exceed {MAX_INSPECTED_STYLES} styles"
        );
        for (name, value) in &element.styles {
            validate_dev_text(name, "style name")?;
            validate_dev_text(value, "style value")?;
        }
        if let Some(bounds) = element.bounds {
            validate_geometry(bounds, "element bounds")?;
        }
        anyhow::ensure!(
            count
                .checked_add(stack.len())
                .and_then(|pending| pending.checked_add(element.children.len()))
                .is_some_and(|pending| pending <= MAX_INSPECTED_ELEMENTS),
            "inspected tree cannot exceed {MAX_INSPECTED_ELEMENTS} elements"
        );
        stack.extend(
            element
                .children
                .iter()
                .rev()
                .map(|child| (child, depth + 1)),
        );
    }
    Ok(())
}

fn find_by_id_recursive<'a>(
    element: &'a InspectedElement,
    id: &str,
) -> Option<&'a InspectedElement> {
    if element.id == id {
        return Some(element);
    }
    for child in &element.children {
        if let Some(found) = find_by_id_recursive(child, id) {
            return Some(found);
        }
    }
    None
}

fn find_by_type_recursive<'a>(
    element: &'a InspectedElement,
    element_type: &str,
    results: &mut Vec<&'a InspectedElement>,
) {
    if element.element_type == element_type {
        results.push(element);
    }
    for child in &element.children {
        find_by_type_recursive(child, element_type, results);
    }
}

fn tree_depth(element: &InspectedElement) -> usize {
    if element.children.is_empty() {
        return 1;
    }
    1 + element.children.iter().map(tree_depth).max().unwrap_or(0)
}

fn tree_count(element: &InspectedElement) -> usize {
    1 + element.children.iter().map(tree_count).sum::<usize>()
}

/// Describes a layout overlay for a single element, showing bounds, margin, and padding.
#[derive(Debug, Clone)]
pub struct LayoutOverlay {
    /// The element this overlay is attached to.
    pub element_id: String,
    /// Layout bounds as (x, y, width, height).
    pub bounds: (f32, f32, f32, f32),
    /// Margin as (top, right, bottom, left).
    pub margin: (f32, f32, f32, f32),
    /// Padding as (top, right, bottom, left).
    pub padding: (f32, f32, f32, f32),
    /// Optional label to display on the overlay.
    pub label: Option<String>,
}

/// Manages a collection of layout overlays for debugging element bounds.
#[derive(Debug, Default)]
pub struct OverlayManager {
    overlays: Vec<LayoutOverlay>,
    visible: bool,
}

impl OverlayManager {
    /// Creates a new overlay manager with no overlays.
    pub fn new() -> Self {
        Self {
            overlays: Vec::new(),
            visible: true,
        }
    }

    /// Adds an overlay to the manager.
    pub fn add(&mut self, overlay: LayoutOverlay) {
        let _ = self.add_checked(overlay);
    }

    /// Adds a validated overlay while enforcing bounded retention.
    pub fn add_checked(&mut self, overlay: LayoutOverlay) -> Result<()> {
        anyhow::ensure!(
            self.overlays.len() < MAX_OVERLAYS,
            "overlay manager cannot exceed {MAX_OVERLAYS} overlays"
        );
        validate_dev_text(&overlay.element_id, "overlay element id")?;
        validate_geometry(overlay.bounds, "overlay bounds")?;
        validate_insets(overlay.margin, "overlay margin")?;
        validate_insets(overlay.padding, "overlay padding")?;
        if let Some(label) = &overlay.label {
            validate_dev_text(label, "overlay label")?;
        }
        self.overlays.push(overlay);
        Ok(())
    }

    /// Removes all overlays matching the given element id.
    pub fn remove(&mut self, element_id: &str) {
        self.overlays.retain(|o| o.element_id != element_id);
    }

    /// Removes all overlays.
    pub fn clear(&mut self) {
        self.overlays.clear();
    }

    /// Returns a slice of all current overlays.
    pub fn list(&self) -> &[LayoutOverlay] {
        &self.overlays
    }

    /// Toggles overlay visibility on or off, returning the new state.
    pub fn toggle_visibility(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// Returns whether overlays are currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

/// A record of a single rendered frame with timing breakdowns.
#[derive(Debug, Clone, Copy)]
pub struct FrameRecord {
    /// Monotonically increasing frame number.
    pub frame_number: u64,
    /// Timestamp when the frame started, in microseconds.
    pub start_us: u64,
    /// Total frame duration in microseconds.
    pub duration_us: u64,
    /// Time spent in layout phase, in microseconds.
    pub layout_us: u64,
    /// Time spent in paint phase, in microseconds.
    pub paint_us: u64,
    /// Time spent in GPU operations, in microseconds.
    pub gpu_us: u64,
    /// Number of elements rendered in this frame.
    pub element_count: u32,
}

const DEFAULT_RING_CAPACITY: usize = 300;

/// A ring-buffer-backed timeline of frame performance records.
#[derive(Debug)]
pub struct FrameTimeline {
    buffer: Vec<FrameRecord>,
    head: usize,
    len: usize,
    capacity: usize,
}

impl FrameTimeline {
    /// Creates a new frame timeline with the default capacity (300 frames).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_RING_CAPACITY)
    }

    /// Creates a new frame timeline with the given ring buffer capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.clamp(1, MAX_FRAME_TIMELINE_CAPACITY);
        Self {
            buffer: Vec::with_capacity(capacity),
            head: 0,
            len: 0,
            capacity,
        }
    }

    /// Creates a timeline after validating its requested retention capacity.
    pub fn with_capacity_checked(capacity: usize) -> Result<Self> {
        anyhow::ensure!(capacity > 0, "frame timeline capacity must be nonzero");
        anyhow::ensure!(
            capacity <= MAX_FRAME_TIMELINE_CAPACITY,
            "frame timeline capacity cannot exceed {MAX_FRAME_TIMELINE_CAPACITY}"
        );
        Ok(Self::with_capacity(capacity))
    }

    /// Records a new frame in the timeline.
    pub fn record(&mut self, record: FrameRecord) {
        if self.buffer.len() < self.capacity {
            self.buffer.push(record);
            self.len = self.buffer.len();
        } else {
            self.buffer[self.head] = record;
            self.head = (self.head + 1) % self.capacity;
            self.len = self.capacity;
        }
    }

    /// Returns all frame records in chronological order.
    pub fn history(&self) -> Vec<FrameRecord> {
        if self.buffer.len() < self.capacity {
            return self.buffer.clone();
        }
        let mut result = Vec::with_capacity(self.len);
        result.extend_from_slice(&self.buffer[self.head..]);
        result.extend_from_slice(&self.buffer[..self.head]);
        result
    }

    /// Returns the number of recorded frames.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the timeline is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns the average frame duration in microseconds, or `None` if empty.
    pub fn average_duration_us(&self) -> Option<f64> {
        if self.len == 0 {
            return None;
        }
        let sum: u128 = self.buffer[..self.buffer.len().min(self.len)]
            .iter()
            .map(|f| u128::from(f.duration_us))
            .sum();
        Some(sum as f64 / self.len as f64)
    }

    /// Returns the p95 frame duration in microseconds, or `None` if empty.
    pub fn p95_duration_us(&self) -> Option<u64> {
        self.percentile_duration(95)
    }

    /// Returns the p99 frame duration in microseconds, or `None` if empty.
    pub fn p99_duration_us(&self) -> Option<u64> {
        self.percentile_duration(99)
    }

    /// Detects frames that took longer than the given threshold (jank).
    pub fn detect_jank(&self, threshold_us: u64) -> Vec<FrameRecord> {
        self.history()
            .into_iter()
            .filter(|f| f.duration_us > threshold_us)
            .collect()
    }

    fn percentile_duration(&self, percentile: usize) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        let mut durations: Vec<u64> = self.buffer[..self.buffer.len().min(self.len)]
            .iter()
            .map(|f| f.duration_us)
            .collect();
        durations.sort_unstable();
        let index = (percentile as f64 / 100.0 * (durations.len() - 1) as f64).ceil() as usize;
        Some(durations[index.min(durations.len() - 1)])
    }
}

impl Default for FrameTimeline {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of a background job's current state.
#[derive(Debug, Clone)]
pub struct JobSnapshot {
    /// Unique identifier for the job.
    pub id: String,
    /// Human-readable name for the job.
    pub name: String,
    /// Current status (e.g., "queued", "running", "completed", "failed").
    pub status: String,
    /// Completion progress from 0.0 to 1.0, if available.
    pub progress: Option<f32>,
    /// Timestamp when the job was queued, in microseconds.
    pub queued_at: u64,
    /// Timestamp when execution started, in microseconds.
    pub started_at: Option<u64>,
    /// Timestamp when execution completed, in microseconds.
    pub completed_at: Option<u64>,
}

/// Tracks and queries snapshots of background jobs.
#[derive(Debug, Default)]
pub struct JobQueueViewer {
    snapshots: Vec<JobSnapshot>,
}

impl JobQueueViewer {
    /// Creates a new empty job queue viewer.
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
        }
    }

    /// Adds a job snapshot to the viewer.
    pub fn add(&mut self, snapshot: JobSnapshot) {
        let _ = self.add_checked(snapshot);
    }

    /// Adds a validated snapshot while enforcing bounded retention.
    pub fn add_checked(&mut self, snapshot: JobSnapshot) -> Result<()> {
        validate_job_snapshot(&snapshot)?;
        anyhow::ensure!(
            self.snapshots.len() < MAX_JOB_SNAPSHOTS,
            "job viewer cannot exceed {MAX_JOB_SNAPSHOTS} snapshots"
        );
        anyhow::ensure!(
            !self
                .snapshots
                .iter()
                .any(|existing| existing.id == snapshot.id),
            "job snapshot id is already present"
        );
        self.snapshots.push(snapshot);
        Ok(())
    }

    /// Updates an existing job snapshot by id, or adds it if not found.
    pub fn update(&mut self, snapshot: JobSnapshot) {
        let _ = self.update_checked(snapshot);
    }

    /// Updates or inserts a validated snapshot.
    pub fn update_checked(&mut self, snapshot: JobSnapshot) -> Result<()> {
        validate_job_snapshot(&snapshot)?;
        if let Some(existing) = self.snapshots.iter_mut().find(|s| s.id == snapshot.id) {
            *existing = snapshot;
        } else {
            anyhow::ensure!(
                self.snapshots.len() < MAX_JOB_SNAPSHOTS,
                "job viewer cannot exceed {MAX_JOB_SNAPSHOTS} snapshots"
            );
            self.snapshots.push(snapshot);
        }
        Ok(())
    }

    /// Returns a slice of all job snapshots.
    pub fn list(&self) -> &[JobSnapshot] {
        &self.snapshots
    }

    /// Returns the number of jobs currently running.
    pub fn active_count(&self) -> usize {
        self.snapshots
            .iter()
            .filter(|s| s.status == "running")
            .count()
    }

    /// Returns the number of completed jobs.
    pub fn completed_count(&self) -> usize {
        self.snapshots
            .iter()
            .filter(|s| s.status == "completed")
            .count()
    }

    /// Returns the average duration of completed jobs in microseconds, or `None` if none completed.
    pub fn average_duration_us(&self) -> Option<f64> {
        let mut count = 0_u64;
        let total = self.snapshots.iter().fold(0_u128, |total, snapshot| {
            let Some((started, completed)) = snapshot.started_at.zip(snapshot.completed_at) else {
                return total;
            };
            count += 1;
            total + u128::from(completed.saturating_sub(started))
        });
        if count == 0 {
            return None;
        }
        Some(total as f64 / count as f64)
    }
}

fn validate_job_snapshot(snapshot: &JobSnapshot) -> Result<()> {
    validate_dev_text(&snapshot.id, "job id")?;
    validate_dev_text(&snapshot.name, "job name")?;
    validate_dev_text(&snapshot.status, "job status")?;
    if let Some(progress) = snapshot.progress {
        anyhow::ensure!(
            progress.is_finite() && (0.0..=1.0).contains(&progress),
            "job progress must be finite and between zero and one"
        );
    }
    if let Some(started) = snapshot.started_at {
        anyhow::ensure!(
            started >= snapshot.queued_at,
            "job starts before it is queued"
        );
    }
    if let Some(completed) = snapshot.completed_at {
        let started = snapshot
            .started_at
            .ok_or_else(|| anyhow::anyhow!("completed job is missing its start timestamp"))?;
        anyhow::ensure!(completed >= started, "job completes before it starts");
    }
    Ok(())
}

/// Severity level for log entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    /// Most verbose: function-level tracing.
    Trace,
    /// Development diagnostics.
    Debug,
    /// Normal operational messages.
    Info,
    /// Potential issues that deserve attention.
    Warn,
    /// Failures that need investigation.
    Error,
}

/// A single structured log entry.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Timestamp in microseconds since an arbitrary epoch.
    pub timestamp: u64,
    /// Severity level.
    pub level: LogLevel,
    /// The subsystem or component that produced this entry.
    pub source: String,
    /// The log message body.
    pub message: String,
}

/// A filterable, bounded log viewer for developer tools.
#[derive(Debug, Default)]
pub struct LogViewer {
    entries: Vec<LogEntry>,
}

impl LogViewer {
    /// Creates a new empty log viewer.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Appends a log entry.
    pub fn append(&mut self, entry: LogEntry) {
        let _ = self.append_checked(entry);
    }

    /// Appends a validated log entry and evicts the oldest entry at capacity.
    pub fn append_checked(&mut self, entry: LogEntry) -> Result<()> {
        validate_dev_text(&entry.source, "log source")?;
        validate_dev_text(&entry.message, "log message")?;
        if self.entries.len() == MAX_LOG_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(entry);
        Ok(())
    }

    /// Returns entries at or above the given severity level.
    pub fn filter_by_level(&self, min_level: LogLevel) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| e.level >= min_level)
            .collect()
    }

    /// Returns entries whose source matches the given string exactly.
    pub fn filter_by_source(&self, source: &str) -> Vec<&LogEntry> {
        self.entries.iter().filter(|e| e.source == source).collect()
    }

    /// Returns entries whose message contains the given substring.
    pub fn filter_by_text(&self, text: &str) -> Vec<&LogEntry> {
        self.entries
            .iter()
            .filter(|e| e.message.contains(text))
            .collect()
    }

    /// Removes all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns the most recent `n` entries, in chronological order.
    pub fn recent(&self, n: usize) -> &[LogEntry] {
        let start = self.entries.len().saturating_sub(n);
        &self.entries[start..]
    }

    /// Returns the total number of entries.
    pub fn count(&self) -> usize {
        self.entries.len()
    }
}

/// An event recorded by the telemetry system.
#[derive(Debug, Clone)]
pub enum TelemetryEvent {
    /// A performance metric with a named measurement and value.
    Performance {
        /// Name of the metric (e.g., "frame_time_ms").
        metric: String,
        /// Measured value.
        value: f64,
    },
    /// A lifecycle event such as startup or shutdown.
    Lifecycle {
        /// Description of the lifecycle event.
        event: String,
    },
    /// Tracks how often a feature is used.
    FeatureUsage {
        /// Name of the feature.
        feature: String,
        /// Usage count.
        count: u64,
    },
}

/// Controls whether and how telemetry data is collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelemetryMode {
    /// No telemetry is collected.
    Disabled,
    /// Telemetry is collected and stored locally only.
    LocalOnly,
    /// Telemetry is collected and may be shared with explicit user consent.
    OptIn,
}

/// Collects, filters, and manages telemetry events with privacy controls.
#[derive(Debug)]
pub struct TelemetryCollector {
    mode: TelemetryMode,
    events: Vec<TelemetryEvent>,
}

impl TelemetryCollector {
    /// Creates a new telemetry collector with the given mode.
    pub fn new(mode: TelemetryMode) -> Self {
        Self {
            mode,
            events: Vec::new(),
        }
    }

    /// Returns the current telemetry mode.
    pub fn mode(&self) -> TelemetryMode {
        self.mode
    }

    /// Sets the telemetry mode.
    pub fn set_mode(&mut self, mode: TelemetryMode) {
        self.mode = mode;
        if mode == TelemetryMode::Disabled {
            self.events.clear();
        }
    }

    /// Records a telemetry event if the mode is not `Disabled`.
    pub fn record(&mut self, event: TelemetryEvent) {
        let _ = self.record_checked(event);
    }

    /// Records a validated event while enforcing bounded retention.
    pub fn record_checked(&mut self, event: TelemetryEvent) -> Result<bool> {
        if self.mode == TelemetryMode::Disabled {
            return Ok(false);
        }
        validate_telemetry_event(&event)?;
        if self.events.len() == MAX_TELEMETRY_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
        Ok(true)
    }

    /// Drains and returns all collected events, clearing the internal buffer.
    pub fn flush(&mut self) -> Vec<TelemetryEvent> {
        std::mem::take(&mut self.events)
    }

    /// Returns a reference to all collected events.
    pub fn events(&self) -> &[TelemetryEvent] {
        &self.events
    }

    /// Returns only performance events.
    pub fn filter_performance(&self) -> Vec<&TelemetryEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, TelemetryEvent::Performance { .. }))
            .collect()
    }

    /// Returns only lifecycle events.
    pub fn filter_lifecycle(&self) -> Vec<&TelemetryEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, TelemetryEvent::Lifecycle { .. }))
            .collect()
    }

    /// Returns only feature usage events.
    pub fn filter_feature_usage(&self) -> Vec<&TelemetryEvent> {
        self.events
            .iter()
            .filter(|e| matches!(e, TelemetryEvent::FeatureUsage { .. }))
            .collect()
    }

    /// Strips potentially identifying information from all stored events.
    ///
    /// Replaces metric names and event descriptions that look like file paths,
    /// email addresses, or user identifiers with sanitized placeholders.
    pub fn privacy_filter(&mut self) {
        for event in &mut self.events {
            match event {
                TelemetryEvent::Performance { metric, .. } => {
                    *metric = strip_pii(metric);
                }
                TelemetryEvent::Lifecycle { event: ev } => {
                    *ev = strip_pii(ev);
                }
                TelemetryEvent::FeatureUsage { feature, .. } => {
                    *feature = strip_pii(feature);
                }
            }
        }
    }
}

fn validate_telemetry_event(event: &TelemetryEvent) -> Result<()> {
    match event {
        TelemetryEvent::Performance { metric, value } => {
            validate_dev_text(metric, "telemetry metric")?;
            anyhow::ensure!(value.is_finite(), "telemetry value must be finite");
        }
        TelemetryEvent::Lifecycle { event } => {
            validate_dev_text(event, "telemetry lifecycle event")?;
        }
        TelemetryEvent::FeatureUsage { feature, .. } => {
            validate_dev_text(feature, "telemetry feature")?;
        }
    }
    Ok(())
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new(TelemetryMode::Disabled)
    }
}

fn strip_pii(input: &str) -> String {
    if input.contains('@') || input.contains('/') || input.contains('\\') {
        "[redacted]".to_string()
    } else {
        input.to_string()
    }
}

fn validate_dev_text(value: &str, field: &str) -> Result<()> {
    anyhow::ensure!(!value.trim().is_empty(), "{field} cannot be empty");
    anyhow::ensure!(
        value.len() <= MAX_DEV_TEXT_BYTES,
        "{field} cannot exceed {MAX_DEV_TEXT_BYTES} bytes"
    );
    anyhow::ensure!(
        !value.chars().any(char::is_control),
        "{field} cannot contain control characters"
    );
    Ok(())
}

fn validate_geometry(bounds: (f32, f32, f32, f32), field: &str) -> Result<()> {
    let (x, y, width, height) = bounds;
    anyhow::ensure!(
        [x, y, width, height].into_iter().all(f32::is_finite),
        "{field} must contain finite values"
    );
    anyhow::ensure!(
        width >= 0.0 && height >= 0.0,
        "{field} dimensions cannot be negative"
    );
    Ok(())
}

fn validate_insets(insets: (f32, f32, f32, f32), field: &str) -> Result<()> {
    let (top, right, bottom, left) = insets;
    anyhow::ensure!(
        [top, right, bottom, left].into_iter().all(f32::is_finite),
        "{field} must contain finite values"
    );
    Ok(())
}

/// Live-reload design tokens from a styles file without recompiling.
///
/// Watches `path` (a JSON or TOML theme file carrying any subset of the color,
/// typography, spacing, radius, and shadow tokens) and applies every successful
/// reload to the global [`crate::Theme`], so edits to spacing/color/typography
/// take effect in the running app on save. This is the styles slice of hot
/// reload — the cheapest iteration-speed win while full code hot-patch is out of
/// scope. The initial load is applied immediately; a parse error on a later edit
/// is logged and the previous styles are kept.
///
/// ```no_run
/// # use kael::App;
/// # fn demo(cx: &mut App) -> anyhow::Result<()> {
/// kael::dev::watch_styles(cx, "theme.toml")?;
/// # Ok(())
/// # }
/// ```
pub fn watch_styles(cx: &mut crate::App, path: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
    cx.observe_theme_file(path, |theme, cx| cx.set_global(theme))
}

/// Developer ergonomics front door (`kael::dev::*`).
///
/// A stable, discoverable namespace for iteration-speed tools such as
/// [`watch_styles`](dev::watch_styles) live token reload.
pub mod dev {
    pub use super::watch_styles;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[kael::test]
    fn watch_styles_live_applies_spacing_and_color_tokens(cx: &mut crate::TestAppContext) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT_DIR: AtomicU32 = AtomicU32::new(0);

        let directory = std::env::temp_dir().join(format!(
            "kael-watch-styles-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let styles_path = directory.join("styles.toml");

        cx.on_quit({
            let directory = directory.clone();
            move || {
                let _ = std::fs::remove_dir_all(directory);
            }
        });

        std::fs::write(
            &styles_path,
            "[spacing]\nmd = 20.0\n[colors]\nprimary = \"#2563eb\"\n",
        )
        .unwrap();

        cx.update(|cx| {
            watch_styles(cx, &styles_path).unwrap();
        });

        let expected = crate::Theme::from_path(&styles_path).unwrap();
        cx.read_global::<crate::Theme, _>(|theme, _| {
            assert_eq!(
                theme, &expected,
                "watch_styles must apply the styles file to the global theme"
            );
            assert_eq!(
                theme.spacing.md,
                crate::px(20.0),
                "a non-color (spacing) token must hot-apply, not just colors"
            );
        });
    }

    fn sample_tree() -> InspectedElement {
        InspectedElement {
            id: "root".into(),
            element_type: "Div".into(),
            bounds: Some((0.0, 0.0, 800.0, 600.0)),
            styles: HashMap::from([("display".into(), "flex".into())]),
            children: vec![
                InspectedElement {
                    id: "header".into(),
                    element_type: "Div".into(),
                    bounds: Some((0.0, 0.0, 800.0, 50.0)),
                    styles: HashMap::new(),
                    children: vec![InspectedElement {
                        id: "title".into(),
                        element_type: "Text".into(),
                        bounds: Some((10.0, 10.0, 200.0, 30.0)),
                        styles: HashMap::new(),
                        children: vec![],
                    }],
                },
                InspectedElement {
                    id: "btn1".into(),
                    element_type: "Button".into(),
                    bounds: Some((0.0, 50.0, 100.0, 40.0)),
                    styles: HashMap::from([("color".into(), "red".into())]),
                    children: vec![],
                },
            ],
        }
    }

    #[test]
    fn inspector_empty() {
        let inspector = ElementInspector::new();
        assert!(inspector.root().is_none());
        assert_eq!(inspector.depth(), 0);
        assert_eq!(inspector.count(), 0);
    }

    #[test]
    fn inspector_build_tree() {
        let mut inspector = ElementInspector::new();
        inspector.build_tree(sample_tree());
        assert!(inspector.root().is_some());
        assert_eq!(inspector.root().unwrap().id, "root");
    }

    #[test]
    fn inspector_find_by_id() {
        let mut inspector = ElementInspector::new();
        inspector.build_tree(sample_tree());
        assert!(inspector.find_by_id("title").is_some());
        assert_eq!(inspector.find_by_id("title").unwrap().element_type, "Text");
        assert!(inspector.find_by_id("nonexistent").is_none());
    }

    #[test]
    fn inspector_find_by_type() {
        let mut inspector = ElementInspector::new();
        inspector.build_tree(sample_tree());
        let divs = inspector.find_by_type("Div");
        assert_eq!(divs.len(), 2);
        let buttons = inspector.find_by_type("Button");
        assert_eq!(buttons.len(), 1);
        assert!(inspector.find_by_type("Missing").is_empty());
    }

    #[test]
    fn inspector_depth() {
        let mut inspector = ElementInspector::new();
        inspector.build_tree(sample_tree());
        assert_eq!(inspector.depth(), 3);
    }

    #[test]
    fn inspector_count() {
        let mut inspector = ElementInspector::new();
        inspector.build_tree(sample_tree());
        assert_eq!(inspector.count(), 4);
    }

    #[test]
    fn overlay_manager_add_remove() {
        let mut mgr = OverlayManager::new();
        mgr.add(LayoutOverlay {
            element_id: "a".into(),
            bounds: (0.0, 0.0, 100.0, 100.0),
            margin: (0.0, 0.0, 0.0, 0.0),
            padding: (5.0, 5.0, 5.0, 5.0),
            label: None,
        });
        mgr.add(LayoutOverlay {
            element_id: "b".into(),
            bounds: (100.0, 0.0, 100.0, 100.0),
            margin: (0.0, 0.0, 0.0, 0.0),
            padding: (0.0, 0.0, 0.0, 0.0),
            label: Some("sidebar".into()),
        });
        assert_eq!(mgr.list().len(), 2);
        mgr.remove("a");
        assert_eq!(mgr.list().len(), 1);
        assert_eq!(mgr.list()[0].element_id, "b");
    }

    #[test]
    fn overlay_manager_clear() {
        let mut mgr = OverlayManager::new();
        mgr.add(LayoutOverlay {
            element_id: "x".into(),
            bounds: (0.0, 0.0, 1.0, 1.0),
            margin: (0.0, 0.0, 0.0, 0.0),
            padding: (0.0, 0.0, 0.0, 0.0),
            label: None,
        });
        mgr.clear();
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn overlay_manager_visibility() {
        let mut mgr = OverlayManager::new();
        assert!(mgr.is_visible());
        let toggled = mgr.toggle_visibility();
        assert!(!toggled);
        assert!(!mgr.is_visible());
        mgr.toggle_visibility();
        assert!(mgr.is_visible());
    }

    #[test]
    fn frame_timeline_empty() {
        let timeline = FrameTimeline::new();
        assert!(timeline.is_empty());
        assert_eq!(timeline.len(), 0);
        assert!(timeline.average_duration_us().is_none());
        assert!(timeline.p95_duration_us().is_none());
        assert!(timeline.p99_duration_us().is_none());
    }

    fn make_frame(num: u64, duration: u64) -> FrameRecord {
        FrameRecord {
            frame_number: num,
            start_us: num * 16_000,
            duration_us: duration,
            layout_us: duration / 4,
            paint_us: duration / 4,
            gpu_us: duration / 2,
            element_count: 100,
        }
    }

    #[test]
    fn frame_timeline_record_and_history() {
        let mut timeline = FrameTimeline::with_capacity(3);
        timeline.record(make_frame(1, 16_000));
        timeline.record(make_frame(2, 17_000));
        assert_eq!(timeline.len(), 2);
        let history = timeline.history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].frame_number, 1);
        assert_eq!(history[1].frame_number, 2);
    }

    #[test]
    fn frame_timeline_ring_buffer_wraps() {
        let mut timeline = FrameTimeline::with_capacity(3);
        timeline.record(make_frame(1, 10_000));
        timeline.record(make_frame(2, 11_000));
        timeline.record(make_frame(3, 12_000));
        timeline.record(make_frame(4, 13_000));
        assert_eq!(timeline.len(), 3);
        let history = timeline.history();
        assert_eq!(history[0].frame_number, 2);
        assert_eq!(history[2].frame_number, 4);
    }

    #[test]
    fn frame_timeline_average() {
        let mut timeline = FrameTimeline::new();
        timeline.record(make_frame(1, 10_000));
        timeline.record(make_frame(2, 20_000));
        let avg = timeline.average_duration_us().unwrap();
        assert!((avg - 15_000.0).abs() < 0.1);
    }

    #[test]
    fn frame_timeline_percentiles() {
        let mut timeline = FrameTimeline::with_capacity(100);
        for i in 1..=100 {
            timeline.record(make_frame(i, i as u64 * 1000));
        }
        let p95 = timeline.p95_duration_us().unwrap();
        assert!(p95 >= 95_000);
        let p99 = timeline.p99_duration_us().unwrap();
        assert!(p99 >= 99_000);
    }

    #[test]
    fn frame_timeline_detect_jank() {
        let mut timeline = FrameTimeline::new();
        timeline.record(make_frame(1, 16_000));
        timeline.record(make_frame(2, 50_000));
        timeline.record(make_frame(3, 15_000));
        let janky = timeline.detect_jank(20_000);
        assert_eq!(janky.len(), 1);
        assert_eq!(janky[0].frame_number, 2);
    }

    #[test]
    fn job_queue_viewer_add_and_list() {
        let mut viewer = JobQueueViewer::new();
        viewer.add(JobSnapshot {
            id: "j1".into(),
            name: "indexing".into(),
            status: "running".into(),
            progress: Some(0.5),
            queued_at: 1000,
            started_at: Some(2000),
            completed_at: None,
        });
        assert_eq!(viewer.list().len(), 1);
    }

    #[test]
    fn job_queue_viewer_update() {
        let mut viewer = JobQueueViewer::new();
        viewer.add(JobSnapshot {
            id: "j1".into(),
            name: "build".into(),
            status: "queued".into(),
            progress: None,
            queued_at: 100,
            started_at: None,
            completed_at: None,
        });
        viewer.update(JobSnapshot {
            id: "j1".into(),
            name: "build".into(),
            status: "running".into(),
            progress: Some(0.3),
            queued_at: 100,
            started_at: Some(200),
            completed_at: None,
        });
        assert_eq!(viewer.list().len(), 1);
        assert_eq!(viewer.list()[0].status, "running");
    }

    #[test]
    fn job_queue_viewer_update_adds_if_missing() {
        let mut viewer = JobQueueViewer::new();
        viewer.update(JobSnapshot {
            id: "j2".into(),
            name: "lint".into(),
            status: "completed".into(),
            progress: Some(1.0),
            queued_at: 50,
            started_at: Some(60),
            completed_at: Some(80),
        });
        assert_eq!(viewer.list().len(), 1);
    }

    #[test]
    fn job_queue_viewer_counts() {
        let mut viewer = JobQueueViewer::new();
        viewer.add(JobSnapshot {
            id: "j1".into(),
            name: "a".into(),
            status: "running".into(),
            progress: None,
            queued_at: 0,
            started_at: Some(1),
            completed_at: None,
        });
        viewer.add(JobSnapshot {
            id: "j2".into(),
            name: "b".into(),
            status: "completed".into(),
            progress: Some(1.0),
            queued_at: 0,
            started_at: Some(1),
            completed_at: Some(10),
        });
        viewer.add(JobSnapshot {
            id: "j3".into(),
            name: "c".into(),
            status: "completed".into(),
            progress: Some(1.0),
            queued_at: 0,
            started_at: Some(1),
            completed_at: Some(20),
        });
        assert_eq!(viewer.active_count(), 1);
        assert_eq!(viewer.completed_count(), 2);
    }

    #[test]
    fn job_queue_viewer_average_duration() {
        let mut viewer = JobQueueViewer::new();
        viewer.add(JobSnapshot {
            id: "j1".into(),
            name: "a".into(),
            status: "completed".into(),
            progress: Some(1.0),
            queued_at: 0,
            started_at: Some(100),
            completed_at: Some(200),
        });
        viewer.add(JobSnapshot {
            id: "j2".into(),
            name: "b".into(),
            status: "completed".into(),
            progress: Some(1.0),
            queued_at: 0,
            started_at: Some(100),
            completed_at: Some(300),
        });
        let avg = viewer.average_duration_us().unwrap();
        assert!((avg - 150.0).abs() < 0.1);
    }

    #[test]
    fn job_queue_viewer_no_completed_average() {
        let viewer = JobQueueViewer::new();
        assert!(viewer.average_duration_us().is_none());
    }

    #[test]
    fn log_viewer_append_and_count() {
        let mut viewer = LogViewer::new();
        viewer.append(LogEntry {
            timestamp: 1,
            level: LogLevel::Info,
            source: "app".into(),
            message: "started".into(),
        });
        assert_eq!(viewer.count(), 1);
    }

    #[test]
    fn log_viewer_filter_by_level() {
        let mut viewer = LogViewer::new();
        viewer.append(LogEntry {
            timestamp: 1,
            level: LogLevel::Debug,
            source: "s".into(),
            message: "debug msg".into(),
        });
        viewer.append(LogEntry {
            timestamp: 2,
            level: LogLevel::Warn,
            source: "s".into(),
            message: "warn msg".into(),
        });
        viewer.append(LogEntry {
            timestamp: 3,
            level: LogLevel::Error,
            source: "s".into(),
            message: "error msg".into(),
        });
        let warnings_up = viewer.filter_by_level(LogLevel::Warn);
        assert_eq!(warnings_up.len(), 2);
    }

    #[test]
    fn log_viewer_filter_by_source() {
        let mut viewer = LogViewer::new();
        viewer.append(LogEntry {
            timestamp: 1,
            level: LogLevel::Info,
            source: "ui".into(),
            message: "rendering".into(),
        });
        viewer.append(LogEntry {
            timestamp: 2,
            level: LogLevel::Info,
            source: "db".into(),
            message: "query".into(),
        });
        let ui_logs = viewer.filter_by_source("ui");
        assert_eq!(ui_logs.len(), 1);
        assert_eq!(ui_logs[0].message, "rendering");
    }

    #[test]
    fn log_viewer_filter_by_text() {
        let mut viewer = LogViewer::new();
        viewer.append(LogEntry {
            timestamp: 1,
            level: LogLevel::Info,
            source: "s".into(),
            message: "connection established".into(),
        });
        viewer.append(LogEntry {
            timestamp: 2,
            level: LogLevel::Error,
            source: "s".into(),
            message: "connection refused".into(),
        });
        let results = viewer.filter_by_text("connection");
        assert_eq!(results.len(), 2);
        let refused = viewer.filter_by_text("refused");
        assert_eq!(refused.len(), 1);
    }

    #[test]
    fn log_viewer_clear() {
        let mut viewer = LogViewer::new();
        viewer.append(LogEntry {
            timestamp: 1,
            level: LogLevel::Info,
            source: "s".into(),
            message: "msg".into(),
        });
        viewer.clear();
        assert_eq!(viewer.count(), 0);
    }

    #[test]
    fn log_viewer_recent() {
        let mut viewer = LogViewer::new();
        for i in 0..10 {
            viewer.append(LogEntry {
                timestamp: i,
                level: LogLevel::Info,
                source: "s".into(),
                message: format!("msg {i}"),
            });
        }
        let recent = viewer.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].message, "msg 7");
        assert_eq!(recent[2].message, "msg 9");
    }

    #[test]
    fn log_viewer_recent_more_than_available() {
        let mut viewer = LogViewer::new();
        viewer.append(LogEntry {
            timestamp: 1,
            level: LogLevel::Info,
            source: "s".into(),
            message: "only".into(),
        });
        let recent = viewer.recent(100);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Trace < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }

    #[test]
    fn telemetry_disabled_ignores_events() {
        let mut collector = TelemetryCollector::new(TelemetryMode::Disabled);
        collector.record(TelemetryEvent::Lifecycle {
            event: "startup".into(),
        });
        assert!(collector.events().is_empty());
    }

    #[test]
    fn telemetry_local_records_events() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::Performance {
            metric: "fps".into(),
            value: 60.0,
        });
        assert_eq!(collector.events().len(), 1);
    }

    #[test]
    fn telemetry_opt_in_records_events() {
        let mut collector = TelemetryCollector::new(TelemetryMode::OptIn);
        collector.record(TelemetryEvent::FeatureUsage {
            feature: "search".into(),
            count: 5,
        });
        assert_eq!(collector.events().len(), 1);
    }

    #[test]
    fn telemetry_flush() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::Lifecycle {
            event: "boot".into(),
        });
        let flushed = collector.flush();
        assert_eq!(flushed.len(), 1);
        assert!(collector.events().is_empty());
    }

    #[test]
    fn telemetry_filter_performance() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::Performance {
            metric: "cpu".into(),
            value: 42.0,
        });
        collector.record(TelemetryEvent::Lifecycle {
            event: "init".into(),
        });
        assert_eq!(collector.filter_performance().len(), 1);
    }

    #[test]
    fn telemetry_filter_lifecycle() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::Lifecycle {
            event: "shutdown".into(),
        });
        collector.record(TelemetryEvent::Performance {
            metric: "mem".into(),
            value: 512.0,
        });
        assert_eq!(collector.filter_lifecycle().len(), 1);
    }

    #[test]
    fn telemetry_filter_feature_usage() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::FeatureUsage {
            feature: "autocomplete".into(),
            count: 10,
        });
        collector.record(TelemetryEvent::Performance {
            metric: "latency".into(),
            value: 5.0,
        });
        assert_eq!(collector.filter_feature_usage().len(), 1);
    }

    #[test]
    fn telemetry_mode_change() {
        let mut collector = TelemetryCollector::new(TelemetryMode::Disabled);
        assert_eq!(collector.mode(), TelemetryMode::Disabled);
        collector.set_mode(TelemetryMode::OptIn);
        assert_eq!(collector.mode(), TelemetryMode::OptIn);
        collector.record(TelemetryEvent::Lifecycle {
            event: "test".into(),
        });
        assert_eq!(collector.events().len(), 1);
    }

    #[test]
    fn telemetry_privacy_filter_email() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::Lifecycle {
            event: "user@example.com logged in".into(),
        });
        collector.privacy_filter();
        if let TelemetryEvent::Lifecycle { event } = &collector.events()[0] {
            assert_eq!(event, "[redacted]");
        } else {
            panic!("expected lifecycle event");
        }
    }

    #[test]
    fn telemetry_privacy_filter_path() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::Performance {
            metric: "/home/user/documents/report.txt".into(),
            value: 1.0,
        });
        collector.privacy_filter();
        if let TelemetryEvent::Performance { metric, .. } = &collector.events()[0] {
            assert_eq!(metric, "[redacted]");
        } else {
            panic!("expected performance event");
        }
    }

    #[test]
    fn telemetry_privacy_filter_safe_string() {
        let mut collector = TelemetryCollector::new(TelemetryMode::LocalOnly);
        collector.record(TelemetryEvent::FeatureUsage {
            feature: "search".into(),
            count: 1,
        });
        collector.privacy_filter();
        if let TelemetryEvent::FeatureUsage { feature, .. } = &collector.events()[0] {
            assert_eq!(feature, "search");
        } else {
            panic!("expected feature usage event");
        }
    }

    #[test]
    fn telemetry_default_is_disabled() {
        let collector = TelemetryCollector::default();
        assert_eq!(collector.mode(), TelemetryMode::Disabled);
    }

    #[test]
    fn frame_timeline_default() {
        let timeline = FrameTimeline::default();
        assert!(timeline.is_empty());
    }

    #[test]
    fn frame_timeline_single_frame() {
        let mut timeline = FrameTimeline::new();
        timeline.record(make_frame(1, 16_000));
        assert_eq!(timeline.len(), 1);
        let avg = timeline.average_duration_us().unwrap();
        assert!((avg - 16_000.0).abs() < 0.1);
        assert_eq!(timeline.p95_duration_us().unwrap(), 16_000);
        assert_eq!(timeline.p99_duration_us().unwrap(), 16_000);
    }

    #[test]
    fn inspector_find_by_id_root() {
        let mut inspector = ElementInspector::new();
        inspector.build_tree(sample_tree());
        let root = inspector.find_by_id("root").unwrap();
        assert_eq!(root.element_type, "Div");
    }

    #[test]
    fn overlay_manager_remove_nonexistent() {
        let mut mgr = OverlayManager::new();
        mgr.add(LayoutOverlay {
            element_id: "x".into(),
            bounds: (0.0, 0.0, 1.0, 1.0),
            margin: (0.0, 0.0, 0.0, 0.0),
            padding: (0.0, 0.0, 0.0, 0.0),
            label: None,
        });
        mgr.remove("nonexistent");
        assert_eq!(mgr.list().len(), 1);
    }

    #[test]
    fn inspector_rejects_duplicate_ids_and_excessive_depth() {
        let mut duplicate = sample_tree();
        duplicate.children[1].id = "header".into();
        let mut inspector = ElementInspector::new();
        assert!(inspector.build_tree_checked(duplicate).is_err());
        assert!(inspector.root().is_none());

        let mut tree = InspectedElement {
            id: format!("node-{MAX_INSPECTED_DEPTH}"),
            element_type: "Div".into(),
            bounds: None,
            styles: HashMap::new(),
            children: Vec::new(),
        };
        for depth in (0..MAX_INSPECTED_DEPTH).rev() {
            tree = InspectedElement {
                id: format!("node-{depth}"),
                element_type: "Div".into(),
                bounds: None,
                styles: HashMap::new(),
                children: vec![tree],
            };
        }
        assert!(inspector.build_tree_checked(tree).is_err());
    }

    #[test]
    fn frame_timeline_bounds_capacity_and_overflow_safe_average() {
        assert!(FrameTimeline::with_capacity_checked(0).is_err());
        assert!(FrameTimeline::with_capacity_checked(MAX_FRAME_TIMELINE_CAPACITY + 1).is_err());
        assert_eq!(
            FrameTimeline::with_capacity(usize::MAX).capacity,
            MAX_FRAME_TIMELINE_CAPACITY
        );

        let mut timeline = FrameTimeline::with_capacity(2);
        timeline.record(make_frame(1, u64::MAX));
        timeline.record(make_frame(2, u64::MAX));
        assert_eq!(timeline.average_duration_us(), Some(u64::MAX as f64));
    }

    #[test]
    fn diagnostic_ingestion_rejects_invalid_values_and_bounds_retention() {
        let mut jobs = JobQueueViewer::new();
        assert!(
            jobs.add_checked(JobSnapshot {
                id: "job".into(),
                name: "Job".into(),
                status: "running".into(),
                progress: Some(f32::NAN),
                queued_at: 2,
                started_at: Some(1),
                completed_at: None,
            })
            .is_err()
        );
        assert!(jobs.list().is_empty());

        let mut logs = LogViewer::new();
        assert!(
            logs.append_checked(LogEntry {
                timestamp: 0,
                level: LogLevel::Info,
                source: "source".into(),
                message: "bad\nmessage".into(),
            })
            .is_err()
        );
        for timestamp in 0..=MAX_LOG_ENTRIES as u64 {
            logs.append(LogEntry {
                timestamp,
                level: LogLevel::Info,
                source: "source".into(),
                message: "message".into(),
            });
        }
        assert_eq!(logs.count(), MAX_LOG_ENTRIES);
        assert_eq!(logs.entries[0].timestamp, 1);

        let mut telemetry = TelemetryCollector::new(TelemetryMode::LocalOnly);
        assert!(
            telemetry
                .record_checked(TelemetryEvent::Performance {
                    metric: "latency".into(),
                    value: f64::INFINITY,
                })
                .is_err()
        );
        assert!(telemetry.events().is_empty());
        telemetry.record(TelemetryEvent::Lifecycle {
            event: "started".into(),
        });
        telemetry.set_mode(TelemetryMode::Disabled);
        assert!(telemetry.events().is_empty());
    }
}
