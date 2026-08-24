use anyhow::{Context as _, Result, bail};
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
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
    let environment_certificate = env::var("KAEL_MACOS_SIGNING_IDENTITY")
        .ok()
        .filter(|certificate| !certificate.trim().is_empty());
    let certificate = match environment_certificate.as_deref().or_else(|| {
        config
            .signing
            .as_ref()
            .and_then(|signing| signing.macos_certificate.as_deref())
    }) {
        Some(certificate) => certificate,
        None => {
            println!("signing: no macOS certificate configured, skipping");
            return Ok(());
        }
    };

    let mut cmd = Command::new("codesign");
    cmd.args(macos_codesign_args(certificate, artifact));

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

fn macos_codesign_args(certificate: &str, artifact: &Path) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("--sign"),
        OsString::from(certificate),
        OsString::from("--force"),
        OsString::from("--timestamp"),
    ];
    let is_disk_image = artifact
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dmg"));
    if !is_disk_image {
        args.extend([
            OsString::from("--deep"),
            OsString::from("--options"),
            OsString::from("runtime"),
        ]);
    }
    args.push(artifact.as_os_str().to_owned());
    args
}

fn sign_windows(config: &DistConfig, artifact: &Path, options: &SignOptions) -> Result<()> {
    let environment_certificate = env::var_os("KAEL_WINDOWS_CERTIFICATE")
        .filter(|certificate| !certificate.is_empty())
        .map(PathBuf::from);
    let certificate = match environment_certificate.as_deref().or_else(|| {
        config
            .signing
            .as_ref()
            .and_then(|signing| signing.windows_certificate.as_deref())
    }) {
        Some(certificate) => certificate,
        None => {
            println!("signing: no Windows certificate configured, skipping");
            return Ok(());
        }
    };

    let environment_password = env::var("KAEL_WINDOWS_CERTIFICATE_PASSWORD")
        .ok()
        .filter(|password| !password.is_empty());
    let password = environment_password.as_deref().or_else(|| {
        config
            .signing
            .as_ref()
            .and_then(|signing| signing.windows_certificate_password.as_deref())
    });

    let mut cmd = Command::new("signtool");
    cmd.args(windows_signtool_args(certificate, password, artifact));

    if options.dry_run {
        println!("{}", windows_dry_run_message(artifact, password.is_some()));
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

fn windows_signtool_args(
    certificate: &Path,
    password: Option<&str>,
    artifact: &Path,
) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("sign"),
        OsString::from("/f"),
        certificate.as_os_str().to_owned(),
    ];
    if let Some(password) = password {
        args.extend([OsString::from("/p"), OsString::from(password)]);
    }
    args.extend([
        OsString::from("/tr"),
        OsString::from("http://timestamp.digicert.com"),
        OsString::from("/td"),
        OsString::from("sha256"),
        OsString::from("/fd"),
        OsString::from("sha256"),
        artifact.as_os_str().to_owned(),
    ]);
    args
}

fn windows_dry_run_message(artifact: &Path, password_configured: bool) -> String {
    format!(
        "dry-run: would sign {} with signtool, SHA-256, and a trusted timestamp{}",
        artifact.display(),
        if password_configured {
            " (certificate password configured and redacted)"
        } else {
            ""
        }
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_disk_image_signing_uses_a_secure_timestamp_without_app_flags() {
        let args = macos_codesign_args("Developer ID Application: Kael", Path::new("Kael.dmg"));

        assert!(args.contains(&OsString::from("--timestamp")));
        assert!(!args.contains(&OsString::from("--deep")));
        assert!(!args.contains(&OsString::from("runtime")));
        assert_eq!(args.last(), Some(&OsString::from("Kael.dmg")));
    }

    #[test]
    fn macos_app_signing_enables_hardened_runtime() {
        let args = macos_codesign_args("Developer ID Application: Kael", Path::new("Kael.app"));

        assert!(args.contains(&OsString::from("--timestamp")));
        assert!(args.contains(&OsString::from("--deep")));
        assert!(args.contains(&OsString::from("runtime")));
    }

    #[test]
    fn windows_dry_run_never_renders_the_certificate_password() {
        let message = windows_dry_run_message(Path::new("Kael.msi"), true);

        assert!(message.contains("password configured and redacted"));
        assert!(!message.contains("do-not-render"));
    }

    #[test]
    fn passwordless_windows_signing_omits_the_password_switch() {
        let args = windows_signtool_args(Path::new("certificate.pfx"), None, Path::new("Kael.msi"));

        assert!(!args.contains(&OsString::from("/p")));
        assert_eq!(args.last(), Some(&OsString::from("Kael.msi")));
    }
}
