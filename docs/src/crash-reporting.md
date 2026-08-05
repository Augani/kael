# Crash Reporting

Kael's `kael_diagnostics` crate captures crashes and persists reports so they can
be submitted on the next launch. There are two layers:

- **Panic capture** — a Rust panic hook (`CrashReporter::install_hook`) records
  the panic message, a resolved backtrace, breadcrumbs, and host info.
- **Native capture** — OS-level handlers (`CrashReporter::install_native`) catch
  crashes that never unwind through Rust: segmentation faults, bus errors,
  illegal instructions, floating-point exceptions, aborts, and crashes
  originating in C/FFI/GPU-driver code.

Without native capture, a segfault or `abort()` produces nothing. With it, the
crash is recorded to disk and turned into a submittable report on the next run.

## Installing

Install both layers at startup. Native capture is opt-in:

```rust
use kael_diagnostics::{BreadcrumbBuffer, CrashConsent, CrashReporter};

let mut reporter = CrashReporter::new("com.example.app", BreadcrumbBuffer::new(64))?;
reporter.set_release(env!("CARGO_PKG_VERSION"));
reporter.set_environment("production");
reporter.set_endpoint("https://crashes.example.com/submit");
reporter.set_http_client(http_client.clone());

reporter.install_hook();    // Rust panics
reporter.install_native()?; // SIGSEGV/SIGBUS/SIGILL/SIGFPE/SIGABRT, FFI, etc.
```

Pre-crash context (app version, environment, OS, architecture, session id, pid)
is captured at `install_native()` time into a pre-opened artifact — never inside
the crash handler, which must stay async-signal-safe.

## Detecting and submitting prior crashes

Call `check_and_submit_pending` early in startup. It detects crashes left by the
previous run, converts them to JSON reports, and — only with consent — submits
all pending reports through the configured HTTP endpoint:

```rust
let summary = reporter.check_and_submit_pending(CrashConsent::granted()).await?;
if summary.detected_any() {
    for message in &summary.messages {
        eprintln!("recovered from prior crash: {message}");
    }
}
```

On orderly shutdown, mark the session clean so the next launch does not treat it
as an unclean exit:

```rust
reporter.mark_clean_exit()?;
```

`PriorCrashSummary` distinguishes:

- `native_crashes` — a handler fired and a signal record was decoded.
- `unclean_exits` — the previous run left a marker but no native record (for
  example `SIGKILL`, an OOM kill, or power loss); reported but with no signal
  detail.

## Consent

`CrashConsent` mirrors the release `UpdatePolicy` style and **defaults to
withheld**. Reports are always collected and retained on disk, but they are never
submitted unless the application explicitly opts in:

```rust
let consent = if user_enabled_crash_reporting {
    CrashConsent::granted()
} else {
    CrashConsent::withheld()
};
reporter.check_and_submit_pending(consent).await?;
```

With consent withheld (or no endpoint/HTTP client configured), prior crashes are
converted to JSON reports and kept on disk but not uploaded.

## What is captured, per platform

The native handler path is deliberately minimal so it can run inside a signal /
exception handler without allocating, locking, or formatting. It writes a small
fixed-shape record; everything human-readable is reconstructed on the next launch.

| Capability | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Mechanism | `sigaction` (SIGSEGV/SIGBUS/SIGABRT/SIGILL/SIGFPE) | `sigaction` (same set) | `SetUnhandledExceptionFilter` |
| Signal / exception code | Yes | Yes | Yes (exception code) |
| Fault address | Yes (`si_addr`) | Yes (`si_addr`) | Yes (`ExceptionAddress`) |
| Backtrace | Frame-pointer walk (x86_64/aarch64) | Frame-pointer walk (x86_64/aarch64) | `RtlCaptureStackBackTrace` |
| Symbolized frames | No (raw addresses) | No (raw addresses) | No (raw addresses) |
| Breadcrumbs at crash time | No | No | No |
| Pre-crash context | Yes (captured at install) | Yes | Yes |
| Verified by CI test | Yes (real SIGSEGV + abort) | cfg-gated, same code path | Implemented, not exercised here |

What is **not** captured by the native path, and why:

- **Full minidumps.** Writing a minidump in-process is not async-signal-safe, and
  the macOS minidump tooling is still early. Kael captures raw return addresses
  instead and symbolizes them offline (below).
- **Resolved symbols / breadcrumbs / heap state.** Resolving symbols or touching
  the breadcrumb buffer would allocate or lock inside the handler. Breadcrumbs
  remain available for the Rust panic path.
- **Backtraces without frame pointers.** The unix backtrace is a frame-pointer
  walk. Builds compiled with frame pointers omitted will capture fewer (or no)
  frames; see the symbolication notes.

## Symbolication

Native frames are raw instruction addresses. To turn them into file/line/function
information you need the unstripped binary (or its separate debug symbols) for the
exact build that crashed.

Keep per-release symbols by archiving the build artifacts:

- **macOS:** keep the `.dSYM` bundle produced alongside the binary.
- **Linux:** keep the unstripped binary, or split debug info into a `.debug` file
  with `objcopy --only-keep-debug`.
- **Windows:** keep the `.pdb` emitted next to the `.exe`.

Resolve captured addresses against the matching build:

```bash
# macOS — addresses are absolute load addresses
atos -o MyApp.app/Contents/MacOS/MyApp -arch arm64 0x1042684a0 0x1042687b0

# Linux
addr2line -e ./my-app -f -C 0x4001 0x4002
```

For frame-pointer-based backtraces to be useful in release builds, compile with
frame pointers preserved:

```toml
# .cargo/config.toml
[build]
rustflags = ["-C", "force-frame-pointers=yes"]
```

If you later adopt full minidumps for richer post-mortem analysis, the captured
`.dmp` files can be inspected with
[`minidump-stackwalk`](https://crates.io/crates/minidump-stackwalk) against the
archived symbols; the current Kael implementation does not write minidumps.
