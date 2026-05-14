//! Cross-platform auto-updater module.
//!
//! Provides an API for checking a configurable URL for available updates,
//! downloading update packages in the background with progress callbacks,
//! and applying updates with application restart.
//!
//! Supports Sparkle appcast XML and a simpler JSON feed format for update
//! discovery.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use futures::AsyncReadExt as _;
use serde::{Deserialize, Serialize};

use semantic_version::SemanticVersion;

/// Configuration for the auto-updater.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoUpdaterConfig {
    /// URL of the update feed (appcast XML or JSON).
    pub feed_url: String,
    /// How often to check for updates.
    #[serde(with = "duration_secs")]
    pub check_interval: Duration,
    /// Whether to include pre-release versions.
    pub allow_prerelease: bool,
}

/// Information about an available update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// The version of the available update.
    pub version: SemanticVersion,
    /// Optional release notes (HTML or plain text).
    pub release_notes: Option<String>,
    /// URL to download the update package.
    pub download_url: String,
    /// Optional cryptographic signature for verification.
    pub signature: Option<String>,
}

/// Progress information during an update download.
#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    /// Bytes downloaded so far.
    pub bytes_downloaded: u64,
    /// Total bytes to download, if known.
    pub total_bytes: Option<u64>,
}

impl DownloadProgress {
    /// Returns the download progress as a fraction in `[0.0, 1.0]`, or `None`
    /// if the total size is unknown.
    pub fn fraction(&self) -> Option<f64> {
        self.total_bytes
            .map(|total| self.bytes_downloaded as f64 / total as f64)
    }
}

/// The current state of the auto-updater.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// No update activity.
    Idle,
    /// Checking the feed for updates.
    Checking,
    /// An update is available.
    UpdateAvailable(SemanticVersion),
    /// Downloading the update package.
    Downloading,
    /// The update has been downloaded and is ready to install.
    ReadyToInstall,
    /// An error occurred.
    Error(String),
}

/// Trait for platform-specific update installation.
///
/// Each platform provides its own implementation:
/// - macOS: Sparkle-compatible `.dmg`/`.zip` handling
/// - Windows: MSI/NSIS `.exe` installer execution
/// - Linux: AppImage delta updates or Flatpak/Snap update channels
pub trait PlatformInstaller: Send + Sync {
    /// Install the update from the downloaded package at `path` and restart
    /// the application.
    fn install_and_restart(&self, package_path: &std::path::Path) -> Result<()>;
}

/// The auto-updater.
///
/// Checks a configurable URL for available updates, downloads update packages
/// in the background, and delegates installation to a [`PlatformInstaller`].
pub struct AutoUpdater {
    config: AutoUpdaterConfig,
    current_version: SemanticVersion,
    http_client: Arc<dyn http_client::HttpClient>,
    installer: Option<Arc<dyn PlatformInstaller>>,
    status: UpdateStatus,
    latest_update: Option<UpdateInfo>,
    downloaded_path: Option<std::path::PathBuf>,
}

impl AutoUpdater {
    /// Create a new auto-updater with the given configuration and current
    /// application version.
    pub fn new(
        config: AutoUpdaterConfig,
        current_version: SemanticVersion,
        http_client: Arc<dyn http_client::HttpClient>,
    ) -> Self {
        Self {
            config,
            current_version,
            http_client,
            installer: None,
            status: UpdateStatus::Idle,
            latest_update: None,
            downloaded_path: None,
        }
    }

    /// Set the platform-specific installer backend.
    pub fn set_installer(&mut self, installer: Arc<dyn PlatformInstaller>) {
        self.installer = Some(installer);
    }

    /// Returns the current update status.
    pub fn status(&self) -> &UpdateStatus {
        &self.status
    }

    /// Returns the latest update info, if an update was found.
    pub fn latest_update(&self) -> Option<&UpdateInfo> {
        self.latest_update.as_ref()
    }

    /// Returns the auto-updater configuration.
    pub fn config(&self) -> &AutoUpdaterConfig {
        &self.config
    }

    /// Check the configured feed URL for available updates.
    ///
    /// Returns `Some(UpdateInfo)` if a newer version is available, `None`
    /// otherwise.
    pub async fn check_for_updates(&mut self) -> Result<Option<UpdateInfo>> {
        self.status = UpdateStatus::Checking;

        let mut response = self
            .http_client
            .get(&self.config.feed_url, Default::default(), false)
            .await
            .context("failed to fetch update feed")?;

        let status = response.status();
        if !status.is_success() {
            let msg = format!("update feed returned HTTP {}", status.as_u16());
            self.status = UpdateStatus::Error(msg.clone());
            bail!("{}", msg);
        }

        let mut body = Vec::new();
        response
            .body_mut()
            .read_to_end(&mut body)
            .await
            .context("failed to read update feed body")?;

        let body_str = String::from_utf8_lossy(&body);

        let updates = parse_update_feed(&body_str)?;

        let latest = updates
            .into_iter()
            // SemanticVersion currently does not preserve pre-release metadata,
            // so feed filtering is limited to version ordering for now.
            .filter(|u| u.version > self.current_version)
            .max_by_key(|u| u.version);

        if let Some(ref update) = latest {
            self.status = UpdateStatus::UpdateAvailable(update.version);
            self.latest_update = Some(update.clone());
        } else {
            self.status = UpdateStatus::Idle;
            self.latest_update = None;
        }

        Ok(latest)
    }

    /// Download the latest available update in the background.
    ///
    /// Calls `on_progress` periodically with download progress information.
    /// Returns the path to the downloaded package on success.
    pub async fn download_update(
        &mut self,
        on_progress: impl Fn(DownloadProgress) + Send + 'static,
    ) -> Result<std::path::PathBuf> {
        let update = self
            .latest_update
            .as_ref()
            .ok_or_else(|| anyhow!("no update available to download"))?
            .clone();

        self.status = UpdateStatus::Downloading;

        let mut response = self
            .http_client
            .get(&update.download_url, Default::default(), false)
            .await
            .context("failed to start update download")?;

        let status = response.status();
        if !status.is_success() {
            let msg = format!("update download returned HTTP {}", status.as_u16());
            self.status = UpdateStatus::Error(msg.clone());
            bail!("{}", msg);
        }

        let total_bytes = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        let mut bytes = Vec::new();
        response
            .body_mut()
            .read_to_end(&mut bytes)
            .await
            .context("failed to read update package")?;

        // Report final progress
        on_progress(DownloadProgress {
            bytes_downloaded: bytes.len() as u64,
            total_bytes,
        });

        // Write to a temp file
        let temp_dir = std::env::temp_dir();
        let filename = update
            .download_url
            .rsplit('/')
            .next()
            .unwrap_or("update_package");
        let download_path = temp_dir.join(format!("gpui_update_{}", filename));

        std::fs::write(&download_path, &bytes).context("failed to write update package to disk")?;

        self.downloaded_path = Some(download_path.clone());
        self.status = UpdateStatus::ReadyToInstall;

        Ok(download_path)
    }

    /// Install the downloaded update and restart the application.
    ///
    /// Requires a [`PlatformInstaller`] to be set via [`set_installer`].
    ///
    /// [`set_installer`]: Self::set_installer
    pub fn install_and_restart(&self) -> Result<()> {
        let installer = self
            .installer
            .as_ref()
            .ok_or_else(|| anyhow!("no platform installer configured"))?;

        let path = self
            .downloaded_path
            .as_ref()
            .ok_or_else(|| anyhow!("no update has been downloaded"))?;

        installer.install_and_restart(path)
    }
}

// ---------------------------------------------------------------------------
// Update feed parsing
// ---------------------------------------------------------------------------

/// Parse an update feed, auto-detecting Sparkle appcast XML vs JSON format.
pub fn parse_update_feed(body: &str) -> Result<Vec<UpdateInfo>> {
    let trimmed = body.trim();
    if trimmed.starts_with('<') {
        parse_appcast_xml(trimmed)
    } else if trimmed.starts_with('[') || trimmed.starts_with('{') {
        parse_json_feed(trimmed)
    } else {
        bail!("unrecognized update feed format");
    }
}

/// A single item from a JSON update feed.
#[derive(Debug, Deserialize)]
struct JsonFeedItem {
    version: String,
    #[serde(default)]
    release_notes: Option<String>,
    download_url: String,
    #[serde(default)]
    signature: Option<String>,
}

/// Parse a JSON update feed.
///
/// Accepts either a JSON array of items or a single object with an `"items"`
/// array.
fn parse_json_feed(body: &str) -> Result<Vec<UpdateInfo>> {
    // Try array-of-items first
    let items: Vec<JsonFeedItem> = if body.trim().starts_with('[') {
        serde_json::from_str(body).context("failed to parse JSON update feed as array")?
    } else {
        #[derive(Deserialize)]
        struct Wrapper {
            items: Vec<JsonFeedItem>,
        }
        let wrapper: Wrapper =
            serde_json::from_str(body).context("failed to parse JSON update feed as object")?;
        wrapper.items
    };

    items
        .into_iter()
        .map(|item| {
            let version = item
                .version
                .parse::<SemanticVersion>()
                .context(format!("invalid version string: {}", item.version))?;
            Ok(UpdateInfo {
                version,
                release_notes: item.release_notes,
                download_url: item.download_url,
                signature: item.signature,
            })
        })
        .collect()
}

/// Parse a Sparkle appcast XML feed.
///
/// Extracts `<item>` elements and reads `<enclosure>` attributes for download
/// URL, version, and signature. Release notes come from `<description>`.
fn parse_appcast_xml(body: &str) -> Result<Vec<UpdateInfo>> {
    let mut updates = Vec::new();

    // Simple streaming XML parser — we don't pull in a full XML crate.
    // Sparkle appcast structure:
    //   <rss><channel>
    //     <item>
    //       <title>...</title>
    //       <description>...</description>
    //       <enclosure url="..." sparkle:version="..." sparkle:dsaSignature="..." />
    //     </item>
    //   </channel></rss>

    for item_block in split_xml_items(body) {
        let version_str = extract_xml_attr(&item_block, "sparkle:version")
            .or_else(|| extract_xml_attr(&item_block, "sparkle:shortVersionString"))
            .or_else(|| extract_xml_tag_content(&item_block, "sparkle:version"));

        let download_url = extract_xml_attr(&item_block, "url");

        let signature = extract_xml_attr(&item_block, "sparkle:dsaSignature")
            .or_else(|| extract_xml_attr(&item_block, "sparkle:edSignature"));

        let release_notes = extract_xml_tag_content(&item_block, "description");

        if let (Some(version_str), Some(download_url)) = (version_str, download_url) {
            if let Ok(version) = version_str.parse::<SemanticVersion>() {
                updates.push(UpdateInfo {
                    version,
                    release_notes,
                    download_url,
                    signature,
                });
            }
        }
    }

    Ok(updates)
}

/// Split XML body into `<item>...</item>` blocks.
fn split_xml_items(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let lower = body.to_lowercase();
    let mut search_from = 0;

    while let Some(pos) = lower[search_from..]
        .find("<item>")
        .or_else(|| lower[search_from..].find("<item "))
    {
        let start = search_from + pos;
        let end = match lower[start..].find("</item>") {
            Some(pos) => start + pos + "</item>".len(),
            None => break,
        };
        items.push(body[start..end].to_string());
        search_from = end;
    }

    items
}

/// Extract the value of an XML attribute by name from a block of XML text.
fn extract_xml_attr(block: &str, attr_name: &str) -> Option<String> {
    let search = format!("{}=\"", attr_name);
    let start = block.find(&search)?;
    let value_start = start + search.len();
    let value_end = block[value_start..].find('"')? + value_start;
    Some(block[value_start..value_end].to_string())
}

/// Extract the text content of an XML tag by name.
fn extract_xml_tag_content(block: &str, tag_name: &str) -> Option<String> {
    let open = format!("<{}", tag_name);
    let close = format!("</{}>", tag_name);

    let start = block.find(&open)?;
    let after_open = block[start..].find('>')? + start + 1;
    let end = block[after_open..].find(&close)? + after_open;

    let content = block[after_open..end].trim().to_string();
    if content.is_empty() {
        None
    } else {
        Some(content)
    }
}

// ---------------------------------------------------------------------------
// Platform-specific installer backends
// ---------------------------------------------------------------------------

/// macOS installer: handles Sparkle-compatible `.dmg` and `.zip` packages.
///
/// For `.zip` packages the archive is extracted to a temporary directory and
/// the contained `.app` bundle is moved over the running application before
/// restarting.
///
/// For `.dmg` packages the disk image is attached via `hdiutil`, the `.app`
/// bundle is copied from the mounted volume, and the image is detached.
#[cfg(target_os = "macos")]
pub struct MacInstaller;

#[cfg(target_os = "macos")]
impl PlatformInstaller for MacInstaller {
    fn install_and_restart(&self, package_path: &std::path::Path) -> Result<()> {
        let ext = package_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let app_bundle = resolve_running_app_bundle()?;

        match ext {
            "zip" => {
                let temp_dir = std::env::temp_dir().join("gpui_update_extract");
                if temp_dir.exists() {
                    std::fs::remove_dir_all(&temp_dir)?;
                }
                std::fs::create_dir_all(&temp_dir)?;

                let status = std::process::Command::new("ditto")
                    .args([
                        "-xk",
                        &package_path.to_string_lossy(),
                        &temp_dir.to_string_lossy(),
                    ])
                    .status()
                    .context("failed to run ditto to extract zip")?;

                if !status.success() {
                    bail!("ditto extraction failed with status {}", status);
                }

                let new_app = find_app_bundle_in(&temp_dir)?;
                replace_app_bundle(&new_app, &app_bundle)?;
            }
            "dmg" => {
                let mount_point = std::env::temp_dir().join("gpui_update_dmg");
                if mount_point.exists() {
                    // Try to detach any previous mount
                    let _ = std::process::Command::new("hdiutil")
                        .args(["detach", &mount_point.to_string_lossy(), "-quiet"])
                        .status();
                    let _ = std::fs::remove_dir_all(&mount_point);
                }
                std::fs::create_dir_all(&mount_point)?;

                let status = std::process::Command::new("hdiutil")
                    .args([
                        "attach",
                        &package_path.to_string_lossy(),
                        "-mountpoint",
                        &mount_point.to_string_lossy(),
                        "-nobrowse",
                        "-quiet",
                    ])
                    .status()
                    .context("failed to run hdiutil attach")?;

                if !status.success() {
                    bail!("hdiutil attach failed with status {}", status);
                }

                let result = (|| -> Result<()> {
                    let new_app = find_app_bundle_in(&mount_point)?;
                    replace_app_bundle(&new_app, &app_bundle)
                })();

                // Always detach
                let _ = std::process::Command::new("hdiutil")
                    .args(["detach", &mount_point.to_string_lossy(), "-quiet"])
                    .status();

                result?;
            }
            other => bail!("unsupported macOS package format: .{}", other),
        }

        // Restart the application
        let status = std::process::Command::new("open")
            .args(["-n", &app_bundle.to_string_lossy()])
            .status()
            .context("failed to restart application")?;

        if !status.success() {
            bail!("failed to restart application, open returned {}", status);
        }

        std::process::exit(0);
    }
}

/// Resolve the path to the currently running `.app` bundle on macOS.
#[cfg(target_os = "macos")]
fn resolve_running_app_bundle() -> Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("failed to get current executable path")?;
    // Typical layout: Foo.app/Contents/MacOS/foo
    let app_bundle = exe
        .parent() // MacOS/
        .and_then(|p| p.parent()) // Contents/
        .and_then(|p| p.parent()) // Foo.app/
        .ok_or_else(|| anyhow!("could not determine .app bundle path from executable"))?;
    Ok(app_bundle.to_path_buf())
}

/// Find the first `.app` bundle inside a directory.
#[cfg(target_os = "macos")]
fn find_app_bundle_in(dir: &std::path::Path) -> Result<std::path::PathBuf> {
    for entry in std::fs::read_dir(dir).context("failed to read extraction directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("app") {
            return Ok(path);
        }
    }
    bail!("no .app bundle found in {}", dir.display())
}

/// Replace the existing app bundle with the new one.
#[cfg(target_os = "macos")]
fn replace_app_bundle(new_app: &std::path::Path, existing_app: &std::path::Path) -> Result<()> {
    let backup = existing_app.with_extension("app.bak");
    if backup.exists() {
        std::fs::remove_dir_all(&backup)?;
    }
    std::fs::rename(existing_app, &backup)
        .context("failed to move existing app bundle to backup")?;

    let status = std::process::Command::new("cp")
        .args([
            "-R",
            &new_app.to_string_lossy(),
            &existing_app.to_string_lossy(),
        ])
        .status()
        .context("failed to copy new app bundle")?;

    if !status.success() {
        // Attempt to restore backup
        let _ = std::fs::rename(&backup, existing_app);
        bail!("failed to copy new app bundle into place");
    }

    let _ = std::fs::remove_dir_all(&backup);
    Ok(())
}

/// Windows installer: executes MSI or NSIS `.exe` installer packages.
///
/// For `.msi` packages, `msiexec /i` is used with quiet mode and restart.
/// For `.exe` packages (NSIS), the installer is executed with `/S` (silent)
/// flag.
#[cfg(target_os = "windows")]
pub struct WindowsInstaller;

#[cfg(target_os = "windows")]
impl PlatformInstaller for WindowsInstaller {
    fn install_and_restart(&self, package_path: &std::path::Path) -> Result<()> {
        let ext = package_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext {
            "msi" => {
                // Use msiexec to install the MSI package silently and restart
                let status = std::process::Command::new("msiexec")
                    .args([
                        "/i",
                        &package_path.to_string_lossy(),
                        "/quiet",
                        "/norestart",
                    ])
                    .status()
                    .context("failed to run msiexec")?;

                if !status.success() {
                    bail!("msiexec failed with status {}", status);
                }
            }
            "exe" => {
                // Execute NSIS installer in silent mode
                let status = std::process::Command::new(package_path)
                    .args(["/S"])
                    .status()
                    .context("failed to run NSIS installer")?;

                if !status.success() {
                    bail!("NSIS installer failed with status {}", status);
                }
            }
            other => bail!("unsupported Windows package format: .{}", other),
        }

        // Restart: re-launch the current executable
        let exe = std::env::current_exe().context("failed to get current executable path")?;
        let _ = std::process::Command::new(exe)
            .spawn()
            .context("failed to restart application")?;

        std::process::exit(0);
    }
}

/// Linux installer: handles AppImage delta updates, Flatpak, and Snap update
/// channels.
///
/// - For AppImage packages (`.AppImage`), the new image replaces the running
///   one and is made executable.
/// - For Flatpak apps, `flatpak update` is invoked.
/// - For Snap apps, `snap refresh` is invoked.
/// - Generic packages are treated as AppImage replacements.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub struct LinuxInstaller {
    /// The packaging format hint. When `None`, the installer infers the
    /// format from the package file extension or the running environment.
    pub format_hint: Option<LinuxPackageFormat>,
}

/// Supported Linux packaging formats for auto-update.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxPackageFormat {
    /// AppImage — replace the running image file.
    AppImage,
    /// Flatpak — delegate to `flatpak update`.
    Flatpak,
    /// Snap — delegate to `snap refresh`.
    Snap,
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl LinuxInstaller {
    /// Create a new Linux installer with automatic format detection.
    pub fn new() -> Self {
        Self { format_hint: None }
    }

    /// Create a new Linux installer with an explicit format hint.
    pub fn with_format(format: LinuxPackageFormat) -> Self {
        Self {
            format_hint: Some(format),
        }
    }

    /// Detect the packaging format from the environment or package path.
    fn detect_format(&self, package_path: &std::path::Path) -> LinuxPackageFormat {
        if let Some(hint) = self.format_hint {
            return hint;
        }

        // Check file extension
        let ext = package_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        if ext.eq_ignore_ascii_case("appimage") {
            return LinuxPackageFormat::AppImage;
        }

        // Check environment for Flatpak
        if std::env::var("FLATPAK_ID").is_ok() {
            return LinuxPackageFormat::Flatpak;
        }

        // Check environment for Snap
        if std::env::var("SNAP").is_ok() {
            return LinuxPackageFormat::Snap;
        }

        // Default to AppImage replacement
        LinuxPackageFormat::AppImage
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl PlatformInstaller for LinuxInstaller {
    fn install_and_restart(&self, package_path: &std::path::Path) -> Result<()> {
        let format = self.detect_format(package_path);

        match format {
            LinuxPackageFormat::AppImage => {
                let exe =
                    std::env::current_exe().context("failed to get current executable path")?;

                // Replace the running AppImage with the new one
                let backup = exe.with_extension("bak");
                if backup.exists() {
                    std::fs::remove_file(&backup)?;
                }
                std::fs::rename(&exe, &backup)
                    .context("failed to move current AppImage to backup")?;

                if let Err(e) = std::fs::copy(package_path, &exe) {
                    // Attempt to restore backup
                    let _ = std::fs::rename(&backup, &exe);
                    return Err(e).context("failed to copy new AppImage into place");
                }

                // Make executable
                let status = std::process::Command::new("chmod")
                    .args(["+x", &exe.to_string_lossy()])
                    .status()
                    .context("failed to chmod new AppImage")?;

                if !status.success() {
                    let _ = std::fs::rename(&backup, &exe);
                    bail!("chmod failed with status {}", status);
                }

                let _ = std::fs::remove_file(&backup);

                // Restart
                let _ = std::process::Command::new(&exe)
                    .spawn()
                    .context("failed to restart AppImage")?;

                std::process::exit(0);
            }
            LinuxPackageFormat::Flatpak => {
                let app_id =
                    std::env::var("FLATPAK_ID").unwrap_or_else(|_| "current-app".to_string());

                let status = std::process::Command::new("flatpak")
                    .args(["update", "-y", &app_id])
                    .status()
                    .context("failed to run flatpak update")?;

                if !status.success() {
                    bail!("flatpak update failed with status {}", status);
                }

                // Restart via flatpak run
                let _ = std::process::Command::new("flatpak")
                    .args(["run", &app_id])
                    .spawn()
                    .context("failed to restart Flatpak application")?;

                std::process::exit(0);
            }
            LinuxPackageFormat::Snap => {
                let snap_name =
                    std::env::var("SNAP_NAME").unwrap_or_else(|_| "current-app".to_string());

                let status = std::process::Command::new("snap")
                    .args(["refresh", &snap_name])
                    .status()
                    .context("failed to run snap refresh")?;

                if !status.success() {
                    bail!("snap refresh failed with status {}", status);
                }

                // Restart via snap run
                let _ = std::process::Command::new("snap")
                    .args(["run", &snap_name])
                    .spawn()
                    .context("failed to restart Snap application")?;

                std::process::exit(0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Serde helper for Duration as seconds
// ---------------------------------------------------------------------------

mod duration_secs {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_secs())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_feed_array() {
        let json = r#"[
            {
                "version": "1.2.3",
                "release_notes": "Bug fixes",
                "download_url": "https://example.com/update-1.2.3.zip",
                "signature": "abc123"
            },
            {
                "version": "1.1.0",
                "download_url": "https://example.com/update-1.1.0.zip"
            }
        ]"#;

        let updates = parse_update_feed(json).unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].version, SemanticVersion::new(1, 2, 3));
        assert_eq!(updates[0].release_notes.as_deref(), Some("Bug fixes"));
        assert_eq!(
            updates[0].download_url,
            "https://example.com/update-1.2.3.zip"
        );
        assert_eq!(updates[0].signature.as_deref(), Some("abc123"));
        assert_eq!(updates[1].version, SemanticVersion::new(1, 1, 0));
        assert!(updates[1].release_notes.is_none());
        assert!(updates[1].signature.is_none());
    }

    #[test]
    fn test_parse_json_feed_object_wrapper() {
        let json = r#"{
            "items": [
                {
                    "version": "2.0.0",
                    "download_url": "https://example.com/v2.zip"
                }
            ]
        }"#;

        let updates = parse_update_feed(json).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].version, SemanticVersion::new(2, 0, 0));
    }

    #[test]
    fn test_parse_appcast_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
        <rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
            <channel>
                <title>My App Updates</title>
                <item>
                    <title>Version 3.1.0</title>
                    <description>New features and improvements</description>
                    <enclosure url="https://example.com/MyApp-3.1.0.dmg"
                               sparkle:version="3.1.0"
                               sparkle:dsaSignature="sig123"
                               length="12345678"
                               type="application/octet-stream" />
                </item>
                <item>
                    <title>Version 3.0.0</title>
                    <enclosure url="https://example.com/MyApp-3.0.0.dmg"
                               sparkle:version="3.0.0"
                               length="11111111"
                               type="application/octet-stream" />
                </item>
            </channel>
        </rss>"#;

        let updates = parse_update_feed(xml).unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].version, SemanticVersion::new(3, 1, 0));
        assert_eq!(
            updates[0].download_url,
            "https://example.com/MyApp-3.1.0.dmg"
        );
        assert_eq!(updates[0].signature.as_deref(), Some("sig123"));
        assert_eq!(
            updates[0].release_notes.as_deref(),
            Some("New features and improvements")
        );
        assert_eq!(updates[1].version, SemanticVersion::new(3, 0, 0));
        assert!(updates[1].signature.is_none());
    }

    #[test]
    fn test_parse_appcast_xml_with_ed_signature() {
        let xml = r#"<rss><channel>
            <item>
                <enclosure url="https://example.com/app.zip"
                           sparkle:version="1.0.0"
                           sparkle:edSignature="ed_sig_value" />
            </item>
        </channel></rss>"#;

        let updates = parse_update_feed(xml).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].signature.as_deref(), Some("ed_sig_value"));
    }

    #[test]
    fn test_parse_empty_json_array() {
        let updates = parse_update_feed("[]").unwrap();
        assert!(updates.is_empty());
    }

    #[test]
    fn test_parse_unrecognized_format() {
        let result = parse_update_feed("this is not valid");
        assert!(result.is_err());
    }

    #[test]
    fn test_download_progress_fraction() {
        let progress = DownloadProgress {
            bytes_downloaded: 50,
            total_bytes: Some(100),
        };
        assert_eq!(progress.fraction(), Some(0.5));

        let unknown = DownloadProgress {
            bytes_downloaded: 50,
            total_bytes: None,
        };
        assert_eq!(unknown.fraction(), None);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/appcast.xml".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: AutoUpdaterConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.feed_url, config.feed_url);
        assert_eq!(deserialized.check_interval, config.check_interval);
        assert_eq!(deserialized.allow_prerelease, config.allow_prerelease);
    }

    #[test]
    fn test_update_info_serialization_roundtrip() {
        let info = UpdateInfo {
            version: SemanticVersion::new(2, 5, 1),
            release_notes: Some("Fixed a bug".to_string()),
            download_url: "https://example.com/v2.5.1.zip".to_string(),
            signature: Some("sig_value".to_string()),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: UpdateInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, info.version);
        assert_eq!(deserialized.release_notes, info.release_notes);
        assert_eq!(deserialized.download_url, info.download_url);
        assert_eq!(deserialized.signature, info.signature);
    }

    #[test]
    fn test_auto_updater_initial_state() {
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let client = http_client::FakeHttpClient::with_200_response();
        let updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);

        assert_eq!(*updater.status(), UpdateStatus::Idle);
        assert!(updater.latest_update().is_none());
    }

    #[test]
    fn test_install_without_installer_errors() {
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let client = http_client::FakeHttpClient::with_200_response();
        let updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);

        let result = updater.install_and_restart();
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // Platform installer tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_platform_installer_trait_is_object_safe() {
        // Verify PlatformInstaller can be used as a trait object
        fn _assert_object_safe(_: &dyn PlatformInstaller) {}
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_mac_installer_rejects_unsupported_format() {
        let installer = MacInstaller;
        let path = std::path::Path::new("/tmp/update.tar.gz");
        let result = installer.install_and_restart(path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unsupported macOS package format")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_windows_installer_rejects_unsupported_format() {
        let installer = WindowsInstaller;
        let path = std::path::Path::new("C:\\temp\\update.tar.gz");
        let result = installer.install_and_restart(path);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unsupported Windows package format")
        );
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn test_linux_installer_default_format_detection() {
        let installer = LinuxInstaller::new();
        let appimage_path = std::path::Path::new("/tmp/MyApp.AppImage");
        assert_eq!(
            installer.detect_format(appimage_path),
            LinuxPackageFormat::AppImage
        );
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn test_linux_installer_explicit_format_hint() {
        let installer = LinuxInstaller::with_format(LinuxPackageFormat::Flatpak);
        // Even with an AppImage extension, the hint should take precedence
        let path = std::path::Path::new("/tmp/MyApp.AppImage");
        assert_eq!(installer.detect_format(path), LinuxPackageFormat::Flatpak);
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[test]
    fn test_linux_installer_unknown_extension_defaults_to_appimage() {
        let installer = LinuxInstaller::new();
        let path = std::path::Path::new("/tmp/update.bin");
        // Without FLATPAK_ID or SNAP env vars, should default to AppImage
        assert_eq!(installer.detect_format(path), LinuxPackageFormat::AppImage);
    }

    #[test]
    fn test_appcast_skips_invalid_versions() {
        let xml = r#"<rss><channel>
            <item>
                <enclosure url="https://example.com/app.zip"
                           sparkle:version="not-a-version" />
            </item>
            <item>
                <enclosure url="https://example.com/app2.zip"
                           sparkle:version="1.0.0" />
            </item>
        </channel></rss>"#;

        let updates = parse_update_feed(xml).unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].version, SemanticVersion::new(1, 0, 0));
    }
}
