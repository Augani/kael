//! Platform services diagnostics scaffolding for Kael.

#![deny(missing_docs)]

/// Breadcrumb storage and types.
pub mod breadcrumb;
/// Crash capture and persistence.
pub mod crash;
/// Metrics and tracing services.
pub mod metrics;
/// Native (non-panic) crash capture via OS-level signal/exception handlers.
pub mod native;
/// Platform backends for diagnostics services.
pub mod platform;
/// Global diagnostics configuration and reporting facade.
pub mod reporter;

pub use anyhow::Result;
pub use breadcrumb::{Breadcrumb, BreadcrumbBuffer, Level};
pub use crash::{
    CrashReport, CrashReporter, OsInfo, capture_crash_report, collect_os_info, write_crash_report,
};
pub use metrics::{MetricsRegistry, Span, TraceEvent, TracePhase, Tracer, Transaction};
pub use native::{NativeContext, NativeSignal, PendingNativeCrash};
pub use reporter::{
    Diagnostics, DiagnosticsConfig, add_breadcrumb, capture_error, init, record_counter,
    record_gauge, record_histogram, start_transaction,
};

/// Returns the current diagnostics backend label for the active target.
pub const fn backend_name() -> &'static str {
    platform::backend_name()
}

#[cfg(test)]
mod tests {
    use super::backend_name;

    #[test]
    fn backend_name_is_available() {
        assert!(!backend_name().is_empty());
    }
}
