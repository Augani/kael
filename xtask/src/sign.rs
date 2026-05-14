use anyhow::{Context as _, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::DistConfig;

pub struct SignOptions {
    pub dry_run: bool,
}

pub fn run(config: &DistConfig, artifact: &Path, options: &SignOptions) -> Result<()> {
    if cfg!(target_os = "macos") {
        sign_macos(config, artifact, options)
    } else if cfg!(target_os = "windows") {
        sign_windows(config, artifact, options)
    } else if cfg!(target_os = "linux") {
        sign_linux(config, artifact, options)
    } else {
        bail!("unsupported target OS for signing");
    }
}

fn sign_macos(config: &DistConfig, artifact: &Path, options: &SignOptions) -> Result<()> {
    let signing = match &config.signing {
        Some(s) => s,
        None => {
            println!("signing: no signing config, skipping");
            return Ok(());
        }
    };

    let certificate = match &signing.macos_certificate {
        Some(c) => c,
        None => {
            println!("signing: no macOS certificate configured, skipping");
            return Ok(());
        }
    };

    let mut cmd = Command::new("codesign");
    cmd.args([
        "--sign",
        certificate,
        "--deep",
        "--force",
        "--options",
        "runtime",
    ])
    .arg(artifact);

    if options.dry_run {
        println!("dry-run: would run {:?}", cmd);
        return Ok(());
    }

    let status = cmd
        .status()
        .with_context(|| "failed to run codesign — is it installed?")?;

    if !status.success() {
        bail!("codesign exited with status: {}", status);
    }

    println!("signed: {}", artifact.display());
    Ok(())
}

fn sign_windows(config: &DistConfig, artifact: &Path, options: &SignOptions) -> Result<()> {
    let signing = match &config.signing {
        Some(s) => s,
        None => {
            println!("signing: no signing config, skipping");
            return Ok(());
        }
    };

    let certificate = match &signing.windows_certificate {
        Some(c) => c,
        None => {
            println!("signing: no Windows certificate configured, skipping");
            return Ok(());
        }
    };

    let password = signing
        .windows_certificate_password
        .as_deref()
        .unwrap_or("");

    let mut cmd = Command::new("signtool");
    cmd.args(["sign", "/f"])
        .arg(certificate)
        .arg("/p")
        .arg(password)
        .arg("/tr")
        .arg("http://timestamp.digicert.com")
        .arg("/td")
        .arg("sha256")
        .arg("/fd")
        .arg("sha256")
        .arg(artifact);

    if options.dry_run {
        println!("dry-run: would run {:?}", cmd);
        return Ok(());
    }

    let status = cmd
        .status()
        .with_context(|| "failed to run signtool — is it installed?")?;

    if !status.success() {
        bail!("signtool exited with status: {}", status);
    }

    println!("signed: {}", artifact.display());
    Ok(())
}

fn sign_linux(_config: &DistConfig, artifact: &Path, options: &SignOptions) -> Result<()> {
    let mut cmd = Command::new("gpg");
    cmd.args(["--detach-sign", "--armor", "--output"])
        .arg(format!("{}.asc", artifact.display()))
        .arg(artifact);

    if options.dry_run {
        println!("dry-run: would run {:?}", cmd);
        return Ok(());
    }

    let status = cmd
        .status()
        .with_context(|| "failed to run gpg — is it installed and configured?")?;

    if !status.success() {
        bail!("gpg exited with status: {}", status);
    }

    println!("signed: {}", artifact.display());
    Ok(())
}
