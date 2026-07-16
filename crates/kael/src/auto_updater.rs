//! Cross-platform auto-updater module.
//!
//! Provides an API for checking a configurable URL for available updates,
//! downloading update packages in the background with progress callbacks,
//! and applying updates with application restart.
//!
//! Supports Sparkle appcast XML and a simpler JSON feed format for update
//! discovery.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{fs::OpenOptions, io::Write as _};

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

const MAX_UPDATE_FEED_BYTES: usize = 4 * 1024 * 1024;
const MAX_UPDATE_PACKAGE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_RELEASE_NOTES_BYTES: usize = 1024 * 1024;
const MAX_UPDATE_URL_BYTES: usize = 16 * 1024;

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
        anyhow::ensure!(
            self.release_notes
                .as_ref()
                .is_none_or(|notes| notes.len() <= MAX_RELEASE_NOTES_BYTES),
            "update release notes exceed {MAX_RELEASE_NOTES_BYTES} bytes"
        );
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
            .filter(|total| *total > 0)
            .map(|total| (self.bytes_downloaded as f64 / total as f64).min(1.0))
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

    /// Whether this request includes expected SHA-256 integrity metadata.
    pub fn has_sha256(&self) -> bool {
        self.sha256.is_some()
    }

    /// Whether this request includes an expected download size.
    pub fn has_size(&self) -> bool {
        self.size_bytes.is_some()
    }

    /// Whether a network policy will be checked before the download starts.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// Returns a compact, credential-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        let url = download_url_summary(&self.url);
        let size = self
            .size_bytes
            .map(|bytes| format!("{bytes} bytes"))
            .unwrap_or_else(|| "unknown".to_string());

        format!(
            "download request from {url} to {}, sha256 {}, size {size}, create parent dirs {}, network policy {}",
            self.destination.display(),
            if self.has_sha256() { "present" } else { "none" },
            self.create_parent_dirs,
            if self.has_network_policy() {
                "present"
            } else {
                "none"
            }
        )
    }

    /// Returns a host/path/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        format!(
            "download request: url true, destination true, sha256 {}, size {}, create parent dirs {}, network policy {}",
            self.has_sha256(),
            self.has_size(),
            self.create_parent_dirs,
            self.has_network_policy()
        )
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

    /// Whether this builder includes expected SHA-256 integrity metadata.
    pub fn has_sha256(&self) -> bool {
        self.sha256.is_some()
    }

    /// Whether this builder includes an expected download size.
    pub fn has_size(&self) -> bool {
        self.size_bytes.is_some()
    }

    /// Whether this builder will allow creating missing parent directories.
    pub fn creates_parent_dirs(&self) -> bool {
        self.create_parent_dirs
    }

    /// Whether a network policy will be checked before the download starts.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// Validate the planned request without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        self.as_request().validate()
    }

    /// Returns a compact, credential-safe summary for logs and agent traces.
    pub fn to_text(&self) -> String {
        self.as_request().to_text()
    }

    /// Returns a host/path/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        self.as_request().to_safe_text()
    }

    /// Validate and build the request.
    pub fn build_checked(self) -> Result<DownloadRequest> {
        let request = self.as_request();
        request.validate()?;
        Ok(request)
    }

    fn as_request(&self) -> DownloadRequest {
        DownloadRequest {
            url: self.url.clone(),
            destination: self.destination.clone(),
            sha256: self.sha256.clone(),
            size_bytes: self.size_bytes,
            create_parent_dirs: self.create_parent_dirs,
            network_policy: self.network_policy.clone(),
        }
    }
}

/// Next app-builder action for a checked download destination plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadDestinationNextAction {
    /// Ask the user/app for a concrete destination path before queueing.
    PromptForDestination,
    /// Review overwrite behavior before queueing this destination.
    ReviewOverwritePolicy,
    /// Build a native `DownloadRequest` and queue it through the download handoff.
    BuildRequest,
}

impl DownloadDestinationNextAction {
    /// Stable label for logs, UI state, and generated agents.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::PromptForDestination => "prompt-for-destination",
            Self::ReviewOverwritePolicy => "review-overwrite-policy",
            Self::BuildRequest => "build-request",
        }
    }
}

/// Checked destination-selection plan for app-owned downloads.
#[derive(Debug, Clone)]
pub struct DownloadDestinationPlan {
    url: String,
    suggested_file_name: Option<String>,
    destination: Option<PathBuf>,
    sha256: Option<String>,
    size_bytes: Option<u64>,
    network_policy: Option<NetworkPolicy>,
    create_parent_dirs: bool,
    existing_file_policy: DownloadExistingFilePolicy,
    next_action: DownloadDestinationNextAction,
}

impl DownloadDestinationPlan {
    /// Source URL for the planned app-owned download.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Suggested filename for Save As UI, when provided.
    pub fn suggested_file_name(&self) -> Option<&str> {
        self.suggested_file_name.as_deref()
    }

    /// Concrete destination path when already selected.
    pub fn destination(&self) -> Option<&Path> {
        self.destination.as_deref()
    }

    /// Recommended next action.
    pub fn next_action(&self) -> DownloadDestinationNextAction {
        self.next_action
    }

    /// Whether the plan already has a concrete destination path.
    pub fn has_destination(&self) -> bool {
        self.destination.is_some()
    }

    /// Whether a Save As prompt is still required.
    pub fn needs_destination_prompt(&self) -> bool {
        self.next_action == DownloadDestinationNextAction::PromptForDestination
    }

    /// Whether overwrite behavior should be reviewed before queueing.
    pub fn needs_overwrite_review(&self) -> bool {
        self.next_action == DownloadDestinationNextAction::ReviewOverwritePolicy
    }

    /// Whether the destination can be converted into a native download request.
    pub fn can_build_request(&self) -> bool {
        self.next_action == DownloadDestinationNextAction::BuildRequest
    }

    /// Whether the eventual request may create missing parent directories.
    pub fn creates_parent_dirs(&self) -> bool {
        self.create_parent_dirs
    }

    /// Existing-file policy selected for this destination.
    pub fn existing_file_policy(&self) -> DownloadExistingFilePolicy {
        self.existing_file_policy
    }

    /// Whether integrity metadata is complete enough for strict handoff queues.
    pub fn has_integrity_metadata(&self) -> bool {
        self.sha256.is_some() && self.size_bytes.is_some()
    }

    /// Whether a network policy will be attached to the request.
    pub fn has_network_policy(&self) -> bool {
        self.network_policy.is_some()
    }

    /// Build a native download request builder when a concrete destination exists.
    pub fn request_builder(&self) -> Result<DownloadRequestBuilder> {
        let destination = self.destination.clone().ok_or_else(|| {
            anyhow!("download destination plan requires a destination before building request")
        })?;
        let mut request = DownloadRequest::builder(self.url.clone(), destination);
        if let Some(sha256) = &self.sha256 {
            request = request.sha256(sha256.clone());
        }
        if let Some(size_bytes) = self.size_bytes {
            request = request.size_bytes(size_bytes);
        }
        if let Some(policy) = &self.network_policy {
            request = request.network_policy(policy.clone());
        }
        if self.create_parent_dirs {
            request = request.create_parent_dirs();
        }
        Ok(request)
    }

    /// Validate and build a native download request.
    pub fn build_request_checked(&self) -> Result<DownloadRequest> {
        self.request_builder()?.build_checked()
    }

    /// Host/path/size-safe summary for builder and agent traces.
    pub fn to_text(&self) -> String {
        format!(
            "download destination plan: destination {}, suggested name {}, sha256 {}, size {}, network policy {}, create parent dirs {}, existing policy {}, next action {}",
            self.has_destination(),
            self.suggested_file_name.is_some(),
            self.sha256.is_some(),
            self.size_bytes.is_some(),
            self.has_network_policy(),
            self.create_parent_dirs,
            self.existing_file_policy.to_text(),
            self.next_action().to_text()
        )
    }
}

/// Builder for Save As / destination-selection download flows.
#[derive(Debug, Clone)]
pub struct DownloadDestinationPlanBuilder {
    url: String,
    suggested_file_name: Option<String>,
    download_dir: Option<PathBuf>,
    destination: Option<PathBuf>,
    sha256: Option<String>,
    size_bytes: Option<u64>,
    network_policy: Option<NetworkPolicy>,
    create_parent_dirs: bool,
    existing_file_policy: DownloadExistingFilePolicy,
}

impl DownloadDestinationPlanBuilder {
    /// Create a destination plan for a download URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            suggested_file_name: None,
            download_dir: None,
            destination: None,
            sha256: None,
            size_bytes: None,
            network_policy: None,
            create_parent_dirs: false,
            existing_file_policy: DownloadExistingFilePolicy::FailIfExists,
        }
    }

    /// Set a suggested filename for Save As UI or download-directory joins.
    pub fn suggested_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.suggested_file_name = Some(file_name.into());
        self
    }

    /// Set a download directory. Requires a suggested filename to build a request.
    pub fn download_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.download_dir = Some(dir.into());
        self.destination = None;
        self
    }

    /// Set an explicit destination path selected by the app or user.
    pub fn destination(mut self, destination: impl Into<PathBuf>) -> Self {
        self.destination = Some(destination.into());
        self
    }

    /// Attach expected SHA-256 metadata.
    pub fn sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = Some(sha256.into());
        self
    }

    /// Attach expected byte size metadata.
    pub fn size_bytes(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }

    /// Attach outbound network policy.
    pub fn network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Allow the worker to create destination parent directories.
    pub fn create_parent_dirs(mut self) -> Self {
        self.create_parent_dirs = true;
        self
    }

    /// Require destination parent directories to already exist.
    pub fn require_existing_parent(mut self) -> Self {
        self.create_parent_dirs = false;
        self
    }

    /// Fail if the destination already exists.
    pub fn fail_if_exists(mut self) -> Self {
        self.existing_file_policy = DownloadExistingFilePolicy::FailIfExists;
        self
    }

    /// Allow replacing an existing destination after explicit review.
    pub fn overwrite_existing(mut self) -> Self {
        self.existing_file_policy = DownloadExistingFilePolicy::Overwrite;
        self
    }

    /// Whether a concrete destination was supplied.
    pub fn has_destination(&self) -> bool {
        self.destination.is_some()
            || (self.download_dir.is_some() && self.suggested_file_name.is_some())
    }

    /// Validate URL, filename, destination shape, and optional metadata.
    pub fn validate(&self) -> Result<()> {
        validate_update_url(&self.url, "download URL")?;
        if let Some(file_name) = &self.suggested_file_name {
            validate_download_file_name(file_name)?;
        }
        if let Some(dir) = &self.download_dir {
            validate_download_directory(dir)?;
        }
        if let Some(destination) = &self.destination {
            validate_download_destination(destination)?;
        }
        validate_optional_sha256(self.sha256.as_deref())?;
        validate_optional_size(self.size_bytes)?;
        if let Some(policy) = &self.network_policy {
            policy.validate()?;
            anyhow::ensure!(
                policy.check_url(&self.url)?,
                "download URL is denied by network policy"
            );
        }
        Ok(())
    }

    /// Build the checked destination plan.
    pub fn build_checked(self) -> Result<DownloadDestinationPlan> {
        self.validate()?;
        let destination = self.resolve_destination()?;
        if let Some(destination) = &destination {
            let mut request = DownloadRequest::builder(self.url.clone(), destination.clone());
            if let Some(sha256) = &self.sha256 {
                request = request.sha256(sha256.clone());
            }
            if let Some(size_bytes) = self.size_bytes {
                request = request.size_bytes(size_bytes);
            }
            if let Some(policy) = &self.network_policy {
                request = request.network_policy(policy.clone());
            }
            if self.create_parent_dirs {
                request = request.create_parent_dirs();
            }
            request.validate()?;
        }

        let next_action = if destination.is_none() {
            DownloadDestinationNextAction::PromptForDestination
        } else if destination.as_ref().is_some_and(|path| path.exists()) {
            DownloadDestinationNextAction::ReviewOverwritePolicy
        } else {
            DownloadDestinationNextAction::BuildRequest
        };

        Ok(DownloadDestinationPlan {
            url: self.url,
            suggested_file_name: self.suggested_file_name,
            destination,
            sha256: self.sha256,
            size_bytes: self.size_bytes,
            network_policy: self.network_policy,
            create_parent_dirs: self.create_parent_dirs,
            existing_file_policy: self.existing_file_policy,
            next_action,
        })
    }

    /// Host/path/size-safe summary before destination selection completes.
    pub fn to_text(&self) -> String {
        format!(
            "download destination plan builder: destination {}, download dir {}, suggested name {}, sha256 {}, size {}, network policy {}, create parent dirs {}, existing policy {}",
            self.destination.is_some(),
            self.download_dir.is_some(),
            self.suggested_file_name.is_some(),
            self.sha256.is_some(),
            self.size_bytes.is_some(),
            self.network_policy.is_some(),
            self.create_parent_dirs,
            self.existing_file_policy.to_text()
        )
    }

    fn resolve_destination(&self) -> Result<Option<PathBuf>> {
        if let Some(destination) = &self.destination {
            return Ok(Some(destination.clone()));
        }
        match (&self.download_dir, &self.suggested_file_name) {
            (Some(dir), Some(file_name)) => Ok(Some(dir.join(file_name))),
            _ => Ok(None),
        }
    }
}

/// A checked group of app-owned downloads that can be queued together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadBatch {
    requests: Vec<DownloadRequest>,
}

impl DownloadBatch {
    /// Create a download batch builder.
    pub fn builder() -> DownloadBatchBuilder {
        DownloadBatchBuilder::new()
    }

    /// Checked requests in queue order.
    pub fn requests(&self) -> &[DownloadRequest] {
        &self.requests
    }

    /// Consume the batch and return its requests.
    pub fn into_requests(self) -> Vec<DownloadRequest> {
        self.requests
    }

    /// Number of downloads in the batch.
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Number of downloads with integrity metadata.
    pub fn sha256_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|request| request.has_sha256())
            .count()
    }

    /// Number of downloads with expected sizes.
    pub fn size_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|request| request.has_size())
            .count()
    }

    /// Number of downloads that may create parent directories.
    pub fn create_parent_dirs_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|request| request.create_parent_dirs)
            .count()
    }

    /// Number of downloads checked against an outbound network policy.
    pub fn network_policy_count(&self) -> usize {
        self.requests
            .iter()
            .filter(|request| request.has_network_policy())
            .count()
    }

    /// Whether the batch contains no downloads.
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Validate every request in the batch.
    pub fn validate(&self) -> Result<()> {
        validate_download_batch(&self.requests)
    }

    /// Content-safe summary for download queues.
    pub fn to_text(&self) -> String {
        download_batch_summary("download batch", &self.requests)
    }

    /// Host/path/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        format!(
            "download batch: requests {}, sha256 {}, sizes {}, create parent dirs {}, network policies {}",
            self.request_count(),
            self.sha256_count(),
            self.size_count(),
            self.create_parent_dirs_count(),
            self.network_policy_count()
        )
    }
}

/// Builder for checked app-owned download batches.
#[derive(Debug, Clone, Default)]
pub struct DownloadBatchBuilder {
    requests: Vec<DownloadRequest>,
}

impl DownloadBatchBuilder {
    /// Create an empty batch builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a prebuilt checked request.
    pub fn request(mut self, request: DownloadRequest) -> Self {
        self.requests.push(request);
        self
    }

    /// Add a request builder after checking it.
    pub fn request_builder(mut self, request: DownloadRequestBuilder) -> Result<Self> {
        self.requests.push(request.build_checked()?);
        Ok(self)
    }

    /// Add multiple prebuilt checked requests.
    pub fn requests(mut self, requests: impl IntoIterator<Item = DownloadRequest>) -> Self {
        self.requests.extend(requests);
        self
    }

    /// Add a URL/destination pair with default request options.
    pub fn url(mut self, url: impl Into<String>, destination: impl Into<PathBuf>) -> Result<Self> {
        self.requests
            .push(DownloadRequest::builder(url, destination).build_checked()?);
        Ok(self)
    }

    /// Number of configured downloads.
    pub fn request_count(&self) -> usize {
        self.requests.len()
    }

    /// Whether this builder has no configured downloads.
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Validate the batch without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        validate_download_batch(&self.requests)
    }

    /// Content-safe summary before build.
    pub fn to_text(&self) -> String {
        download_batch_summary("download batch builder", &self.requests)
    }

    /// Host/path/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        DownloadBatch {
            requests: self.requests.clone(),
        }
        .to_safe_text()
    }

    /// Validate and build the batch.
    pub fn build_checked(self) -> Result<DownloadBatch> {
        validate_download_batch(&self.requests)?;
        Ok(DownloadBatch {
            requests: self.requests,
        })
    }
}

/// Policy for destinations that already exist before an app-owned download starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DownloadExistingFilePolicy {
    /// Fail before starting so generated downloads do not overwrite user files by accident.
    #[default]
    FailIfExists,
    /// Allow the worker to replace an existing destination after validation.
    Overwrite,
}

impl DownloadExistingFilePolicy {
    /// Stable summary text for logs and agent traces.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::FailIfExists => "fail if exists",
            Self::Overwrite => "overwrite",
        }
    }

    /// Whether the policy allows replacing an existing destination.
    pub fn overwrites_existing(self) -> bool {
        matches!(self, Self::Overwrite)
    }
}

/// Checked execution policy for a native app-owned download queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadExecutionPlan {
    batch: DownloadBatch,
    max_parallel: usize,
    retry_attempts: u8,
    temporary_file_extension: Option<String>,
    existing_file_policy: DownloadExistingFilePolicy,
}

impl DownloadExecutionPlan {
    /// Create a builder for a checked native download execution plan.
    pub fn builder(batch: DownloadBatch) -> DownloadExecutionPlanBuilder {
        DownloadExecutionPlanBuilder::new(batch)
    }

    /// Create a builder for a single checked request.
    pub fn from_request(request: DownloadRequest) -> DownloadExecutionPlanBuilder {
        DownloadExecutionPlanBuilder::from_request(request)
    }

    /// Checked queue of downloads.
    pub fn batch(&self) -> &DownloadBatch {
        &self.batch
    }

    /// Number of downloads in the queue.
    pub fn request_count(&self) -> usize {
        self.batch.request_count()
    }

    /// Maximum number of downloads a worker should run at once.
    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    /// Number of retry attempts a worker may make after the first failed attempt.
    pub fn retry_attempts(&self) -> u8 {
        self.retry_attempts
    }

    /// Whether workers should write to a temporary filename before finalizing.
    pub fn uses_temporary_files(&self) -> bool {
        self.temporary_file_extension.is_some()
    }

    /// Temporary filename extension, without a leading dot, when configured.
    pub fn temporary_file_extension(&self) -> Option<&str> {
        self.temporary_file_extension.as_deref()
    }

    /// Existing-file policy for destination paths.
    pub fn existing_file_policy(&self) -> DownloadExistingFilePolicy {
        self.existing_file_policy
    }

    /// Whether the plan allows replacing existing destinations.
    pub fn overwrites_existing(&self) -> bool {
        self.existing_file_policy.overwrites_existing()
    }

    /// Validate the queue and execution policy.
    pub fn validate(&self) -> Result<()> {
        validate_download_execution_plan(
            &self.batch,
            self.max_parallel,
            self.retry_attempts,
            self.temporary_file_extension.as_deref(),
            self.existing_file_policy,
        )
    }

    /// Content-safe summary for native download queues.
    pub fn to_text(&self) -> String {
        format!(
            "download execution plan: requests {}, max parallel {}, retries {}, temp files {}, existing policy {}, sha256 {}, sizes {}, network policies {}",
            self.request_count(),
            self.max_parallel,
            self.retry_attempts,
            self.uses_temporary_files(),
            self.existing_file_policy.to_text(),
            self.batch.sha256_count(),
            self.batch.size_count(),
            self.batch.network_policy_count()
        )
    }

    /// Host/path/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        self.to_text()
    }

    /// Wrap this checked execution plan in a builder/agent handoff.
    pub fn handoff(&self) -> DownloadHandoff {
        DownloadHandoff::from_execution_plan(self.clone())
    }
}

/// Recommended next action for an app-owned download handoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadHandoffNextAction {
    /// Review or replace overwrite policy before queueing downloads.
    ReviewOverwritePolicy,
    /// Add outbound host policy before worker/plugin/agent execution.
    AddNetworkPolicy,
    /// Add expected hash or size metadata before claiming verified downloads.
    AddIntegrityMetadata,
    /// Queue the checked native download execution plan.
    QueueDownloads,
}

impl DownloadHandoffNextAction {
    /// Stable action label for logs, setup UI, and agents.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::ReviewOverwritePolicy => "review-overwrite-policy",
            Self::AddNetworkPolicy => "add-network-policy",
            Self::AddIntegrityMetadata => "add-integrity-metadata",
            Self::QueueDownloads => "queue-downloads",
        }
    }
}

/// One-object handoff for native app-owned downloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadHandoff {
    execution_plan: DownloadExecutionPlan,
    next_action: DownloadHandoffNextAction,
}

impl DownloadHandoff {
    /// Build a handoff from an already checked execution plan.
    pub fn from_execution_plan(execution_plan: DownloadExecutionPlan) -> Self {
        let next_action = download_handoff_next_action(&execution_plan);
        Self {
            execution_plan,
            next_action,
        }
    }

    /// Checked execution plan for the native download worker.
    pub fn execution_plan(&self) -> &DownloadExecutionPlan {
        &self.execution_plan
    }

    /// Recommended first action before queueing downloads.
    pub fn next_action(&self) -> DownloadHandoffNextAction {
        self.next_action
    }

    /// Number of downloads in the handoff.
    pub fn request_count(&self) -> usize {
        self.execution_plan.request_count()
    }

    /// Whether every download has a network policy.
    pub fn has_complete_network_policy(&self) -> bool {
        self.execution_plan.batch.network_policy_count() == self.request_count()
    }

    /// Whether every download has expected SHA-256 and size metadata.
    pub fn has_complete_integrity_metadata(&self) -> bool {
        self.execution_plan.batch.sha256_count() == self.request_count()
            && self.execution_plan.batch.size_count() == self.request_count()
    }

    /// Whether overwrite policy should be reviewed before execution.
    pub fn needs_overwrite_review(&self) -> bool {
        self.execution_plan.overwrites_existing()
    }

    /// Whether the handoff is ready to queue with full policy and integrity metadata.
    pub fn is_queue_ready(&self) -> bool {
        self.next_action == DownloadHandoffNextAction::QueueDownloads
    }

    /// Host/path/size-safe summary for generated download handoffs.
    pub fn to_text(&self) -> String {
        format!(
            "download handoff: requests {}, max parallel {}, retries {}, temp files {}, overwrite {}, network policies {}/{}, integrity {}/{}, next action {}",
            self.request_count(),
            self.execution_plan.max_parallel(),
            self.execution_plan.retry_attempts(),
            self.execution_plan.uses_temporary_files(),
            self.execution_plan.overwrites_existing(),
            self.execution_plan.batch.network_policy_count(),
            self.request_count(),
            self.execution_plan
                .batch
                .sha256_count()
                .min(self.execution_plan.batch.size_count()),
            self.request_count(),
            self.next_action().to_text()
        )
    }
}

/// Builder for native app-owned download handoffs.
#[derive(Debug, Clone, Default)]
pub struct DownloadHandoffBuilder {
    batch: DownloadBatchBuilder,
    max_parallel: Option<usize>,
    retry_attempts: Option<u8>,
    temporary_file_extension: Option<Option<String>>,
    existing_file_policy: Option<DownloadExistingFilePolicy>,
}

impl DownloadHandoffBuilder {
    /// Create an empty download handoff builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a prebuilt checked request.
    pub fn request(mut self, request: DownloadRequest) -> Self {
        self.batch = self.batch.request(request);
        self
    }

    /// Add a request builder after checking it.
    pub fn request_builder(mut self, request: DownloadRequestBuilder) -> Result<Self> {
        self.batch = self.batch.request_builder(request)?;
        Ok(self)
    }

    /// Add a URL/destination pair with default request options.
    pub fn url(mut self, url: impl Into<String>, destination: impl Into<PathBuf>) -> Result<Self> {
        self.batch = self.batch.url(url, destination)?;
        Ok(self)
    }

    /// Run downloads one at a time.
    pub fn serial(mut self) -> Self {
        self.max_parallel = Some(1);
        self
    }

    /// Set maximum parallel downloads.
    pub fn max_parallel(mut self, max_parallel: usize) -> Self {
        self.max_parallel = Some(max_parallel);
        self
    }

    /// Set retry attempts.
    pub fn retry_attempts(mut self, retry_attempts: u8) -> Self {
        self.retry_attempts = Some(retry_attempts);
        self
    }

    /// Disable retries.
    pub fn no_retries(mut self) -> Self {
        self.retry_attempts = Some(0);
        self
    }

    /// Write to `destination.<extension>` before finalizing.
    pub fn temporary_file_extension(mut self, extension: impl Into<String>) -> Self {
        self.temporary_file_extension = Some(Some(extension.into()));
        self
    }

    /// Write directly to the final destination path.
    pub fn without_temporary_files(mut self) -> Self {
        self.temporary_file_extension = Some(None);
        self
    }

    /// Fail if destinations already exist.
    pub fn fail_if_exists(mut self) -> Self {
        self.existing_file_policy = Some(DownloadExistingFilePolicy::FailIfExists);
        self
    }

    /// Allow replacing existing destinations.
    pub fn overwrite_existing(mut self) -> Self {
        self.existing_file_policy = Some(DownloadExistingFilePolicy::Overwrite);
        self
    }

    /// Number of configured downloads.
    pub fn request_count(&self) -> usize {
        self.batch.request_count()
    }

    /// Validate without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        self.as_execution_plan_builder()?.validate()
    }

    /// Build the checked handoff.
    pub fn build_checked(self) -> Result<DownloadHandoff> {
        let plan = self.as_execution_plan_builder()?.build_checked()?;
        Ok(plan.handoff())
    }

    /// Host/path/size-safe summary for generated download handoffs.
    pub fn to_text(&self) -> String {
        match self.as_execution_plan_builder() {
            Ok(builder) => DownloadHandoff::from_execution_plan(builder.as_plan()).to_text(),
            Err(_) => format!(
                "download handoff builder: requests {}, invalid true",
                self.request_count()
            ),
        }
    }

    fn as_execution_plan_builder(&self) -> Result<DownloadExecutionPlanBuilder> {
        let batch = self.batch.clone().build_checked()?;
        let mut builder = DownloadExecutionPlan::builder(batch);
        if let Some(max_parallel) = self.max_parallel {
            builder = builder.max_parallel(max_parallel);
        }
        if let Some(retry_attempts) = self.retry_attempts {
            builder = builder.retry_attempts(retry_attempts);
        }
        if let Some(extension) = &self.temporary_file_extension {
            builder = match extension {
                Some(extension) => builder.temporary_file_extension(extension.clone()),
                None => builder.without_temporary_files(),
            };
        }
        if let Some(policy) = self.existing_file_policy {
            builder = match policy {
                DownloadExistingFilePolicy::FailIfExists => builder.fail_if_exists(),
                DownloadExistingFilePolicy::Overwrite => builder.overwrite_existing(),
            };
        }
        Ok(builder)
    }
}

fn download_handoff_next_action(plan: &DownloadExecutionPlan) -> DownloadHandoffNextAction {
    if plan.overwrites_existing() {
        DownloadHandoffNextAction::ReviewOverwritePolicy
    } else if plan.batch.network_policy_count() < plan.request_count() {
        DownloadHandoffNextAction::AddNetworkPolicy
    } else if plan.batch.sha256_count() < plan.request_count()
        || plan.batch.size_count() < plan.request_count()
    {
        DownloadHandoffNextAction::AddIntegrityMetadata
    } else {
        DownloadHandoffNextAction::QueueDownloads
    }
}

/// Builder for native app-owned download queue execution policy.
#[derive(Debug, Clone)]
pub struct DownloadExecutionPlanBuilder {
    batch: DownloadBatch,
    max_parallel: usize,
    retry_attempts: u8,
    temporary_file_extension: Option<String>,
    existing_file_policy: DownloadExistingFilePolicy,
}

impl DownloadExecutionPlanBuilder {
    /// Create a plan builder for an existing checked batch.
    pub fn new(batch: DownloadBatch) -> Self {
        Self {
            batch,
            max_parallel: 2,
            retry_attempts: 2,
            temporary_file_extension: Some("download".to_string()),
            existing_file_policy: DownloadExistingFilePolicy::FailIfExists,
        }
    }

    /// Create a plan builder for one checked request.
    pub fn from_request(request: DownloadRequest) -> Self {
        Self::new(DownloadBatch {
            requests: vec![request],
        })
    }

    /// Run downloads one at a time.
    pub fn serial(mut self) -> Self {
        self.max_parallel = 1;
        self
    }

    /// Set the maximum number of downloads a worker should run at once.
    pub fn max_parallel(mut self, max_parallel: usize) -> Self {
        self.max_parallel = max_parallel;
        self
    }

    /// Set retry attempts after the initial attempt.
    pub fn retry_attempts(mut self, retry_attempts: u8) -> Self {
        self.retry_attempts = retry_attempts;
        self
    }

    /// Disable retries.
    pub fn no_retries(mut self) -> Self {
        self.retry_attempts = 0;
        self
    }

    /// Write to `destination.<extension>` before finalizing the destination.
    pub fn temporary_file_extension(mut self, extension: impl Into<String>) -> Self {
        self.temporary_file_extension = Some(extension.into());
        self
    }

    /// Write directly to the final destination path.
    pub fn without_temporary_files(mut self) -> Self {
        self.temporary_file_extension = None;
        self
    }

    /// Fail if any destination already exists before the worker starts.
    pub fn fail_if_exists(mut self) -> Self {
        self.existing_file_policy = DownloadExistingFilePolicy::FailIfExists;
        self
    }

    /// Allow the worker to replace existing destinations.
    pub fn overwrite_existing(mut self) -> Self {
        self.existing_file_policy = DownloadExistingFilePolicy::Overwrite;
        self
    }

    /// Number of downloads in the queue.
    pub fn request_count(&self) -> usize {
        self.batch.request_count()
    }

    /// Maximum number of downloads a worker should run at once.
    pub fn max_parallel_count(&self) -> usize {
        self.max_parallel
    }

    /// Number of retry attempts configured.
    pub fn retry_attempt_count(&self) -> u8 {
        self.retry_attempts
    }

    /// Whether workers should write to temporary filenames before finalizing.
    pub fn uses_temporary_files(&self) -> bool {
        self.temporary_file_extension.is_some()
    }

    /// Existing-file policy for destination paths.
    pub fn existing_file_policy(&self) -> DownloadExistingFilePolicy {
        self.existing_file_policy
    }

    /// Validate the queue and execution policy without consuming the builder.
    pub fn validate(&self) -> Result<()> {
        validate_download_execution_plan(
            &self.batch,
            self.max_parallel,
            self.retry_attempts,
            self.temporary_file_extension.as_deref(),
            self.existing_file_policy,
        )
    }

    /// Content-safe summary before build.
    pub fn to_text(&self) -> String {
        self.as_plan().to_text()
    }

    /// Host/path/size-safe summary for privacy-sensitive agent traces.
    pub fn to_safe_text(&self) -> String {
        self.as_plan().to_safe_text()
    }

    /// Validate and build the execution plan.
    pub fn build_checked(self) -> Result<DownloadExecutionPlan> {
        let plan = self.as_plan();
        plan.validate()?;
        Ok(plan)
    }

    fn as_plan(&self) -> DownloadExecutionPlan {
        DownloadExecutionPlan {
            batch: self.batch.clone(),
            max_parallel: self.max_parallel,
            retry_attempts: self.retry_attempts,
            temporary_file_extension: self.temporary_file_extension.clone(),
            existing_file_policy: self.existing_file_policy,
        }
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
        let result: Result<Option<UpdateInfo>> = async {
            self.config.validate()?;
            let mut response = self
                .http_client
                .get(&self.config.feed_url, Default::default(), false)
                .await
                .context("failed to fetch update feed")?;

            let status = response.status();
            anyhow::ensure!(
                status.is_success(),
                "update feed returned HTTP {}",
                status.as_u16()
            );

            let mut body = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                let read = response
                    .body_mut()
                    .read(&mut chunk)
                    .await
                    .context("failed to read update feed body")?;
                if read == 0 {
                    break;
                }
                anyhow::ensure!(
                    body.len().saturating_add(read) <= MAX_UPDATE_FEED_BYTES,
                    "update feed exceeds {MAX_UPDATE_FEED_BYTES} byte limit"
                );
                body.extend_from_slice(&chunk[..read]);
            }

            let body = std::str::from_utf8(&body).context("update feed is not valid UTF-8")?;
            let updates = parse_update_feed(body)?;
            for update in &updates {
                let validation = if self.require_signature {
                    update.validate_signed_metadata()
                } else {
                    update.validate()
                };
                validation.context("update feed contains invalid metadata")?;
            }

            Ok(updates
                .into_iter()
                // SemanticVersion currently does not preserve pre-release metadata,
                // so feed filtering is limited to version ordering for now.
                .filter(|update| update.version > self.current_version)
                .max_by_key(|update| update.version))
        }
        .await;

        match result {
            Ok(Some(update)) => {
                self.status = UpdateStatus::UpdateAvailable(update.version);
                self.latest_update = Some(update.clone());
                Ok(Some(update))
            }
            Ok(None) => {
                self.status = UpdateStatus::Idle;
                self.latest_update = None;
                Ok(None)
            }
            Err(error) => {
                self.status = UpdateStatus::Error(error.to_string());
                self.latest_update = None;
                Err(error)
            }
        }
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

        let metadata_result = if self.require_signature {
            update.validate_signed_metadata()
        } else {
            update.validate()
        };
        if let Err(error) = metadata_result {
            self.status = UpdateStatus::Error(error.to_string());
            return Err(error).context("update metadata is invalid");
        }

        self.status = UpdateStatus::Downloading;
        self.downloaded_path = None;

        let mut response = match self
            .http_client
            .get(&update.download_url, Default::default(), false)
            .await
            .context("failed to start update download")
        {
            Ok(response) => response,
            Err(error) => {
                self.status = UpdateStatus::Error(error.to_string());
                return Err(error);
            }
        };

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

        if let Some(total) = total_bytes {
            if total > MAX_UPDATE_PACKAGE_BYTES {
                let error = anyhow!("update package exceeds {MAX_UPDATE_PACKAGE_BYTES} byte limit");
                self.status = UpdateStatus::Error(error.to_string());
                return Err(error);
            }
            if let Some(expected) = update.size_bytes {
                if total != expected {
                    let error = anyhow!("update content length does not match signed size");
                    self.status = UpdateStatus::Error(error.to_string());
                    return Err(error);
                }
            }
        }

        let staging_dir =
            std::env::temp_dir().join(format!("kael_update_{}", uuid::Uuid::new_v4()));
        if let Err(error) =
            std::fs::create_dir(&staging_dir).context("failed to create update staging directory")
        {
            self.status = UpdateStatus::Error(error.to_string());
            return Err(error);
        }
        restrict_dir_permissions(&staging_dir);

        let download_path = staging_dir.join(sanitize_package_filename(&update.download_url));
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&download_path)
            .context("failed to create staged update package")
        {
            Ok(file) => file,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                self.status = UpdateStatus::Error(error.to_string());
                return Err(error);
            }
        };

        let download_result: Result<(u64, String)> = async {
            let body = response.body_mut();
            let mut chunk = [0u8; 64 * 1024];
            let mut downloaded = 0u64;
            let mut hasher = Sha256::new();
            loop {
                let read = body
                    .read(&mut chunk)
                    .await
                    .context("failed to read update package")?;
                if read == 0 {
                    break;
                }
                downloaded = downloaded
                    .checked_add(read as u64)
                    .ok_or_else(|| anyhow!("update download size overflow"))?;
                anyhow::ensure!(
                    downloaded <= MAX_UPDATE_PACKAGE_BYTES,
                    "update package exceeds {MAX_UPDATE_PACKAGE_BYTES} byte limit"
                );
                if let Some(expected) = update.size_bytes {
                    anyhow::ensure!(downloaded <= expected, "update exceeds signed package size");
                }
                file.write_all(&chunk[..read])
                    .context("failed to write staged update package")?;
                hasher.update(&chunk[..read]);
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    on_progress(DownloadProgress {
                        bytes_downloaded: downloaded,
                        total_bytes,
                    });
                }))
                .map_err(|_| anyhow!("update progress callback panicked"))?;
            }
            file.sync_all()
                .context("failed to sync staged update package")?;
            Ok((downloaded, hex::encode(hasher.finalize())))
        }
        .await;

        let (downloaded_size, actual_sha256) = match download_result {
            Ok(result) => result,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&staging_dir);
                self.status = UpdateStatus::Error(error.to_string());
                return Err(error);
            }
        };

        if let Err(err) = self.verify_package_digest(&update, downloaded_size, &actual_sha256) {
            let _ = std::fs::remove_dir_all(&staging_dir);
            self.downloaded_path = None;
            self.status = UpdateStatus::Error(err.to_string());
            return Err(err).context("update package failed verification; refusing to install");
        }

        self.downloaded_path = Some(download_path.clone());
        self.status = UpdateStatus::ReadyToInstall;

        Ok(download_path)
    }

    #[cfg(test)]
    fn verify_package(&self, update: &UpdateInfo, bytes: &[u8]) -> Result<()> {
        self.verify_package_digest(update, bytes.len() as u64, &sha256_hex(bytes))
    }

    fn verify_package_digest(
        &self,
        update: &UpdateInfo,
        downloaded_size: u64,
        actual_sha256: &str,
    ) -> Result<()> {
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
                    if downloaded_size != expected_size {
                        bail!(
                            "update size mismatch: expected {expected_size} bytes, downloaded {}",
                            downloaded_size
                        );
                    }
                }
                if actual_sha256.len() != expected.len()
                    || !actual_sha256.eq_ignore_ascii_case(expected)
                {
                    bail!("update hash mismatch: expected {expected}, downloaded {actual_sha256}");
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

        let update = self
            .latest_update
            .as_ref()
            .ok_or_else(|| anyhow!("downloaded update metadata is unavailable"))?;
        self.verify_downloaded_file(update, path)?;

        installer.install_and_restart(path)
    }

    fn verify_downloaded_file(&self, update: &UpdateInfo, path: &Path) -> Result<()> {
        let metadata = std::fs::symlink_metadata(path)
            .with_context(|| format!("failed to inspect downloaded update: {}", path.display()))?;
        anyhow::ensure!(
            metadata.file_type().is_file(),
            "downloaded update must be a regular file"
        );
        anyhow::ensure!(
            metadata.len() <= MAX_UPDATE_PACKAGE_BYTES,
            "downloaded update exceeds package limit"
        );
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(path)
            .with_context(|| format!("failed to open downloaded update: {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut size = 0u64;
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let read = std::io::Read::read(&mut file, &mut chunk)
                .context("failed to re-read downloaded update")?;
            if read == 0 {
                break;
            }
            size = size
                .checked_add(read as u64)
                .ok_or_else(|| anyhow!("downloaded update size overflow"))?;
            anyhow::ensure!(
                size <= MAX_UPDATE_PACKAGE_BYTES,
                "downloaded update exceeds package limit"
            );
            hasher.update(&chunk[..read]);
        }
        self.verify_package_digest(update, size, &hex::encode(hasher.finalize()))
            .context("downloaded update changed after verification")
    }
}

#[cfg(test)]
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
        url.len() <= MAX_UPDATE_URL_BYTES,
        "{} exceeds {MAX_UPDATE_URL_BYTES} bytes",
        label
    );
    anyhow::ensure!(
        url == url.trim(),
        "{} cannot have leading or trailing whitespace",
        label
    );

    let parsed = http_client::Url::parse(url).with_context(|| format!("{label} is invalid"))?;
    anyhow::ensure!(parsed.scheme() == "https", "{} must use https", label);
    anyhow::ensure!(parsed.host_str().is_some(), "{} must include a host", label);
    if label.starts_with("update") {
        anyhow::ensure!(
            parsed.username().is_empty() && parsed.password().is_none(),
            "{} cannot contain URL credentials",
            label
        );
        anyhow::ensure!(
            parsed.fragment().is_none(),
            "{} cannot contain a fragment",
            label
        );
    }
    Ok(())
}

fn download_url_summary(url: &str) -> String {
    let Ok(parsed) = http_client::Url::parse(url) else {
        return "invalid url".to_string();
    };

    let host = parsed.host_str().unwrap_or("unknown-host");
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();

    format!("{}://{}{}", parsed.scheme(), host, port)
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
            size_bytes > 0 && size_bytes <= MAX_UPDATE_PACKAGE_BYTES,
            "update package size must be between 1 and {MAX_UPDATE_PACKAGE_BYTES} bytes"
        );
    }
    Ok(())
}

fn validate_download_file_name(file_name: &str) -> Result<()> {
    anyhow::ensure!(
        !file_name.trim().is_empty(),
        "download suggested filename cannot be empty"
    );
    anyhow::ensure!(
        file_name == file_name.trim(),
        "download suggested filename cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !file_name.chars().any(|ch| ch == '\0' || ch.is_control()),
        "download suggested filename cannot contain control characters"
    );
    anyhow::ensure!(
        file_name != "." && file_name != "..",
        "download suggested filename cannot be a dot path"
    );
    let path = Path::new(file_name);
    let mut components = path.components();
    anyhow::ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "download suggested filename cannot contain path separators"
    );
    Ok(())
}

fn validate_download_directory(directory: &Path) -> Result<()> {
    let directory_text = directory.to_string_lossy();
    anyhow::ensure!(
        !directory_text.trim().is_empty(),
        "download directory cannot be empty"
    );
    anyhow::ensure!(
        directory.is_absolute(),
        "download directory must be absolute: {}",
        directory.display()
    );
    anyhow::ensure!(
        !directory_text.chars().any(|ch| ch == '\0'),
        "download directory cannot contain NUL characters"
    );
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

fn validate_download_batch(requests: &[DownloadRequest]) -> Result<()> {
    anyhow::ensure!(
        !requests.is_empty(),
        "download batch must contain at least one request"
    );

    let mut destinations = std::collections::HashSet::new();
    for request in requests {
        request.validate()?;
        anyhow::ensure!(
            destinations.insert(request.destination.clone()),
            "download batch destination is duplicated: {}",
            request.destination.display()
        );
    }

    Ok(())
}

fn validate_download_execution_plan(
    batch: &DownloadBatch,
    max_parallel: usize,
    retry_attempts: u8,
    temporary_file_extension: Option<&str>,
    existing_file_policy: DownloadExistingFilePolicy,
) -> Result<()> {
    batch.validate()?;
    anyhow::ensure!(
        max_parallel > 0,
        "download execution plan must allow at least one parallel download"
    );
    anyhow::ensure!(
        max_parallel <= 16,
        "download execution plan cannot run more than 16 downloads in parallel"
    );
    anyhow::ensure!(
        retry_attempts <= 10,
        "download execution plan cannot retry more than 10 times"
    );
    if let Some(extension) = temporary_file_extension {
        validate_temporary_file_extension(extension)?;
    }
    if !existing_file_policy.overwrites_existing() {
        for request in batch.requests() {
            anyhow::ensure!(
                !request.destination.exists(),
                "download destination already exists: {}",
                request.destination.display()
            );
        }
    }
    Ok(())
}

fn validate_temporary_file_extension(extension: &str) -> Result<()> {
    anyhow::ensure!(
        !extension.trim().is_empty(),
        "download temporary extension cannot be empty"
    );
    anyhow::ensure!(
        extension == extension.trim(),
        "download temporary extension cannot have surrounding whitespace"
    );
    anyhow::ensure!(
        !extension.starts_with('.'),
        "download temporary extension should not start with a dot"
    );
    anyhow::ensure!(
        !extension
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch == '\0'),
        "download temporary extension cannot contain path separators or NUL characters"
    );
    Ok(())
}

fn download_batch_summary(label: &str, requests: &[DownloadRequest]) -> String {
    let sha256_count = requests
        .iter()
        .filter(|request| request.has_sha256())
        .count();
    let size_count = requests.iter().filter(|request| request.has_size()).count();
    let create_parent_dirs_count = requests
        .iter()
        .filter(|request| request.create_parent_dirs)
        .count();
    let network_policy_count = requests
        .iter()
        .filter(|request| request.has_network_policy())
        .count();

    format!(
        "{label}: requests {}, sha256 {}, sizes {}, create parent dirs {}, network policies {}",
        requests.len(),
        sha256_count,
        size_count,
        create_parent_dirs_count,
        network_policy_count
    )
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
                let temp_dir = std::env::temp_dir()
                    .join(format!("kael_update_extract_{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&temp_dir)?;

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
                let mount_point =
                    std::env::temp_dir().join(format!("kael_update_dmg_{}", uuid::Uuid::new_v4()));
                std::fs::create_dir(&mount_point)?;

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
    let mut bundles = Vec::new();
    for entry in std::fs::read_dir(dir).context("failed to read extraction directory")? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() && path.extension().and_then(|e| e.to_str()) == Some("app") {
            bundles.push(path);
        }
    }
    anyhow::ensure!(
        bundles.len() == 1,
        "expected exactly one .app bundle in {}, found {}",
        dir.display(),
        bundles.len()
    );
    Ok(bundles.remove(0))
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

    let token = uuid::Uuid::new_v4();
    let staged = parent.join(format!(".{file_name}.{token}.staged"));

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
        backup: parent.join(format!(".{file_name}.{token}.backup")),
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
                use kael_release::apply::{FsInstaller, SwapPlan, atomic_swap_with_rollback};
                use std::os::unix::fs::PermissionsExt as _;

                let exe = match std::env::var_os("APPIMAGE") {
                    Some(path) => PathBuf::from(path),
                    None => {
                        std::env::current_exe().context("failed to get current executable path")?
                    }
                };
                anyhow::ensure!(exe.is_absolute(), "AppImage path must be absolute");
                anyhow::ensure!(
                    std::fs::symlink_metadata(&exe)?.file_type().is_file(),
                    "AppImage path must be a regular file"
                );
                let parent = exe
                    .parent()
                    .ok_or_else(|| anyhow!("AppImage path has no parent directory"))?;
                let token = uuid::Uuid::new_v4();
                let staged = parent.join(format!(".kael-appimage-{token}.staged"));
                let backup = parent.join(format!(".kael-appimage-{token}.backup"));
                std::fs::copy(package_path, &staged)
                    .context("failed to stage new AppImage on the install volume")?;
                let mut permissions = std::fs::metadata(&staged)?.permissions();
                permissions.set_mode(permissions.mode() | 0o700);
                std::fs::set_permissions(&staged, permissions)?;

                atomic_swap_with_rollback(
                    &FsInstaller,
                    &SwapPlan {
                        live: exe.clone(),
                        staged,
                        backup,
                    },
                )
                .context("failed to atomically replace AppImage")?;

                // Restart
                let _ = std::process::Command::new(&exe)
                    .spawn()
                    .context("failed to restart AppImage")?;

                std::process::exit(0);
            }
            LinuxPackageFormat::Flatpak => {
                let app_id = std::env::var("FLATPAK_ID")
                    .context("FLATPAK_ID is required for Flatpak updates")?;
                validate_package_manager_id(&app_id, "Flatpak app id")?;

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
                    std::env::var("SNAP_NAME").context("SNAP_NAME is required for Snap updates")?;
                validate_package_manager_id(&snap_name, "Snap package name")?;

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

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn validate_package_manager_id(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 255
            && value
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && value
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') }),
        "{label} is invalid"
    );
    Ok(())
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

        let empty = DownloadProgress {
            bytes_downloaded: 0,
            total_bytes: Some(0),
        };
        assert_eq!(empty.fraction(), None);

        let overrun = DownloadProgress {
            bytes_downloaded: 150,
            total_bytes: Some(100),
        };
        assert_eq!(overrun.fraction(), Some(1.0));
    }

    #[test]
    fn download_request_builder_validates_common_downloads() {
        let destination = std::env::temp_dir().join("kael-download-request.bin");
        let builder =
            DownloadRequest::builder("https://example.com/files/report.pdf", &destination)
                .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .size_bytes(1024)
                .network_policy(
                    crate::NetworkPolicyBuilder::new()
                        .allow_host("example.com")
                        .build_checked()
                        .unwrap(),
                );

        assert!(builder.validate().is_ok());
        assert!(builder.has_sha256());
        assert!(builder.has_size());
        assert!(!builder.creates_parent_dirs());
        assert!(builder.has_network_policy());
        assert!(builder.to_text().contains("sha256 present"));
        assert!(!builder.to_text().contains("report.pdf"));
        assert_eq!(
            builder.to_safe_text(),
            "download request: url true, destination true, sha256 true, size true, create parent dirs false, network policy true"
        );

        let request = builder.build_checked().unwrap();

        assert_eq!(request.destination, destination);
        assert_eq!(request.size_bytes, Some(1024));
        assert!(request.has_sha256());
        assert!(request.has_size());
        assert!(request.has_network_policy());
    }

    #[test]
    fn download_request_summary_is_agent_readable_and_credential_safe() {
        let destination = std::env::temp_dir().join("kael-download-request.bin");
        let request = DownloadRequest::builder(
            "https://user:secret@example.com:8443/files/report.pdf?token=sensitive#frag",
            &destination,
        )
        .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .size_bytes(1024)
        .create_parent_dirs()
        .build_checked()
        .unwrap();

        let summary = request.to_text();

        assert!(summary.contains("download request from https://example.com:8443"));
        assert!(summary.contains(&destination.display().to_string()));
        assert!(summary.contains("sha256 present"));
        assert!(summary.contains("size 1024 bytes"));
        assert!(summary.contains("create parent dirs true"));
        assert!(summary.contains("network policy none"));
        assert!(!summary.contains("user"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("report.pdf"));
        assert!(!summary.contains("frag"));

        let safe_summary = request.to_safe_text();

        assert_eq!(
            safe_summary,
            "download request: url true, destination true, sha256 true, size true, create parent dirs true, network policy false"
        );
        assert!(!safe_summary.contains("example.com"));
        assert!(!safe_summary.contains(&destination.display().to_string()));
        assert!(!safe_summary.contains("1024"));
    }

    #[test]
    fn download_request_builder_summary_is_available_before_build() {
        let destination = std::env::temp_dir()
            .join("kael-download-builder-summary")
            .join("model.bin");
        let builder = DownloadRequest::builder(
            "https://token:secret@cdn.example.com/private/model.bin?signature=sensitive",
            &destination,
        )
        .size_bytes(4096)
        .create_parent_dirs();

        let summary = builder.to_text();

        assert!(builder.validate().is_ok());
        assert!(!builder.has_sha256());
        assert!(builder.has_size());
        assert!(builder.creates_parent_dirs());
        assert!(!builder.has_network_policy());
        assert!(summary.contains("download request from https://cdn.example.com"));
        assert!(summary.contains("sha256 none"));
        assert!(summary.contains("size 4096 bytes"));
        assert!(summary.contains("create parent dirs true"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("signature"));
        assert!(!summary.contains("model.bin?"));

        let safe_summary = builder.to_safe_text();

        assert_eq!(
            safe_summary,
            "download request: url true, destination true, sha256 false, size true, create parent dirs true, network policy false"
        );
        assert!(!safe_summary.contains("cdn.example.com"));
        assert!(!safe_summary.contains("model.bin"));
        assert!(!safe_summary.contains("4096"));
    }

    #[test]
    fn download_batch_builder_validates_and_summarizes_queue() {
        let first_destination = std::env::temp_dir()
            .join("kael-download-batch")
            .join("model-a.bin");
        let second_destination = std::env::temp_dir()
            .join("kael-download-batch")
            .join("model-b.bin");
        let batch = DownloadBatch::builder()
            .request_builder(
                DownloadRequest::builder(
                    "https://user:secret@cdn.example.com/private/model-a.bin?token=sensitive",
                    &first_destination,
                )
                .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .size_bytes(4096)
                .create_parent_dirs(),
            )
            .unwrap()
            .request_builder(
                DownloadRequest::builder(
                    "https://assets.example.com/offline/model-b.bin",
                    &second_destination,
                )
                .size_bytes(8192)
                .create_parent_dirs(),
            )
            .unwrap();

        assert_eq!(batch.request_count(), 2);
        assert!(!batch.is_empty());
        assert!(batch.validate().is_ok());

        let summary = batch.to_text();
        assert!(summary.contains("download batch builder"));
        assert!(summary.contains("requests 2"));
        assert!(summary.contains("sha256 1"));
        assert!(summary.contains("sizes 2"));
        assert!(summary.contains("create parent dirs 2"));
        assert!(!summary.contains("cdn.example.com"));
        assert!(!summary.contains("assets.example.com"));
        assert!(!summary.contains("secret"));
        assert!(!summary.contains("token"));
        assert!(!summary.contains("model-a.bin"));
        assert!(!summary.contains("4096"));

        let batch = batch.build_checked().unwrap();
        assert_eq!(batch.request_count(), 2);
        assert_eq!(batch.sha256_count(), 1);
        assert_eq!(batch.size_count(), 2);
        assert_eq!(batch.create_parent_dirs_count(), 2);
        assert_eq!(batch.network_policy_count(), 0);
        assert_eq!(batch.requests().len(), 2);

        let safe_summary = batch.to_safe_text();
        assert_eq!(
            safe_summary,
            "download batch: requests 2, sha256 1, sizes 2, create parent dirs 2, network policies 0"
        );
        assert!(!safe_summary.contains("cdn.example.com"));
        assert!(!safe_summary.contains("model-a.bin"));
        assert!(!safe_summary.contains("4096"));
    }

    #[test]
    fn download_batch_builder_rejects_empty_and_duplicate_destinations() {
        assert!(DownloadBatch::builder().build_checked().is_err());

        let destination = std::env::temp_dir()
            .join("kael-download-batch-duplicate")
            .join("model.bin");
        let request_a = DownloadRequest::builder("https://example.com/a.bin", &destination)
            .create_parent_dirs()
            .build_checked()
            .unwrap();
        let request_b = DownloadRequest::builder("https://example.com/b.bin", &destination)
            .create_parent_dirs()
            .build_checked()
            .unwrap();

        assert!(
            DownloadBatch::builder()
                .request(request_a)
                .request(request_b)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn download_destination_plan_prompts_or_builds_native_request() {
        let prompt = DownloadDestinationPlanBuilder::new(
            "https://cdn.example.com/private/report.pdf?token=sensitive",
        )
        .suggested_file_name("report.pdf")
        .build_checked()
        .unwrap();

        assert_eq!(
            prompt.next_action(),
            DownloadDestinationNextAction::PromptForDestination
        );
        assert!(prompt.needs_destination_prompt());
        assert!(!prompt.has_destination());
        assert_eq!(prompt.suggested_file_name(), Some("report.pdf"));
        assert!(prompt.request_builder().is_err());
        assert_eq!(
            prompt.to_text(),
            "download destination plan: destination false, suggested name true, sha256 false, size false, network policy false, create parent dirs false, existing policy fail if exists, next action prompt-for-destination"
        );
        assert!(!prompt.to_text().contains("cdn.example.com"));
        assert!(!prompt.to_text().contains("report.pdf"));

        let dir =
            std::env::temp_dir().join(format!("kael-download-destination-{}", std::process::id()));
        let policy = crate::NetworkPolicyBuilder::new()
            .allow_host("cdn.example.com")
            .build_checked()
            .unwrap();
        let plan = DownloadDestinationPlanBuilder::new("https://cdn.example.com/assets/model.bin")
            .download_dir(&dir)
            .suggested_file_name("model.bin")
            .network_policy(policy)
            .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .size_bytes(4096)
            .create_parent_dirs()
            .build_checked()
            .unwrap();

        assert_eq!(
            plan.next_action(),
            DownloadDestinationNextAction::BuildRequest
        );
        assert!(plan.can_build_request());
        assert_eq!(plan.destination(), Some(dir.join("model.bin").as_path()));
        assert!(plan.has_integrity_metadata());
        assert!(plan.has_network_policy());
        assert!(plan.creates_parent_dirs());

        let request = plan.build_request_checked().unwrap();
        assert_eq!(request.destination, dir.join("model.bin"));
        assert!(request.has_sha256());
        assert_eq!(request.size_bytes, Some(4096));
        assert!(request.create_parent_dirs);
    }

    #[test]
    fn download_destination_plan_reviews_existing_destinations() {
        let root = std::env::temp_dir().join(format!(
            "kael-download-destination-existing-{}",
            std::process::id()
        ));
        let destination = root.join("artifact.bin");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&destination, b"existing").unwrap();

        let plan = DownloadDestinationPlanBuilder::new("https://example.com/artifact.bin")
            .destination(&destination)
            .overwrite_existing()
            .build_checked()
            .unwrap();

        assert_eq!(
            plan.next_action(),
            DownloadDestinationNextAction::ReviewOverwritePolicy
        );
        assert!(plan.needs_overwrite_review());
        assert_eq!(
            plan.existing_file_policy(),
            DownloadExistingFilePolicy::Overwrite
        );
        assert!(!plan.to_text().contains("artifact.bin"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn download_destination_plan_rejects_invalid_generated_shapes() {
        let dir = std::env::temp_dir();

        assert!(
            DownloadDestinationPlanBuilder::new("file:///tmp/model.bin")
                .suggested_file_name("model.bin")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .suggested_file_name("../model.bin")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .suggested_file_name(" model.bin")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .download_dir("relative")
                .suggested_file_name("model.bin")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .destination(&dir)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .download_dir(dir.join("kael-missing-download-parent"))
                .suggested_file_name("model.bin")
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .download_dir(dir.join("kael-missing-download-parent"))
                .suggested_file_name("model.bin")
                .create_parent_dirs()
                .build_checked()
                .is_ok()
        );
        assert!(
            DownloadDestinationPlanBuilder::new("https://example.com/model.bin")
                .size_bytes(0)
                .build_checked()
                .is_err()
        );
    }

    #[test]
    fn download_execution_plan_validates_queue_policy() {
        let first_destination = std::env::temp_dir()
            .join(format!("kael-download-plan-{}-a", std::process::id()))
            .join("model-a.bin");
        let second_destination = std::env::temp_dir()
            .join(format!("kael-download-plan-{}-b", std::process::id()))
            .join("model-b.bin");
        let batch = DownloadBatch::builder()
            .request_builder(
                DownloadRequest::builder("https://cdn.example.com/model-a.bin", first_destination)
                    .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                    .size_bytes(4096)
                    .create_parent_dirs(),
            )
            .unwrap()
            .request_builder(
                DownloadRequest::builder("https://cdn.example.com/model-b.bin", second_destination)
                    .size_bytes(8192)
                    .create_parent_dirs(),
            )
            .unwrap()
            .build_checked()
            .unwrap();

        let builder = DownloadExecutionPlan::builder(batch)
            .max_parallel(2)
            .retry_attempts(3)
            .temporary_file_extension("partial");

        assert!(builder.validate().is_ok());
        assert_eq!(builder.request_count(), 2);
        assert_eq!(builder.max_parallel_count(), 2);
        assert_eq!(builder.retry_attempt_count(), 3);
        assert!(builder.uses_temporary_files());
        assert_eq!(
            builder.existing_file_policy(),
            DownloadExistingFilePolicy::FailIfExists
        );
        assert_eq!(
            builder.to_text(),
            "download execution plan: requests 2, max parallel 2, retries 3, temp files true, existing policy fail if exists, sha256 1, sizes 2, network policies 0"
        );

        let plan = builder.build_checked().unwrap();
        assert_eq!(plan.request_count(), 2);
        assert_eq!(plan.max_parallel(), 2);
        assert_eq!(plan.retry_attempts(), 3);
        assert_eq!(plan.temporary_file_extension(), Some("partial"));
        assert!(!plan.overwrites_existing());
        assert_eq!(
            plan.batch().to_safe_text(),
            "download batch: requests 2, sha256 1, sizes 2, create parent dirs 2, network policies 0"
        );
    }

    #[test]
    fn download_execution_plan_rejects_generated_footguns() {
        let destination = std::env::temp_dir()
            .join(format!(
                "kael-download-plan-existing-{}",
                std::process::id()
            ))
            .join("artifact.bin");
        let request = DownloadRequest::builder("https://example.com/artifact.bin", &destination)
            .create_parent_dirs()
            .build_checked()
            .unwrap();
        let batch = DownloadBatch::builder()
            .request(request)
            .build_checked()
            .unwrap();

        assert!(
            DownloadExecutionPlan::builder(batch.clone())
                .max_parallel(0)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadExecutionPlan::builder(batch.clone())
                .max_parallel(17)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadExecutionPlan::builder(batch.clone())
                .retry_attempts(11)
                .build_checked()
                .is_err()
        );
        assert!(
            DownloadExecutionPlan::builder(batch.clone())
                .temporary_file_extension(".partial")
                .build_checked()
                .is_err()
        );

        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&destination, b"existing").unwrap();
        assert!(
            DownloadExecutionPlan::builder(batch.clone())
                .build_checked()
                .is_err()
        );

        let plan = DownloadExecutionPlan::builder(batch)
            .overwrite_existing()
            .without_temporary_files()
            .no_retries()
            .serial()
            .build_checked()
            .unwrap();
        assert!(plan.overwrites_existing());
        assert!(!plan.uses_temporary_files());
        assert_eq!(plan.retry_attempts(), 0);
        assert_eq!(plan.max_parallel(), 1);

        let _ = std::fs::remove_file(destination);
    }

    #[test]
    fn download_execution_plan_summary_is_content_safe() {
        let destination = std::env::temp_dir()
            .join(format!("kael-download-plan-safe-{}", std::process::id()))
            .join("private-model.bin");
        let request = DownloadRequest::builder(
            "https://user:secret@private.example.com/model.bin?token=sensitive",
            &destination,
        )
        .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .size_bytes(1024)
        .create_parent_dirs()
        .build_checked()
        .unwrap();
        let plan = DownloadExecutionPlan::from_request(request)
            .max_parallel(1)
            .retry_attempts(1)
            .build_checked()
            .unwrap();

        assert_eq!(DownloadExistingFilePolicy::Overwrite.to_text(), "overwrite");
        assert!(plan.to_text().contains("requests 1"));
        assert!(plan.to_text().contains("max parallel 1"));
        assert!(plan.to_text().contains("sha256 1"));
        assert!(!plan.to_text().contains("private.example.com"));
        assert!(!plan.to_text().contains("private-model.bin"));
        assert!(!plan.to_text().contains("secret"));
        assert!(!plan.to_text().contains("token"));
        assert!(!plan.to_text().contains("1024"));
    }

    #[test]
    fn download_handoff_reports_policy_and_integrity_next_actions() {
        let destination = std::env::temp_dir()
            .join(format!("kael-download-handoff-{}", std::process::id()))
            .join("model.bin");

        let missing_policy = DownloadHandoffBuilder::new()
            .request_builder(
                DownloadRequest::builder(
                    "https://user:secret@cdn.example.com/private/model.bin?token=sensitive",
                    &destination,
                )
                .sha256("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .size_bytes(1024)
                .create_parent_dirs(),
            )
            .unwrap()
            .serial()
            .retry_attempts(1)
            .build_checked()
            .unwrap();

        assert_eq!(
            missing_policy.next_action(),
            DownloadHandoffNextAction::AddNetworkPolicy
        );
        assert_eq!(missing_policy.request_count(), 1);
        assert!(!missing_policy.has_complete_network_policy());
        assert!(missing_policy.has_complete_integrity_metadata());
        assert!(!missing_policy.is_queue_ready());
        assert!(!missing_policy.needs_overwrite_review());
        assert_eq!(
            missing_policy.to_text(),
            "download handoff: requests 1, max parallel 1, retries 1, temp files true, overwrite false, network policies 0/1, integrity 1/1, next action add-network-policy"
        );
        assert!(!missing_policy.to_text().contains("cdn.example.com"));
        assert!(!missing_policy.to_text().contains("model.bin"));
        assert!(!missing_policy.to_text().contains("secret"));

        let policy = crate::NetworkPolicyBuilder::new()
            .allow_host("cdn.example.com")
            .build_checked()
            .unwrap();
        let missing_integrity = DownloadHandoffBuilder::new()
            .request_builder(
                DownloadRequest::builder("https://cdn.example.com/private/model.bin", &destination)
                    .network_policy(policy.clone())
                    .create_parent_dirs(),
            )
            .unwrap()
            .build_checked()
            .unwrap();

        assert_eq!(
            missing_integrity.next_action(),
            DownloadHandoffNextAction::AddIntegrityMetadata
        );
        assert!(missing_integrity.has_complete_network_policy());
        assert!(!missing_integrity.has_complete_integrity_metadata());

        let queue_ready = DownloadHandoffBuilder::new()
            .request_builder(
                DownloadRequest::builder("https://cdn.example.com/private/model.bin", &destination)
                    .network_policy(policy)
                    .sha256("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
                    .size_bytes(2048)
                    .create_parent_dirs(),
            )
            .unwrap()
            .max_parallel(1)
            .no_retries()
            .without_temporary_files()
            .build_checked()
            .unwrap();

        assert_eq!(
            queue_ready.next_action(),
            DownloadHandoffNextAction::QueueDownloads
        );
        assert!(queue_ready.is_queue_ready());
        assert!(!queue_ready.execution_plan().uses_temporary_files());
        assert_eq!(queue_ready.execution_plan().retry_attempts(), 0);
        assert_eq!(
            DownloadHandoffNextAction::QueueDownloads.to_text(),
            "queue-downloads"
        );
        assert_eq!(
            queue_ready.execution_plan().handoff().next_action(),
            DownloadHandoffNextAction::QueueDownloads
        );
    }

    #[test]
    fn download_handoff_reviews_overwrite_policy_first() {
        let destination = std::env::temp_dir()
            .join(format!(
                "kael-download-handoff-overwrite-{}",
                std::process::id()
            ))
            .join("artifact.bin");
        let request = DownloadRequest::builder("https://example.com/artifact.bin", &destination)
            .network_policy(
                crate::NetworkPolicyBuilder::new()
                    .allow_host("example.com")
                    .build_checked()
                    .unwrap(),
            )
            .sha256("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
            .size_bytes(16)
            .create_parent_dirs()
            .build_checked()
            .unwrap();

        let handoff = DownloadHandoffBuilder::new()
            .request(request)
            .overwrite_existing()
            .build_checked()
            .unwrap();

        assert_eq!(
            handoff.next_action(),
            DownloadHandoffNextAction::ReviewOverwritePolicy
        );
        assert!(handoff.needs_overwrite_review());
        assert!(!handoff.is_queue_ready());
        assert!(handoff.has_complete_network_policy());
        assert!(handoff.has_complete_integrity_metadata());
        assert!(DownloadHandoffBuilder::new().validate().is_err());
        assert_eq!(
            DownloadHandoffBuilder::new().to_text(),
            "download handoff builder: requests 0, invalid true"
        );
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
            AutoUpdaterConfigBuilder::new(format!(
                "https://example.com/{}",
                "a".repeat(MAX_UPDATE_URL_BYTES)
            ))
            .validate()
            .is_err()
        );
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
    fn update_feed_size_limit_sets_error_and_discards_stale_offer() {
        use http_client::{AsyncBody, FakeHttpClient, Response};

        let client = FakeHttpClient::create(|_request| async {
            Ok(Response::builder()
                .status(200)
                .body(AsyncBody::from(vec![b' '; MAX_UPDATE_FEED_BYTES + 1]))
                .unwrap())
        });
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.latest_update = Some(UpdateInfo {
            version: SemanticVersion::new(1, 1, 0),
            release_notes: None,
            download_url: "https://example.com/stale.zip".to_string(),
            signature: None,
            sha256: None,
            size_bytes: None,
        });

        let error = smol::block_on(updater.check_for_updates()).unwrap_err();
        assert!(error.to_string().contains("feed exceeds"));
        assert!(matches!(updater.status(), UpdateStatus::Error(_)));
        assert!(updater.latest_update().is_none());
    }

    #[test]
    fn update_feed_rejects_invalid_utf8_and_discards_stale_offer() {
        use http_client::{AsyncBody, FakeHttpClient, Response};

        let client = FakeHttpClient::create(|_request| async {
            Ok(Response::builder()
                .status(200)
                .body(AsyncBody::from(vec![0xff, 0xfe]))
                .unwrap())
        });
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.latest_update = Some(UpdateInfo {
            version: SemanticVersion::new(1, 1, 0),
            release_notes: None,
            download_url: "https://example.com/stale.zip".to_string(),
            signature: None,
            sha256: None,
            size_bytes: None,
        });

        let error = smol::block_on(updater.check_for_updates()).unwrap_err();
        assert!(error.to_string().contains("valid UTF-8"));
        assert!(matches!(updater.status(), UpdateStatus::Error(_)));
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
    fn test_download_update_rejects_oversized_content_length_before_body_read() {
        use http_client::{AsyncBody, FakeHttpClient, Response};

        let genuine = b"small signed update".to_vec();
        let (key, update) = signed_update_fixture(&genuine, UpdateChannel::Stable);
        let client = FakeHttpClient::create(move |_request| async move {
            Ok(Response::builder()
                .status(200)
                .header("content-length", (MAX_UPDATE_PACKAGE_BYTES + 1).to_string())
                .body(AsyncBody::from(Vec::<u8>::new()))
                .unwrap())
        });
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.set_public_key(key.as_bytes()).unwrap();
        updater.latest_update = Some(update);

        let error = smol::block_on(updater.download_update(|_| {})).unwrap_err();
        assert!(error.to_string().contains("package exceeds"));
        assert!(matches!(updater.status(), UpdateStatus::Error(_)));
        assert!(updater.downloaded_path.is_none());
    }

    #[test]
    fn test_download_start_failure_sets_error_and_discards_stale_package() {
        use http_client::FakeHttpClient;

        let genuine = b"small signed update".to_vec();
        let (key, update) = signed_update_fixture(&genuine, UpdateChannel::Stable);
        let client =
            FakeHttpClient::create(
                move |_request| async move { Err(anyhow!("network unavailable")) },
            );
        let config = AutoUpdaterConfig {
            feed_url: "https://example.com/feed".to_string(),
            check_interval: Duration::from_secs(3600),
            allow_prerelease: false,
        };
        let mut updater = AutoUpdater::new(config, SemanticVersion::new(1, 0, 0), client);
        updater.set_public_key(key.as_bytes()).unwrap();
        updater.latest_update = Some(update);
        updater.downloaded_path = Some(std::env::temp_dir().join("stale-update.zip"));

        let error = smol::block_on(updater.download_update(|_| {})).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to start update download")
        );
        assert!(matches!(updater.status(), UpdateStatus::Error(_)));
        assert!(updater.downloaded_path.is_none());
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
