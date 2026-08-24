//! Browser compatibility surface for native crash reporting.
//!
//! Browsers do not expose process signals or exception handlers. Rust panic
//! and recoverable error reports are still persisted by `CrashReporter`; only
//! the native-process marker and signal-record APIs are unavailable.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

/// Pre-crash context retained for API parity with native targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeContext {
    /// Unique identifier for this application session.
    pub session_id: String,
    /// Application release/version string at install time.
    pub app_version: Option<String>,
    /// Deployment environment at install time.
    pub environment: Option<String>,
    /// Operating-system family.
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// Process identifier. Browsers report zero.
    pub pid: u32,
    /// Milliseconds since the Unix epoch when the session started.
    pub started_at_ms: u128,
}

/// Native signal data retained for serialization and shared application code.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NativeSignal {
    /// Platform signal or exception number.
    pub signal: i64,
    /// Platform-specific signal code.
    pub code: Option<i64>,
    /// Faulting address when available.
    pub fault_address: Option<u64>,
    /// Captured instruction addresses.
    pub frames: Vec<String>,
}

/// A pending native crash. Browser backends never create these records.
#[derive(Debug, Clone)]
pub struct PendingNativeCrash {
    /// Session context.
    pub context: NativeContext,
    /// Native signal data.
    pub signal: Option<NativeSignal>,
    /// Virtual metadata path.
    pub meta_path: PathBuf,
    /// Virtual dump path.
    pub dump_path: Option<PathBuf>,
}

impl PendingNativeCrash {
    /// Whether a native signal record is present.
    pub fn has_native_record(&self) -> bool {
        self.signal.is_some()
    }

    /// Return a stable human-readable summary.
    pub fn summary(&self) -> String {
        "browser sessions do not expose native crash signals".to_owned()
    }

    /// Browsers cannot provide a native stack-address trace.
    pub fn backtrace_text(&self) -> String {
        String::new()
    }
}

/// Generate a session identifier without requiring process APIs.
pub fn new_session_id() -> String {
    let millis = web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.time_origin() + performance.now())
        .unwrap_or_default()
        .max(0.0) as u128;
    let sequence = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    format!("browser-{millis:032x}-{sequence:016x}")
}

/// Native signal handlers are unavailable in a browser sandbox.
pub fn install(_reports_dir: &Path, _context: NativeContext) -> Result<bool> {
    Err(anyhow!(
        "native signal and exception handlers are unavailable in browsers"
    ))
}

/// Browser sessions do not leave native crash artifacts.
pub fn pending_crashes(_reports_dir: &Path) -> Result<Vec<PendingNativeCrash>> {
    Ok(Vec::new())
}

/// Clearing a browser-native artifact is a no-op because none can exist.
pub fn clear_crash(_crash: &PendingNativeCrash) -> Result<()> {
    Ok(())
}

/// Browser sessions have no native clean-exit marker.
pub fn mark_clean_exit(_reports_dir: &Path) -> Result<()> {
    Ok(())
}
