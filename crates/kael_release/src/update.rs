//! Delta update management, channels, and rollback support.

use serde::{Deserialize, Serialize};

/// Distribution channel for updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateChannel {
    /// Stable production releases.
    Stable,
    /// Beta pre-release builds.
    Beta,
    /// Nightly development builds.
    Nightly,
    /// A user-defined custom channel.
    Custom(String),
}

/// Describes an available update.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    /// Version string for this update.
    pub version: String,
    /// Distribution channel.
    pub channel: UpdateChannel,
    /// Download URL.
    pub url: String,
    /// SHA-256 hash of the update artifact.
    pub sha256: String,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Optional release notes.
    pub release_notes: Option<String>,
    /// Minimum application version required to apply this update.
    pub min_version: Option<String>,
}

impl UpdateManifest {
    /// Checks whether this update can be applied to the given current version.
    pub fn is_compatible_with(&self, current_version: &str) -> bool {
        let Some(min) = &self.min_version else {
            return true;
        };
        match (parse_semver(current_version), parse_semver(min)) {
            (Some(current), Some(minimum)) => current >= minimum,
            _ => false,
        }
    }

    /// Validates the manifest fields.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version.is_empty() {
            anyhow::bail!("update version must not be empty");
        }
        if self.url.is_empty() {
            anyhow::bail!("update URL must not be empty");
        }
        if self.sha256.len() != 64 {
            anyhow::bail!("sha256 must be exactly 64 hex characters");
        }
        if !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("sha256 must contain only hex characters");
        }
        if self.size_bytes == 0 {
            anyhow::bail!("size_bytes must be greater than zero");
        }
        Ok(())
    }
}

/// Policy governing automatic update behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePolicy {
    /// Which channel to track.
    pub channel: UpdateChannel,
    /// Whether to automatically check for updates.
    pub auto_check: bool,
    /// Whether to automatically download updates.
    pub auto_download: bool,
    /// Whether to automatically install downloaded updates.
    pub auto_install: bool,
    /// Interval in seconds between update checks.
    pub check_interval_secs: u64,
}

impl UpdatePolicy {
    /// Returns a conservative default policy (check only, no auto-download/install).
    pub fn default_stable() -> Self {
        Self {
            channel: UpdateChannel::Stable,
            auto_check: true,
            auto_download: false,
            auto_install: false,
            check_interval_secs: 86400,
        }
    }
}

/// Information needed to roll back to a previous version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackInfo {
    /// Version to roll back to.
    pub previous_version: String,
    /// Path to the rollback artifact.
    pub rollback_path: String,
    /// Unix timestamp when the rollback info was created.
    pub created_at: u64,
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_sha256() -> String {
        "a".repeat(64)
    }

    fn valid_manifest() -> UpdateManifest {
        UpdateManifest {
            version: "2.0.0".to_string(),
            channel: UpdateChannel::Stable,
            url: "https://example.com/update.tar.gz".to_string(),
            sha256: valid_sha256(),
            size_bytes: 1024,
            release_notes: Some("Bug fixes".to_string()),
            min_version: Some("1.0.0".to_string()),
        }
    }

    #[test]
    fn compatible_when_no_min_version() {
        let mut m = valid_manifest();
        m.min_version = None;
        assert!(m.is_compatible_with("0.1.0"));
    }

    #[test]
    fn compatible_when_current_equals_min() {
        let m = valid_manifest();
        assert!(m.is_compatible_with("1.0.0"));
    }

    #[test]
    fn compatible_when_current_above_min() {
        let m = valid_manifest();
        assert!(m.is_compatible_with("1.5.0"));
    }

    #[test]
    fn incompatible_when_current_below_min() {
        let m = valid_manifest();
        assert!(!m.is_compatible_with("0.9.0"));
    }

    #[test]
    fn validate_valid_manifest() {
        valid_manifest().validate().unwrap();
    }

    #[test]
    fn validate_empty_version() {
        let mut m = valid_manifest();
        m.version = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_empty_url() {
        let mut m = valid_manifest();
        m.url = String::new();
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_bad_sha256_length() {
        let mut m = valid_manifest();
        m.sha256 = "abc".to_string();
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_bad_sha256_chars() {
        let mut m = valid_manifest();
        m.sha256 = "g".repeat(64);
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_zero_size() {
        let mut m = valid_manifest();
        m.size_bytes = 0;
        assert!(m.validate().is_err());
    }

    #[test]
    fn default_policy_is_conservative() {
        let policy = UpdatePolicy::default_stable();
        assert!(policy.auto_check);
        assert!(!policy.auto_download);
        assert!(!policy.auto_install);
        assert_eq!(policy.channel, UpdateChannel::Stable);
    }

    #[test]
    fn rollback_info_serialization() {
        let info = RollbackInfo {
            previous_version: "1.0.0".to_string(),
            rollback_path: "/backups/v1.0.0".to_string(),
            created_at: 1700000000,
        };
        let json = serde_json::to_string(&info).unwrap();
        let restored: RollbackInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.previous_version, "1.0.0");
    }

    #[test]
    fn custom_channel_equality() {
        let a = UpdateChannel::Custom("internal".to_string());
        let b = UpdateChannel::Custom("internal".to_string());
        assert_eq!(a, b);
        assert_ne!(a, UpdateChannel::Stable);
    }

    #[test]
    fn manifest_serialization_roundtrip() {
        let m = valid_manifest();
        let json = serde_json::to_string(&m).unwrap();
        let restored: UpdateManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.version, "2.0.0");
        assert_eq!(restored.channel, UpdateChannel::Stable);
    }
}
