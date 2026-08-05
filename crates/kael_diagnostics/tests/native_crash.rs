//! Real native-crash integration test.
//!
//! Spawns this integration-test executable in a dedicated helper mode, installs
//! the native crash handler, and triggers a genuine SIGSEGV or SIGABRT. The
//! parent process asserts that the on-disk crash artifacts (meta marker + dump)
//! were produced by the handler and parse into a submittable report.

#![cfg(unix)]

use std::{io::Write as _, path::Path, process::Command, time::Duration};

use kael_diagnostics::{BreadcrumbBuffer, CrashConsent, CrashReporter};

fn run_helper(reports_dir: &Path, mode: &str) -> (String, std::process::Output) {
    let output =
        Command::new(std::env::current_exe().expect("test executable should be available"))
            .args(["--exact", "crash_helper_process", "--nocapture"])
            .env("KAEL_CRASH_HELPER_DIR", reports_dir)
            .env("KAEL_CRASH_HELPER_MODE", mode)
            .output()
            .expect("failed to spawn native-crash helper process");
    let session_id = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.trim().strip_prefix("KAEL_SESSION_ID="))
        .unwrap_or_default()
        .to_string();
    (session_id, output)
}

#[test]
fn crash_helper_process() {
    let Some(reports_dir) = std::env::var_os("KAEL_CRASH_HELPER_DIR") else {
        return;
    };
    let mode = std::env::var("KAEL_CRASH_HELPER_MODE").expect("helper mode should be set");
    let mut reporter = CrashReporter::new(
        "dev.kael.diagnostics.crash-helper",
        BreadcrumbBuffer::new(8),
    )
    .unwrap();
    reporter.set_reports_dir(reports_dir).unwrap();
    reporter.install_native().unwrap();
    println!("KAEL_SESSION_ID={}", reporter.session_id());
    std::io::stdout().flush().unwrap();

    match mode.as_str() {
        "segv" => unsafe {
            libc::raise(libc::SIGSEGV);
        },
        "abort" => std::process::abort(),
        _ => panic!("unsupported crash mode {mode}"),
    }
    unreachable!("crash signal unexpectedly returned");
}

fn wait_for_dump(reports_dir: &Path, session_id: &str) -> Vec<u8> {
    let dump = reports_dir.join(format!("{session_id}.crashdump"));
    for _ in 0..100 {
        if let Ok(bytes) = std::fs::read(&dump) {
            if !bytes.is_empty() {
                return bytes;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("crash dump never appeared at {}", dump.display());
}

#[test]
fn captures_real_sigsegv() {
    let directory = tempfile::tempdir().unwrap();
    let (session_id, output) = run_helper(directory.path(), "segv");

    assert!(
        !output.status.success(),
        "helper should have crashed, exited with {:?}",
        output.status
    );
    assert!(!session_id.is_empty(), "helper should print its session id");

    let meta = directory
        .path()
        .join(format!("{session_id}.crashmeta.json"));
    assert!(meta.exists(), "pre-crash meta marker should exist");

    let dump = wait_for_dump(directory.path(), &session_id);
    let text = String::from_utf8_lossy(&dump);
    assert!(
        text.contains("signal="),
        "dump must record the signal: {text}"
    );
    assert!(
        text.contains("frame="),
        "dump must record at least one backtrace frame: {text}"
    );

    let reporter = CrashReporter::new("dev.kael.crash-helper", BreadcrumbBuffer::new(8)).unwrap();
    let mut reporter = reporter;
    reporter.set_reports_dir(directory.path()).unwrap();

    let pending = reporter.pending_native_crashes().unwrap();
    let crash = pending
        .iter()
        .find(|crash| crash.context.session_id == session_id)
        .expect("the crashed session should be detected");
    assert!(crash.has_native_record(), "a native record should decode");
    let signal = crash.signal.as_ref().unwrap();
    assert_eq!(
        signal.signal,
        libc::SIGSEGV as i64,
        "signal should be SIGSEGV"
    );
    assert!(!signal.frames.is_empty(), "frames should be captured");
    assert!(crash.summary().contains("SIGSEGV"));

    let summary =
        pollster::block_on(reporter.check_and_submit_pending(CrashConsent::withheld())).unwrap();
    assert_eq!(summary.native_crashes, 1);
    assert!(!summary.submitted, "no submit without consent");

    let json_reports = reporter.pending_reports().unwrap();
    assert!(
        !json_reports.is_empty(),
        "a JSON crash report should be assembled"
    );
}

#[test]
fn captures_real_abort() {
    let directory = tempfile::tempdir().unwrap();
    let (session_id, output) = run_helper(directory.path(), "abort");

    assert!(!output.status.success(), "abort helper should not succeed");
    assert!(!session_id.is_empty());

    let dump = wait_for_dump(directory.path(), &session_id);
    let text = String::from_utf8_lossy(&dump);
    let expected = format!("signal={}", libc::SIGABRT);
    assert!(
        text.contains(&expected),
        "dump should record SIGABRT ({expected}): {text}"
    );
}
