use anyhow::{Context as _, Result, bail};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct PublishOptions {
    pub dry_run: bool,
    pub tag: Option<String>,
}

pub fn run(artifacts: &[&Path], options: &PublishOptions) -> Result<()> {
    anyhow::ensure!(
        !artifacts.is_empty(),
        "at least one release artifact is required"
    );
    let tag = options
        .tag
        .as_deref()
        .context("a concrete GitHub release tag is required")?;
    validate_release_tag(tag)?;

    for artifact in artifacts {
        validate_upload_artifact(artifact, options.dry_run)?;
    }

    let gh_available = options.dry_run
        || Command::new("gh")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    if !gh_available {
        bail!("gh CLI is required to upload release artifacts");
    }

    for artifact in artifacts {
        if options.dry_run {
            println!(
                "dry-run: would upload {} to GitHub release {}",
                artifact.display(),
                tag
            );
            continue;
        }

        let mut cmd = Command::new("gh");
        cmd.args(["release", "upload", tag])
            .arg(artifact)
            .arg("--clobber");

        let status = cmd
            .status()
            .with_context(|| "failed to run gh release upload")?;

        if !status.success() {
            bail!(
                "gh release upload failed for {} with status {status}",
                artifact.display()
            );
        }
        println!("published: {} -> release {}", artifact.display(), tag);
    }

    Ok(())
}

fn validate_upload_artifact(artifact: &Path, allow_planned: bool) -> Result<()> {
    match fs::symlink_metadata(artifact) {
        Ok(metadata) if metadata.file_type().is_file() && (allow_planned || metadata.len() > 0) => {
            Ok(())
        }
        Ok(metadata) if metadata.file_type().is_file() => {
            bail!("release artifact must not be empty: {}", artifact.display())
        }
        Ok(_) => bail!(
            "release artifact must be a regular file: {}",
            artifact.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound && allow_planned => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect release artifact: {}", artifact.display())),
    }
}

fn validate_release_tag(tag: &str) -> Result<()> {
    if tag.is_empty()
        || tag.len() > 255
        || tag.starts_with('-')
        || tag.chars().any(|character| character.is_control())
    {
        bail!("release tag must be a safe, non-empty value");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_validation_rejects_directories_and_missing_real_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        assert!(validate_upload_artifact(directory.path(), false).is_err());
        assert!(validate_upload_artifact(&directory.path().join("missing.dmg"), false).is_err());
        let empty = directory.path().join("empty.dmg");
        fs::write(&empty, []).unwrap();
        assert!(validate_upload_artifact(&empty, false).is_err());
        assert!(validate_upload_artifact(&empty, true).is_ok());
    }

    #[test]
    fn upload_validation_accepts_files_and_planned_dry_run_paths() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("app.dmg");
        fs::write(&file, b"artifact").unwrap();
        assert!(validate_upload_artifact(&file, false).is_ok());
        assert!(validate_upload_artifact(&directory.path().join("planned.dmg"), true).is_ok());
    }

    #[test]
    fn release_tags_must_be_explicit_and_safe() {
        assert!(validate_release_tag("v1.2.3").is_ok());
        assert!(validate_release_tag("").is_err());
        assert!(validate_release_tag("--help").is_err());
        assert!(validate_release_tag("v1.2.3\nmalicious").is_err());
    }
}
