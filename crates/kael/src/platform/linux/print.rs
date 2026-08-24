use crate::{PlatformPrintJob, PrintOrientation, platform::print_pdf::render_print_job_pdf};
use anyhow::{Context as _, Result, anyhow, ensure};
use ashpd::{
    WindowIdentifier,
    desktop::print::{
        Orientation, OutputFileFormat, PageSetup, PreparePrintOptions, PrintOptions, PrintProxy,
        Settings,
    },
};
use std::{
    fs::{File, OpenOptions},
    io::{Seek as _, SeekFrom, Write as _},
    os::{
        fd::AsFd as _,
        unix::fs::{MetadataExt as _, OpenOptionsExt as _},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

const POINTS_PER_INCH: f64 = 72.0;
const MILLIMETERS_PER_INCH: f64 = 25.4;

struct TemporaryPrintPdf {
    file: File,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl Drop for TemporaryPrintPdf {
    fn drop(&mut self) {
        // Only unlink the directory entry created by this instance. Another
        // process could rename it and replace the path while a portal request
        // is active; blindly removing that replacement would be destructive.
        match std::fs::symlink_metadata(&self.path) {
            Ok(metadata)
                if metadata.file_type().is_file()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode =>
            {
                if let Err(error) = std::fs::remove_file(&self.path)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    log::warn!(
                        "failed to remove temporary Kael print PDF {}: {error}",
                        self.path.display()
                    );
                }
            }
            Ok(_) => log::warn!(
                "temporary Kael print PDF path was replaced before cleanup: {}",
                self.path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!(
                "failed to inspect temporary Kael print PDF {}: {error}",
                self.path.display()
            ),
        }
    }
}

impl TemporaryPrintPdf {
    fn new(bytes: &[u8]) -> Result<Self> {
        let directory = std::env::temp_dir();
        ensure!(
            directory.is_absolute(),
            "the OS temporary directory is not absolute"
        );
        for _ in 0..16 {
            let path = directory.join(format!("kael-print-{}.pdf", uuid::Uuid::new_v4()));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut file) => {
                    if let Err(error) = (|| -> std::io::Result<()> {
                        file.write_all(bytes)?;
                        file.flush()?;
                        file.seek(SeekFrom::Start(0))?;
                        Ok(())
                    })() {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return Err(error).context("writing temporary Kael print PDF");
                    }
                    let metadata = match file.metadata() {
                        Ok(metadata) => metadata,
                        Err(error) => {
                            drop(file);
                            let _ = std::fs::remove_file(&path);
                            return Err(error).context("inspecting temporary Kael print PDF");
                        }
                    };
                    return Ok(Self {
                        file,
                        path,
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).context("creating temporary Kael print PDF");
                }
            }
        }
        Err(anyhow!(
            "could not allocate a unique temporary Kael print PDF after 16 attempts"
        ))
    }
}

/// Spool without user interaction through the system CUPS client. A missing
/// CUPS client or default printer is a real error; this function never opens a
/// dialog as an implicit fallback.
pub(crate) fn print_silent(job: PlatformPrintJob) -> Result<()> {
    let title = job.title.clone();
    let pdf = render_print_job_pdf(&job)?;
    let (program, title_flag) = resolve_cups_client().ok_or_else(|| {
        anyhow!(
            "silent Linux printing requires the CUPS `lp` or `lpr` client at a standard system path"
        )
    })?;
    let mut child = Command::new(program)
        .arg(title_flag)
        .arg(title.as_ref())
        // With no file operands, both CUPS clients read the document from
        // stdin. This keeps an attacker-controlled temporary directory from
        // substituting a different path between creation and spooler open.
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("starting Linux print spooler {}", program.display()))?;
    write_pdf_to_child_stdin(&mut child, &pdf)
        .with_context(|| format!("streaming PDF to Linux print spooler {}", program.display()))?;
    let status = child
        .wait()
        .with_context(|| format!("waiting for Linux print spooler {}", program.display()))?;
    ensure!(
        status.success(),
        "Linux print spooler rejected the job with status {status}"
    );
    Ok(())
}

fn write_pdf_to_child_stdin(child: &mut Child, pdf: &[u8]) -> Result<()> {
    let write_result = (|| -> Result<()> {
        let mut stdin = child
            .stdin
            .take()
            .context("Linux print spooler stdin was not piped")?;
        stdin.write_all(pdf)?;
        stdin.flush()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        // Never leave a failed spooler child or zombie behind. kill() can
        // legitimately fail if the client exited after closing its pipe.
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    Ok(())
}

fn resolve_cups_client() -> Option<(&'static Path, &'static str)> {
    const CLIENTS: [(&str, &str); 4] = [
        ("/usr/bin/lp", "-t"),
        ("/bin/lp", "-t"),
        ("/usr/bin/lpr", "-J"),
        ("/bin/lpr", "-J"),
    ];
    CLIENTS
        .into_iter()
        .find(|(path, _)| Path::new(path).is_file())
        .map(|(path, title_flag)| (Path::new(path), title_flag))
}

/// Show the desktop's native print UI through the standardized XDG Print
/// portal and submit the exact PDF accepted by the user.
pub(crate) async fn show_print_dialog(
    job: PlatformPrintJob,
    parent: Option<WindowIdentifier>,
) -> Result<()> {
    let title = job.title.clone();
    let orientation = match job.orientation {
        PrintOrientation::Portrait => Orientation::Portrait,
        PrintOrientation::Landscape => Orientation::Landscape,
    };
    let points_to_mm = |points: f32| f64::from(points) * MILLIMETERS_PER_INCH / POINTS_PER_INCH;
    let settings = Settings::default()
        .set_orientation(orientation)
        .set_n_copies(1u32);
    let page_setup = PageSetup::default()
        .set_width(points_to_mm(job.page_size.width.0))
        .set_height(points_to_mm(job.page_size.height.0))
        .set_margin_top(points_to_mm(job.margins.top.0))
        .set_margin_right(points_to_mm(job.margins.right.0))
        .set_margin_bottom(points_to_mm(job.margins.bottom.0))
        .set_margin_left(points_to_mm(job.margins.left.0))
        .set_orientation(orientation);
    let pdf = render_print_job_pdf(&job)?;
    let temporary = TemporaryPrintPdf::new(&pdf)?;
    let proxy = PrintProxy::new()
        .await
        .context("connecting to the XDG desktop Print portal")?;
    let prepared = proxy
        .prepare_print(
            parent.as_ref(),
            title.as_ref(),
            settings,
            page_setup,
            PreparePrintOptions::default()
                .set_modal(true)
                .set_accept_label("Print")
                .set_supported_output_file_formats([OutputFileFormat::Pdf]),
        )
        .await
        .context("showing the XDG desktop print dialog")?
        .response()
        .context("the XDG desktop print dialog was cancelled or rejected")?;
    proxy
        .print(
            parent.as_ref(),
            title.as_ref(),
            &temporary.file.as_fd(),
            PrintOptions::default()
                .set_modal(true)
                .set_token(prepared.token)
                .set_supported_output_file_formats([OutputFileFormat::Pdf]),
        )
        .await
        .context("submitting PDF to the XDG desktop Print portal")?
        .response()
        .context("the XDG desktop Print portal rejected the PDF")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_pdf_is_private_and_removed_on_drop() {
        let path = {
            let temporary = TemporaryPrintPdf::new(b"%PDF-1.7\n%%EOF\n").unwrap();
            let metadata = temporary.file.metadata().unwrap();
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
            assert!(temporary.path.exists());
            temporary.path.clone()
        };
        assert!(!path.exists());
    }

    #[test]
    fn cups_client_resolution_never_uses_path_lookup() {
        if let Some((path, flag)) = resolve_cups_client() {
            assert!(path.is_absolute());
            assert!(matches!(flag, "-t" | "-J"));
        }
    }

    #[test]
    fn pdf_bytes_are_streamed_through_child_stdin() {
        let pdf = b"%PDF-1.7\nstream proof\n%%EOF\n";
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        write_pdf_to_child_stdin(&mut child, pdf).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, pdf);
    }

    #[test]
    fn cleanup_does_not_remove_a_replaced_path() {
        let temporary = TemporaryPrintPdf::new(b"original").unwrap();
        let replacement_path = temporary.path.clone();
        let moved_path = replacement_path.with_extension(format!("{}.moved", uuid::Uuid::new_v4()));
        std::fs::rename(&replacement_path, &moved_path).unwrap();
        std::fs::write(&replacement_path, b"replacement").unwrap();

        drop(temporary);

        assert_eq!(std::fs::read(&replacement_path).unwrap(), b"replacement");
        std::fs::remove_file(replacement_path).unwrap();
        std::fs::remove_file(moved_path).unwrap();
    }
}
