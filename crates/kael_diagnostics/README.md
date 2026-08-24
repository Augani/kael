# kael_diagnostics

Bounded diagnostics, metrics, tracing, and crash reporting for desktop and
browser applications built with Kael primitives or any other UI stack.

The crate keeps its hot paths in memory, applies fixed retention and payload
limits, writes reports atomically, and treats upload as an explicit application
decision. Rust panics and native crashes are persisted locally; native signal or
exception handlers and network submission are opt-in.

On `wasm32-unknown-unknown`, recoverable errors and Rust panic-hook reports use
bounded origin-local browser storage and remain enumerable/uploadable through
the same `CrashReporter` API. Chrome Trace JSON uses browser-safe monotonic
clocks. Browsers do not expose OS signals, process identifiers, or native file
paths: `install_native` and `Tracer::write_to_file` return precise errors, while
`Tracer::export_to_chrome_json` pairs with Kael's browser file-export API.

## Quick start

```no_run
use kael_diagnostics::{Breadcrumb, Diagnostics, DiagnosticsConfig, Level};
use std::{collections::HashMap, time::SystemTime};

fn main() -> kael_diagnostics::Result<()> {
    let diagnostics = Diagnostics::new(DiagnosticsConfig {
        app_id: "com.example.product".to_string(),
        release: "1.0.0".to_string(),
        environment: "production".to_string(),
        install_panic_hook: true,
        install_native_handler: true,
        ..DiagnosticsConfig::default()
    })?;

    diagnostics.add_breadcrumb(Breadcrumb {
        category: "workspace".to_string(),
        message: "opened project".to_string(),
        level: Level::Info,
        timestamp: SystemTime::now(),
        data: HashMap::new(),
    });
    diagnostics.record_counter("workspace.opened", 1);

    let transaction = diagnostics.start_transaction("index-project");
    let span = transaction.start_span("scan-files");
    span.finish();
    transaction.finish();

    diagnostics.tracer().write_to_file("trace.json")?;
    diagnostics.crash_reporter().mark_clean_exit()?;
    Ok(())
}
```

`Diagnostics::new` creates an independently owned instance and does not replace
the process-global tracer. Its panic hook is still process-wide when enabled.
Use `init` when the process-wide convenience functions and global tracer are
desired.

## Crash reporting contract

- `install_panic_hook` controls Rust panic capture.
- `install_native_handler` (or `CrashReporter::install_native`) opts into OS
  signal or exception handlers.
- Reports remain on disk until the application supplies an HTTP client and a
  credential-free HTTPS endpoint without a query string or fragment; configure
  authentication on the HTTP client instead of embedding it in the URL.
- `CrashConsent::withheld`, the default, never submits retained reports.
- `before_send` may redact or reject a report before it is persisted.
- Call `mark_clean_exit` during orderly shutdown after installing native crash
  capture, otherwise the next launch correctly treats the session as unclean.
- Browser report paths are virtual identifiers backed by origin-local storage;
  they are not native filesystem paths. Storage denial or quota exhaustion is
  returned to the application instead of silently dropping a report.

The API reference is available on
[docs.rs](https://docs.rs/kael_diagnostics). Repository-level architecture and
production guidance live in the [Kael repository](https://github.com/Augani/kael).

## License

Licensed under the Apache License, Version 2.0. See [LICENSE-APACHE](LICENSE-APACHE).
