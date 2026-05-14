# Production Readiness Guide

This guide closes the gap between the platform parity checklist and the broader
"can teams confidently ship this?" bar described in the Electron-competitive
roadmap.

## What Landed

The framework now includes first-class modules for:

- `SessionStore` for restoring window/session state with legacy-format fallback.
- `CrashReporter` for panic capture, durable report persistence, and retryable submission.
- `Tracer` for Chrome Trace-compatible exports plus built-in frame/layout/input instrumentation.
- `FileWatcher` for foreground-dispatched file watching.
- `AutoUpdater` for update feed parsing and platform installer handoff.

## Recommended App Startup Pattern

```rust
use kael::{CrashReporter, SessionStore, TracePhase, Tracer};

fn bootstrap(app_id: &str) -> anyhow::Result<(SessionStore, CrashReporter, Tracer)> {
    let session_store = SessionStore::new(app_id)?;

    let mut crash_reporter = CrashReporter::new(app_id)?;
    crash_reporter.install_hook();

    let tracer = Tracer::default();
    tracer.enable();
    tracer.install_global();
    tracer.record("app.startup", "lifecycle", TracePhase::Instant);

    Ok((session_store, crash_reporter, tracer))
}
```

The global tracer installation is what turns on Kael's built-in probes for:

- `window.draw_frame`
- `window.present`
- `window.dispatch_event`
- `text.layout`
- `text.layout_with_features`

## Automated Verification

Use the committed verification surfaces instead of ad hoc release commands:

```sh
cargo fmt --all --check
bash scripts/ci/verify-kael.sh default
cargo test -p kael --test worker_process
cargo test -p kael --test extension_process
cargo run -p xtask -- dry-run
```

Additional platform sweeps:

```sh
# Linux only
bash scripts/ci/install-linux-deps.sh
bash scripts/ci/verify-kael.sh linux-x11
bash scripts/ci/verify-kael.sh linux-wayland

# Windows only
pwsh -File scripts/ci/verify-kael.ps1
```

CI runs the same contract in `.github/workflows/platform-readiness.yml`.

### Runtime Isolation Proofs

The repository includes real child-process tests for the process and extension
runtime paths. These should stay green before claiming regressions are safe:

```sh
cargo test -p kael --test worker_process
cargo test -p kael --test extension_process
```

These tests prove:

- a worker child process can bootstrap, answer health checks, and process typed requests;
- a worker crash is reported to the host without panicking the host process;
- an external-process extension can handshake, activate, and deactivate through the real transport path.

They do not replace manual OS validation for signed release artifacts,
screen-reader behavior, or real capture devices.

## Validation Matrix

Run these checks before calling a release production-ready.

| Area | macOS | Windows | Linux X11 | Linux Wayland | What to Verify |
| --- | --- | --- | --- | --- | --- |
| Tray and dock/taskbar integration | Yes | Yes | Yes | Yes | Icon appears, actions route correctly, badges update |
| Notifications with actions | Yes | Yes | Yes | Yes | Action callback fires with the expected `id` |
| Dialog and clipboard parity | Yes | Yes | Yes | Yes | Open/save flows work and rich clipboard formats round-trip |
| Accessibility | VoiceOver | Narrator/NVDA | Orca | Orca | Focus order, labels, roles, and announcements |
| Session restore | Yes | Yes | Yes | Yes | Window bounds restore and disconnected displays fall back sanely |
| Crash reporting | Yes | Yes | Yes | Yes | Panic creates a report on disk and retry submission clears sent reports |
| Tracing export | Yes | Yes | Yes | Yes | Trace JSON opens in Chrome Trace or Perfetto without malformed events |
| File watching | Yes | Yes | Yes | Yes | Recursive watches, depth limits, and error callbacks behave correctly |
| Screen capture | Yes | Yes | Yes | Yes | Feature-gated capture produces stable frames without UI stalls |

## Benchmark and Profiling Workflow

Use the existing perf harness for repeatable micro-benchmarks:

```sh
cargo perf-test -p kael
```

Capture benchmark snapshots for comparison:

```sh
cargo perf-test -p kael -- --json=baseline
cargo perf-test -p kael -- --json=candidate
cargo perf-compare candidate baseline
```

For frame-rate and visual smoothness checks, run the included benchmark example:

```sh
cargo run -p kael --example perf_bench
```

For ad hoc frame timing during development:

```sh
ZED_MEASUREMENTS=1 cargo run -p kael --example perf_bench
```

For trace capture:

1. Enable and install a `Tracer`.
2. Exercise the UI flow you want to inspect.
3. Call `tracer.write_to_file("trace.json")`.
4. Open the result in Chrome Trace or Perfetto.

## Release Sweep

Before cutting a release:

1. Run `.github/workflows/platform-readiness.yml` or the equivalent local scripts in `scripts/ci/`.
2. Walk `docs/guides/manual-verification-matrix.md` on at least one machine per supported OS family.
3. Verify a panic produces a crash report and that pending reports submit successfully.
4. Export one trace from a real interaction and confirm frame/layout/input events appear.
5. Run the perf harness or `perf_bench` and compare against the previous baseline.
6. Use `docs/guides/platform-distribution.md` for the signed/notarized packaging sweep.

## Security Configuration

Kael includes a capability-based permission broker (`PermissionBroker`) that gates dangerous operations. Applications should configure the broker at startup and treat unset grants as deny for all high-risk capabilities.

### Configuring the PermissionBroker

```rust
use kael::{App, PermissionBroker, Capability, PathScope, ProcessClass, ProcessId, ThreatModel};

fn configure_security(cx: &mut App) {
    let mut broker = PermissionBroker::new();
    let worker = ProcessId(1);
    broker.register_process(worker, ProcessClass::Worker);
    broker.apply_threat_model(&ThreatModel::new());
    broker.grant(worker, Capability::FilesystemRead {
        scope: PathScope::AppData,
    });
    cx.set_permission_broker(broker);
}
```

### Default Capability Grants per Process Class

The broker is initialized automatically when `App` is created. `ThreatModel::new()` supplies the following defaults:

| Process Class | Default Capabilities |
| --- | --- |
| `Ui` | `OpenExternalUrl`, `ClipboardRead`, `ClipboardWrite`, `Notification` |
| `Worker` | `FilesystemRead(AppData)`, `FilesystemWrite(AppData)` |
| `Media` | `Microphone`, `Camera`, `ScreenCapture` |
| `Extension` | None (default deny) |

### Privileged Operations Checked by the Broker

The `App` methods below check the broker before delegating to the platform:

- `open_url` requires `OpenExternalUrl`
- `read_from_clipboard` / `read_from_primary` require `ClipboardRead`
- `show_notification` / `show_notification_with_actions` require `Notification`
- `prompt_for_paths` requires `FilesystemRead(UserSelected)`
- `prompt_for_new_path` requires `FilesystemWrite(UserSelected)`

When a capability is denied, these methods return a clear `Err` instead of silently failing.

### Plugin and Media Capture Integration

- `ExtensionHost::activate` validates all requested capabilities against the broker before activating an extension.
- `CaptureManager::create_session` checks `Microphone`, `Camera`, or `ScreenCapture` before creating a session.

## Reference Surfaces

Useful entry points while validating or demonstrating the stack:

- `crates/kael/examples/platform_features.rs`
- `crates/kael/examples/perf_bench.rs`
- `crates/kael/examples/daemon_app.rs`
- `crates/kael/examples/window.rs`
