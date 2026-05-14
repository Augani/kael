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

use anyhow::{Context as _, Result};

use crate::{CrashReport, OsInfo};

type PanicHook = dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static;

static NEXT_REPORT_ID: AtomicU64 = AtomicU64::new(1);

/// A crash reporter that captures Rust panics, persists them to disk, and
/// attempts to submit them on the next launch.
///
/// The reporter installs a global panic hook via [`std::panic::set_hook`] that
/// serializes crash information to a platform-appropriate data directory.
/// On the following launch, stored reports can be submitted to a configured
/// HTTP endpoint.
pub struct CrashReporter {
    reports_dir: PathBuf,
    endpoint: Option<String>,
    http_client: Option<Arc<dyn http_client::HttpClient>>,
    previous_hook: Option<Arc<PanicHook>>,
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
    /// Create a new crash reporter for the given application identifier.
    ///
    /// The reporter does not install the panic hook automatically; call
    /// [`install_hook`] after configuring the endpoint and HTTP client.
    ///
    /// [`install_hook`]: Self::install_hook
    pub fn new(app_id: impl Into<String>) -> Result<Self> {
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
        })
    }

    /// Set the HTTP endpoint URL for crash report submission.
    pub fn set_endpoint(&mut self, endpoint: impl Into<String>) {
        self.endpoint = Some(endpoint.into());
    }

    /// Set the HTTP client used for submitting reports.
    pub fn set_http_client(&mut self, client: Arc<dyn http_client::HttpClient>) {
        self.http_client = Some(client);
    }

    /// Returns the directory where unsent crash reports are stored.
    pub fn reports_dir(&self) -> &Path {
        &self.reports_dir
    }

    /// Install the global panic hook.
    ///
    /// The hook captures the panic message and backtrace, writes a
    /// `CrashReport` JSON file to the reports directory, and then chains
    /// to any previously-installed hook.
    pub fn install_hook(&mut self) {
        let reports_dir = self.reports_dir.clone();
        let previous: Arc<PanicHook> = std::panic::take_hook().into();
        self.previous_hook = Some(previous.clone());

        std::panic::set_hook(Box::new(move |info| {
            let report = capture_crash_report(info);
            let _ = write_crash_report(&reports_dir, &report);
            previous(info);
        }));
    }

    /// Restore the previous panic hook, if any.
    ///
    /// This is primarily useful in tests.
    pub fn uninstall_hook(&mut self) {
        if let Some(previous) = self.previous_hook.take() {
            let _ = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| previous(info)));
        }
    }

    /// Returns a list of paths to unsent crash reports.
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
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                reports.push(path);
            }
        }

        reports.sort();
        Ok(reports)
    }

    /// Submit all pending crash reports to the configured endpoint.
    ///
    /// Requires both an endpoint and an HTTP client to be set. Successfully
    /// submitted reports are deleted from disk; failures are left for the
    /// next attempt.
    pub async fn submit_pending_reports(&self) -> Result<()> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no crash report endpoint configured"))?;
        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no HTTP client configured for crash reporter"))?;

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

/// Capture a [`CrashReport`] from the current panic information.
pub fn capture_crash_report(info: &std::panic::PanicHookInfo<'_>) -> CrashReport {
    let message = info
        .payload()
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| info.payload().downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown panic".to_string());

    let backtrace = format!("{}", Backtrace::capture());

    CrashReport {
        message,
        backtrace,
        os_info: collect_os_info(),
        app_version: option_env!("CARGO_PKG_VERSION").map(ToString::to_string),
    }
}

/// Write a crash report JSON file to the given directory.
pub fn write_crash_report(dir: &Path, report: &CrashReport) -> Result<PathBuf> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed);

    let filename = format!("crash_report_{timestamp}_{sequence}.json");
    let path = dir.join(&filename);
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

/// Returns a platform-appropriate directory for crash reports.
///
/// - macOS: `~/Library/Application Support/{app_id}/crash_reports`
/// - Windows: `%APPDATA%/{app_id}/crash_reports`
/// - Linux/FreeBSD: `$XDG_DATA_HOME/{app_id}/crash_reports` or `~/.local/share/{app_id}/crash_reports`
fn crash_reports_dir(app_id: &str) -> Result<PathBuf> {
    let base = base_data_dir()?;
    Ok(base.join(app_id).join("crash_reports"))
}

#[cfg(target_os = "macos")]
fn base_data_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| anyhow::anyhow!("HOME environment variable not set"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support"))
}

#[cfg(target_os = "windows")]
fn base_data_dir() -> Result<PathBuf> {
    let app_data = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .ok_or_else(|| anyhow::anyhow!("APPDATA or LOCALAPPDATA environment variable not set"))?;
    Ok(PathBuf::from(app_data))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn base_data_dir() -> Result<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or_else(|| anyhow::anyhow!("XDG_DATA_HOME or HOME environment variable not set"))
}

fn collect_os_info() -> OsInfo {
    OsInfo {
        name: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        version: String::new().into(),
        locale: String::new().into(),
        hostname: String::new().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crash_report_roundtrip() {
        let temp_dir = std::env::temp_dir().join(format!("gpui_crash_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let report = CrashReport {
            message: "test panic".to_string(),
            backtrace: "frame1\nframe2".to_string(),
            os_info: collect_os_info(),
            app_version: Some("1.0.0".to_string()),
        };

        let path = write_crash_report(&temp_dir, &report).unwrap();
        let json = fs::read_to_string(&path).unwrap();
        let loaded: CrashReport = serde_json::from_str(&json).unwrap();
        let _ = fs::remove_dir_all(&temp_dir);

        assert_eq!(report.message, loaded.message);
        assert_eq!(report.backtrace, loaded.backtrace);
        assert_eq!(report.app_version, loaded.app_version);
    }

    #[test]
    fn test_pending_reports_lists_json_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("gpui_crash_pending_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        fs::write(temp_dir.join("crash_report_1.json"), b"{}").unwrap();
        fs::write(temp_dir.join("crash_report_2.json"), b"{}").unwrap();
        fs::write(temp_dir.join("not_a_report.txt"), b"").unwrap();

        let reporter = CrashReporter {
            reports_dir: temp_dir.clone(),
            endpoint: None,
            http_client: None,
            previous_hook: None,
        };

        let pending = reporter.pending_reports().unwrap();
        let _ = fs::remove_dir_all(&temp_dir);

        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_install_hook_preserves_previous_hook_for_uninstall() {
        let temp_dir = std::env::temp_dir().join(format!("gpui_crash_hook_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let mut reporter = CrashReporter {
            reports_dir: temp_dir.clone(),
            endpoint: None,
            http_client: None,
            previous_hook: None,
        };

        reporter.install_hook();
        assert!(reporter.previous_hook.is_some());
        reporter.uninstall_hook();

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_write_crash_report_uses_unique_paths() {
        let temp_dir =
            std::env::temp_dir().join(format!("gpui_crash_unique_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let report = CrashReport {
            message: "test panic".to_string(),
            backtrace: "frame1\nframe2".to_string(),
            os_info: collect_os_info(),
            app_version: Some("1.0.0".to_string()),
        };

        let first = write_crash_report(&temp_dir, &report).unwrap();
        let second = write_crash_report(&temp_dir, &report).unwrap();
        let _ = fs::remove_dir_all(&temp_dir);

        assert_ne!(first, second);
    }
}
