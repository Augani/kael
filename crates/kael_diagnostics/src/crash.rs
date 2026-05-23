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

use crate::breadcrumb::{Breadcrumb, BreadcrumbBuffer};

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
        })
    }

    /// Returns the directory where pending crash reports are stored.
    pub fn reports_dir(&self) -> &Path {
        &self.reports_dir
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

    use crate::breadcrumb::{Breadcrumb, BreadcrumbBuffer, Level};

    use super::{CrashReport, CrashReporter, collect_os_info, write_crash_report};

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

        let reporter = CrashReporter {
            reports_dir: directory.path().to_path_buf(),
            endpoint: None,
            http_client: None,
            previous_hook: None,
            before_send: None,
            breadcrumbs: BreadcrumbBuffer::new(8),
            release: None,
            environment: None,
        };

        let pending = reporter.pending_reports().unwrap();
        assert_eq!(pending.len(), 2);
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
