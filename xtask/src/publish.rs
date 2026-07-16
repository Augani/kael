use anyhow::{Context as _, Result};
use std::path::Path;
use std::process::{Command, Stdio};

pub struct PublishOptions {
    pub dry_run: bool,
    pub tag: Option<String>,
}

pub fn run(artifacts: &[&Path], options: &PublishOptions) -> Result<()> {
    let tag = options.tag.as_deref().unwrap_or("latest");

    let gh_available = Command::new("gh")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());

    for artifact in artifacts {
        if options.dry_run {
            println!(
                "dry-run: would upload {} to GitHub release {}",
                artifact.display(),
                tag
            );
            continue;
        }

        if gh_available {
            let mut cmd = Command::new("gh");
            cmd.args(["release", "upload", tag])
                .arg(artifact)
                .arg("--clobber");

            let status = cmd
                .status()
                .with_context(|| "failed to run gh release upload")?;

            if !status.success() {
                eprintln!(
                    "warning: gh release upload failed for {}",
                    artifact.display()
                );
            } else {
                println!("published: {} -> release {}", artifact.display(), tag);
            }
        } else {
            println!(
                "publishing: {} (gh CLI not available, manual upload required)",
                artifact.display()
            );
        }
    }

    Ok(())
}
