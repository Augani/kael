use anyhow::{Context as _, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::DistConfig;

pub struct NotarizeOptions {
    pub dry_run: bool,
}

pub fn run(config: &DistConfig, artifact: &Path, options: &NotarizeOptions) -> Result<()> {
    if cfg!(target_os = "macos") {
        notarize_macos(config, artifact, options)
    } else {
        println!("notarization: only supported on macOS, skipping");
        Ok(())
    }
}

fn notarize_macos(config: &DistConfig, artifact: &Path, options: &NotarizeOptions) -> Result<()> {
    let signing = match &config.signing {
        Some(s) => s,
        None => {
            println!("notarization: no signing config, skipping");
            return Ok(());
        }
    };

    let team_id = match &signing.macos_team_id {
        Some(id) => id,
        None => {
            println!("notarization: no macOS team ID configured, skipping");
            return Ok(());
        }
    };

    let mut submit_cmd = Command::new("xcrun");
    submit_cmd
        .args(["notarytool", "submit", "--wait", "--team-id", team_id])
        .arg(artifact);

    if options.dry_run {
        println!("dry-run: would run {:?}", submit_cmd);
        println!("dry-run: would run stapler staple {}", artifact.display());
        return Ok(());
    }

    let status = submit_cmd
        .status()
        .with_context(|| "failed to run notarytool — is Xcode Command Line Tools installed?")?;

    if !status.success() {
        bail!("notarytool submit exited with status: {}", status);
    }

    let mut staple_cmd = Command::new("xcrun");
    staple_cmd.args(["stapler", "staple"]).arg(artifact);

    let status = staple_cmd
        .status()
        .with_context(|| "failed to run stapler")?;

    if !status.success() {
        bail!("stapler exited with status: {}", status);
    }

    println!("notarized and stapled: {}", artifact.display());
    Ok(())
}
