#[cfg(unix)]
use std::io::Write as _;

use anyhow::{Result, bail};
#[cfg(unix)]
use kael_diagnostics::{BreadcrumbBuffer, CrashReporter};

#[cfg(unix)]
fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let reports_dir = arguments
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing reports directory"))?;
    let mode = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(|| anyhow::anyhow!("missing crash mode"))?;
    if arguments.next().is_some() {
        bail!("unexpected crash-helper arguments");
    }

    let mut reporter = CrashReporter::new(
        "dev.kael.diagnostics.crash-helper",
        BreadcrumbBuffer::new(8),
    )?;
    reporter.set_reports_dir(reports_dir)?;
    reporter.install_native()?;
    println!("{}", reporter.session_id());
    std::io::stdout().flush()?;

    match mode.as_str() {
        "segv" => unsafe {
            libc::raise(libc::SIGSEGV);
        },
        "abort" => std::process::abort(),
        _ => bail!("unsupported crash mode {mode}"),
    }
    unreachable!("crash signal unexpectedly returned")
}

#[cfg(not(unix))]
fn main() -> Result<()> {
    bail!("the crash helper is only available on Unix targets")
}
