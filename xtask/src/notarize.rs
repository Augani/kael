use anyhow::{Context as _, Result, bail};
use std::env;
use std::ffi::OsString;
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

    let _team_id = match &signing.macos_team_id {
        Some(id) => id,
        None => {
            println!("notarization: no macOS team ID configured, skipping");
            return Ok(());
        }
    };

    let configured_profile = env::var("KAEL_NOTARY_PROFILE")
        .ok()
        .filter(|profile| !profile.trim().is_empty());
    let profile = match configured_profile.as_deref() {
        Some(profile) => profile,
        None if options.dry_run => "<KAEL_NOTARY_PROFILE>",
        None => {
            bail!(
                "KAEL_NOTARY_PROFILE is required for non-interactive macOS notarization; create it with `xcrun notarytool store-credentials`"
            )
        }
    };

    let mut submit_cmd = Command::new("xcrun");
    submit_cmd.args(notarytool_submit_args(artifact, profile));

    if options.dry_run {
        if configured_profile.is_none() {
            println!("dry-run: KAEL_NOTARY_PROFILE must be configured before notarization");
        }
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

fn notarytool_submit_args(artifact: &Path, profile: &str) -> Vec<OsString> {
    vec![
        OsString::from("notarytool"),
        OsString::from("submit"),
        artifact.as_os_str().to_owned(),
        OsString::from("--keychain-profile"),
        OsString::from(profile),
        OsString::from("--wait"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notarization_uses_the_non_interactive_keychain_profile() {
        let args = notarytool_submit_args(Path::new("Kael.dmg"), "kael-notary");

        assert_eq!(
            args,
            [
                "notarytool",
                "submit",
                "Kael.dmg",
                "--keychain-profile",
                "kael-notary",
                "--wait",
            ]
            .map(OsString::from)
        );
    }
}
