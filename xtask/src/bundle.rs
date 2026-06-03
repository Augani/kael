use anyhow::{bail, Context as _, Result};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

use crate::DistConfig;

pub struct BundleOptions {
    pub dry_run: bool,
    pub binary: Option<PathBuf>,
}

pub fn run(config: &DistConfig, output: &Path, options: &BundleOptions) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(output)?;

    let binary_path = resolve_binary(config, options)?;
    let artifacts = if cfg!(target_os = "macos") {
        bundle_macos(config, output, binary_path.as_deref(), options)?
    } else if cfg!(target_os = "windows") {
        bundle_windows(config, output, binary_path.as_deref(), options)?
    } else if cfg!(target_os = "linux") {
        bundle_linux(config, output, binary_path.as_deref(), options)?
    } else {
        bail!("unsupported target OS");
    };

    Ok(artifacts)
}

fn resolve_binary(config: &DistConfig, options: &BundleOptions) -> Result<Option<PathBuf>> {
    if let Some(ref binary) = options.binary {
        if binary.exists() {
            return Ok(Some(binary.clone()));
        } else {
            bail!("specified binary not found: {}", binary.display());
        }
    }

    let app_name = config.name.to_lowercase().replace(' ', "-");
    let default_path = PathBuf::from("target/release").join(&app_name);
    if default_path.exists() {
        Ok(Some(default_path))
    } else if options.dry_run {
        eprintln!(
            "warning: default binary not found at {}",
            default_path.display()
        );
        Ok(None)
    } else {
        bail!(
            "default binary not found at {}; build release binary first or pass --binary",
            default_path.display()
        );
    }
}

fn bundle_macos(
    config: &DistConfig,
    output: &Path,
    binary: Option<&Path>,
    options: &BundleOptions,
) -> Result<Vec<PathBuf>> {
    let app_name = &config.name;
    let app_name_slug = config.name.to_lowercase().replace(' ', "-");
    let bundle_name = format!("{}.app", app_name);
    let bundle_dir = output.join(&bundle_name);
    let contents_dir = bundle_dir.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let resources_dir = contents_dir.join("Resources");

    if !options.dry_run {
        fs::create_dir_all(&macos_dir)?;
        fs::create_dir_all(&resources_dir)?;
    }

    let plist_path = contents_dir.join("Info.plist");
    let plist = generate_info_plist(config, &app_name_slug);
    if options.dry_run {
        println!(
            "dry-run: would write Info.plist to {}",
            plist_path.display()
        );
    } else {
        fs::write(&plist_path, plist)?;
    }

    if let Some(binary) = binary {
        let dest = macos_dir.join(&app_name_slug);
        if options.dry_run {
            println!("dry-run: would copy binary to {}", dest.display());
        } else {
            fs::copy(binary, &dest)
                .with_context(|| format!("failed to copy binary to {}", dest.display()))?;
        }
    }

    if let Some(ref icon) = config.icons.macos {
        let dest = resources_dir.join(icon.file_name().unwrap_or_default());
        if options.dry_run {
            println!("dry-run: would copy icon to {}", dest.display());
        } else {
            copy_required_asset(icon, &dest)?;
        }
    }

    println!("macOS bundle created: {}", bundle_dir.display());

    let mut artifacts = vec![bundle_dir.clone()];
    #[cfg(target_os = "macos")]
    if !options.dry_run && binary.is_some() {
        let identity = config
            .signing
            .as_ref()
            .and_then(|signing| signing.macos_certificate.as_deref());
        match identity {
            Some(identity) => {
                codesign_app(&bundle_dir, identity).context("failed to codesign .app")?;
                println!(
                    "codesigned {} with identity {identity}",
                    bundle_dir.display()
                );
            }
            None => println!(
                "note: no signing.macos_certificate configured — producing an unsigned build"
            ),
        }

        let dmg_path = output.join(format!("{app_name_slug}.dmg"));
        let dmg =
            create_dmg(&bundle_dir, &dmg_path, app_name).context("failed to create macOS .dmg")?;
        println!("macOS .dmg created: {}", dmg.display());

        if identity.is_some() {
            match std::env::var("KAEL_NOTARY_PROFILE").ok() {
                Some(profile) if !profile.is_empty() => {
                    notarize_and_staple(&dmg, &profile).context("failed to notarize macOS .dmg")?;
                    println!("notarized and stapled {}", dmg.display());
                }
                _ => println!(
                    "note: KAEL_NOTARY_PROFILE not set — skipping notarization of {}",
                    dmg.display()
                ),
            }
        }
        artifacts.push(dmg);
    }
    Ok(artifacts)
}

/// Build a compressed `.dmg` disk image containing `app_bundle` using `hdiutil`.
///
/// This produces the installer container; code-signing and notarization (which
/// require an Apple Developer certificate) are a separate downstream step.
#[cfg(target_os = "macos")]
fn create_dmg(app_bundle: &Path, dmg_path: &Path, volume_name: &str) -> Result<PathBuf> {
    if dmg_path.exists() {
        fs::remove_file(dmg_path)?;
    }
    let status = Command::new("hdiutil")
        .arg("create")
        .arg("-volname")
        .arg(volume_name)
        .arg("-srcfolder")
        .arg(app_bundle)
        .arg("-ov")
        .arg("-format")
        .arg("UDZO")
        .arg(dmg_path)
        .status()
        .context("failed to run hdiutil create")?;
    if !status.success() {
        bail!("hdiutil create failed with status {status}");
    }
    Ok(dmg_path.to_path_buf())
}

/// Code-sign an `.app` bundle with the hardened runtime, ready for notarization.
///
/// `identity` is a keychain identity (e.g. `"Developer ID Application: Acme (TEAMID)"`).
/// Requires a valid Apple Developer certificate in the keychain.
#[cfg(target_os = "macos")]
fn codesign_app(app_bundle: &Path, identity: &str) -> Result<()> {
    let status = Command::new("codesign")
        .args(["--force", "--deep", "--options", "runtime", "--timestamp"])
        .arg("--sign")
        .arg(identity)
        .arg(app_bundle)
        .status()
        .context("failed to run codesign")?;
    if !status.success() {
        bail!("codesign failed with status {status}");
    }
    Ok(())
}

/// Submit a `.dmg` to Apple's notary service and staple the resulting ticket.
///
/// `keychain_profile` is a profile name previously stored with
/// `xcrun notarytool store-credentials`. Blocks until notarization completes.
#[cfg(target_os = "macos")]
fn notarize_and_staple(dmg: &Path, keychain_profile: &str) -> Result<()> {
    let submit = Command::new("xcrun")
        .args(["notarytool", "submit"])
        .arg(dmg)
        .args(["--keychain-profile", keychain_profile, "--wait"])
        .status()
        .context("failed to run xcrun notarytool submit")?;
    if !submit.success() {
        bail!("notarytool submit failed with status {submit}");
    }
    let staple = Command::new("xcrun")
        .args(["stapler", "staple"])
        .arg(dmg)
        .status()
        .context("failed to run xcrun stapler")?;
    if !staple.success() {
        bail!("stapler staple failed with status {staple}");
    }
    Ok(())
}

fn generate_info_plist(config: &DistConfig, executable_name: &str) -> String {
    let bundle_id = &config.app_id;
    let bundle_name = &config.name;
    let version = &config.version;
    let copyright = config.bundle.copyright.as_deref().unwrap_or("");
    let category = config.bundle.category.as_deref().unwrap_or("");
    let min_version = config
        .bundle
        .minimum_system_version
        .as_deref()
        .unwrap_or("");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>{bundle_id}</string>
    <key>CFBundleName</key>
    <string>{bundle_name}</string>
    <key>CFBundleDisplayName</key>
    <string>{bundle_name}</string>
    <key>CFBundleVersion</key>
    <string>{version}</string>
    <key>CFBundleShortVersionString</key>
    <string>{version}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>{executable_name}</string>
    <key>LSMinimumSystemVersion</key>
    <string>{min_version}</string>
    <key>NSHumanReadableCopyright</key>
    <string>{copyright}</string>
    <key>LSApplicationCategoryType</key>
    <string>{category}</string>
</dict>
</plist>"#
    )
}

fn bundle_windows(
    config: &DistConfig,
    output: &Path,
    binary: Option<&Path>,
    options: &BundleOptions,
) -> Result<Vec<PathBuf>> {
    let app_name = config.name.replace(' ', "-");
    let bundle_dir = output.join(&app_name);

    if !options.dry_run {
        fs::create_dir_all(&bundle_dir)?;
    }

    let wix_path = bundle_dir.join(format!("{app_name}.wxs"));
    let wix_source = generate_wix_source(config, &app_name);
    if options.dry_run {
        println!(
            "dry-run: would write WiX installer source to {}",
            wix_path.display()
        );
    } else {
        fs::write(&wix_path, wix_source)?;
        println!(
            "WiX installer source written: {} (build the .msi with `wix build {}`)",
            wix_path.display(),
            wix_path.display()
        );
    }

    if let Some(binary) = binary {
        let dest = bundle_dir.join(format!("{}.exe", app_name));
        if options.dry_run {
            println!("dry-run: would copy binary to {}", dest.display());
        } else {
            fs::copy(binary, &dest)
                .with_context(|| format!("failed to copy binary to {}", dest.display()))?;
        }
    }

    if let Some(ref icon) = config.icons.windows {
        let dest = bundle_dir.join(icon.file_name().unwrap_or_default());
        if options.dry_run {
            println!("dry-run: would copy icon to {}", dest.display());
        } else {
            copy_required_asset(icon, &dest)?;
        }
    }

    println!("Windows bundle created: {}", bundle_dir.display());
    Ok(vec![bundle_dir])
}

fn bundle_linux(
    config: &DistConfig,
    output: &Path,
    binary: Option<&Path>,
    options: &BundleOptions,
) -> Result<Vec<PathBuf>> {
    let app_name = config.name.to_lowercase().replace(' ', "-");
    let app_dir = output.join(format!("{}.AppDir", app_name));

    let usr_bin = app_dir.join("usr/bin");
    if !options.dry_run {
        fs::create_dir_all(&usr_bin)?;
    }

    let desktop_path = app_dir.join(format!("{}.desktop", app_name));
    let desktop = generate_desktop_entry(config);
    if options.dry_run {
        println!(
            "dry-run: would write .desktop to {}",
            desktop_path.display()
        );
    } else {
        fs::write(&desktop_path, desktop)?;
    }

    let app_run_path = app_dir.join("AppRun");
    let app_run = format!(
        "#!/bin/sh\nexec \"${{APPDIR}}/usr/bin/{}\" \"$@\"\n",
        app_name
    );
    if options.dry_run {
        println!("dry-run: would write AppRun to {}", app_run_path.display());
    } else {
        fs::write(&app_run_path, app_run)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&app_run_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&app_run_path, perms)?;
        }
    }

    if let Some(binary) = binary {
        let dest = usr_bin.join(&app_name);
        if options.dry_run {
            println!("dry-run: would copy binary to {}", dest.display());
        } else {
            fs::copy(binary, &dest)
                .with_context(|| format!("failed to copy binary to {}", dest.display()))?;
        }
    }

    if let Some(ref icon) = config.icons.linux {
        let dest = app_dir.join(icon.file_name().unwrap_or_default());
        if options.dry_run {
            println!("dry-run: would copy icon to {}", dest.display());
        } else {
            copy_required_asset(icon, &dest)?;
        }
    }

    println!("Linux AppDir created: {}", app_dir.display());
    Ok(vec![app_dir])
}

fn copy_required_asset(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() {
        bail!("configured asset does not exist: {}", source.display());
    }
    fs::copy(source, destination)
        .with_context(|| format!("failed to copy asset to {}", destination.display()))?;
    Ok(())
}

fn generate_desktop_entry(config: &DistConfig) -> String {
    let name = &config.name;
    let exec = config.name.to_lowercase().replace(' ', "-");
    let categories = config
        .bundle
        .linux_categories
        .as_ref()
        .map(|c| c.join(";"))
        .unwrap_or_default();

    format!(
        "[Desktop Entry]\nName={name}\nExec={exec}\nType=Application\nCategories={categories}\n"
    )
}

/// Generate a WiX v4 installer source (`.wxs`) that a CI runner compiles into a
/// signed `.msi` via `wix build`. The `UpgradeCode` is derived deterministically
/// from `app_id` so successive releases are recognised as upgrades of one product.
fn generate_wix_source(config: &DistConfig, exe_name: &str) -> String {
    let name = xml_escape(&config.name);
    let version = xml_escape(&config.version);
    let manufacturer = xml_escape(config.bundle.copyright.as_deref().unwrap_or(&config.name));
    let upgrade_code = deterministic_guid(&format!("{}::wix-upgrade-code", config.app_id));
    let exe = xml_escape(exe_name);

    let icon_block = config
        .icons
        .windows
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(|icon_file| {
            let icon = xml_escape(icon_file);
            format!(
                "    <Icon Id=\"AppIcon\" SourceFile=\"{icon}\" />\n    <Property Id=\"ARPPRODUCTICON\" Value=\"AppIcon\" />\n"
            )
        })
        .unwrap_or_default();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="{name}" Manufacturer="{manufacturer}" Version="{version}" UpgradeCode="{upgrade_code}" Language="1033" Scope="perMachine" Compressed="yes">
    <MajorUpgrade DowngradeErrorMessage="A newer version of {name} is already installed." />
    <MediaTemplate EmbedCab="yes" />
{icon_block}    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="{name}">
        <Component Id="MainExecutable">
          <File Id="AppExe" Source="{exe}.exe" KeyPath="yes" />
        </Component>
      </Directory>
    </StandardDirectory>
    <Feature Id="Main" Title="{name}" Level="1">
      <ComponentRef Id="MainExecutable" />
    </Feature>
  </Package>
</Wix>
"#
    )
}

/// Derive a stable RFC-4122 GUID from `seed` (SHA-256 based, version-5 style).
///
/// Used for the WiX `UpgradeCode`, which must be identical across releases of the
/// same product but distinct between products — exactly what a content hash gives.
fn deterministic_guid(seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = hex::encode_upper(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn xml_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_artifact_planning_macos() {
        let config = DistConfig {
            app_id: "com.test.app".to_string(),
            name: "Test App".to_string(),
            version: "1.0.0".to_string(),
            icons: crate::IconSet {
                macos: None,
                windows: None,
                linux: None,
            },
            bundle: crate::BundleMetadata {
                copyright: None,
                category: None,
                minimum_system_version: None,
                file_description: None,
                linux_categories: None,
            },
            signing: None,
            updater: None,
        };
        let app_name = &config.name;
        let expected = format!("{}.app", app_name);
        assert_eq!(expected, "Test App.app");
    }

    #[test]
    fn test_bundle_artifact_planning_linux() {
        let config = DistConfig {
            app_id: "com.test.app".to_string(),
            name: "Test App".to_string(),
            version: "1.0.0".to_string(),
            icons: crate::IconSet {
                macos: None,
                windows: None,
                linux: None,
            },
            bundle: crate::BundleMetadata {
                copyright: None,
                category: None,
                minimum_system_version: None,
                file_description: None,
                linux_categories: None,
            },
            signing: None,
            updater: None,
        };
        let app_name = config.name.to_lowercase().replace(' ', "-");
        let expected = format!("{}.AppDir", app_name);
        assert_eq!(expected, "test-app.AppDir");
    }

    fn sample_config() -> DistConfig {
        DistConfig {
            app_id: "com.kael.demo".to_string(),
            name: "Kael Demo".to_string(),
            version: "1.2.3".to_string(),
            icons: crate::IconSet {
                macos: None,
                windows: None,
                linux: None,
            },
            bundle: crate::BundleMetadata {
                copyright: Some("Acme Inc".to_string()),
                category: None,
                minimum_system_version: None,
                file_description: None,
                linux_categories: None,
            },
            signing: None,
            updater: None,
        }
    }

    #[test]
    fn wix_source_has_buildable_structure() {
        let wix = generate_wix_source(&sample_config(), "kael-demo");
        assert!(wix.contains("<Wix xmlns=\"http://wixtoolset.org/schemas/v4/wxs\">"));
        assert!(wix.contains("<Package Name=\"Kael Demo\""));
        assert!(wix.contains("Manufacturer=\"Acme Inc\""));
        assert!(wix.contains("Version=\"1.2.3\""));
        assert!(wix.contains("Source=\"kael-demo.exe\""));
        assert!(wix.contains("<Feature Id=\"Main\""));
        assert!(wix.contains("UpgradeCode=\""));
    }

    #[test]
    fn wix_upgrade_code_is_deterministic_and_valid_guid() {
        let first = generate_wix_source(&sample_config(), "kael-demo");
        let second = generate_wix_source(&sample_config(), "kael-demo");
        assert_eq!(first, second);

        let guid = deterministic_guid("com.kael.demo::wix-upgrade-code");
        let parts: Vec<&str> = guid.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(guid.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(&guid[14..15], "5");
        assert!(matches!(&guid[19..20], "8" | "9" | "A" | "B"));

        let other = deterministic_guid("com.other.app::wix-upgrade-code");
        assert_ne!(guid, other);
    }

    #[test]
    fn xml_escape_escapes_markup_metacharacters() {
        assert_eq!(
            xml_escape("a & b <c> \"d\" 'e'"),
            "a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"
        );
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod dmg_tests {
    use super::*;

    #[test]
    fn create_dmg_produces_a_real_image() {
        let scratch = std::env::temp_dir().join(format!("kael_dmg_test_{}", std::process::id()));
        let app = scratch.join("Demo.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "<plist></plist>").unwrap();
        fs::write(app.join("Contents/MacOS/demo"), b"#!/bin/sh\n").unwrap();

        let dmg_path = scratch.join("demo.dmg");
        let dmg = create_dmg(&app, &dmg_path, "Demo").expect("hdiutil create");

        assert!(dmg.exists(), "dmg should exist");
        assert!(dmg.metadata().unwrap().len() > 0, "dmg should be non-empty");

        let _ = fs::remove_dir_all(&scratch);
    }
}
