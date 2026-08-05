//! Crash capture and persistence.

use std::{
    backtrace::Backtrace,
    collections::BinaryHeap,
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use anyhow::{Context as _, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::{
    breadcrumb::{Breadcrumb, BreadcrumbBuffer},
    native::{self, NativeContext, PendingNativeCrash},
};

type PanicHook = dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static;
type BeforeSend = dyn Fn(&mut CrashReport) -> bool + Sync + Send + 'static;

static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(1);
const MAX_REPORT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_APP_ID_BYTES: usize = 255;
const MAX_ENDPOINT_BYTES: usize = 16 * 1024;
const MAX_CONTEXT_BYTES: usize = 4 * 1024;
const MAX_ERROR_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_BACKTRACE_BYTES: usize = 6 * 1024 * 1024;
const MAX_PENDING_REPORTS_PER_BATCH: usize = 256;

/// Information about the host operating system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsInfo {
    /// The operating system name.
    pub name: String,
    /// The operating system version if known.
    pub version: String,
    /// The CPU architecture.
    pub arch: String,
    /// The locale if known.
    pub locale: String,
    /// The host name if known.
    pub hostname: String,
}

/// Information persisted for a crash report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashReport {
    /// The human-readable error message.
    pub message: String,
    /// The captured backtrace.
    pub backtrace: String,
    /// Information about the host operating system.
    pub os_info: OsInfo,
    /// The application release identifier, if one was configured.
    pub app_version: Option<String>,
    /// The deployment environment, if one was configured.
    pub environment: Option<String>,
    /// Breadcrumbs captured leading up to the crash.
    pub breadcrumbs: Vec<Breadcrumb>,
}

/// User-consent policy governing crash report submission, mirroring the
/// release `UpdatePolicy` style. The reporter never submits anything unless
/// the application has opted in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CrashConsent {
    /// Whether the application may submit collected crash reports at all.
    pub submit_enabled: bool,
}

impl CrashConsent {
    /// Consent granted: collected reports may be submitted.
    pub const fn granted() -> Self {
        Self {
            submit_enabled: true,
        }
    }

    /// Consent withheld (the default): reports are retained on disk but never
    /// submitted.
    pub const fn withheld() -> Self {
        Self {
            submit_enabled: false,
        }
    }
}

impl Default for CrashConsent {
    fn default() -> Self {
        Self::withheld()
    }
}

/// Summary of prior crashes detected at startup by
/// [`CrashReporter::check_and_submit_pending`].
#[derive(Debug, Clone, Default)]
pub struct PriorCrashSummary {
    /// Number of native crashes with a decoded signal record.
    pub native_crashes: usize,
    /// Number of prior sessions that exited uncleanly without a native record.
    pub unclean_exits: usize,
    /// Whether reports were submitted (requires consent and a configured
    /// endpoint + HTTP client).
    pub submitted: bool,
    /// Human-readable one-line summaries of each detected crash.
    pub messages: Vec<String>,
}

impl PriorCrashSummary {
    /// Whether any prior crash or unclean exit was detected.
    pub fn detected_any(&self) -> bool {
        self.native_crashes > 0 || self.unclean_exits > 0
    }
}

/// A crash reporter that captures Rust panics and persists reports to disk.
pub struct CrashReporter {
    reports_dir: PathBuf,
    endpoint: Option<String>,
    http_client: Option<Arc<dyn http_client::HttpClient>>,
    hook_enabled: Option<Arc<AtomicBool>>,
    before_send: Option<Arc<BeforeSend>>,
    breadcrumbs: BreadcrumbBuffer,
    release: Option<String>,
    environment: Option<String>,
    session_id: String,
    native_installed: bool,
}

impl std::fmt::Debug for CrashReporter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrashReporter")
            .field("reports_dir", &self.reports_dir)
            .field("has_endpoint", &self.endpoint.is_some())
            .field("has_http_client", &self.http_client.is_some())
            .finish_non_exhaustive()
    }
}

impl CrashReporter {
    /// Creates a new crash reporter for the given application identifier.
    pub fn new(app_id: impl Into<String>, breadcrumbs: BreadcrumbBuffer) -> Result<Self> {
        let app_id = app_id.into();
        validate_app_id(&app_id)?;
        let reports_dir = crash_reports_dir(&app_id)?;
        prepare_reports_dir(&reports_dir)?;

        Ok(Self {
            reports_dir,
            endpoint: None,
            http_client: None,
            hook_enabled: None,
            before_send: None,
            breadcrumbs,
            release: None,
            environment: None,
            session_id: crate::native::new_session_id(),
            native_installed: false,
        })
    }

    /// Returns the directory where pending crash reports are stored.
    pub fn reports_dir(&self) -> &Path {
        &self.reports_dir
    }

    /// Overrides the directory used for crash report and native artifact
    /// storage. Must be called before [`install_hook`](Self::install_hook) or
    /// [`install_native`](Self::install_native).
    pub fn set_reports_dir(&mut self, reports_dir: impl Into<PathBuf>) -> Result<()> {
        if self.hook_enabled.is_some() || self.native_installed {
            return Err(anyhow!(
                "crash reports directory cannot change after a crash handler is installed"
            ));
        }
        let reports_dir = reports_dir.into();
        prepare_reports_dir(&reports_dir)?;
        self.reports_dir = reports_dir;
        Ok(())
    }

    /// Sets the HTTPS endpoint used for deferred crash report submission.
    ///
    /// Credential-bearing URLs, query strings, and URL fragments are rejected
    /// because crash reports can contain sensitive application and host
    /// information. Configure authentication on the HTTP client instead.
    pub fn set_endpoint(&mut self, endpoint: impl Into<String>) -> Result<()> {
        let endpoint = endpoint.into();
        validate_endpoint(&endpoint)?;
        self.endpoint = Some(endpoint);
        Ok(())
    }

    /// Sets the HTTP client used for deferred crash report submission.
    pub fn set_http_client(&mut self, client: Arc<dyn http_client::HttpClient>) {
        self.http_client = Some(client);
    }

    /// Sets the release string included in emitted crash reports.
    pub fn set_release(&mut self, release: impl Into<String>) {
        self.release = Some(truncate_text(release.into(), MAX_CONTEXT_BYTES));
    }

    /// Sets the environment string included in emitted crash reports.
    pub fn set_environment(&mut self, environment: impl Into<String>) {
        self.environment = Some(truncate_text(environment.into(), MAX_CONTEXT_BYTES));
    }

    /// Sets an optional hook that can mutate or drop crash reports before they are persisted.
    pub fn set_before_send(&mut self, before_send: Arc<BeforeSend>) {
        self.before_send = Some(before_send);
    }

    /// Installs a panic hook that persists crash reports.
    pub fn install_hook(&mut self) {
        if self.hook_enabled.is_some() {
            return;
        }
        let reports_dir = self.reports_dir.clone();
        let breadcrumbs = self.breadcrumbs.clone();
        let release = self.release.clone();
        let environment = self.environment.clone();
        let before_send = self.before_send.clone();
        let previous: Arc<PanicHook> = std::panic::take_hook().into();
        let enabled = Arc::new(AtomicBool::new(true));
        self.hook_enabled = Some(enabled.clone());

        std::panic::set_hook(Box::new(move |info| {
            if enabled.load(Ordering::Relaxed) {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut report = capture_crash_report(
                        info,
                        breadcrumbs.snapshot(),
                        release.clone(),
                        environment.clone(),
                    );
                    let should_send = before_send.as_ref().is_none_or(|before_send| {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            before_send(&mut report)
                        }))
                        .unwrap_or(false)
                    });

                    if should_send {
                        let _ = write_crash_report(&reports_dir, &report);
                    }
                }));
            }
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| previous(info)));
        }));
    }

    /// Disables this reporter's panic capture without replacing hooks that may
    /// have been installed later. The chaining wrapper remains process-wide
    /// and continues to invoke the hook that preceded it.
    pub fn uninstall_hook(&mut self) {
        if let Some(enabled) = self.hook_enabled.take() {
            enabled.store(false, Ordering::Relaxed);
        }
    }

    /// Returns the identifier for the current process session.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Installs OS-level native crash handlers (opt-in).
    ///
    /// This complements [`install_hook`](Self::install_hook): the panic hook
    /// only captures Rust panics, while native handlers capture hardware faults
    /// (SIGSEGV/SIGBUS/SIGILL/SIGFPE), aborts (SIGABRT), and foreign-code
    /// crashes. Pre-crash context (app version, environment, OS, session id) is
    /// captured here, at install time, into a pre-opened artifact; the handler
    /// itself only writes an async-signal-safe record.
    ///
    /// Returns `true` if handlers were installed by this call, `false` if they
    /// were already installed in this process.
    pub fn install_native(&mut self) -> Result<bool> {
        let started_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();

        let context = NativeContext {
            session_id: self.session_id.clone(),
            app_version: self.release.clone(),
            environment: self.environment.clone(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            pid: std::process::id(),
            started_at_ms,
        };

        let installed = native::install(&self.reports_dir, context)?;
        self.native_installed = installed || self.native_installed;
        Ok(installed)
    }

    /// Marks this session as a clean exit, removing the native crash marker so
    /// the next launch does not treat this run as an unclean exit. Call during
    /// orderly shutdown when native handlers are installed.
    pub fn mark_clean_exit(&self) -> Result<()> {
        if self.native_installed {
            native::mark_clean_exit(&self.reports_dir)?;
        }
        Ok(())
    }

    /// Decodes native crashes left by previous unclean exits without submitting
    /// them. Each entry describes one prior session.
    pub fn pending_native_crashes(&self) -> Result<Vec<PendingNativeCrash>> {
        native::pending_crashes(&self.reports_dir)
    }

    /// Persists a crash report for a non-panic error.
    pub fn capture_error(&self, error: &dyn std::error::Error) -> Result<PathBuf> {
        let mut report = CrashReport {
            message: truncate_text(error.to_string(), MAX_ERROR_MESSAGE_BYTES),
            backtrace: truncate_text(format!("{}", Backtrace::capture()), MAX_BACKTRACE_BYTES),
            os_info: collect_os_info(),
            app_version: self.release.clone(),
            environment: self.environment.clone(),
            breadcrumbs: self.breadcrumbs.snapshot(),
        };

        let should_send = run_before_send(self.before_send.as_ref(), &mut report);

        if !should_send {
            return Err(anyhow!("crash report was dropped by before_send"));
        }

        write_crash_report(&self.reports_dir, &report)
    }

    /// Lists the pending crash report files.
    pub fn pending_reports(&self) -> Result<Vec<PathBuf>> {
        let mut reports = BinaryHeap::with_capacity(MAX_PENDING_REPORTS_PER_BATCH + 1);
        if !self.reports_dir.exists() {
            return Ok(Vec::new());
        }

        for entry in fs::read_dir(&self.reports_dir).with_context(|| {
            format!(
                "failed to read reports directory: {}",
                self.reports_dir.display()
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_file() {
                let Some(sort_key) = crash_report_sort_key(&path) else {
                    continue;
                };
                reports.push((sort_key, path));
                if reports.len() > MAX_PENDING_REPORTS_PER_BATCH {
                    reports.pop();
                }
            }
        }

        let mut reports = reports.into_vec();
        reports.sort_by(|(left_key, left_path), (right_key, right_path)| {
            left_key
                .cmp(right_key)
                .then_with(|| left_path.cmp(right_path))
        });
        Ok(reports.into_iter().map(|(_, path)| path).collect())
    }

    /// Attempts to upload all pending crash reports.
    pub async fn submit_pending_reports(&self) -> Result<()> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| anyhow!("no crash report endpoint configured"))?;
        validate_endpoint(endpoint)?;
        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| anyhow!("no HTTP client configured for crash reporter"))?;

        for path in self.pending_reports()? {
            let json = read_crash_report(&path)?;

            let response = client
                .post_json(endpoint, json.into())
                .await
                .with_context(|| format!("failed to submit crash report: {}", path.display()))?;

            if !response.status().is_success() {
                return Err(anyhow!(
                    "crash report submission returned HTTP {} for {}",
                    response.status(),
                    path.display()
                ));
            }
            fs::remove_file(&path).with_context(|| {
                format!(
                    "failed to remove submitted crash report: {}",
                    path.display()
                )
            })?;
        }

        Ok(())
    }

    /// Detects native crashes and unclean exits left by previous runs, converts
    /// them into JSON crash reports in the report directory, and (only when
    /// `consent` permits) submits all pending reports through the existing HTTP
    /// path.
    ///
    /// Always returns a [`PriorCrashSummary`] describing what was found, even
    /// when consent is withheld or no endpoint is configured; in those cases
    /// reports are converted and retained on disk but not submitted.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # async fn run() -> anyhow::Result<()> {
    /// use kael_diagnostics::{BreadcrumbBuffer, CrashConsent, CrashReporter};
    ///
    /// let mut reporter = CrashReporter::new("com.example.app", BreadcrumbBuffer::new(64))?;
    /// reporter.set_release("1.2.3");
    /// reporter.install_native()?;
    ///
    /// // On the next launch, surface and (with consent) submit prior crashes.
    /// let summary = reporter
    ///     .check_and_submit_pending(CrashConsent::granted())
    ///     .await?;
    /// if summary.detected_any() {
    ///     for message in &summary.messages {
    ///         eprintln!("prior crash: {message}");
    ///     }
    /// }
    ///
    /// // ... run the app; on orderly shutdown:
    /// reporter.mark_clean_exit()?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn check_and_submit_pending(
        &self,
        consent: CrashConsent,
    ) -> Result<PriorCrashSummary> {
        let mut summary = PriorCrashSummary::default();

        for crash in self.pending_native_crashes()? {
            let message = crash.summary();
            summary.messages.push(message.clone());
            if crash.has_native_record() {
                summary.native_crashes += 1;
            } else {
                summary.unclean_exits += 1;
            }

            let mut report = self.report_from_native(&crash);
            let should_keep = run_before_send(self.before_send.as_ref(), &mut report);

            if should_keep {
                write_crash_report(&self.reports_dir, &report)?;
            }
            native::clear_crash(&crash)?;
        }

        if consent.submit_enabled && self.endpoint.is_some() && self.http_client.is_some() {
            self.submit_pending_reports().await?;
            summary.submitted = true;
        }

        Ok(summary)
    }

    fn report_from_native(&self, crash: &PendingNativeCrash) -> CrashReport {
        let mut os_info = collect_os_info();
        os_info.name = crash.context.os.clone();
        os_info.arch = crash.context.arch.clone();

        CrashReport {
            message: crash.summary(),
            backtrace: crash.backtrace_text(),
            os_info,
            app_version: crash.context.app_version.clone(),
            environment: crash.context.environment.clone(),
            breadcrumbs: Vec::new(),
        }
    }
}

/// Captures a crash report from panic information.
pub fn capture_crash_report(
    info: &std::panic::PanicHookInfo<'_>,
    breadcrumbs: Vec<Breadcrumb>,
    release: Option<String>,
    environment: Option<String>,
) -> CrashReport {
    let message = info
        .payload()
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            info.payload()
                .downcast_ref::<&str>()
                .map(|message| message.to_string())
        })
        .unwrap_or_else(|| "unknown panic".to_string());

    CrashReport {
        message: truncate_text(message, MAX_ERROR_MESSAGE_BYTES),
        backtrace: truncate_text(format!("{}", Backtrace::capture()), MAX_BACKTRACE_BYTES),
        os_info: collect_os_info(),
        app_version: release,
        environment,
        breadcrumbs,
    }
}

/// Persists a crash report to disk.
pub fn write_crash_report(dir: &Path, report: &CrashReport) -> Result<PathBuf> {
    prepare_reports_dir(dir)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_REPORT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| anyhow!("crash report identifier space exhausted"))?;
    let process_id = std::process::id();
    let filename = format!("crash_report_{timestamp}_{process_id}_{sequence}.json");
    let path = dir.join(filename);
    let mut json = BoundedJsonWriter::new(MAX_REPORT_BYTES as usize);
    if let Err(error) = serde_json::to_writer_pretty(&mut json, report) {
        if json.exceeded_limit() {
            return Err(anyhow!(
                "serialized crash report exceeds {MAX_REPORT_BYTES} byte limit"
            ));
        }
        return Err(error).context("failed to serialize crash report");
    }

    let mut file = tempfile::Builder::new()
        .prefix(".kael-crash-report-")
        .tempfile_in(dir)
        .with_context(|| {
            format!(
                "failed to create temporary crash report in {}",
                dir.display()
            )
        })?;
    file.write_all(json.as_bytes())
        .with_context(|| format!("failed to write crash report for {}", path.display()))?;
    file.as_file()
        .sync_all()
        .with_context(|| format!("failed to sync crash report for {}", path.display()))?;
    file.persist_noclobber(&path).map_err(|error| {
        anyhow::Error::new(error.error).context(format!(
            "failed to finalize crash report: {}",
            path.display()
        ))
    })?;
    sync_parent_dir(dir)?;

    Ok(path)
}

pub(crate) struct BoundedJsonWriter {
    bytes: Vec<u8>,
    limit: usize,
    exceeded_limit: bool,
}

impl BoundedJsonWriter {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded_limit: false,
        }
    }

    pub(crate) fn exceeded_limit(&self) -> bool {
        self.exceeded_limit
    }

    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl std::io::Write for BoundedJsonWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let remaining = self.limit.saturating_sub(self.bytes.len());
        if remaining == 0 && !bytes.is_empty() {
            self.exceeded_limit = true;
            return Err(std::io::Error::other("crash report byte limit exceeded"));
        }
        let written = remaining.min(bytes.len());
        self.bytes.extend_from_slice(&bytes[..written]);
        if written < bytes.len() {
            self.exceeded_limit = true;
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Collects basic operating-system information for a crash report.
pub fn collect_os_info() -> OsInfo {
    let hostname = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_default();
    let locale = std::env::var("LANG").unwrap_or_default();

    OsInfo {
        name: std::env::consts::OS.to_string(),
        version: String::new(),
        arch: std::env::consts::ARCH.to_string(),
        locale: truncate_text(locale, MAX_CONTEXT_BYTES),
        hostname: truncate_text(hostname, MAX_CONTEXT_BYTES),
    }
}

fn crash_reports_dir(app_id: &str) -> Result<PathBuf> {
    let base = base_data_dir()?;
    Ok(base.join(app_id).join("crash_reports"))
}

fn validate_app_id(app_id: &str) -> Result<()> {
    let bytes = app_id.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= MAX_APP_ID_BYTES
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !matches!(bytes.first(), Some(b'.'))
        && !matches!(bytes.last(), Some(b'.'))
        && !is_windows_reserved_identifier(app_id);
    if !valid {
        return Err(anyhow!(
            "application identifier must be a portable 1-{MAX_APP_ID_BYTES} byte ASCII identifier"
        ));
    }
    Ok(())
}

fn is_windows_reserved_identifier(identifier: &str) -> bool {
    let base = identifier
        .split('.')
        .next()
        .unwrap_or(identifier)
        .trim_end_matches([' ', '.']);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| base.eq_ignore_ascii_case(reserved))
    {
        return true;
    }

    let bytes = base.as_bytes();
    bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    if endpoint.is_empty()
        || endpoint.len() > MAX_ENDPOINT_BYTES
        || endpoint.trim() != endpoint
        || endpoint.chars().any(char::is_control)
    {
        return Err(anyhow!(
            "crash report endpoint must be a non-empty URL of at most {MAX_ENDPOINT_BYTES} bytes without surrounding whitespace or control characters"
        ));
    }

    let parsed = http_client::Url::parse(endpoint).context("crash report endpoint is invalid")?;
    if parsed.scheme() != "https" {
        return Err(anyhow!("crash report endpoint must use https"));
    }
    if parsed.host_str().is_none() {
        return Err(anyhow!("crash report endpoint must include a host"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(anyhow!("crash report endpoint cannot contain credentials"));
    }
    if parsed.fragment().is_some() {
        return Err(anyhow!("crash report endpoint cannot contain a fragment"));
    }
    if parsed.query().is_some() {
        return Err(anyhow!(
            "crash report endpoint cannot contain a query; configure credentials on the HTTP client"
        ));
    }
    Ok(())
}

#[cfg(test)]
fn is_crash_report_path(path: &Path) -> bool {
    crash_report_sort_key(path).is_some()
}

fn crash_report_sort_key(path: &Path) -> Option<(u128, u64, u64)> {
    let Some(stem) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("crash_report_"))
        .and_then(|name| name.strip_suffix(".json"))
    else {
        return None;
    };
    let parts = stem.split('_').collect::<Vec<_>>();
    match parts.as_slice() {
        [timestamp, sequence] => Some((timestamp.parse().ok()?, 0, sequence.parse().ok()?)),
        [timestamp, process_id, sequence] => Some((
            timestamp.parse().ok()?,
            process_id.parse().ok()?,
            sequence.parse().ok()?,
        )),
        _ => None,
    }
}

fn read_crash_report(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect crash report: {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_REPORT_BYTES {
        return Err(anyhow!(
            "crash report must be a regular file of at most {MAX_REPORT_BYTES} bytes: {}",
            path.display()
        ));
    }

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .with_context(|| format!("failed to open crash report: {}", path.display()))?;
    let opened_metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect crash report: {}", path.display()))?;
    if !opened_metadata.is_file() || opened_metadata.len() > MAX_REPORT_BYTES {
        return Err(anyhow!(
            "crash report changed while opening: {}",
            path.display()
        ));
    }

    let mut bytes = Vec::with_capacity(opened_metadata.len() as usize);
    file.take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read crash report: {}", path.display()))?;
    if bytes.len() as u64 > MAX_REPORT_BYTES {
        return Err(anyhow!(
            "crash report exceeds {MAX_REPORT_BYTES} byte limit: {}",
            path.display()
        ));
    }
    String::from_utf8(bytes)
        .with_context(|| format!("crash report is not UTF-8: {}", path.display()))
}

fn truncate_text(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }
    let mut boundary = max_bytes;
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text.truncate(boundary);
    text
}

pub(crate) fn prepare_reports_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| {
        format!(
            "failed to create crash reports directory: {}",
            dir.display()
        )
    })?;
    let metadata = fs::symlink_metadata(dir).with_context(|| {
        format!(
            "failed to inspect crash reports directory: {}",
            dir.display()
        )
    })?;
    if !metadata.file_type().is_dir() {
        return Err(anyhow!(
            "crash reports path must be a real directory: {}",
            dir.display()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = metadata.permissions();
        if permissions.mode() & 0o777 != 0o700 {
            permissions.set_mode(0o700);
            fs::set_permissions(dir, permissions).with_context(|| {
                format!(
                    "failed to restrict crash reports directory permissions: {}",
                    dir.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(crate) fn sync_parent_dir(dir: &Path) -> Result<()> {
    fs::File::open(dir)
        .with_context(|| format!("failed to open crash reports directory: {}", dir.display()))?
        .sync_all()
        .with_context(|| format!("failed to sync crash reports directory: {}", dir.display()))
}

#[cfg(not(unix))]
pub(crate) fn sync_parent_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

fn run_before_send(before_send: Option<&Arc<BeforeSend>>, report: &mut CrashReport) -> bool {
    before_send.is_none_or(|before_send| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| before_send(report)))
            .unwrap_or(false)
    })
}

#[cfg(target_os = "macos")]
fn base_data_dir() -> Result<PathBuf> {
    let home =
        std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME environment variable not set"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support"))
}

#[cfg(target_os = "windows")]
fn base_data_dir() -> Result<PathBuf> {
    let app_data = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .ok_or_else(|| anyhow!("LOCALAPPDATA or APPDATA environment variable not set"))?;
    Ok(PathBuf::from(app_data))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn base_data_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or_else(|| anyhow!("XDG_DATA_HOME or HOME environment variable not set"))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::tempdir;

    use std::path::Path;

    use crate::breadcrumb::{Breadcrumb, BreadcrumbBuffer, Level};

    use super::{CrashConsent, CrashReport, CrashReporter, collect_os_info, write_crash_report};

    fn reporter_in(directory: &Path) -> CrashReporter {
        CrashReporter {
            reports_dir: directory.to_path_buf(),
            endpoint: None,
            http_client: None,
            hook_enabled: None,
            before_send: None,
            breadcrumbs: BreadcrumbBuffer::new(8),
            release: Some("9.9.9".to_string()),
            environment: Some("test".to_string()),
            session_id: crate::native::new_session_id(),
            native_installed: false,
        }
    }

    fn write_native_artifacts(directory: &Path, session: &str, dump: Option<&str>) {
        let meta = format!(
            r#"{{"session_id":"{session}","app_version":"1.0.0","environment":"prod","os":"macos","arch":"aarch64","pid":1234,"started_at_ms":0}}"#
        );
        std::fs::write(directory.join(format!("{session}.crashmeta.json")), meta).unwrap();
        if let Some(dump) = dump {
            std::fs::write(directory.join(format!("{session}.crashdump")), dump).unwrap();
        }
    }

    #[test]
    fn round_trips_crash_report_json() {
        let directory = tempdir().unwrap();
        let report = CrashReport {
            message: "test panic".to_string(),
            backtrace: "frame1\nframe2".to_string(),
            os_info: collect_os_info(),
            app_version: Some("1.0.0".to_string()),
            environment: Some("test".to_string()),
            breadcrumbs: vec![Breadcrumb {
                category: "test".to_string(),
                message: "boot".to_string(),
                level: Level::Info,
                timestamp: std::time::SystemTime::UNIX_EPOCH,
                data: HashMap::new(),
            }],
        };

        let path = write_crash_report(directory.path(), &report).unwrap();
        assert!(super::is_crash_report_path(&path));
        let json = std::fs::read_to_string(path).unwrap();
        let loaded: CrashReport = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.message, report.message);
        assert_eq!(loaded.environment, report.environment);
        assert_eq!(loaded.breadcrumbs.len(), 1);
    }

    #[test]
    fn lists_pending_json_reports() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("crash_report_1_1.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("crash_report_2_1.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("crash_report_secret.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("current.crashmeta.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("unrelated.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("ignored.txt"), b"ignore").unwrap();

        let reporter = reporter_in(directory.path());

        let pending = reporter.pending_reports().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("crash_report_")
        }));
    }

    #[test]
    fn validates_and_redacts_upload_endpoints() {
        let directory = tempdir().unwrap();
        let mut reporter = reporter_in(directory.path());

        assert!(reporter.set_endpoint("http://example.com/crashes").is_err());
        assert!(
            reporter
                .set_endpoint("https://user:secret@example.com/crashes")
                .is_err()
        );
        assert!(
            reporter
                .set_endpoint("https://example.com/crashes?token=secret")
                .is_err()
        );
        reporter
            .set_endpoint("https://example.com/crashes")
            .unwrap();

        let debug = format!("{reporter:?}");
        assert!(debug.contains("has_endpoint: true"));
        assert!(!debug.contains("example.com"));
    }

    #[test]
    fn rejects_report_directory_changes_after_handler_installation() {
        let directory = tempdir().unwrap();
        let replacement = tempdir().unwrap();
        let mut reporter = reporter_in(directory.path());
        reporter.native_installed = true;

        assert!(reporter.set_reports_dir(replacement.path()).is_err());
        assert_eq!(reporter.reports_dir(), directory.path());
    }

    #[test]
    fn pending_report_batches_are_bounded_and_keep_oldest_names() {
        let directory = tempdir().unwrap();
        for index in 0..258 {
            std::fs::write(
                directory
                    .path()
                    .join(format!("crash_report_{index}_1.json")),
                b"{}",
            )
            .unwrap();
        }

        let reporter = reporter_in(directory.path());
        let pending = reporter.pending_reports().unwrap();
        assert_eq!(pending.len(), super::MAX_PENDING_REPORTS_PER_BATCH);
        assert_eq!(
            pending.last().unwrap().file_name().unwrap(),
            "crash_report_255_1.json"
        );
    }

    #[cfg(unix)]
    #[test]
    fn report_storage_uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let report = CrashReport {
            message: "private".to_string(),
            backtrace: String::new(),
            os_info: collect_os_info(),
            app_version: None,
            environment: None,
            breadcrumbs: Vec::new(),
        };

        let path = write_crash_report(directory.path(), &report).unwrap();

        assert_eq!(
            std::fs::metadata(directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn rejects_path_like_application_identifiers() {
        let breadcrumbs = BreadcrumbBuffer::new(1);
        for app_id in [
            "../escape",
            "nested/app",
            ".hidden",
            "trailing.",
            "CON",
            "con.app",
            "COM1",
            "lpt9.app",
        ] {
            assert!(CrashReporter::new(app_id, breadcrumbs.clone()).is_err());
        }
        for app_id in ["com.example.app", "COM10", "LPT0"] {
            assert!(super::validate_app_id(app_id).is_ok());
        }
    }

    #[test]
    fn serialized_reports_are_bounded_before_persistence() {
        let directory = tempdir().unwrap();
        let report = CrashReport {
            message: "x".repeat(super::MAX_REPORT_BYTES as usize),
            backtrace: String::new(),
            os_info: collect_os_info(),
            app_version: None,
            environment: None,
            breadcrumbs: Vec::new(),
        };

        assert!(write_crash_report(directory.path(), &report).is_err());
        assert!(
            std::fs::read_dir(directory.path())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn panicking_before_send_drops_non_panic_report_without_unwinding() {
        let directory = tempdir().unwrap();
        let mut reporter = reporter_in(directory.path());
        reporter.set_before_send(std::sync::Arc::new(|_| panic!("callback failed")));
        let error = std::io::Error::other("captured");
        assert!(reporter.capture_error(&error).is_err());
        assert!(reporter.pending_reports().unwrap().is_empty());
    }

    #[test]
    fn marker_protocol_distinguishes_crash_from_unclean_exit() {
        let directory = tempdir().unwrap();
        write_native_artifacts(
            directory.path(),
            "sessioncrash",
            Some("signal=11\ncode=1\naddress=0x10\nframe=0x4001\nframe=0x4002\n"),
        );
        write_native_artifacts(directory.path(), "sessionunclean", None);

        let reporter = reporter_in(directory.path());
        let pending = reporter.pending_native_crashes().unwrap();
        assert_eq!(pending.len(), 2);

        let crash = pending
            .iter()
            .find(|crash| crash.context.session_id == "sessioncrash")
            .unwrap();
        assert!(crash.has_native_record());
        assert!(crash.summary().contains("SIGSEGV"));

        let unclean = pending
            .iter()
            .find(|crash| crash.context.session_id == "sessionunclean")
            .unwrap();
        assert!(!unclean.has_native_record());
        assert!(unclean.summary().contains("unclean exit"));
    }

    #[test]
    fn check_and_submit_converts_native_crash_and_retains_without_consent() {
        let directory = tempdir().unwrap();
        write_native_artifacts(
            directory.path(),
            "abc123",
            Some("signal=11\naddress=0xdead\nframe=0x1000\n"),
        );

        let reporter = reporter_in(directory.path());
        let summary =
            pollster::block_on(reporter.check_and_submit_pending(CrashConsent::withheld()))
                .unwrap();

        assert_eq!(summary.native_crashes, 1);
        assert_eq!(summary.unclean_exits, 0);
        assert!(!summary.submitted);
        assert!(summary.detected_any());

        let json_reports = reporter.pending_reports().unwrap();
        assert_eq!(json_reports.len(), 1);
        let report: CrashReport =
            serde_json::from_str(&std::fs::read_to_string(&json_reports[0]).unwrap()).unwrap();
        assert!(report.message.contains("SIGSEGV"));
        assert_eq!(report.app_version.as_deref(), Some("1.0.0"));
        assert!(report.backtrace.contains("0x1000"));

        assert!(
            reporter.pending_native_crashes().unwrap().is_empty(),
            "native artifacts should be cleared after conversion"
        );
    }

    #[test]
    fn check_and_submit_reports_no_prior_crash_on_clean_dir() {
        let directory = tempdir().unwrap();
        let reporter = reporter_in(directory.path());
        let summary =
            pollster::block_on(reporter.check_and_submit_pending(CrashConsent::granted())).unwrap();
        assert!(!summary.detected_any());
        assert!(!summary.submitted);
    }

    #[test]
    fn captures_breadcrumbs_for_non_panic_errors() {
        let directory = tempdir().unwrap();
        let breadcrumbs = BreadcrumbBuffer::new(8);
        let breadcrumb = Breadcrumb {
            category: "test".to_string(),
            message: "before error".to_string(),
            level: Level::Warning,
            timestamp: std::time::SystemTime::UNIX_EPOCH,
            data: HashMap::new(),
        };
        breadcrumbs.push(breadcrumb.clone());

        let mut reporter = CrashReporter::new("dev.kael.tests", breadcrumbs).unwrap();
        reporter.reports_dir = directory.path().to_path_buf();
        let error = std::io::Error::other("boom");

        let path = reporter.capture_error(&error).unwrap();
        let json = std::fs::read_to_string(path).unwrap();
        let report: CrashReport = serde_json::from_str(&json).unwrap();

        assert_eq!(report.breadcrumbs, vec![breadcrumb]);
    }
}
