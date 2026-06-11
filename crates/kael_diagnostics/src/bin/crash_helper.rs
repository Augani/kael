//! Test helper that installs native crash handlers and then triggers a real
//! native crash on demand. Driven by the `native_crash` integration test.
//!
//! Usage: `crash_helper <reports_dir> <mode>` where mode is `segv` or `abort`.

use std::{io::Write as _, path::PathBuf};

use kael_diagnostics::{BreadcrumbBuffer, CrashReporter};

fn main() {
    let mut args = std::env::args().skip(1);
    let reports_dir = PathBuf::from(args.next().expect("reports_dir argument required"));
    let mode = args.next().unwrap_or_else(|| "segv".to_string());

    std::fs::create_dir_all(&reports_dir).expect("failed to create reports dir");

    let mut reporter =
        CrashReporter::new("dev.kael.crash-helper", BreadcrumbBuffer::new(8)).expect("reporter");
    reporter.set_release("test-release");
    reporter.set_environment("test");
    reporter
        .set_reports_dir(&reports_dir)
        .expect("set reports dir");

    let session_id = reporter.session_id().to_string();
    println!("{session_id}");
    std::io::stdout().flush().ok();

    reporter.install_native().expect("install native handlers");

    match mode.as_str() {
        "segv" => trigger_segv(),
        "abort" => trigger_abort(),
        other => panic!("unknown crash mode: {other}"),
    }
}

#[inline(never)]
fn trigger_segv() {
    // SAFETY: intentionally dereferences a null pointer to provoke a real
    // SIGSEGV for the crash-capture integration test.
    unsafe {
        let pointer = std::ptr::null_mut::<u8>();
        std::ptr::write_volatile(pointer, 42);
    }
    std::process::exit(0);
}

fn trigger_abort() {
    std::process::abort();
}
