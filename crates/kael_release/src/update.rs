//! Delta update management, channels, and rollback support.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use url::Url;

const MAX_UPDATE_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_CHANNEL_BYTES: usize = 128;

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

impl UpdateChannel {
    /// Returns the string representation of the channel.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Nightly => "nightly",
            Self::Custom(name) => name,
        }
    }
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
        if parse_semver(&self.version).is_none() {
            anyhow::bail!("update version must be a strict MAJOR.MINOR.PATCH triplet");
        }
        if let Some(min_version) = &self.min_version
            && parse_semver(min_version).is_none()
        {
            anyhow::bail!("minimum version must be a strict MAJOR.MINOR.PATCH triplet");
        }
        if let UpdateChannel::Custom(channel) = &self.channel
            && (channel.is_empty()
                || channel.len() > MAX_CHANNEL_BYTES
                || !channel
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
        {
            anyhow::bail!("custom update channel is invalid");
        }
        let url = Url::parse(&self.url).map_err(|_| anyhow::anyhow!("update URL is invalid"))?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            anyhow::bail!("update URL must be an absolute credential-free https URL");
        }
        if self.sha256.len() != 64 {
            anyhow::bail!("sha256 must be exactly 64 hex characters");
        }
        if !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("sha256 must contain only hex characters");
        }
        if self.size_bytes == 0 || self.size_bytes > MAX_UPDATE_BYTES {
            anyhow::bail!("size_bytes must be between 1 and {MAX_UPDATE_BYTES}");
        }
        if self
            .release_notes
            .as_ref()
            .is_some_and(|notes| notes.len() > MAX_TEXT_BYTES)
        {
            anyhow::bail!("release notes exceed {MAX_TEXT_BYTES} bytes");
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
    /// Whether the client must refuse updates that are not signed by the
    /// configured public key (fail closed). Defaults to `true`.
    #[serde(default = "default_true")]
    pub require_signed_feeds: bool,
}

fn default_true() -> bool {
    true
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
            require_signed_feeds: true,
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

fn manifest_signing_payload(manifest: &UpdateManifest) -> Vec<u8> {
    let mut payload = b"kael-update-v2".to_vec();
    put_field(&mut payload, manifest.version.as_bytes());
    match &manifest.channel {
        UpdateChannel::Stable => put_field(&mut payload, b"stable"),
        UpdateChannel::Beta => put_field(&mut payload, b"beta"),
        UpdateChannel::Nightly => put_field(&mut payload, b"nightly"),
        UpdateChannel::Custom(channel) => {
            put_field(&mut payload, b"custom");
            put_field(&mut payload, channel.as_bytes());
        }
    }
    put_field(&mut payload, manifest.url.as_bytes());
    put_field(&mut payload, manifest.sha256.as_bytes());
    put_field(&mut payload, &manifest.size_bytes.to_be_bytes());
    put_optional_field(&mut payload, manifest.release_notes.as_deref());
    put_optional_field(&mut payload, manifest.min_version.as_deref());
    payload
}

fn put_field(payload: &mut Vec<u8>, value: &[u8]) {
    payload.extend_from_slice(&(value.len() as u64).to_be_bytes());
    payload.extend_from_slice(value);
}

fn put_optional_field(payload: &mut Vec<u8>, value: Option<&str>) {
    payload.push(u8::from(value.is_some()));
    if let Some(value) = value {
        put_field(payload, value.as_bytes());
    }
}

/// Signs an [`UpdateManifest`] with the given [`SigningKey`].
pub fn sign_manifest(manifest: &UpdateManifest, key: &SigningKey) -> Signature {
    let payload = manifest_signing_payload(manifest);
    key.sign(&payload)
}

/// Verifies a manifest [`Signature`] against the given [`VerifyingKey`].
pub fn verify_manifest(
    manifest: &UpdateManifest,
    signature: &Signature,
    key: &VerifyingKey,
) -> bool {
    if manifest.validate().is_err() {
        return false;
    }
    let payload = manifest_signing_payload(manifest);
    key.verify(&payload, signature).is_ok()
}

/// Parses an ed25519 signing (private) key from a 64-character hex string.
///
/// The hex must encode exactly the 32-byte ed25519 secret seed, i.e. the value
/// produced by [`SigningKey::to_bytes`]. This is the format expected for the
/// `KAEL_UPDATE_SIGNING_KEY` environment variable consumed by the release feed
/// signer.
pub fn signing_key_from_hex(hex_key: &str) -> anyhow::Result<SigningKey> {
    let bytes = hex::decode(hex_key.trim())
        .map_err(|_| anyhow::anyhow!("ed25519 signing key is not valid hex"))?;
    let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!("ed25519 signing key must be exactly 32 bytes (64 hex chars)")
    })?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Parses an ed25519 verifying (public) key from a 64-character hex string.
pub fn verifying_key_from_hex(hex_key: &str) -> anyhow::Result<VerifyingKey> {
    let bytes = hex::decode(hex_key.trim())
        .map_err(|_| anyhow::anyhow!("ed25519 public key is not valid hex"))?;
    let key: [u8; 32] = bytes.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!("ed25519 public key must be exactly 32 bytes (64 hex chars)")
    })?;
    VerifyingKey::from_bytes(&key).map_err(|_| anyhow::anyhow!("invalid ed25519 public key"))
}

/// Encodes a manifest [`Signature`] as standard base64.
///
/// This is the wire format the update client expects for the `signature` field
/// of a feed entry.
pub fn signature_to_base64(signature: &Signature) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(signature.to_bytes())
}

/// Generates a fresh ed25519 keypair, returning `(private_hex, public_hex)`.
///
/// The private hex is suitable for the `KAEL_UPDATE_SIGNING_KEY` secret; the
/// public hex is embedded into the client so it can authenticate signed feeds.
pub fn generate_keypair() -> (String, String) {
    let signing_key = SigningKey::generate(&mut rand::thread_rng());
    let verifying_key = signing_key.verifying_key();
    (
        hex::encode(signing_key.to_bytes()),
        hex::encode(verifying_key.to_bytes()),
    )
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    fn component(part: &str) -> Option<u64> {
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        part.parse().ok()
    }

    let mut parts = version.split('.');
    let parsed = (
        component(parts.next()?)?,
        component(parts.next()?)?,
        component(parts.next()?)?,
    );
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

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
    fn manifest_rejects_insecure_update_url() {
        let mut m = valid_manifest();
        m.url = "http://example.com/update.zip".to_string();
        assert!(m.validate().is_err());

        m.url = "https://example.com/update.zip".to_string();
        assert!(m.validate().is_ok());
    }

    #[test]
    fn manifest_rejects_malformed_versions_urls_and_oversized_artifacts() {
        for version in ["01.0.0", "1.0.0-beta", "1.0.0.1", "١.0.0"] {
            let mut manifest = valid_manifest();
            manifest.version = version.to_string();
            assert!(manifest.validate().is_err());
        }
        for url in [
            "https://",
            "https://user:secret@example.com/update.zip",
            "https://example.com/update.zip#fragment",
        ] {
            let mut manifest = valid_manifest();
            manifest.url = url.to_string();
            assert!(manifest.validate().is_err());
        }
        let mut manifest = valid_manifest();
        manifest.size_bytes = MAX_UPDATE_BYTES + 1;
        assert!(manifest.validate().is_err());
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

    #[test]
    fn sign_and_verify_manifest() {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();

        let manifest = UpdateManifest {
            version: "1.2.0".into(),
            channel: UpdateChannel::Stable,
            url: "https://example.com/update.tar.gz".into(),
            sha256: valid_sha256(),
            size_bytes: 1024,
            release_notes: None,
            min_version: None,
        };

        let signature = sign_manifest(&manifest, &signing_key);
        assert!(verify_manifest(&manifest, &signature, &verifying_key));
    }

    #[test]
    fn tampered_manifest_fails_verification() {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();

        let mut manifest = UpdateManifest {
            version: "1.2.0".into(),
            channel: UpdateChannel::Stable,
            url: "https://example.com/update.tar.gz".into(),
            sha256: valid_sha256(),
            size_bytes: 1024,
            release_notes: None,
            min_version: None,
        };

        let signature = sign_manifest(&manifest, &signing_key);
        manifest.url = "https://evil.com/malware.tar.gz".into();
        assert!(!verify_manifest(&manifest, &signature, &verifying_key));
    }

    #[test]
    fn signature_covers_optional_fields_and_channel_variant() {
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        let manifest = valid_manifest();
        let signature = sign_manifest(&manifest, &signing_key);

        let mut tampered = manifest.clone();
        tampered.release_notes = Some("Different notes".to_string());
        assert!(!verify_manifest(&tampered, &signature, &verifying_key));
        tampered = manifest.clone();
        tampered.min_version = Some("1.1.0".to_string());
        assert!(!verify_manifest(&tampered, &signature, &verifying_key));
        tampered = manifest.clone();
        tampered.channel = UpdateChannel::Custom("stable".to_string());
        assert!(!verify_manifest(&tampered, &signature, &verifying_key));
    }

    #[test]
    fn downgrade_rejected() {
        let manifest = UpdateManifest {
            version: "1.0.0".into(),
            channel: UpdateChannel::Stable,
            url: "https://example.com/update.tar.gz".into(),
            sha256: "abc".into(),
            size_bytes: 1024,
            release_notes: None,
            min_version: Some("1.5.0".into()),
        };
        assert!(!manifest.is_compatible_with("1.2.0"));
    }

    #[test]
    fn signing_key_hex_roundtrips_and_signs() {
        let (private_hex, public_hex) = generate_keypair();
        let signing_key = signing_key_from_hex(&private_hex).unwrap();
        let verifying_key = verifying_key_from_hex(&public_hex).unwrap();
        assert_eq!(
            signing_key.verifying_key().to_bytes(),
            verifying_key.to_bytes()
        );

        let manifest = valid_manifest();
        let signature = sign_manifest(&manifest, &signing_key);
        assert!(verify_manifest(&manifest, &signature, &verifying_key));
    }

    #[test]
    fn signing_key_from_hex_rejects_bad_input() {
        assert!(signing_key_from_hex("zz").is_err());
        assert!(signing_key_from_hex(&"a".repeat(10)).is_err());
        assert!(verifying_key_from_hex("not-hex").is_err());
    }

    #[test]
    fn upgrade_accepted() {
        let manifest = UpdateManifest {
            version: "2.0.0".into(),
            channel: UpdateChannel::Stable,
            url: "https://example.com/update.tar.gz".into(),
            sha256: "abc".into(),
            size_bytes: 1024,
            release_notes: None,
            min_version: Some("1.0.0".into()),
        };
        assert!(manifest.is_compatible_with("1.5.0"));
    }
}
