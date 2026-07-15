//! Global diagnostics configuration and reporting facade.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, anyhow};

use crate::{
    Breadcrumb, BreadcrumbBuffer, CrashReport, CrashReporter, Level, MetricsRegistry, Span, Tracer,
    Transaction,
};

type BeforeSend = dyn Fn(&mut CrashReport) -> bool + Send + Sync + 'static;

static GLOBAL_DIAGNOSTICS: OnceLock<Diagnostics> = OnceLock::new();

/// Configuration used to initialize diagnostics.
pub struct DiagnosticsConfig {
    /// The application identifier used for crash report storage.
    pub app_id: String,
    /// An optional crash-report submission endpoint.
    pub dsn: Option<String>,
    /// The current release identifier.
    pub release: String,
    /// The current deployment environment.
    pub environment: String,
    /// The maximum number of breadcrumbs to retain.
    pub max_breadcrumbs: usize,
    /// The trace sampling rate from 0.0 to 1.0.
    pub sample_rate: f64,
    /// An optional hook that can mutate or drop crash reports before persistence.
    pub before_send: Option<Box<BeforeSend>>,
    /// An optional HTTP client used for deferred crash report submission.
    pub http_client: Option<Arc<dyn http_client::HttpClient>>,
    /// Whether a panic hook should be installed during initialization.
    pub install_panic_hook: bool,
}

impl std::fmt::Debug for DiagnosticsConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagnosticsConfig")
            .field("app_id", &self.app_id)
            .field("dsn", &self.dsn)
            .field("release", &self.release)
            .field("environment", &self.environment)
            .field("max_breadcrumbs", &self.max_breadcrumbs)
            .field("sample_rate", &self.sample_rate)
            .field("has_before_send", &self.before_send.is_some())
            .field("has_http_client", &self.http_client.is_some())
            .field("install_panic_hook", &self.install_panic_hook)
            .finish()
    }
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            app_id: "kael.app".to_string(),
            dsn: None,
            release: env!("CARGO_PKG_VERSION").to_string(),
            environment: "development".to_string(),
            max_breadcrumbs: 100,
            sample_rate: 1.0,
            before_send: None,
            http_client: None,
            install_panic_hook: true,
        }
    }
}

/// The initialized diagnostics subsystem.
pub struct Diagnostics {
    breadcrumb_buffer: BreadcrumbBuffer,
    crash_reporter: CrashReporter,
    metrics: MetricsRegistry,
    tracer: Tracer,
    sample_rate: f64,
    sample_counter: AtomicU64,
}

impl Diagnostics {
    /// Creates diagnostics from a configuration.
    pub fn new(config: DiagnosticsConfig) -> Result<Self> {
        if !config.sample_rate.is_finite() || !(0.0..=1.0).contains(&config.sample_rate) {
            return Err(anyhow!(
                "trace sample rate must be finite and between 0.0 and 1.0"
            ));
        }
        let sample_rate = config.sample_rate;
        let breadcrumb_buffer = BreadcrumbBuffer::new(config.max_breadcrumbs);
        let mut crash_reporter = CrashReporter::new(config.app_id, breadcrumb_buffer.clone())?;
        crash_reporter.set_release(config.release);
        crash_reporter.set_environment(config.environment);

        if let Some(dsn) = config.dsn {
            crash_reporter.set_endpoint(dsn);
        }

        if let Some(client) = config.http_client {
            crash_reporter.set_http_client(client);
        }

        if let Some(before_send) = config.before_send {
            crash_reporter.set_before_send(Arc::from(before_send));
        }

        if config.install_panic_hook {
            crash_reporter.install_hook();
        }

        let tracer = Tracer::default();
        if sample_rate > 0.0 {
            tracer.enable();
            tracer.install_global();
        }

        Ok(Self {
            breadcrumb_buffer,
            crash_reporter,
            metrics: MetricsRegistry::default(),
            tracer,
            sample_rate,
            sample_counter: AtomicU64::new(0),
        })
    }

    /// Adds a breadcrumb to the in-memory buffer.
    pub fn add_breadcrumb(&self, breadcrumb: Breadcrumb) {
        self.breadcrumb_buffer.push(breadcrumb);
    }

    /// Captures a non-panic error report.
    pub fn capture_error(&self, error: &dyn std::error::Error) -> Result<()> {
        self.crash_reporter.capture_error(error)?;
        Ok(())
    }

    /// Starts a transaction tied to the configured tracer.
    pub fn start_transaction(&self, name: &str) -> Transaction {
        let tracer = self.should_sample().then(|| self.tracer.clone());
        Transaction::new(name, tracer)
    }

    /// Records a gauge value.
    pub fn record_gauge(&self, name: &str, value: f64) {
        self.metrics.record_gauge(name, value);
    }

    /// Records a counter delta.
    pub fn record_counter(&self, name: &str, delta: i64) {
        self.metrics.record_counter(name, delta);
    }

    /// Records a histogram sample.
    pub fn record_histogram(&self, name: &str, value: f64) {
        self.metrics.record_histogram(name, value);
    }

    /// Returns the shared tracer.
    pub fn tracer(&self) -> &Tracer {
        &self.tracer
    }

    /// Returns the in-memory metrics registry.
    pub fn metrics(&self) -> &MetricsRegistry {
        &self.metrics
    }

    /// Returns the crash reporter.
    pub fn crash_reporter(&self) -> &CrashReporter {
        &self.crash_reporter
    }

    fn should_sample(&self) -> bool {
        if self.sample_rate <= 0.0 {
            return false;
        }
        if self.sample_rate >= 1.0 {
            return true;
        }

        // SplitMix64 gives a stable, allocation-free distribution and avoids
        // adding a random-number dependency to a hot diagnostics path.
        let mut value = self.sample_counter.fetch_add(1, Ordering::Relaxed);
        value = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        let unit = (value as f64) / (u64::MAX as f64);
        unit < self.sample_rate
    }
}

/// Initializes the process-global diagnostics subsystem.
pub fn init(config: DiagnosticsConfig) -> Result<&'static Diagnostics> {
    if let Some(diagnostics) = GLOBAL_DIAGNOSTICS.get() {
        return Ok(diagnostics);
    }

    let diagnostics = Diagnostics::new(config)?;
    let _ = GLOBAL_DIAGNOSTICS.set(diagnostics);
    Ok(GLOBAL_DIAGNOSTICS
        .get()
        .expect("diagnostics must be initialized after set"))
}

/// Returns the process-global diagnostics subsystem if it has been initialized.
pub fn global() -> Option<&'static Diagnostics> {
    GLOBAL_DIAGNOSTICS.get()
}

/// Adds a breadcrumb to the process-global diagnostics subsystem.
pub fn add_breadcrumb(breadcrumb: Breadcrumb) {
    if let Some(diagnostics) = global() {
        diagnostics.add_breadcrumb(breadcrumb);
    }
}

/// Captures a non-panic error using the process-global diagnostics subsystem.
pub fn capture_error(error: &dyn std::error::Error) {
    if let Some(diagnostics) = global() {
        let _ = diagnostics.capture_error(error);
    }
}

/// Starts a transaction using the process-global diagnostics subsystem.
pub fn start_transaction(name: &str) -> Transaction {
    global()
        .map(|diagnostics| diagnostics.start_transaction(name))
        .unwrap_or_else(|| Transaction::new(name, None))
}

/// Records a gauge using the process-global diagnostics subsystem.
pub fn record_gauge(name: &str, value: f64) {
    if let Some(diagnostics) = global() {
        diagnostics.record_gauge(name, value);
    }
}

/// Records a counter using the process-global diagnostics subsystem.
pub fn record_counter(name: &str, delta: i64) {
    if let Some(diagnostics) = global() {
        diagnostics.record_counter(name, delta);
    }
}

/// Records a histogram sample using the process-global diagnostics subsystem.
pub fn record_histogram(name: &str, value: f64) {
    if let Some(diagnostics) = global() {
        diagnostics.record_histogram(name, value);
    }
}

/// Creates a breadcrumb with a default timestamp.
pub fn breadcrumb(
    category: impl Into<String>,
    message: impl Into<String>,
    level: Level,
) -> Breadcrumb {
    Breadcrumb {
        category: category.into(),
        message: message.into(),
        level,
        timestamp: std::time::SystemTime::now(),
        data: std::collections::HashMap::new(),
    }
}

/// Starts a span from a transaction convenience wrapper.
pub fn start_span(transaction: &Transaction, operation: &str) -> Span {
    transaction.start_span(operation)
}
