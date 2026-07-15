//! Structured crash-report capture for the Rust-panic path (part of P3-D).
//!
//! This captures a serializable [`CrashReport`] from a panic — message, source location,
//! thread, timestamp, and build version — and can install a global panic hook that
//! forwards reports to a handler (a log, an upload queue, a local file). Native crashes
//! (segfaults, aborts) still require an out-of-process handler such as Crashpad; this
//! covers the in-process Rust-panic case that handler cannot.

use std::any::Any;
use std::panic::PanicHookInfo;

use serde::{Deserialize, Serialize};

/// A structured record of a Rust panic, ready to log or upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReport {
    /// The panic message (extracted from the panic payload).
    pub message: String,
    /// Source location as `file:line:column`, if the runtime provided one.
    pub location: Option<String>,
    /// Name of the thread that panicked (`"unnamed"` if it had none).
    pub thread: String,
    /// Capture time in milliseconds since the Unix epoch (supplied by the caller).
    pub timestamp_ms: u64,
    /// The application/build version string.
    pub version: String,
}

impl CrashReport {
    /// Serialize to a single-line JSON record.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| String::from("{}"))
    }
}

/// Build a [`CrashReport`] from a panic payload and its already-formatted context.
///
/// Kept free of any global state so it is unit-testable: the panic hook formats the
/// location and reads the thread/timestamp, then delegates here.
pub fn capture_panic(
    payload: &(dyn Any + 'static),
    location: Option<String>,
    thread: &str,
    timestamp_ms: u64,
    version: &str,
) -> CrashReport {
    CrashReport {
        message: panic_message(payload),
        location,
        thread: thread.to_string(),
        timestamp_ms,
        version: version.to_string(),
    }
}

fn panic_message(payload: &(dyn Any + 'static)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

/// Build a report directly from a [`PanicHookInfo`] inside a panic hook.
pub fn report_from_hook(info: &PanicHookInfo<'_>, timestamp_ms: u64, version: &str) -> CrashReport {
    let location = info.location().map(|location| {
        format!(
            "{}:{}:{}",
            location.file(),
            location.line(),
            location.column()
        )
    });
    let thread = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    capture_panic(info.payload(), location, &thread, timestamp_ms, version)
}

/// Install a global panic hook that builds a [`CrashReport`] for every panic and passes
/// it to `handler`. `now` supplies the capture timestamp (injected so the caller owns the
/// clock). Replaces any previously installed hook.
pub fn install_panic_handler<N, H>(version: String, now: N, handler: H)
where
    N: Fn() -> u64 + Send + Sync + 'static,
    H: Fn(CrashReport) + Send + Sync + 'static,
{
    std::panic::set_hook(Box::new(move |info| {
        // A panic hook must never panic itself: doing so while the runtime is already
        // unwinding aborts the process before any fallback crash handling can run.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handler(report_from_hook(info, now(), &version));
        }));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_str_and_string_payloads() {
        let report = capture_panic(&"boom", None, "main", 1000, "1.2.3");
        assert_eq!(report.message, "boom");
        assert_eq!(report.thread, "main");
        assert_eq!(report.timestamp_ms, 1000);
        assert_eq!(report.version, "1.2.3");

        let owned = String::from("owned message");
        let report = capture_panic(&owned, Some("src/x.rs:4:2".to_string()), "worker", 5, "0.1");
        assert_eq!(report.message, "owned message");
        assert_eq!(report.location.as_deref(), Some("src/x.rs:4:2"));
    }

    #[test]
    fn unknown_payload_is_labeled() {
        let report = capture_panic(&42i32, None, "main", 0, "1");
        assert_eq!(report.message, "unknown panic payload");
    }

    #[test]
    fn report_serializes_to_json_with_all_fields() {
        let report = capture_panic(&"x", Some("a.rs:1:1".to_string()), "t", 7, "9");
        let json = report.to_json();
        let round: CrashReport = serde_json::from_str(&json).unwrap();
        assert_eq!(round, report);
        assert!(json.contains("\"message\":\"x\""));
        assert!(json.contains("\"location\":\"a.rs:1:1\""));
    }

    #[test]
    fn installed_hook_captures_a_real_panic() {
        use std::sync::{Arc, Mutex};

        let captured: Arc<Mutex<Option<CrashReport>>> = Arc::new(Mutex::new(None));
        let sink = Arc::clone(&captured);
        let previous = std::panic::take_hook();
        install_panic_handler(
            "test-version".to_string(),
            || 123,
            move |report| {
                *sink.lock().unwrap() = Some(report);
            },
        );

        let result = std::panic::catch_unwind(|| panic!("deliberate test panic"));
        std::panic::set_hook(previous);

        assert!(result.is_err());
        let report = captured.lock().unwrap().take().expect("report captured");
        assert_eq!(report.message, "deliberate test panic");
        assert_eq!(report.timestamp_ms, 123);
        assert_eq!(report.version, "test-version");
        assert!(report.location.is_some());
    }
}
