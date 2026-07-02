//! Cross-platform auto-updater module.
//!
//! Provides an API for checking a configurable URL for available updates,
//! downloading update packages in the background with progress callbacks,
//! and applying updates with application restart.
//!
//! Supports Sparkle appcast XML and a simpler JSON feed format for update
//! discovery.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signature, VerifyingKey};
use futures::AsyncReadExt as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use kael_release::update::{UpdateChannel, UpdateManifest, UpdatePolicy, verify_manifest};
use semantic_version::SemanticVersion;

use crate::NetworkPolicy;

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

impl AutoUpdaterConfig {
    /// Validate update feed configuration before creating an updater.
    pub fn validate(&self) -> Result<()> {
        validate_update_feed_url(&self.feed_url)?;
        anyhow::ensure!(
            self.check_interval > Duration::ZERO,
            "update check interval must be greater than zero"
        );
        Ok(())
    }
}

/// Builder for auto-updater configuration.
#[derive(Debug, Clone)]
pub struct AutoUpdaterConfigBuilder {
    feed_url: String,
    check_interval: Duration,
    allow_prerelease: bool,
}

impl AutoUpdaterConfigBuilder {
    /// Create an updater config builder with a feed URL.
    pub fn new(feed_url: impl Into<String>) -> Self {
        Self {
            feed_url: feed_url.into(),
            check_interval: Duration::from_secs(86_400),
            allow_prerelease: false,
        }
    }

    /// Set how often the host app should check for updates.
    pub fn check_interval(mut self, interval: Duration) -> Self {
        self.check_interval = interval;
        self
    }

    /// Include pre-release versions in update checks.
    pub fn allow_prerelease(mut self, allow: bool) -> Self {
        self.allow_prerelease = allow;
        self
    }

    /// Restrict update checks to stable releases.
    pub fn stable_only(mut self) -> Self {
        self.allow_prerelease = false;
        self
    }

    /// Return the configured feed URL.
    pub fn feed_url(&self) -> &str {
        &self.feed_url
    }

    /// Return the configured check interval.
    pub fn configured_check_interval(&self) -> Duration {
        self.check_interval
    }

    /// Return whether pre-release updates are allowed.
    pub fn allows_prerelease(&self) -> bool {
        self.allow_prerelease
    }

    /// Validate the configured update settings.
    pub fn validate(&self) -> Result<()> {
        self.as_config().validate()
    }

    /// Build a validated updater config.
    pub fn build_checked(self) -> Result<AutoUpdaterConfig> {
        let config = self.as_config();
        config.validate()?;
        Ok(config)
    }

    fn as_config(&self) -> AutoUpdaterConfig {
        AutoUpdaterConfig {
            feed_url: self.feed_url.clone(),
            check_interval: self.check_interval,
            allow_prerelease: self.allow_prerelease,
        }
    }
}

impl From<AutoUpdaterConfig> for AutoUpdaterConfigBuilder {
    fn from(config: AutoUpdaterConfig) -> Self {
        Self {
            feed_url: config.feed_url,
            check_interval: config.check_interval,
            allow_prerelease: config.allow_prerelease,
        }
    }
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
    /// Base64-encoded ed25519 signature over the release manifest.
    pub signature: Option<String>,
    /// Expected SHA-256 (lowercase hex) of the downloaded package.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Expected size of the downloaded package in bytes.
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

impl UpdateInfo {
    /// Validate update metadata before offering or downloading it.
    pub fn validate(&self) -> Result<()> {
        validate_update_url(&self.download_url, "update download URL")?;
        validate_optional_sha256(self.sha256.as_deref())?;
        validate_optional_size(self.size_bytes)?;
        validate_optional_signature(self.signature.as_deref())?;
        Ok(())
    }

    /// Validate metadata required for signed update verification.
    pub fn validate_signed_metadata(&self) -> Result<()> {
        self.validate()?;
        anyhow::ensure!(
            self.signature.is_some(),
            "signed update metadata requires a signature"
        );
        anyhow::ensure!(
            self.sha256.is_some(),
            "signed update metadata requires a sha256 hash"
        );
        anyhow::ensure!(
            self.size_bytes.is_some(),
            "signed update metadata requires a package size"
        );
        Ok(())
    }
}

/// Builder for update metadata entries.
#[derive(Debug, Clone)]
pub struct UpdateInfoBuilder {
    version: SemanticVersion,
    release_notes: Option<String>,
    download_url: String,
    signature: Option<String>,
    sha256: Option<String>,
    size_bytes: Option<u64>,
}

impl UpdateInfoBuilder {
    /// Create update metadata for a version and package URL.
    pub fn new(version: SemanticVersion, download_url: impl Into<String>) -> Self {
        Self {
            version,
            release_notes: None,
            download_url: download_url.into(),
            signature: None,
            sha256: None,
            size_bytes: None,
        }
    }

    /// Set optional release notes.
    pub fn release_notes(mut self, notes: impl Into<String>) -> Self {
        self.release_notes = Some(notes.into());
        self
    }

    /// Set the base64-encoded ed25519 signature.
    pub fn signature(mut self, signature: impl Into<String>) -> Self {
        self.signature = Some(signature.into());
        self
    }

    /// Set the expected lowercase SHA-256 hex digest.
    pub fn sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = Some(sha256.into());
        self
    }

    /// Set the expected package size in bytes.
    pub fn size_bytes(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }

    /// Build update metadata without requiring signed-package fields.
    pub fn build_checked(self) -> Result<UpdateInfo> {
        let update = self.as_update_info();
        update.validate()?;
        Ok(update)
    }

    /// Build update metadata and require fields needed for signed verification.
    pub fn build_signed_checked(self) -> Result<UpdateInfo> {
        let update = self.as_update_info();
        update.validate_signed_metadata()?;
        Ok(update)
    }

    fn as_update_info(&self) -> UpdateInfo {
        UpdateInfo {
            version: self.version,
            release_notes: self.release_notes.clone(),
            download_url: self.download_url.clone(),
            signature: self.signature.clone(),
            sha256: self.sha256.clone(),
            size_bytes: self.size_bytes,
        }
    }
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

/// A checked descriptor for an app-owned download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadRequest {
    /// URL to download.
    pub url: String,
    /// Destination path to write.
    pub destination: PathBuf,
    /// Optional expected SHA-256 (lowercase hex) of the downloaded bytes.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Optional expected size of the downloaded bytes.
    #[serde(default)]
    pub size_bytes: Option<u64>,
    /// Whether parent directories may be created by the download worker.
    pub create_parent_dirs: bool,
    /// Optional outbound network policy to check before starting the download.
    #[serde(default)]
    pub network_policy: Option<NetworkPolicy>,
}

impl DownloadRequest {
    /// Create a checked download request builder.
    pub fn builder(
        url: impl Into<String>,
        destination: impl Into<PathBuf>,
    ) -> DownloadRequestBuilder {
        DownloadRequestBuilder::new(url, destination)
    }

    /// Validate URL, destination, expected metadata, and network policy.
    pub fn validate(&self) -> Result<()> {
        validate_update_url(&self.url, "download URL")?;
        validate_download_destination(&self.destination)?;
        validate_optional_sha256(self.sha256.as_deref())?;
        validate_optional_size(self.size_bytes)?;
        if let Some(policy) = &self.network_policy {
            policy.validate()?;
            anyhow::ensure!(
                policy.check_url(&self.url)?,
                "download URL is denied by network policy"
            );
        }
        if !self.create_parent_dirs {
            let parent = self.destination.parent().ok_or_else(|| {
                anyhow::anyhow!("download destination must have a parent directory")
            })?;
            anyhow::ensure!(
                parent.exists(),
                "download destination parent directory must exist: {}",
                parent.display()
            );
        }
        Ok(())
    }
}

/// Builder for checked app-owned downloads.
#[derive(Debug, Clone)]
pub struct DownloadRequestBuilder {
    url: String,
    destination: PathBuf,
    sha256: Option<String>,
    size_bytes: Option<u64>,
    create_parent_dirs: bool,
    network_policy: Option<NetworkPolicy>,
}

impl DownloadRequestBuilder {
    /// Create a builder from a URL and destination path.
    pub fn new(url: impl Into<String>, destination: impl Into<PathBuf>) -> Self {
        Self {
            url: url.into(),
            destination: destination.into(),
            sha256: None,
            size_bytes: None,
            create_parent_dirs: false,
            network_policy: None,
        }
    }

    /// Set the expected lowercase SHA-256 hex digest.
    pub fn sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = Some(sha256.into());
        self
    }

    /// Set the expected size in bytes.
    pub fn size_bytes(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }

    /// Allow the download worker to create missing parent directories.
    pub fn create_parent_dirs(mut self) -> Self {
        self.create_parent_dirs = true;
        self
    }

    /// Require the destination parent directory to already exist.
    pub fn require_existing_parent(mut self) -> Self {
        self.create_parent_dirs = false;
        self
    }

    /// Attach an outbound network policy.
    pub fn network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Validate and build the request.
    pub fn build_checked(self) -> Result<DownloadRequest> {
        let request = DownloadRequest {
            url: self.url,
            destination: self.destination,
            sha256: self.sha256,
            size_bytes: self.size_bytes,
            create_parent_dirs: self.create_parent_dirs,
            network_policy: self.network_policy,
        };
        request.validate()?;
        Ok(request)
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
    verifying_key: Option<VerifyingKey>,
    update_channel: UpdateChannel,
    require_signature: bool,
    policy: Option<UpdatePolicy>,
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
            verifying_key: None,
            update_channel: UpdateChannel::Stable,
            require_signature: true,
            policy: None,
        }
    }

    /// Create a new auto-updater after validating its configuration.
    pub fn new_checked(
        config: impl Into<AutoUpdaterConfigBuilder>,
        current_version: SemanticVersion,
        http_client: Arc<dyn http_client::HttpClient>,
    ) -> Result<Self> {
        Ok(Self::new(
            config.into().build_checked()?,
            current_version,
            http_client,
        ))
    }

    /// Set the platform-specific installer backend.
    pub fn set_installer(&mut self, installer: Arc<dyn PlatformInstaller>) {
        self.installer = Some(installer);
    }

    /// Configure the ed25519 public key used to authenticate update manifests.
    ///
    /// `public_key` must be the 32-byte ed25519 public key whose private
    /// counterpart signs release manifests. Once configured, every downloaded
    /// package is refused before installation unless it carries a valid
    /// signature over a manifest matching its advertised version, channel, URL,
    /// hash, and size, and the downloaded bytes hash to that signed SHA-256.
    pub fn set_public_key(&mut self, public_key: &[u8]) -> Result<()> {
        let key_array: [u8; 32] = public_key
            .try_into()
            .map_err(|_| anyhow!("ed25519 public key must be exactly 32 bytes"))?;
        let key = VerifyingKey::from_bytes(&key_array)
            .map_err(|_| anyhow!("invalid ed25519 public key"))?;
        self.verifying_key = Some(key);
        Ok(())
    }

    /// Configure the ed25519 public key from a hex-encoded string.
    pub fn set_public_key_hex(&mut self, hex_key: &str) -> Result<()> {
        let bytes = hex::decode(hex_key.trim()).context("update public key is not valid hex")?;
        self.set_public_key(&bytes)
    }

    /// Set the release channel whose manifests this updater trusts.
    ///
    /// The channel participates in the signed manifest payload, so it must match
    /// the channel the publisher signed for. Defaults to the stable channel.
    pub fn set_update_channel(&mut self, channel: impl AsRef<str>) {
        self.update_channel = channel_from_str(channel.as_ref());
    }

    /// Control whether a valid signature and hash are mandatory before install.
    ///
    /// Defaults to `true` (fail closed). Disabling this re-opens the updater to
    /// installing unverified packages and is intended only for tests or
    /// environments that guarantee package integrity by other means.
    pub fn set_require_signature(&mut self, require: bool) {
        self.require_signature = require;
    }

    /// Apply a [`UpdatePolicy`] from `kael_release`.
    ///
    /// This adopts the policy's channel and its `require_signed_feeds` setting
    /// (mapped onto signature enforcement). The policy's auto-check/download/
    /// install flags and check interval are surfaced via [`Self::policy`] for
    /// the host application to drive scheduling; they do not change verification
    /// behavior.
    pub fn apply_policy(&mut self, policy: &UpdatePolicy) {
        self.update_channel = policy.channel.clone();
        self.require_signature = policy.require_signed_feeds;
        self.config.check_interval = Duration::from_secs(policy.check_interval_secs);
        self.policy = Some(policy.clone());
    }

    /// Returns the policy applied via [`Self::apply_policy`], if any.
    pub fn policy(&self) -> Option<&UpdatePolicy> {
        self.policy.as_ref()
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

        let mut bytes: Vec<u8> = match total_bytes {
            Some(total) => Vec::with_capacity(total.min(64 * 1024 * 1024) as usize),
            None => Vec::new(),
        };
        let body = response.body_mut();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let read = body
                .read(&mut chunk)
                .await
                .context("failed to read update package")?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..read]);
            on_progress(DownloadProgress {
                bytes_downloaded: bytes.len() as u64,
                total_bytes,
            });
        }

        if let Err(err) = self.verify_package(&update, &bytes) {
            self.downloaded_path = None;
            self.status = UpdateStatus::Error(err.to_string());
            return Err(err).context("update package failed verification; refusing to install");
        }

        let staging_dir =
            std::env::temp_dir().join(format!("kael_update_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&staging_dir)
            .context("failed to create update staging directory")?;
        restrict_dir_permissions(&staging_dir);

        let download_path = staging_dir.join(sanitize_package_filename(&update.download_url));
        std::fs::write(&download_path, &bytes).context("failed to write update package to disk")?;

        self.downloaded_path = Some(download_path.clone());
        self.status = UpdateStatus::ReadyToInstall;

        Ok(download_path)
    }

    fn verify_package(&self, update: &UpdateInfo, bytes: &[u8]) -> Result<()> {
        match self.verifying_key.as_ref() {
            Some(key) => {
                let signature_b64 = update.signature.as_deref().ok_or_else(|| {
                    anyhow!("update is unsigned but signature verification is required")
                })?;
                let signature_bytes = BASE64
                    .decode(signature_b64)
                    .context("update signature is not valid base64")?;
                let signature_array: [u8; 64] = signature_bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| anyhow!("update signature must be 64 bytes"))?;
                let signature = Signature::from_bytes(&signature_array);

                let sha256 = update
                    .sha256
                    .as_deref()
                    .ok_or_else(|| anyhow!("signed update is missing its sha256 hash"))?;
                let size_bytes = update
                    .size_bytes
                    .ok_or_else(|| anyhow!("signed update is missing its size"))?;

                let manifest = UpdateManifest {
                    version: update.version.to_string(),
                    channel: self.update_channel.clone(),
                    url: update.download_url.clone(),
                    sha256: sha256.to_string(),
                    size_bytes,
                    release_notes: None,
                    min_version: None,
                };
                if !verify_manifest(&manifest, &signature, key) {
                    bail!("update signature verification failed");
                }
            }
            None => {
                if self.require_signature {
                    bail!(
                        "auto-update signature verification is required but no public key is configured"
                    );
                }
            }
        }

        match update.sha256.as_deref() {
            Some(expected) => {
                if let Some(expected_size) = update.size_bytes {
                    if bytes.len() as u64 != expected_size {
                        bail!(
                            "update size mismatch: expected {expected_size} bytes, downloaded {}",
                            bytes.len()
                        );
                    }
                }
                let actual = sha256_hex(bytes);
                if actual.len() != expected.len() || !actual.eq_ignore_ascii_case(expected) {
                    bail!("update hash mismatch: expected {expected}, downloaded {actual}");
                }
            }
            None => {
                if self.require_signature {
                    bail!("update is missing a sha256 hash; cannot verify integrity");
                }
            }
        }

        Ok(())
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

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn channel_from_str(channel: &str) -> UpdateChannel {
    let trimmed = channel.trim();
    if trimmed.eq_ignore_ascii_case("stable") {
        UpdateChannel::Stable
    } else if trimmed.eq_ignore_ascii_case("beta") {
        UpdateChannel::Beta
    } else if trimmed.eq_ignore_ascii_case("nightly") {
        UpdateChannel::Nightly
    } else {
        UpdateChannel::Custom(trimmed.to_string())
    }
}

fn validate_update_feed_url(feed_url: &str) -> Result<()> {
    validate_update_url(feed_url, "update feed URL")
}

fn validate_update_url(url: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!url.trim().is_empty(), "{} cannot be empty", label);
    anyhow::ensure!(
        url == url.trim(),
        "{} cannot have leading or trailing whitespace",
        label
    );

    let parsed = http_client::Url::parse(url).with_context(|| format!("{label} is invalid"))?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "https" | "http"),
        "{} must use http or https",
        label
    );
    anyhow::ensure!(parsed.host_str().is_some(), "{} must include a host", label);
    Ok(())
}

fn validate_optional_sha256(sha256: Option<&str>) -> Result<()> {
    let Some(sha256) = sha256 else {
        return Ok(());
    };

    anyhow::ensure!(
        sha256.len() == 64 && sha256.chars().all(|ch| ch.is_ascii_hexdigit()),
        "update sha256 must be a 64-character hex digest"
    );
    Ok(())
}

fn validate_optional_size(size_bytes: Option<u64>) -> Result<()> {
    if let Some(size_bytes) = size_bytes {
        anyhow::ensure!(
            size_bytes > 0,
            "update package size must be greater than zero"
        );
    }
    Ok(())
}

fn validate_download_destination(destination: &std::path::Path) -> Result<()> {
    let destination_text = destination.to_string_lossy();
    anyhow::ensure!(
        !destination_text.trim().is_empty(),
        "download destination cannot be empty"
    );
    anyhow::ensure!(
        destination.is_absolute(),
        "download destination must be absolute: {}",
        destination.display()
    );
    anyhow::ensure!(
        !destination_text.chars().any(|ch| ch == '\0'),
        "download destination cannot contain NUL characters"
    );
    anyhow::ensure!(
        !destination.is_dir(),
        "download destination cannot be an existing directory: {}",
        destination.display()
    );
    Ok(())
}

fn validate_optional_signature(signature: Option<&str>) -> Result<()> {
    let Some(signature) = signature else {
        return Ok(());
    };

    anyhow::ensure!(
        !signature.trim().is_empty(),
        "update signature cannot be empty"
    );
    anyhow::ensure!(
        signature == signature.trim(),
        "update signature cannot have leading or trailing whitespace"
    );
    let signature_bytes = BASE64
        .decode(signature)
        .context("update signature is not valid base64")?;
    anyhow::ensure!(
        signature_bytes.len() == 64,
        "update signature must decode to 64 bytes"
    );
    Ok(())
}

fn sanitize_package_filename(download_url: &str) -> String {
    let candidate = download_url
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let cleaned: String = candidate
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "update_package".to_string()
    } else {
        cleaned
    }
}

fn restrict_dir_permissions(dir: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
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
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
}

/// The platform-keyed update feed emitted by `xtask generate-update-metadata`
/// (mirrors `xtask::update_feed::UpdateFeed`).
#[derive(Debug, Deserialize)]
struct PlatformFeed {
    version: String,
    #[serde(default)]
    notes_url: Option<String>,
    platforms: Vec<PlatformFeedEntry>,
}

/// One platform's entry in a [`PlatformFeed`].
#[derive(Debug, Deserialize)]
struct PlatformFeedEntry {
    platform: String,
    url: String,
    #[serde(default)]
    signature: Option<String>,
    checksum: String,
    #[serde(default)]
    size_bytes: Option<u64>,
}

/// Parse a JSON update feed.
///
/// Accepts three shapes:
/// - a JSON array of feed items,
/// - an object with an `"items"` array, or
/// - the platform-keyed feed produced by `xtask generate-update-metadata`
///   (an object with `"version"` and a `"platforms"` array). For that shape the
///   entry matching the running operating system is selected.
fn parse_json_feed(body: &str) -> Result<Vec<UpdateInfo>> {
    let trimmed = body.trim();

    if trimmed.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(trimmed).context("failed to parse JSON update feed as object")?;
        if value.get("platforms").is_some() {
            return parse_platform_feed(trimmed);
        }
    }

    let items: Vec<JsonFeedItem> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).context("failed to parse JSON update feed as array")?
    } else {
        #[derive(Deserialize)]
        struct Wrapper {
            items: Vec<JsonFeedItem>,
        }
        let wrapper: Wrapper =
            serde_json::from_str(trimmed).context("failed to parse JSON update feed as object")?;
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
                sha256: item.sha256,
                size_bytes: item.size_bytes,
            })
        })
        .collect()
}

/// The platform identifier matching the running operating system, as written
/// by `xtask`'s `detect_platform`.
fn current_platform_id() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// Map the [`PlatformFeed`] entry for the running OS into an [`UpdateInfo`].
fn parse_platform_feed(body: &str) -> Result<Vec<UpdateInfo>> {
    let feed: PlatformFeed =
        serde_json::from_str(body).context("failed to parse platform update feed")?;
    let version = feed
        .version
        .parse::<SemanticVersion>()
        .context(format!("invalid version string: {}", feed.version))?;

    let wanted = current_platform_id();
    let Some(entry) = feed
        .platforms
        .into_iter()
        .find(|entry| entry.platform.eq_ignore_ascii_case(wanted))
    else {
        return Ok(Vec::new());
    };

    Ok(vec![UpdateInfo {
        version,
        release_notes: feed.notes_url,
        download_url: entry.url,
        signature: entry.signature,
        sha256: Some(entry.checksum),
        size_bytes: entry.size_bytes,
    }])
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

        let signature = extract_xml_attr(&item_block, "sparkle:edSignature")
            .or_else(|| extract_xml_attr(&item_block, "sparkle:dsaSignature"));

        let sha256 = extract_xml_attr(&item_block, "sparkle:sha256")
            .or_else(|| extract_xml_attr(&item_block, "sha256"));

        let size_bytes =
            extract_xml_attr(&item_block, "length").and_then(|len| len.parse::<u64>().ok());

        let release_notes = extract_xml_tag_content(&item_block, "description");

        if let (Some(version_str), Some(download_url)) = (version_str, download_url) {
            if let Ok(version) = version_str.parse::<SemanticVersion>() {
                updates.push(UpdateInfo {
                    version,
                    release_notes,
                    download_url,
                    signature,
                    sha256,
                    size_bytes,
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

/// Validate the code signature of `new_app`, then atomically swap it over
/// `existing_app` with rollback on failure.
///
/// The new bundle is first copied into a staging location on the same volume as
/// the live install so the swap can use an atomic `rename`. Verification uses
/// `codesign --verify`; an invalid signature aborts before any swap occurs.
#[cfg(target_os = "macos")]
fn replace_app_bundle(new_app: &std::path::Path, existing_app: &std::path::Path) -> Result<()> {
    use kael_release::apply::{FsInstaller, SwapPlan, atomic_swap_with_rollback, verify_codesign};

    verify_codesign(new_app).context("downloaded app bundle failed codesign verification")?;

    let parent = existing_app
        .parent()
        .ok_or_else(|| anyhow!("could not determine parent directory of {existing_app:?}"))?;
    let file_name = existing_app
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Kael.app");

    let staged = parent.join(format!(".{file_name}.staged"));
    if staged.exists() {
        std::fs::remove_dir_all(&staged)?;
    }

    let status = std::process::Command::new("cp")
        .args(["-R", &new_app.to_string_lossy(), &staged.to_string_lossy()])
        .status()
        .context("failed to stage new app bundle")?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&staged);
        bail!("failed to stage new app bundle into place");
    }

    let plan = SwapPlan {
        live: existing_app.to_path_buf(),
        staged,
        backup: existing_app.with_extension("app.backup"),
    };

    atomic_swap_with_rollback(&FsInstaller, &plan)
        .context("failed to swap new app bundle into place")?;
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
    fn test_parse_platform_feed_selects_current_os() {
        let feed = r#"{
            "version": "4.2.0",
            "channel": "stable",
            "url": "https://dl.kael.dev/feed",
            "notes_url": "https://dl.kael.dev/notes/4.2.0",
            "pub_date": "2026-06-11T00:00:00Z",
            "platforms": [
                {
                    "platform": "macos",
                    "url": "https://dl.kael.dev/Kael-macos.zip",
                    "signature": "c2ln",
                    "checksum": "aa",
                    "size_bytes": 1234
                },
                {
                    "platform": "windows",
                    "url": "https://dl.kael.dev/Kael.msi",
                    "signature": "c2ln",
                    "checksum": "bb",
                    "size_bytes": 5678
                },
                {
                    "platform": "linux",
                    "url": "https://dl.kael.dev/Kael-linux.tar.gz",
                    "signature": "c2ln",
                    "checksum": "cc",
                    "size_bytes": 9012
                }
            ]
        }"#;

        let updates = parse_update_feed(feed).unwrap();
        assert_eq!(updates.len(), 1, "exactly one entry for the running OS");
        let update = &updates[0];
        assert_eq!(update.version, SemanticVersion::new(4, 2, 0));

        let expected_url = match current_platform_id() {
            "macos" => "https://dl.kael.dev/Kael-macos.zip",
            "windows" => "https://dl.kael.dev/Kael.msi",
            _ => "https://dl.kael.dev/Kael-linux.tar.gz",
        };
        assert_eq!(update.download_url, expected_url);
        assert!(update.sha256.is_some());
        assert!(update.size_bytes.is_some());
        assert_eq!(
            update.release_notes.as_deref(),
            Some("https://dl.kael.dev/notes/4.2.0")
        );
    }

    #[test]
    fn test_platform_feed_without_current_os_yields_none() {
        // A feed that only carries a platform the running OS never matches.
        let bogus = if current_platform_id() == "linux" {
            "windows"
        } else {
            "linux"
        };
        let feed = format!(
            r#"{{
                "version": "1.0.0",
                "channel": "stable",
                "url": "https://dl.kael.dev/feed",
                "pub_date": "2026-06-11T00:00:00Z",
                "platforms": [
                    {{"platform": "{bogus}", "url": "https://dl.kael.dev/x", "checksum": "aa", "size_bytes": 1}}
                ]
            }}"#
        );
        let updates = parse_update_feed(&feed).unwrap();
        assert!(updates.is_empty());
    }

    #[test]
    fn test_platform_feed_version_comparison_filters_older() {
        // check_for_updates filters by version; emulate the same comparison the
        // updater performs against an older current version.
        let feed = r#"{
            "version": "2.0.0",
            "channel": "stable",
            "url": "https://dl.kael.dev/feed",
            "pub_date": "2026-06-11T00:00:00Z",
            "platforms": [
                {"platform": "macos", "url": "https://dl.kael.dev/m", "checksum": "aa", "size_bytes": 1},
                {"platform": "windows", "url": "https://dl.kael.dev/w", "checksum": "bb", "size_bytes": 1},
                {"platform": "linux", "url": "https://dl.kael.dev/l", "checksum": "cc", "size_bytes": 1}
            ]
        }"#;
        let updates = parse_update_feed(feed).unwrap();
        let current = SemanticVersion::new(1, 0, 0);
        let newer: Vec<_> = updates.iter().filter(|u| u.version > current).collect();
        assert_eq!(newer.len(), 1);

        let same_or_newer_current = SemanticVersion::new(2, 0, 0);
        let none: Vec<_> = updates
            .iter()
            .filter(|u| u.version > same_or_newer_current)
            .collect();
        assert!(
            none.is_empty(),
            "equal version must not be offered as update"
        );
    }

    #[test]
    fn test_apply_policy_sets_channel_and_signature_requirement() {
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let client = http_client::FakeHttpClient::with_200_response();
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);

        let mut policy = UpdatePolicy::default_stable();
        policy.channel = UpdateChannel::Beta;
        policy.require_signed_feeds = false;
        policy.check_interval_secs = 7200;
        updater.apply_policy(&policy);

        assert_eq!(updater.update_channel, UpdateChannel::Beta);
        assert!(!updater.require_signature);
        assert_eq!(updater.config().check_interval, Duration::from_secs(7200));
        assert!(updater.policy().is_some());
    }

    #[test]
    fn test_apply_policy_fails_closed_when_requiring_signed_feeds() {
        let bytes = b"genuine update payload".to_vec();
        let (_key, update) = signed_update_fixture(&bytes, UpdateChannel::Stable);
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let client = http_client::FakeHttpClient::with_200_response();
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);

        // Default policy requires signed feeds; with no public key configured,
        // verification must fail closed.
        updater.apply_policy(&UpdatePolicy::default_stable());
        let err = updater.verify_package(&update, &bytes).unwrap_err();
        assert!(
            err.to_string().contains("no public key is configured"),
            "{err}"
        );
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
    fn download_request_builder_validates_common_downloads() {
        let destination = std::env::temp_dir().join("kael-download-request.bin");
        let request =
            DownloadRequest::builder("https://example.com/files/report.pdf", &destination)
                .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .size_bytes(1024)
                .network_policy(
                    crate::NetworkPolicyBuilder::new()
                        .allow_host("example.com")
                        .build_checked()
                        .unwrap(),
                )
                .build_checked()
                .unwrap();

        assert_eq!(request.destination, destination);
        assert_eq!(request.size_bytes, Some(1024));
    }

    #[test]
    fn download_request_builder_rejects_generated_footguns() {
        let destination = std::env::temp_dir().join("kael-download-request.bin");
        assert!(
            DownloadRequest::builder("file:///tmp/data.bin", &destination)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadRequest::builder(" https://example.com/data.bin", &destination)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadRequest::builder("https://example.com/data.bin", "relative.bin")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadRequest::builder("https://example.com/data.bin", std::env::temp_dir())
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadRequest::builder("https://example.com/data.bin", &destination)
                .sha256("bad")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadRequest::builder("https://example.com/data.bin", &destination)
                .size_bytes(0)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn download_request_builder_checks_policy_and_parent_dirs() {
        let missing_parent = std::env::temp_dir()
            .join(format!(
                "kael-missing-download-parent-{}",
                std::process::id()
            ))
            .join("file.bin");

        assert!(
            DownloadRequest::builder("https://example.com/data.bin", &missing_parent)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadRequest::builder("https://example.com/data.bin", &missing_parent)
                .create_parent_dirs()
                .build_checked()
                .is_ok()
        );
        assert!(
            DownloadRequest::builder("https://blocked.example.com/data.bin", missing_parent)
                .network_policy(
                    crate::NetworkPolicyBuilder::new()
                        .allow_host("example.com")
                        .build_checked()
                        .unwrap(),
                )
                .create_parent_dirs()
                .build_checked()
                .is_err()
        );
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
    fn test_auto_updater_config_builder_validates_feed_and_interval() {
        let config = AutoUpdaterConfigBuilder::new("https://example.com/feed.json")
            .check_interval(Duration::from_secs(60))
            .allow_prerelease(true)
            .build_checked()
            .unwrap();

        assert_eq!(config.feed_url, "https://example.com/feed.json");
        assert_eq!(config.check_interval, Duration::from_secs(60));
        assert!(config.allow_prerelease);

        let default_config = AutoUpdaterConfigBuilder::new("https://example.com/feed.json");
        assert_eq!(default_config.feed_url(), "https://example.com/feed.json");
        assert_eq!(
            default_config.configured_check_interval(),
            Duration::from_secs(86_400)
        );
        assert!(!default_config.allows_prerelease());

        assert!(
            AutoUpdaterConfigBuilder::new(" https://example.com/feed.json")
                .validate()
                .is_err()
        );
        assert!(AutoUpdaterConfigBuilder::new("").validate().is_err());
        assert!(
            AutoUpdaterConfigBuilder::new("file:///tmp/feed.json")
                .validate()
                .is_err()
        );
        assert!(
            AutoUpdaterConfigBuilder::new("https://example.com/feed.json")
                .check_interval(Duration::ZERO)
                .validate()
                .is_err()
        );

        let raw = AutoUpdaterConfig {
            feed_url: "https://example.com/feed.json".to_string(),
            check_interval: Duration::from_secs(1),
            allow_prerelease: false,
        };
        assert!(raw.validate().is_ok());
        assert!(
            AutoUpdaterConfig {
                feed_url: "not a url".to_string(),
                check_interval: Duration::from_secs(1),
                allow_prerelease: false,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn test_auto_updater_new_checked_validates_config() {
        let client = http_client::FakeHttpClient::with_200_response();
        let updater = AutoUpdater::new_checked(
            AutoUpdaterConfigBuilder::new("https://example.com/feed.json"),
            SemanticVersion::new(1, 0, 0),
            client.clone(),
        )
        .unwrap();

        assert_eq!(
            updater.config().feed_url,
            "https://example.com/feed.json".to_string()
        );
        assert!(
            AutoUpdater::new_checked(
                AutoUpdaterConfigBuilder::new("https://example.com/feed.json")
                    .check_interval(Duration::ZERO),
                SemanticVersion::new(1, 0, 0),
                client,
            )
            .is_err()
        );
    }

    #[test]
    fn test_update_info_serialization_roundtrip() {
        let info = UpdateInfo {
            version: SemanticVersion::new(2, 5, 1),
            release_notes: Some("Fixed a bug".to_string()),
            download_url: "https://example.com/v2.5.1.zip".to_string(),
            signature: Some("sig_value".to_string()),
            sha256: Some("a".repeat(64)),
            size_bytes: Some(4096),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: UpdateInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.version, info.version);
        assert_eq!(deserialized.release_notes, info.release_notes);
        assert_eq!(deserialized.download_url, info.download_url);
        assert_eq!(deserialized.signature, info.signature);
        assert_eq!(deserialized.sha256, info.sha256);
        assert_eq!(deserialized.size_bytes, info.size_bytes);
    }

    #[test]
    fn test_update_info_builder_validates_metadata() {
        let signature = BASE64.encode([7u8; 64]);
        let update = UpdateInfoBuilder::new(
            SemanticVersion::new(2, 5, 1),
            "https://example.com/v2.5.1.zip",
        )
        .release_notes("Fixed a bug")
        .signature(signature.clone())
        .sha256("a".repeat(64))
        .size_bytes(4096)
        .build_signed_checked()
        .unwrap();

        assert_eq!(update.version, SemanticVersion::new(2, 5, 1));
        assert_eq!(update.release_notes.as_deref(), Some("Fixed a bug"));
        assert_eq!(update.signature.as_deref(), Some(signature.as_str()));
        assert!(update.validate_signed_metadata().is_ok());

        assert!(
            UpdateInfoBuilder::new(SemanticVersion::new(2, 5, 1), "file:///tmp/update.zip")
                .build_checked()
                .is_err()
        );
        assert!(
            UpdateInfoBuilder::new(
                SemanticVersion::new(2, 5, 1),
                "https://example.com/update.zip",
            )
            .sha256("not-a-sha")
            .build_checked()
            .is_err()
        );
        assert!(
            UpdateInfoBuilder::new(
                SemanticVersion::new(2, 5, 1),
                "https://example.com/update.zip",
            )
            .size_bytes(0)
            .build_checked()
            .is_err()
        );
        assert!(
            UpdateInfoBuilder::new(
                SemanticVersion::new(2, 5, 1),
                "https://example.com/update.zip",
            )
            .signature("not-base64")
            .build_checked()
            .is_err()
        );
        assert!(
            UpdateInfoBuilder::new(
                SemanticVersion::new(2, 5, 1),
                "https://example.com/update.zip",
            )
            .sha256("a".repeat(64))
            .size_bytes(4096)
            .build_signed_checked()
            .is_err()
        );
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
    // Update verification tests (RCE hotfix)
    // -----------------------------------------------------------------------

    fn signed_update_fixture(bytes: &[u8], channel: UpdateChannel) -> (VerifyingKey, UpdateInfo) {
        use ed25519_dalek::SigningKey;

        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let verifying_key = signing_key.verifying_key();

        let sha256 = sha256_hex(bytes);
        let size_bytes = bytes.len() as u64;
        let version = SemanticVersion::new(1, 2, 0);
        let download_url = "https://example.com/MyApp-1.2.0.zip".to_string();

        let manifest = UpdateManifest {
            version: version.to_string(),
            channel,
            url: download_url.clone(),
            sha256: sha256.clone(),
            size_bytes,
            release_notes: None,
            min_version: None,
        };
        let signature = kael_release::update::sign_manifest(&manifest, &signing_key);
        let signature_b64 = BASE64.encode(signature.to_bytes());

        (
            verifying_key,
            UpdateInfo {
                version,
                release_notes: None,
                download_url,
                signature: Some(signature_b64),
                sha256: Some(sha256),
                size_bytes: Some(size_bytes),
            },
        )
    }

    fn updater_with_key(key: &VerifyingKey) -> AutoUpdater {
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let client = http_client::FakeHttpClient::with_200_response();
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.set_public_key(key.as_bytes()).unwrap();
        updater
    }

    #[test]
    fn test_verify_package_accepts_genuine_payload() {
        let bytes = b"genuine update payload".to_vec();
        let (key, update) = signed_update_fixture(&bytes, UpdateChannel::Stable);
        let updater = updater_with_key(&key);
        assert!(updater.verify_package(&update, &bytes).is_ok());
    }

    #[test]
    fn test_verify_package_rejects_tampered_bytes() {
        let bytes = b"genuine update payload".to_vec();
        let (key, update) = signed_update_fixture(&bytes, UpdateChannel::Stable);
        let updater = updater_with_key(&key);

        let tampered = b"malware payload xxxxxx".to_vec();
        assert_eq!(tampered.len(), bytes.len());
        let err = updater.verify_package(&update, &tampered).unwrap_err();
        assert!(err.to_string().contains("hash mismatch"), "{err}");
    }

    #[test]
    fn test_verify_package_rejects_unsigned_when_key_configured() {
        let bytes = b"genuine update payload".to_vec();
        let (key, mut update) = signed_update_fixture(&bytes, UpdateChannel::Stable);
        update.signature = None;
        let updater = updater_with_key(&key);
        let err = updater.verify_package(&update, &bytes).unwrap_err();
        assert!(err.to_string().contains("unsigned"), "{err}");
    }

    #[test]
    fn test_verify_package_rejects_wrong_key() {
        let bytes = b"genuine update payload".to_vec();
        let (_real_key, update) = signed_update_fixture(&bytes, UpdateChannel::Stable);
        let other = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]).verifying_key();
        let updater = updater_with_key(&other);
        let err = updater.verify_package(&update, &bytes).unwrap_err();
        assert!(
            err.to_string().contains("signature verification failed"),
            "{err}"
        );
    }

    #[test]
    fn test_verify_package_rejects_channel_mismatch() {
        let bytes = b"genuine update payload".to_vec();
        let (key, update) = signed_update_fixture(&bytes, UpdateChannel::Beta);
        let mut updater = updater_with_key(&key);
        updater.set_update_channel("stable");
        assert!(updater.verify_package(&update, &bytes).is_err());
    }

    #[test]
    fn test_verify_fails_closed_without_public_key() {
        let bytes = b"genuine update payload".to_vec();
        let (_key, update) = signed_update_fixture(&bytes, UpdateChannel::Stable);
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let client = http_client::FakeHttpClient::with_200_response();
        let updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        let err = updater.verify_package(&update, &bytes).unwrap_err();
        assert!(
            err.to_string().contains("no public key is configured"),
            "{err}"
        );
    }

    #[test]
    fn test_sanitize_package_filename_stays_a_single_path_component() {
        use std::path::{Component, Path};

        let adversarial = [
            "https://example.com/releases/kael-1.2.3.dmg",
            "https://example.com/kael.dmg?token=secret#frag",
            "https://example.com/../../etc/passwd",
            "https://example.com/foo/..",
            "https://example.com/a\\b\\evil.exe",
            "https://example.com/",
            "https://example.com/???",
            "file:///etc/shadow",
            "../../../../root/.ssh/authorized_keys",
            "",
            ".",
            "..",
            "/absolute/evil",
        ];

        for url in adversarial {
            let name = sanitize_package_filename(url);
            assert!(!name.is_empty(), "empty name for {url:?}");
            assert!(
                !name.contains('/') && !name.contains('\\'),
                "separator survived for {url:?}: {name:?}"
            );
            assert_ne!(name, "..", "traversal token survived for {url:?}");

            let components: Vec<_> = Path::new(&name).components().collect();
            assert_eq!(
                components.len(),
                1,
                "{url:?} -> {name:?} is not exactly one path component"
            );
            assert!(
                matches!(components[0], Component::Normal(_)),
                "{url:?} -> {name:?} is not a normal path component"
            );
        }
    }

    #[test]
    fn test_download_update_rejects_tampered_before_ready() {
        use http_client::{AsyncBody, FakeHttpClient, Response};

        let genuine = b"genuine update payload".to_vec();
        let (key, update) = signed_update_fixture(&genuine, UpdateChannel::Stable);

        let served = b"malware payload xxxxxx".to_vec();
        let client = FakeHttpClient::create(move |_req| {
            let body = served.clone();
            async move {
                Ok(Response::builder()
                    .status(200)
                    .body(AsyncBody::from(body))
                    .unwrap())
            }
        });

        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.set_public_key(key.as_bytes()).unwrap();
        updater.latest_update = Some(update);

        let result = smol::block_on(updater.download_update(|_| {}));
        assert!(result.is_err());
        assert!(matches!(updater.status(), UpdateStatus::Error(_)));
        assert_ne!(*updater.status(), UpdateStatus::ReadyToInstall);
        assert!(updater.downloaded_path.is_none());
        assert!(updater.install_and_restart().is_err());
    }

    #[test]
    fn test_download_update_accepts_genuine_and_marks_ready() {
        use http_client::{AsyncBody, FakeHttpClient, Response};

        let genuine = b"genuine update payload".to_vec();
        let (key, update) = signed_update_fixture(&genuine, UpdateChannel::Stable);

        let served = genuine.clone();
        let client = FakeHttpClient::create(move |_req| {
            let body = served.clone();
            async move {
                Ok(Response::builder()
                    .status(200)
                    .body(AsyncBody::from(body))
                    .unwrap())
            }
        });

        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.set_public_key(key.as_bytes()).unwrap();
        updater.latest_update = Some(update);

        let path = smol::block_on(updater.download_update(|_| {})).unwrap();
        assert_eq!(*updater.status(), UpdateStatus::ReadyToInstall);
        assert!(path.exists());

        let path_str = path.to_string_lossy();
        assert!(path_str.contains("kael_update_"), "{path_str}");
        assert!(!path_str.contains("gpui_update_"), "{path_str}");

        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk, genuine);

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
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

    /// Exercise the real atomic apply path against a dummy `.app` bundle in a
    /// tempdir, including rollback. Uses the shared kael_release apply machinery
    /// directly so no `codesign`/`open`/`hdiutil` side effects are triggered.
    #[test]
    fn test_apply_swaps_dummy_app_bundle_in_tempdir() {
        use kael_release::apply::{FsInstaller, SwapPlan, atomic_swap_with_rollback};

        let root = std::env::temp_dir().join(format!("kael_apply_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();

        let make_bundle = |path: &std::path::Path, marker: &str| {
            let macos = path.join("Contents").join("MacOS");
            std::fs::create_dir_all(&macos).unwrap();
            std::fs::write(path.join("Contents").join("Info.plist"), marker).unwrap();
            std::fs::write(macos.join("kael"), marker).unwrap();
        };

        let live = root.join("Kael.app");
        let staged = root.join(".Kael.app.staged");
        let backup = root.join("Kael.app.backup");
        make_bundle(&live, "v1");
        make_bundle(&staged, "v2");

        let plan = SwapPlan {
            live: live.clone(),
            staged: staged.clone(),
            backup: backup.clone(),
        };
        let state = atomic_swap_with_rollback(&FsInstaller, &plan).unwrap();
        assert!(state.is_committed());
        assert_eq!(
            std::fs::read_to_string(live.join("Contents").join("Info.plist")).unwrap(),
            "v2"
        );
        assert!(!backup.exists());
        assert!(!staged.exists());

        // Now drive a rollback: a staged path that does not exist forces the
        // swap to fail, and the original bundle must be restored intact.
        let missing = root.join(".missing.app.staged");
        let plan = SwapPlan {
            live: live.clone(),
            staged: missing,
            backup: root.join("Kael.app.backup2"),
        };
        let result = atomic_swap_with_rollback(&FsInstaller, &plan);
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(live.join("Contents").join("Info.plist")).unwrap(),
            "v2",
            "live bundle must be restored after a failed swap"
        );

        let _ = std::fs::remove_dir_all(&root);
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
