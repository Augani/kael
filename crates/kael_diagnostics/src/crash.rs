//! Crash capture and persistence.

use std::{
    backtrace::Backtrace,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
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
    previous_hook: Option<Arc<PanicHook>>,
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
            .field("endpoint", &self.endpoint)
            .field("has_http_client", &self.http_client.is_some())
            .finish_non_exhaustive()
    }
}

impl CrashReporter {
    /// Creates a new crash reporter for the given application identifier.
    pub fn new(app_id: impl Into<String>, breadcrumbs: BreadcrumbBuffer) -> Result<Self> {
        let app_id = app_id.into();
        let reports_dir = crash_reports_dir(&app_id)?;
        fs::create_dir_all(&reports_dir).with_context(|| {
            format!(
                "failed to create crash reports directory: {}",
                reports_dir.display()
            )
        })?;

        Ok(Self {
            reports_dir,
            endpoint: None,
            http_client: None,
            previous_hook: None,
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
    /// storage. Must be called before [`install_native`](Self::install_native).
    pub fn set_reports_dir(&mut self, reports_dir: impl Into<PathBuf>) -> Result<()> {
        let reports_dir = reports_dir.into();
        fs::create_dir_all(&reports_dir).with_context(|| {
            format!(
                "failed to create crash reports directory: {}",
                reports_dir.display()
            )
        })?;
        self.reports_dir = reports_dir;
        Ok(())
    }

    /// Sets the endpoint used for deferred crash report submission.
    pub fn set_endpoint(&mut self, endpoint: impl Into<String>) {
        self.endpoint = Some(endpoint.into());
    }

    /// Sets the HTTP client used for deferred crash report submission.
    pub fn set_http_client(&mut self, client: Arc<dyn http_client::HttpClient>) {
        self.http_client = Some(client);
    }

    /// Sets the release string included in emitted crash reports.
    pub fn set_release(&mut self, release: impl Into<String>) {
        self.release = Some(release.into());
    }

    /// Sets the environment string included in emitted crash reports.
    pub fn set_environment(&mut self, environment: impl Into<String>) {
        self.environment = Some(environment.into());
    }

    /// Sets an optional hook that can mutate or drop crash reports before they are persisted.
    pub fn set_before_send(&mut self, before_send: Arc<BeforeSend>) {
        self.before_send = Some(before_send);
    }

    /// Installs a panic hook that persists crash reports.
    pub fn install_hook(&mut self) {
        let reports_dir = self.reports_dir.clone();
        let breadcrumbs = self.breadcrumbs.clone();
        let release = self.release.clone();
        let environment = self.environment.clone();
        let before_send = self.before_send.clone();
        let previous: Arc<PanicHook> = std::panic::take_hook().into();
        self.previous_hook = Some(previous.clone());

        std::panic::set_hook(Box::new(move |info| {
            let mut report = capture_crash_report(
                info,
                breadcrumbs.snapshot(),
                release.clone(),
                environment.clone(),
            );
            let should_send = before_send
                .as_ref()
                .map(|before_send| before_send(&mut report))
                .unwrap_or(true);

            if should_send {
                let _ = write_crash_report(&reports_dir, &report);
            }

            previous(info);
        }));
    }

    /// Restores the previously installed panic hook.
    pub fn uninstall_hook(&mut self) {
        if let Some(previous) = self.previous_hook.take() {
            let _ = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| previous(info)));
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
    pub fn mark_clean_exit(&self) {
        if self.native_installed {
            native::mark_clean_exit(&self.reports_dir);
        }
    }

    /// Decodes native crashes left by previous unclean exits without submitting
    /// them. Each entry describes one prior session.
    pub fn pending_native_crashes(&self) -> Result<Vec<PendingNativeCrash>> {
        native::pending_crashes(&self.reports_dir)
    }

    /// Persists a crash report for a non-panic error.
    pub fn capture_error(&self, error: &dyn std::error::Error) -> Result<PathBuf> {
        let mut report = CrashReport {
            message: error.to_string(),
            backtrace: format!("{}", Backtrace::capture()),
            os_info: collect_os_info(),
            app_version: self.release.clone(),
            environment: self.environment.clone(),
            breadcrumbs: self.breadcrumbs.snapshot(),
        };

        let should_send = self
            .before_send
            .as_ref()
            .map(|before_send| before_send(&mut report))
            .unwrap_or(true);

        if !should_send {
            return Err(anyhow!("crash report was dropped by before_send"));
        }

        write_crash_report(&self.reports_dir, &report)
    }

    /// Lists the pending crash report files.
    pub fn pending_reports(&self) -> Result<Vec<PathBuf>> {
        let mut reports = Vec::new();
        if !self.reports_dir.exists() {
            return Ok(reports);
        }

        for entry in fs::read_dir(&self.reports_dir).with_context(|| {
            format!(
                "failed to read reports directory: {}",
                self.reports_dir.display()
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
                reports.push(path);
            }
        }

        reports.sort();
        Ok(reports)
    }

    /// Attempts to upload all pending crash reports.
    pub async fn submit_pending_reports(&self) -> Result<()> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| anyhow!("no crash report endpoint configured"))?;
        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| anyhow!("no HTTP client configured for crash reporter"))?;

        for path in self.pending_reports()? {
            let json = fs::read_to_string(&path)
                .with_context(|| format!("failed to read crash report: {}", path.display()))?;

            let response = client
                .post_json(endpoint, json.into())
                .await
                .with_context(|| format!("failed to submit crash report: {}", path.display()))?;

            if response.status().is_success() {
                let _ = fs::remove_file(&path);
            }
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
    /// reporter.mark_clean_exit();
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
            let should_keep = self
                .before_send
                .as_ref()
                .map(|before_send| before_send(&mut report))
                .unwrap_or(true);

            if should_keep {
                let _ = write_crash_report(&self.reports_dir, &report);
            }
            native::clear_crash(&crash);
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
        message,
        backtrace: format!("{}", Backtrace::capture()),
        os_info: collect_os_info(),
        app_version: release,
        environment,
        breadcrumbs,
    }
}

/// Persists a crash report to disk.
pub fn write_crash_report(dir: &Path, report: &CrashReport) -> Result<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed);
    let filename = format!("crash_report_{timestamp}_{sequence}.json");
    let path = dir.join(filename);
    let temp_path = path.with_extension("json.tmp");

    let json = serde_json::to_string_pretty(report).context("failed to serialize crash report")?;
    let mut file = fs::File::create(&temp_path).with_context(|| {
        format!(
            "failed to create temporary crash report file: {}",
            temp_path.display()
        )
    })?;
    file.write_all(json.as_bytes())
        .with_context(|| format!("failed to write crash report file: {}", temp_path.display()))?;
    file.flush()
        .with_context(|| format!("failed to flush crash report file: {}", temp_path.display()))?;
    fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "failed to finalize crash report file from {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;

    Ok(path)
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
        locale,
        hostname,
    }
}

fn crash_reports_dir(app_id: &str) -> Result<PathBuf> {
    let base = base_data_dir()?;
    Ok(base.join(app_id).join("crash_reports"))
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
            previous_hook: None,
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
        let json = std::fs::read_to_string(path).unwrap();
        let loaded: CrashReport = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.message, report.message);
        assert_eq!(loaded.environment, report.environment);
        assert_eq!(loaded.breadcrumbs.len(), 1);
    }

    #[test]
    fn lists_pending_json_reports() {
        let directory = tempdir().unwrap();
        std::fs::write(directory.path().join("crash_report_1.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("crash_report_2.json"), b"{}").unwrap();
        std::fs::write(directory.path().join("ignored.txt"), b"ignore").unwrap();

        let reporter = reporter_in(directory.path());

        let pending = reporter.pending_reports().unwrap();
        assert_eq!(pending.len(), 2);
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
