use anyhow::{Context as _, Result, bail};
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
        let dmg_path = output.join(format!("{app_name_slug}.dmg"));
        let dmg =
            create_dmg(&bundle_dir, &dmg_path, app_name).context("failed to create macOS .dmg")?;
        println!("macOS .dmg created: {}", dmg.display());
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

    let metadata_path = bundle_dir.join("installer.json");
    let metadata = serde_json::json!({
        "name": config.name,
        "version": config.version,
        "description": config.bundle.file_description,
    });
    if options.dry_run {
        println!(
            "dry-run: would write installer metadata to {}",
            metadata_path.display()
        );
    } else {
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
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
