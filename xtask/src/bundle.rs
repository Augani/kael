use anyhow::{Context as _, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
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

    let artifacts = vec![bundle_dir.clone()];
    #[cfg(target_os = "macos")]
    let mut artifacts = artifacts;
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

    let mut artifacts = vec![bundle_dir.clone()];

    let msi_path = output.join(format!("{app_name}.msi"));
    if options.dry_run {
        println!("dry-run: would build MSI at {}", msi_path.display());
        return Ok(artifacts);
    }

    match find_wix() {
        Some(wix) => {
            let built = build_msi(&wix, &wix_path, &msi_path)?;
            println!("Windows .msi created: {}", built.display());

            if let Some(signing) = config.signing.as_ref()
                && let Some(certificate) = signing.windows_certificate.as_deref()
            {
                sign_msi(
                    &built,
                    certificate,
                    signing.windows_certificate_password.as_deref(),
                )?;
                println!("signed {}", built.display());
            } else {
                println!(
                    "note: no signing.windows_certificate configured — producing an unsigned .msi"
                );
            }

            artifacts.push(built);
        }
        None => {
            eprintln!(
                "warning: WiX v4 toolset not found (set WIX or add `wix` to PATH); \
                 skipping .msi build. Source written to {}",
                wix_path.display()
            );
        }
    }

    Ok(artifacts)
}

/// Locate the WiX v4 CLI (`wix`), mirroring the `fxc` locator in
/// `crates/kael/build.rs`: honour the `WIX` environment variable first, then
/// fall back to a `where`/`which` lookup on `PATH`. Returns `None` when WiX is
/// not installed so callers can skip MSI builds with a warning.
fn find_wix() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("WIX")
        && !path.is_empty()
    {
        let candidate = PathBuf::from(&path);
        if candidate.is_file() {
            return Some(candidate);
        }
        for nested in ["wix", "wix.exe", "bin/wix", "bin/wix.exe"] {
            let joined = candidate.join(nested);
            if joined.is_file() {
                return Some(joined);
            }
        }
    }

    let locator = if cfg!(target_os = "windows") {
        "where.exe"
    } else {
        "which"
    };
    let query = if cfg!(target_os = "windows") {
        "wix.exe"
    } else {
        "wix"
    };
    if let Ok(output) = Command::new(locator).arg(query).output()
        && output.status.success()
    {
        let found = String::from_utf8_lossy(&output.stdout);
        if let Some(first) = found.lines().next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }

    None
}

/// Build the argument vector for `wix build <wxs> -o <out>.msi` (WiX v4 CLI).
///
/// Split out so the command construction can be unit-tested without invoking
/// the toolset (which is not present on non-Windows hosts).
fn wix_build_args(wxs: &Path, msi: &Path) -> Vec<String> {
    vec![
        "build".to_string(),
        wxs.to_string_lossy().into_owned(),
        "-o".to_string(),
        msi.to_string_lossy().into_owned(),
    ]
}

/// Compile a `.wxs` source into an `.msi` using the WiX v4 `wix build` command.
///
/// WiX v4 only — there is no candle/light (WiX v3) fallback by design.
fn build_msi(wix: &Path, wxs: &Path, msi: &Path) -> Result<PathBuf> {
    let status = Command::new(wix)
        .args(wix_build_args(wxs, msi))
        .status()
        .with_context(|| format!("failed to run {}", wix.display()))?;
    if !status.success() {
        bail!("wix build failed with status {status}");
    }
    if !msi.exists() {
        bail!(
            "wix build reported success but {} was not produced",
            msi.display()
        );
    }
    Ok(msi.to_path_buf())
}

/// Build the `signtool sign` argument vector for an `.msi`, matching the
/// existing Windows signing step in `sign.rs`.
fn signtool_args(certificate: &Path, password: Option<&str>, artifact: &Path) -> Vec<String> {
    let mut args = vec![
        "sign".to_string(),
        "/f".to_string(),
        certificate.to_string_lossy().into_owned(),
        "/p".to_string(),
        password.unwrap_or("").to_string(),
        "/tr".to_string(),
        "http://timestamp.digicert.com".to_string(),
        "/td".to_string(),
        "sha256".to_string(),
        "/fd".to_string(),
        "sha256".to_string(),
    ];
    args.push(artifact.to_string_lossy().into_owned());
    args
}

/// Sign an `.msi` with the configured Windows code-signing certificate via
/// `signtool` (the same tool the standalone `sign` command uses).
fn sign_msi(msi: &Path, certificate: &Path, password: Option<&str>) -> Result<()> {
    let status = Command::new("signtool")
        .args(signtool_args(certificate, password, msi))
        .status()
        .with_context(|| "failed to run signtool — is it installed?")?;
    if !status.success() {
        bail!("signtool exited with status {status}");
    }
    Ok(())
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

    let mut artifacts = vec![app_dir.clone()];

    if options.dry_run {
        println!(
            "dry-run: would build .deb at {}",
            output
                .join(format!("{app_name}_{}_amd64.deb", config.version))
                .display()
        );
        println!("dry-run: would build AppImage from {}", app_dir.display());
        return Ok(artifacts);
    }

    let deb_path = output.join(format!("{app_name}_{}_amd64.deb", config.version));
    build_deb(config, &app_name, binary, &deb_path).context("failed to build .deb package")?;
    println!("Linux .deb created: {}", deb_path.display());
    artifacts.push(deb_path);

    let appimage_path = output.join(format!("{app_name}-{}-x86_64.AppImage", config.version));
    match find_appimagetool() {
        Some(tool) => {
            let built = build_appimage(&tool, &app_dir, &appimage_path)?;
            println!("Linux AppImage created: {}", built.display());
            artifacts.push(built);
        }
        None => {
            eprintln!(
                "warning: appimagetool not found on PATH; skipping AppImage build. \
                 The AppDir at {} is ready to package on a Linux host with appimagetool.",
                app_dir.display()
            );
        }
    }

    Ok(artifacts)
}

/// Locate `appimagetool` on `PATH`. Returns `None` (so the caller can
/// skip-with-warning) when it is not installed — it is Linux-only tooling.
fn find_appimagetool() -> Option<PathBuf> {
    let locator = if cfg!(target_os = "windows") {
        "where.exe"
    } else {
        "which"
    };
    if let Ok(output) = Command::new(locator).arg("appimagetool").output()
        && output.status.success()
    {
        let found = String::from_utf8_lossy(&output.stdout);
        if let Some(first) = found.lines().next() {
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                return Some(PathBuf::from(trimmed));
            }
        }
    }
    None
}

/// Run `appimagetool <AppDir> <output>.AppImage`. Update-information embedding
/// (the `-u` flag) is supported via the optional `KAEL_APPIMAGE_UPDATE_INFO`
/// environment variable but is not required.
fn build_appimage(tool: &Path, app_dir: &Path, output: &Path) -> Result<PathBuf> {
    let mut cmd = Command::new(tool);
    if let Ok(update_info) = std::env::var("KAEL_APPIMAGE_UPDATE_INFO")
        && !update_info.is_empty()
    {
        cmd.arg("-u").arg(update_info);
    }
    cmd.arg(app_dir).arg(output);
    let status = cmd
        .status()
        .with_context(|| format!("failed to run {}", tool.display()))?;
    if !status.success() {
        bail!("appimagetool failed with status {status}");
    }
    if !output.exists() {
        bail!(
            "appimagetool reported success but {} was not produced",
            output.display()
        );
    }
    Ok(output.to_path_buf())
}

/// Generate a Debian `control` file from the dist metadata.
///
/// `installed_size` is in kibibytes per Debian policy (§5.6.20).
fn generate_deb_control(config: &DistConfig, package: &str, installed_size_kib: u64) -> String {
    let maintainer = config
        .bundle
        .copyright
        .as_deref()
        .unwrap_or(&config.name)
        .to_string();
    let description = config
        .bundle
        .file_description
        .as_deref()
        .unwrap_or(&config.name);
    let section = config
        .bundle
        .linux_categories
        .as_ref()
        .and_then(|categories| categories.first())
        .map(|category| category.to_lowercase())
        .unwrap_or_else(|| "utils".to_string());

    format!(
        "Package: {package}\n\
         Version: {version}\n\
         Section: {section}\n\
         Priority: optional\n\
         Architecture: amd64\n\
         Maintainer: {maintainer}\n\
         Installed-Size: {installed_size_kib}\n\
         Description: {description}\n",
        version = config.version,
    )
}

/// Build a `.deb` package directly in Rust — an `ar` archive of
/// `debian-binary`, `control.tar.gz`, and `data.tar.gz` — so packaging works
/// on any host (macOS, CI) without `dpkg-deb` or other system tooling.
fn build_deb(
    config: &DistConfig,
    package: &str,
    binary: Option<&Path>,
    output: &Path,
) -> Result<PathBuf> {
    let data_tar = build_deb_data_tar(config, package, binary)?;
    let installed_size_kib = data_tar.len().div_ceil(1024) as u64;
    let control = generate_deb_control(config, package, installed_size_kib);
    let control_tar = build_deb_control_tar(&control)?;

    let members: [(&str, &[u8]); 3] = [
        ("debian-binary", b"2.0\n"),
        ("control.tar.gz", &control_tar),
        ("data.tar.gz", &data_tar),
    ];

    let mut archive = Vec::new();
    write_ar_archive(&mut archive, &members);
    fs::write(output, &archive)
        .with_context(|| format!("failed to write .deb to {}", output.display()))?;
    Ok(output.to_path_buf())
}

/// Build the gzip-compressed `data.tar.gz` payload: the installed file tree
/// rooted at `/`. Lays down `usr/bin/<pkg>` and `usr/share/applications/<pkg>.desktop`.
fn build_deb_data_tar(
    config: &DistConfig,
    package: &str,
    binary: Option<&Path>,
) -> Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));

    if let Some(binary) = binary {
        let bytes = fs::read(binary)
            .with_context(|| format!("failed to read binary {}", binary.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_path(format!("usr/bin/{package}"))?;
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_mtime(0);
        header.set_cksum();
        builder.append(&header, bytes.as_slice())?;
    }

    let desktop = generate_desktop_entry(config);
    let desktop_bytes = desktop.into_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_path(format!("usr/share/applications/{package}.desktop"))?;
    header.set_size(desktop_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder.append(&header, desktop_bytes.as_slice())?;

    let encoder = builder.into_inner()?;
    encoder.finish().context("failed to finish data.tar.gz")
}

/// Build the gzip-compressed `control.tar.gz`: a tarball containing the single
/// `./control` file describing the package.
fn build_deb_control_tar(control: &str) -> Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let bytes = control.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_path("./control")?;
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder.append(&header, bytes)?;

    let encoder = builder.into_inner()?;
    encoder.finish().context("failed to finish control.tar.gz")
}

/// Write a Unix `ar` "common" format archive (the container `.deb` uses).
///
/// Layout: the magic `!<arch>\n`, then per member a 60-byte ASCII header
/// (name, mtime, uid, gid, mode, size, terminator) followed by the data,
/// padded to an even byte boundary. ~30 lines, no external `ar` crate needed.
fn write_ar_archive(out: &mut Vec<u8>, members: &[(&str, &[u8])]) {
    out.extend_from_slice(b"!<arch>\n");
    for (name, data) in members {
        let header = format!(
            "{name:<16}{mtime:<12}{uid:<6}{gid:<6}{mode:<8}{size:<10}`\n",
            name = name,
            mtime = 0,
            uid = 0,
            gid = 0,
            mode = "100644",
            size = data.len(),
        );
        debug_assert_eq!(header.len(), 60, "ar header must be 60 bytes");
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
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

    #[test]
    fn wix_build_args_target_msi_output() {
        let args = wix_build_args(Path::new("dist/Kael/Kael.wxs"), Path::new("dist/Kael.msi"));
        assert_eq!(
            args,
            vec![
                "build".to_string(),
                "dist/Kael/Kael.wxs".to_string(),
                "-o".to_string(),
                "dist/Kael.msi".to_string(),
            ]
        );
    }

    #[test]
    fn signtool_args_include_certificate_password_and_timestamp() {
        let args = signtool_args(
            Path::new("certs/code.pfx"),
            Some("hunter2"),
            Path::new("dist/Kael.msi"),
        );
        assert_eq!(args[0], "sign");
        assert_eq!(args[1], "/f");
        assert_eq!(args[2], "certs/code.pfx");
        assert_eq!(args[3], "/p");
        assert_eq!(args[4], "hunter2");
        assert!(args.iter().any(|a| a == "/fd"));
        assert!(args.iter().any(|a| a == "sha256"));
        assert!(args.iter().any(|a| a == "http://timestamp.digicert.com"));
        assert_eq!(args.last().unwrap(), "dist/Kael.msi");
    }

    #[test]
    fn signtool_args_default_to_empty_password() {
        let args = signtool_args(
            Path::new("certs/code.pfx"),
            None,
            Path::new("dist/Kael.msi"),
        );
        assert_eq!(args[4], "");
    }

    fn linux_config() -> DistConfig {
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
                copyright: Some("Augustus Otu <dev@kael.dev>".to_string()),
                category: None,
                minimum_system_version: None,
                file_description: Some("A GPU-accelerated framework".to_string()),
                linux_categories: Some(vec!["Development".to_string(), "Utility".to_string()]),
            },
            signing: None,
            updater: None,
        }
    }

    #[test]
    fn desktop_entry_is_freedesktop_compliant() {
        let desktop = generate_desktop_entry(&linux_config());
        assert!(desktop.starts_with("[Desktop Entry]\n"));
        assert!(desktop.contains("Name=Kael Demo\n"));
        assert!(desktop.contains("Exec=kael-demo\n"));
        assert!(desktop.contains("Type=Application\n"));
        assert!(desktop.contains("Categories=Development;Utility\n"));
    }

    #[test]
    fn deb_control_contains_required_fields() {
        let control = generate_deb_control(&linux_config(), "kael-demo", 512);
        assert!(control.contains("Package: kael-demo\n"));
        assert!(control.contains("Version: 1.2.3\n"));
        assert!(control.contains("Architecture: amd64\n"));
        assert!(control.contains("Maintainer: Augustus Otu <dev@kael.dev>\n"));
        assert!(control.contains("Installed-Size: 512\n"));
        assert!(control.contains("Section: development\n"));
        assert!(control.contains("Priority: optional\n"));
        assert!(control.contains("Description: A GPU-accelerated framework\n"));
        assert!(control.ends_with('\n'));
    }

    #[test]
    fn deb_control_falls_back_to_name_when_no_metadata() {
        let mut config = linux_config();
        config.bundle.copyright = None;
        config.bundle.file_description = None;
        config.bundle.linux_categories = None;
        let control = generate_deb_control(&config, "kael-demo", 0);
        assert!(control.contains("Maintainer: Kael Demo\n"));
        assert!(control.contains("Description: Kael Demo\n"));
        assert!(control.contains("Section: utils\n"));
    }

    #[test]
    fn ar_archive_has_magic_and_padded_members() {
        let mut out = Vec::new();
        let members: [(&str, &[u8]); 2] = [("debian-binary", b"2.0\n"), ("odd", b"abc")];
        write_ar_archive(&mut out, &members);

        assert_eq!(&out[0..8], b"!<arch>\n");
        let header1 = &out[8..68];
        assert_eq!(header1.len(), 60);
        assert!(header1.starts_with(b"debian-binary"));
        assert_eq!(&header1[58..60], b"`\n");

        let data1_start = 68;
        assert_eq!(&out[data1_start..data1_start + 4], b"2.0\n");

        let header2_start = data1_start + 4;
        let header2 = &out[header2_start..header2_start + 60];
        assert!(header2.starts_with(b"odd"));
        let data2_start = header2_start + 60;
        assert_eq!(&out[data2_start..data2_start + 3], b"abc");
        assert_eq!(out[data2_start + 3], b'\n');
    }

    #[test]
    fn deb_round_trips_through_ar_and_tar() {
        use flate2::read::GzDecoder;

        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("kael-demo");
        fs::write(&binary, b"\x7fELF fake binary contents").unwrap();
        let deb = dir.path().join("kael-demo_1.2.3_amd64.deb");

        build_deb(&linux_config(), "kael-demo", Some(&binary), &deb).unwrap();

        let bytes = fs::read(&deb).unwrap();
        assert_eq!(&bytes[0..8], b"!<arch>\n");

        let members = parse_ar(&bytes);
        let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec!["debian-binary", "control.tar.gz", "data.tar.gz"]
        );
        assert_eq!(members[0].1, b"2.0\n");

        let control_tar = members.iter().find(|(n, _)| n == "control.tar.gz").unwrap();
        let mut control_archive = tar::Archive::new(GzDecoder::new(control_tar.1.as_slice()));
        let mut control_contents = String::new();
        let mut saw_control = false;
        for entry in control_archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            if path.ends_with("control") {
                saw_control = true;
                use std::io::Read as _;
                entry.read_to_string(&mut control_contents).unwrap();
            }
        }
        assert!(saw_control, "control.tar.gz must contain a control file");
        assert!(control_contents.contains("Package: kael-demo\n"));
        assert!(control_contents.contains("Version: 1.2.3\n"));

        let data_tar = members.iter().find(|(n, _)| n == "data.tar.gz").unwrap();
        let mut data_archive = tar::Archive::new(GzDecoder::new(data_tar.1.as_slice()));
        let mut data_paths = Vec::new();
        let mut binary_mode = None;
        for entry in data_archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            if path == "usr/bin/kael-demo" {
                binary_mode = Some(entry.header().mode().unwrap());
            }
            data_paths.push(path);
        }
        assert!(data_paths.iter().any(|p| p == "usr/bin/kael-demo"));
        assert!(
            data_paths
                .iter()
                .any(|p| p == "usr/share/applications/kael-demo.desktop")
        );
        assert_eq!(binary_mode, Some(0o755));
    }

    fn parse_ar(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut members = Vec::new();
        let mut offset = 8;
        while offset + 60 <= bytes.len() {
            let header = &bytes[offset..offset + 60];
            let name = String::from_utf8_lossy(&header[0..16]).trim().to_string();
            let size: usize = String::from_utf8_lossy(&header[48..58])
                .trim()
                .parse()
                .unwrap();
            let data_start = offset + 60;
            let data = bytes[data_start..data_start + size].to_vec();
            members.push((name, data));
            offset = data_start + size;
            if size % 2 == 1 {
                offset += 1;
            }
        }
        members
    }

    #[test]
    fn linux_bundle_pipeline_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("dist");
        let binary = dir.path().join("kael-demo");
        fs::write(&binary, b"\x7fELF dummy binary for smoke test").unwrap();

        let config = linux_config();
        let options = BundleOptions {
            dry_run: false,
            binary: Some(binary.clone()),
        };

        let artifacts = bundle_linux(&config, &output, Some(binary.as_path()), &options).unwrap();

        let app_dir = output.join("kael-demo.AppDir");
        assert!(app_dir.is_dir(), "AppDir must be created");
        assert!(app_dir.join("AppRun").is_file(), "AppRun must exist");
        assert!(
            app_dir.join("kael-demo.desktop").is_file(),
            ".desktop must exist"
        );
        assert!(
            app_dir.join("usr/bin/kael-demo").is_file(),
            "binary must be staged in the AppDir"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(app_dir.join("AppRun"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "AppRun must be executable");
        }

        let deb = output.join("kael-demo_1.2.3_amd64.deb");
        assert!(deb.is_file(), ".deb must be produced on any host");

        let bytes = fs::read(&deb).unwrap();
        assert_eq!(
            &bytes[0..8],
            b"!<arch>\n",
            ".deb must be a valid ar archive"
        );
        let members = parse_ar(&bytes);
        let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec!["debian-binary", "control.tar.gz", "data.tar.gz"]
        );

        assert!(
            artifacts.contains(&app_dir),
            "AppDir should be reported as an artifact"
        );
        assert!(
            artifacts.contains(&deb),
            ".deb should be reported as an artifact"
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
