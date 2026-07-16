use std::{
    backtrace::Backtrace,
    fs,
    io::{Read as _, Write as _},
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
const MAX_REPORT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PANIC_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_BACKTRACE_BYTES: usize = 6 * 1024 * 1024;
const MAX_PENDING_REPORTS_PER_BATCH: usize = 256;
const MAX_CRASH_ENDPOINT_BYTES: usize = 16 * 1024;

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
        validate_crash_app_id(&app_id)?;
        let reports_dir = crash_reports_dir(&app_id)?;
        Self::with_reports_dir(reports_dir)
    }

    fn with_reports_dir(reports_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&reports_dir).with_context(|| {
            format!(
                "failed to create crash reports directory: {}",
                reports_dir.display()
            )
        })?;
        validate_real_reports_dir(&reports_dir)?;
        restrict_reports_dir_permissions(&reports_dir)?;

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
        if self.previous_hook.is_some() {
            return;
        }
        let reports_dir = self.reports_dir.clone();
        let previous: Arc<PanicHook> = std::panic::take_hook().into();
        self.previous_hook = Some(previous.clone());

        std::panic::set_hook(Box::new(move |info| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let report = capture_crash_report(info);
                let _ = write_crash_report(&reports_dir, &report);
            }));
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| previous(info)));
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
            if entry.file_type()?.is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("json")
            {
                reports.push(path);
                if reports.len() == MAX_PENDING_REPORTS_PER_BATCH {
                    break;
                }
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
        validate_crash_endpoint(endpoint)?;
        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no HTTP client configured for crash reporter"))?;

        for path in self.pending_reports()? {
            let json = read_crash_report(&path)?;

            let response = client
                .post_json(endpoint, json.into())
                .await
                .with_context(|| format!("failed to submit crash report: {}", path.display()))?;

            if !response.status().is_success() {
                anyhow::bail!(
                    "crash report submission returned HTTP {} for {}",
                    response.status(),
                    path.display()
                );
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
}

/// Builder for [`CrashReporter`].
///
/// Use this when app identifiers, report directories, or upload endpoints are
/// generated by setup code or AI agents and should be validated before the
/// global panic hook is installed.
#[derive(Clone)]
pub struct CrashReporterBuilder {
    app_id: String,
    reports_dir: Option<PathBuf>,
    endpoint: Option<String>,
    http_client: Option<Arc<dyn http_client::HttpClient>>,
}

impl std::fmt::Debug for CrashReporterBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CrashReporterBuilder")
            .field("app_id", &self.app_id)
            .field("reports_dir", &self.reports_dir)
            .field("endpoint", &self.endpoint)
            .field("has_http_client", &self.http_client.is_some())
            .finish_non_exhaustive()
    }
}

impl CrashReporterBuilder {
    /// Create a crash reporter builder for an application identifier.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            reports_dir: None,
            endpoint: None,
            http_client: None,
        }
    }

    /// Store crash reports in a custom absolute directory.
    ///
    /// If omitted, Kael stores reports under the platform data directory for
    /// the configured app id.
    pub fn reports_dir(mut self, reports_dir: impl Into<PathBuf>) -> Self {
        self.reports_dir = Some(reports_dir.into());
        self
    }

    /// Set the HTTP(S) endpoint used by [`CrashReporter::submit_pending_reports`].
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set the HTTP client used by [`CrashReporter::submit_pending_reports`].
    pub fn http_client(mut self, client: Arc<dyn http_client::HttpClient>) -> Self {
        self.http_client = Some(client);
        self
    }

    /// Return the configured app id.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Return the configured custom reports directory, if any.
    pub fn configured_reports_dir(&self) -> Option<&Path> {
        self.reports_dir.as_deref()
    }

    /// Return the configured endpoint, if any.
    pub fn configured_endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Validate the builder without creating directories.
    pub fn validate(&self) -> Result<()> {
        validate_crash_app_id(&self.app_id)?;
        if let Some(reports_dir) = &self.reports_dir {
            validate_crash_reports_dir(reports_dir)?;
        }
        if let Some(endpoint) = &self.endpoint {
            validate_crash_endpoint(endpoint)?;
        }
        Ok(())
    }

    /// Build a validated crash reporter and create its reports directory.
    pub fn build_checked(self) -> Result<CrashReporter> {
        self.validate()?;

        let reports_dir = if let Some(reports_dir) = self.reports_dir {
            reports_dir
        } else {
            crash_reports_dir(&self.app_id)?
        };

        let mut reporter = CrashReporter::with_reports_dir(reports_dir)?;
        reporter.endpoint = self.endpoint;
        reporter.http_client = self.http_client;
        Ok(reporter)
    }

    /// Build and install the panic hook in one step.
    pub fn install_hook_checked(self) -> Result<CrashReporter> {
        let mut reporter = self.build_checked()?;
        reporter.install_hook();
        Ok(reporter)
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
    let message = truncate_crash_text(message, MAX_PANIC_MESSAGE_BYTES);

    let backtrace = truncate_crash_text(format!("{}", Backtrace::capture()), MAX_BACKTRACE_BYTES);

    CrashReport {
        message,
        backtrace,
        os_info: collect_os_info(),
        app_version: option_env!("CARGO_PKG_VERSION").map(ToString::to_string),
    }
}

/// Write a crash report JSON file to the given directory.
pub fn write_crash_report(dir: &Path, report: &CrashReport) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| {
        format!(
            "failed to create crash reports directory: {}",
            dir.display()
        )
    })?;
    validate_real_reports_dir(dir)?;
    restrict_reports_dir_permissions(dir)?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = NEXT_REPORT_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| anyhow::anyhow!("crash report identifier space exhausted"))?;

    let filename = format!("crash_report_{timestamp}_{sequence}.json");
    let path = dir.join(&filename);
    let temp_path = path.with_extension("json.tmp");

    let json = serde_json::to_string_pretty(report).context("failed to serialize crash report")?;
    anyhow::ensure!(
        json.len() as u64 <= MAX_REPORT_BYTES,
        "serialized crash report exceeds {MAX_REPORT_BYTES} byte limit"
    );
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&temp_path).with_context(|| {
        format!(
            "failed to create temporary crash report file: {}",
            temp_path.display()
        )
    })?;
    let write_result = file
        .write_all(json.as_bytes())
        .with_context(|| format!("failed to write crash report file: {}", temp_path.display()))
        .and_then(|()| {
            file.sync_all().with_context(|| {
                format!("failed to sync crash report file: {}", temp_path.display())
            })
        });
    if let Err(error) = write_result {
        drop(file);
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    drop(file);
    if let Err(error) = fs::rename(&temp_path, &path).with_context(|| {
        format!(
            "failed to finalize crash report file from {} to {}",
            temp_path.display(),
            path.display()
        )
    }) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(path)
}

/// Returns a platform-appropriate directory for crash reports.
///
/// - macOS: `~/Library/Application Support/{app_id}/crash_reports`
/// - Windows: `%APPDATA%/{app_id}/crash_reports`
/// - Linux/FreeBSD: `$XDG_DATA_HOME/{app_id}/crash_reports` or `~/.local/share/{app_id}/crash_reports`
fn crash_reports_dir(app_id: &str) -> Result<PathBuf> {
    let base = crate::util::base_data_dir()?;
    Ok(base.join(app_id).join("crash_reports"))
}

fn validate_crash_app_id(app_id: &str) -> Result<()> {
    anyhow::ensure!(!app_id.is_empty(), "crash reporter app id cannot be empty");
    anyhow::ensure!(
        app_id.trim() == app_id,
        "crash reporter app id cannot have leading or trailing whitespace: {app_id:?}"
    );
    anyhow::ensure!(
        app_id.len() <= 128,
        "crash reporter app id cannot be longer than 128 bytes: {app_id:?}"
    );
    anyhow::ensure!(
        app_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            && app_id != "."
            && app_id != "..",
        "crash reporter app id contains invalid path characters: {app_id:?}"
    );
    anyhow::ensure!(
        !app_id.chars().any(char::is_control),
        "crash reporter app id cannot contain control characters: {app_id:?}"
    );
    Ok(())
}

fn validate_crash_reports_dir(reports_dir: &Path) -> Result<()> {
    anyhow::ensure!(
        reports_dir.is_absolute(),
        "crash reports directory must be absolute: {}",
        reports_dir.display()
    );
    anyhow::ensure!(
        !reports_dir.as_os_str().is_empty(),
        "crash reports directory cannot be empty"
    );
    anyhow::ensure!(
        !reports_dir.to_string_lossy().contains('\0'),
        "crash reports directory cannot contain NUL characters: {}",
        reports_dir.display()
    );
    Ok(())
}

fn validate_crash_endpoint(endpoint: &str) -> Result<()> {
    anyhow::ensure!(
        !endpoint.trim().is_empty(),
        "crash report endpoint cannot be empty"
    );
    anyhow::ensure!(
        endpoint.trim() == endpoint,
        "crash report endpoint cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        endpoint.len() <= MAX_CRASH_ENDPOINT_BYTES,
        "crash report endpoint exceeds {MAX_CRASH_ENDPOINT_BYTES} bytes"
    );
    anyhow::ensure!(
        !endpoint.chars().any(char::is_control),
        "crash report endpoint cannot contain control characters"
    );
    let parsed = http_client::Url::parse(endpoint).context("crash report endpoint is invalid")?;
    anyhow::ensure!(
        parsed.scheme() == "https",
        "crash report endpoint must use https"
    );
    anyhow::ensure!(
        parsed.host_str().is_some(),
        "crash report endpoint must include a host"
    );
    anyhow::ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "crash report endpoint cannot contain credentials"
    );
    anyhow::ensure!(
        parsed.fragment().is_none(),
        "crash report endpoint cannot contain a fragment"
    );
    Ok(())
}

fn truncate_crash_text(mut text: String, max_bytes: usize) -> String {
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

fn read_crash_report(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect crash report: {}", path.display()))?;
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "crash report must be a regular file: {}",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= MAX_REPORT_BYTES,
        "crash report exceeds {MAX_REPORT_BYTES} byte limit: {}",
        path.display()
    );

    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("failed to open crash report: {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect open crash report: {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file(),
        "crash report must be a regular file: {}",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= MAX_REPORT_BYTES,
        "crash report exceeds {MAX_REPORT_BYTES} byte limit: {}",
        path.display()
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    std::io::Read::by_ref(&mut file)
        .take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read crash report: {}", path.display()))?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_REPORT_BYTES,
        "crash report exceeds {MAX_REPORT_BYTES} byte limit: {}",
        path.display()
    );
    String::from_utf8(bytes).context("crash report is not valid UTF-8")
}

fn validate_real_reports_dir(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).with_context(|| {
        format!(
            "failed to inspect crash reports directory: {}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        metadata.file_type().is_dir(),
        "crash reports path must be a real directory: {}",
        path.display()
    );
    Ok(())
}

#[cfg(unix)]
fn restrict_reports_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).with_context(|| {
        format!(
            "failed to secure crash reports directory: {}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn restrict_reports_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
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
    fn pending_reports_are_bounded_per_submission_batch() {
        let temp_dir = std::env::temp_dir().join(format!(
            "kael_crash_batch_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&temp_dir).unwrap();
        for index in 0..(MAX_PENDING_REPORTS_PER_BATCH + 10) {
            fs::write(
                temp_dir.join(format!("crash_report_{index:04}.json")),
                b"{}",
            )
            .unwrap();
        }
        let reporter = CrashReporter::with_reports_dir(temp_dir.clone()).unwrap();

        let pending = reporter.pending_reports().unwrap();
        assert_eq!(pending.len(), MAX_PENDING_REPORTS_PER_BATCH);
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn crash_reporter_rejects_path_like_app_ids() {
        assert!(CrashReporter::new("../escape").is_err());
        assert!(CrashReporter::new("nested/app").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn pending_reports_ignore_symlinks() {
        use std::os::unix::fs::symlink;
        let temp_dir = std::env::temp_dir().join(format!(
            "kael_crash_symlink_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&temp_dir).unwrap();
        let target = temp_dir.join("target.json");
        fs::write(&target, b"{}").unwrap();
        symlink(&target, temp_dir.join("linked.json")).unwrap();
        let reporter = CrashReporter::with_reports_dir(temp_dir.clone()).unwrap();
        assert_eq!(reporter.pending_reports().unwrap(), vec![target]);
        fs::remove_dir_all(temp_dir).unwrap();
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

    #[test]
    fn crash_text_truncation_preserves_utf8_boundaries() {
        let text = "🙂".repeat(10);
        let truncated = truncate_crash_text(text, 9);
        assert_eq!(truncated, "🙂🙂");
        assert!(truncated.len() <= 9);
    }

    #[test]
    fn crash_report_reads_reject_invalid_utf8() {
        let temp_dir = std::env::temp_dir().join(format!(
            "kael_crash_utf8_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&temp_dir).unwrap();
        let path = temp_dir.join("crash_report_invalid.json");
        fs::write(&path, [0xff, 0xfe]).unwrap();

        let error = read_crash_report(&path).unwrap_err();
        assert!(error.to_string().contains("not valid UTF-8"));
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn crash_report_files_and_directory_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp_dir = std::env::temp_dir().join(format!(
            "kael_crash_permissions_{}_{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&temp_dir).unwrap();
        let report = CrashReport {
            message: "private panic".to_string(),
            backtrace: "private trace".to_string(),
            os_info: collect_os_info(),
            app_version: None,
        };
        let path = write_crash_report(&temp_dir, &report).unwrap();

        let directory_mode = fs::metadata(&temp_dir).unwrap().permissions().mode() & 0o777;
        let file_mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(directory_mode, 0o700);
        assert_eq!(file_mode, 0o600);
        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn crash_reporter_builder_creates_checked_reporter() {
        let temp_dir = std::env::temp_dir().join(format!(
            "gpui_crash_builder_{}_{}",
            std::process::id(),
            NEXT_REPORT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&temp_dir);

        let reporter = CrashReporterBuilder::new("com.example.crash-test")
            .reports_dir(&temp_dir)
            .endpoint("https://crashes.example.test/reports")
            .build_checked()
            .unwrap();

        assert_eq!(reporter.reports_dir(), temp_dir.as_path());
        assert!(reporter.reports_dir().exists());
        assert_eq!(
            reporter.endpoint.as_deref(),
            Some("https://crashes.example.test/reports")
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn crash_reporter_builder_rejects_generated_invalid_inputs() {
        assert!(CrashReporterBuilder::new("").validate().is_err());
        assert!(CrashReporterBuilder::new(" app").validate().is_err());
        assert!(CrashReporterBuilder::new("app/id").validate().is_err());
        assert!(CrashReporterBuilder::new("app\0id").validate().is_err());
        assert!(
            CrashReporterBuilder::new("app")
                .reports_dir("relative/crashes")
                .validate()
                .is_err()
        );
        assert!(
            CrashReporterBuilder::new("app")
                .endpoint("file:///tmp/crashes")
                .validate()
                .is_err()
        );
        assert!(
            CrashReporterBuilder::new("app")
                .endpoint("http://crashes.example.test")
                .validate()
                .is_err()
        );
        assert!(
            CrashReporterBuilder::new("app")
                .endpoint("https://user:secret@crashes.example.test/reports")
                .validate()
                .is_err()
        );
        assert!(
            CrashReporterBuilder::new("app")
                .endpoint("https://crashes.example.test/reports#private")
                .validate()
                .is_err()
        );
        assert!(
            CrashReporterBuilder::new("app")
                .endpoint("https://")
                .validate()
                .is_err()
        );
        assert!(
            CrashReporterBuilder::new("app")
                .endpoint(" https://crashes.example.test")
                .validate()
                .is_err()
        );
    }
}
