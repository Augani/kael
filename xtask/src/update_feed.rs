use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use kael_release::update::{
    UpdateChannel, UpdateManifest, sign_manifest, signature_to_base64, signing_key_from_hex,
    verify_manifest, verifying_key_from_hex,
};

use crate::{DistConfig, MAX_METADATA_FILE_BYTES, atomic_write, read_bounded_utf8_file};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFeed {
    pub version: String,
    pub channel: String,
    pub url: String,
    pub notes_url: Option<String>,
    pub pub_date: String,
    pub platforms: Vec<PlatformUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformUpdate {
    pub platform: String,
    pub url: String,
    pub signature: Option<String>,
    pub checksum: String,
    pub size_bytes: u64,
}

pub struct FeedOptions {
    pub dry_run: bool,
    pub signing_key: Option<String>,
}

pub fn run(
    config: &DistConfig,
    output: &Path,
    artifacts: &[PathBuf],
    options: &FeedOptions,
) -> Result<()> {
    let feed = build_feed(
        config,
        artifacts,
        options.signing_key.as_deref(),
        options.dry_run,
    )?;

    let json = serde_json::to_string_pretty(&feed)?;

    if options.dry_run {
        println!("dry-run: would write update feed to {}", output.display());
        println!("{}", json);
        println!("dry-run: update metadata preview generated");
    } else {
        atomic_write(output, json.as_bytes())
            .with_context(|| format!("failed to write update feed: {}", output.display()))?;
        println!("update feed generated: {}", output.display());
    }
    Ok(())
}

pub fn verify(feed_path: &Path, public_key_hex: &str) -> Result<()> {
    use base64::Engine as _;

    let verifying_key = verifying_key_from_hex(public_key_hex)
        .context("update public key is not a valid ed25519 key")?;

    let json = read_bounded_utf8_file(feed_path, MAX_METADATA_FILE_BYTES)
        .with_context(|| format!("failed to read update feed: {}", feed_path.display()))?;
    let feed: UpdateFeed = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse update feed: {}", feed_path.display()))?;

    let channel = match feed.channel.to_ascii_lowercase().as_str() {
        "stable" => UpdateChannel::Stable,
        "beta" => UpdateChannel::Beta,
        "nightly" => UpdateChannel::Nightly,
        _ => UpdateChannel::Custom(feed.channel.clone()),
    };

    if feed.platforms.is_empty() {
        anyhow::bail!("update feed has no platform entries to verify");
    }

    for entry in &feed.platforms {
        let signature_b64 = entry.signature.as_deref().with_context(|| {
            format!(
                "platform entry {} is unsigned but verification was requested",
                entry.platform
            )
        })?;
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(signature_b64)
            .with_context(|| format!("signature for {} is not valid base64", entry.platform))?;
        let sig_array: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("signature for {} is not 64 bytes", entry.platform))?;
        let signature = ed25519_signature(&sig_array);

        let manifest = UpdateManifest {
            version: feed.version.clone(),
            channel: channel.clone(),
            url: entry.url.clone(),
            sha256: entry.checksum.clone(),
            size_bytes: entry.size_bytes,
            release_notes: None,
            min_version: None,
        };

        if !verify_manifest(&manifest, &signature, &verifying_key) {
            anyhow::bail!(
                "signature verification failed for platform {}",
                entry.platform
            );
        }
    }

    Ok(())
}

/// Validate the updater-specific prerequisites required before uploading a
/// real release. This intentionally does not run during metadata simulation:
/// CI can preview planned paths without production signing credentials, but a
/// real publication must prove that every selected updater package produces a
/// non-placeholder manifest signed by the key embedded in the application.
pub(crate) fn validate_publish_readiness(
    config: &DistConfig,
    artifacts: &[PathBuf],
    signing_key_hex: &str,
) -> Result<()> {
    let updater = config
        .updater
        .as_ref()
        .context("updater config is required for updater publication")?;
    let configured_public_key = updater
        .public_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .context("updater.public_key must be configured before publishing")?;
    let signing_key = signing_key_from_hex(signing_key_hex)
        .context("KAEL_UPDATE_SIGNING_KEY is not a valid ed25519 private key")?;
    let verifying_key = verifying_key_from_hex(configured_public_key)
        .context("updater.public_key is not a valid ed25519 public key")?;
    anyhow::ensure!(
        signing_key.verifying_key().to_bytes() == verifying_key.to_bytes(),
        "KAEL_UPDATE_SIGNING_KEY does not match updater.public_key"
    );

    let feed = build_feed(config, artifacts, Some(signing_key_hex), false)?;
    anyhow::ensure!(
        feed.platforms.iter().all(|entry| {
            entry.signature.is_some() && entry.size_bytes > 0 && entry.checksum != "0".repeat(64)
        }),
        "updater metadata is unsigned or contains placeholder artifact metadata"
    );
    Ok(())
}

fn ed25519_signature(bytes: &[u8; 64]) -> kael_release::ed25519_dalek::Signature {
    kael_release::ed25519_dalek::Signature::from_bytes(bytes)
}

fn channel_for(config: &DistConfig) -> UpdateChannel {
    let raw = config
        .updater
        .as_ref()
        .and_then(|updater| updater.channel.as_deref())
        .unwrap_or("stable")
        .trim();
    match raw.to_ascii_lowercase().as_str() {
        "stable" => UpdateChannel::Stable,
        "beta" => UpdateChannel::Beta,
        "nightly" => UpdateChannel::Nightly,
        _ => UpdateChannel::Custom(raw.to_string()),
    }
}

fn build_feed(
    config: &DistConfig,
    artifacts: &[PathBuf],
    signing_key_hex: Option<&str>,
    allow_planned_artifacts: bool,
) -> Result<UpdateFeed> {
    let updater = config
        .updater
        .as_ref()
        .context("updater config is required to generate update feed")?;

    let channel = channel_for(config);

    let signing_key = match signing_key_hex {
        Some(hex) if !hex.trim().is_empty() => Some(
            signing_key_from_hex(hex)
                .context("KAEL_UPDATE_SIGNING_KEY is set but is not a valid ed25519 key")?,
        ),
        _ => {
            eprintln!(
                "warning: KAEL_UPDATE_SIGNING_KEY is not set; producing an UNSIGNED update feed (dev only)"
            );
            None
        }
    };

    anyhow::ensure!(
        !artifacts.is_empty(),
        "at least one update artifact is required"
    );

    let mut seen_platforms = HashSet::new();
    let platforms = artifacts
        .iter()
        .map(|artifact| {
            let platform = detect_platform(artifact);
            anyhow::ensure!(
                platform != "unknown",
                "cannot infer update platform from artifact name: {}",
                artifact.display()
            );
            anyhow::ensure!(
                seen_platforms.insert(platform.clone()),
                "multiple update artifacts were supplied for platform {platform}; select one canonical updater package"
            );
            let file_name = artifact
                .file_name()
                .and_then(|name| name.to_str())
                .with_context(|| {
                    format!(
                        "update artifact must have a UTF-8 file name: {}",
                        artifact.display()
                    )
                })?;
            let url = format!(
                "{}/{}",
                updater.artifact_base_url.trim_end_matches('/'),
                percent_encode_path_segment(file_name)
            );
            let (checksum, size_bytes) = match fs::symlink_metadata(artifact) {
                Ok(metadata) => {
                    anyhow::ensure!(
                        metadata.file_type().is_file(),
                        "update artifact must be a regular file: {}",
                        artifact.display()
                    );
                    anyhow::ensure!(
                        allow_planned_artifacts || metadata.len() > 0,
                        "update artifact must not be empty: {}",
                        artifact.display()
                    );
                    (sha256_file(artifact)?, metadata.len())
                }
                Err(error)
                    if error.kind() == ErrorKind::NotFound && allow_planned_artifacts =>
                {
                    ("0".repeat(64), 0)
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to inspect update artifact: {}", artifact.display())
                    });
                }
            };

            let signature = match signing_key.as_ref() {
                Some(key) => {
                    let manifest = UpdateManifest {
                        version: config.version.clone(),
                        channel: channel.clone(),
                        url: url.clone(),
                        sha256: checksum.clone(),
                        size_bytes,
                        release_notes: None,
                        min_version: None,
                    };
                    let signature = sign_manifest(&manifest, key);
                    if !verify_manifest(&manifest, &signature, &key.verifying_key()) {
                        anyhow::bail!(
                            "internal error: freshly produced signature failed verification for {}",
                            url
                        );
                    }
                    Some(signature_to_base64(&signature))
                }
                None => None,
            };

            Ok(PlatformUpdate {
                platform,
                url,
                signature,
                checksum,
                size_bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(UpdateFeed {
        version: config.version.clone(),
        channel: channel.as_str().to_string(),
        url: updater.feed_url.clone(),
        notes_url: None,
        pub_date: now_rfc3339(),
        platforms,
    })
}

fn percent_encode_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[(byte >> 4) as usize]));
            encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

fn detect_platform(artifact: &Path) -> String {
    let name = artifact.to_string_lossy().to_lowercase();
    if name.ends_with(".app")
        || name.ends_with(".dmg")
        || name.ends_with(".pkg")
        || name.contains("macos")
        || name.contains("darwin")
    {
        "macos".to_string()
    } else if name.ends_with(".exe")
        || name.ends_with(".msi")
        || name.ends_with(".msix")
        || name.contains("windows")
        || name.contains("win32")
    {
        "windows".to_string()
    } else if name.ends_with(".appimage")
        || name.ends_with(".appdir")
        || name.ends_with(".deb")
        || name.ends_with(".rpm")
        || name.ends_with(".flatpak")
        || name.contains("linux")
    {
        "linux".to_string()
    } else {
        "unknown".to_string()
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("failed to open file for checksum: {}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("failed to read file for checksum: {}", path.display()))?;
    Ok(hex::encode(hasher.finalize()))
}

fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let (year, month, day, hour, minute, second) = unix_to_utc(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

fn unix_to_utc(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let mut days = secs / 86400;
    let rem_secs = secs % 86400;
    let mut year = 1970;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let days_in_month = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for (i, &dim) in days_in_month.iter().enumerate() {
        if days < dim {
            month = i as u64 + 1;
            break;
        }
        days -= dim;
    }
    let day = days + 1;

    let hour = rem_secs / 3600;
    let minute = (rem_secs % 3600) / 60;
    let second = rem_secs % 60;

    (year, month, day, hour, minute, second)
}

fn is_leap_year(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BundleMetadata, IconSet, UpdaterConfig};
    use base64::Engine as _;
    use kael_release::update::{generate_keypair, verifying_key_from_hex};

    fn dist_config_with_artifact(artifact_base_url: &str, version: &str) -> DistConfig {
        DistConfig {
            app_id: "com.kael.testapp".to_string(),
            name: "Test App".to_string(),
            version: version.to_string(),
            icons: IconSet {
                macos: None,
                windows: None,
                linux: None,
            },
            bundle: BundleMetadata {
                copyright: None,
                category: None,
                minimum_system_version: None,
                file_description: None,
                linux_categories: None,
            },
            signing: None,
            updater: Some(UpdaterConfig {
                feed_url: "https://updates.kael.dev/feed.json".to_string(),
                artifact_base_url: artifact_base_url.to_string(),
                public_key: None,
                channel: Some("stable".to_string()),
            }),
        }
    }

    fn temp_artifact(contents: &[u8], name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kael_feed_test_{}", uuid_like()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn uuid_like() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn unsigned_feed_when_no_key() {
        let artifact = temp_artifact(b"payload", "Test-macos.zip");
        let config = dist_config_with_artifact("https://dl.kael.dev/feed", "1.2.3");
        let feed = build_feed(&config, std::slice::from_ref(&artifact), None, false).unwrap();
        assert_eq!(feed.version, "1.2.3");
        assert_eq!(feed.channel, "stable");
        assert_eq!(feed.platforms.len(), 1);
        assert_eq!(feed.platforms[0].platform, "macos");
        assert_eq!(
            feed.platforms[0].url,
            "https://dl.kael.dev/feed/Test-macos.zip"
        );
        assert_eq!(feed.platforms[0].size_bytes, 7);
        assert!(feed.platforms[0].signature.is_none());
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }

    #[test]
    fn signed_feed_roundtrips_through_verify_manifest() {
        let artifact = temp_artifact(b"some real bytes here", "Test-macos.zip");
        let config = dist_config_with_artifact("https://dl.kael.dev/feed", "2.0.0");
        let (private_hex, public_hex) = generate_keypair();

        let feed = build_feed(
            &config,
            std::slice::from_ref(&artifact),
            Some(&private_hex),
            false,
        )
        .unwrap();
        let entry = &feed.platforms[0];
        let signature_b64 = entry.signature.as_ref().expect("feed must be signed");

        let manifest = UpdateManifest {
            version: feed.version.clone(),
            channel: UpdateChannel::Stable,
            url: entry.url.clone(),
            sha256: entry.checksum.clone(),
            size_bytes: entry.size_bytes,
            release_notes: None,
            min_version: None,
        };
        let sig_bytes = base64::engine::general_purpose::STANDARD
            .decode(signature_b64)
            .unwrap();
        let sig_array: [u8; 64] = sig_bytes.as_slice().try_into().unwrap();
        let signature = super::ed25519_signature(&sig_array);
        let key = verifying_key_from_hex(&public_hex).unwrap();
        assert!(verify_manifest(&manifest, &signature, &key));

        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }

    #[test]
    fn verify_accepts_feed_produced_by_build_feed() {
        let artifact = temp_artifact(b"verify me end to end", "Test-macos.zip");
        let dir = artifact.parent().unwrap().to_path_buf();
        let config = dist_config_with_artifact("https://dl.kael.dev/feed", "3.1.4");
        let (private_hex, public_hex) = generate_keypair();

        let feed = build_feed(
            &config,
            std::slice::from_ref(&artifact),
            Some(&private_hex),
            false,
        )
        .unwrap();
        let feed_path = dir.join("update-feed.json");
        fs::write(&feed_path, serde_json::to_string_pretty(&feed).unwrap()).unwrap();

        verify(&feed_path, &public_hex).unwrap();

        // A feed signed by a different key must be rejected.
        let (_, other_public) = generate_keypair();
        assert!(verify(&feed_path, &other_public).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_signing_key_is_an_error() {
        let artifact = temp_artifact(b"x", "Test-macos.zip");
        let config = dist_config_with_artifact("https://dl.kael.dev/feed", "1.0.0");
        let result = build_feed(
            &config,
            std::slice::from_ref(&artifact),
            Some("not-hex"),
            false,
        );
        assert!(result.is_err());
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }

    #[test]
    fn artifact_urls_use_the_separate_base_and_encode_file_names() {
        let artifact = temp_artifact(b"payload", "Test App-macos #1.zip");
        let config = dist_config_with_artifact("https://dl.kael.dev/releases/1.2.3/", "1.2.3");
        let feed = build_feed(&config, std::slice::from_ref(&artifact), None, false).unwrap();

        assert_eq!(feed.url, "https://updates.kael.dev/feed.json");
        assert_eq!(
            feed.platforms[0].url,
            "https://dl.kael.dev/releases/1.2.3/Test%20App-macos%20%231.zip"
        );
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }

    #[test]
    fn directories_and_duplicate_platform_packages_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let app_bundle = directory.path().join("Test.app");
        fs::create_dir(&app_bundle).unwrap();
        let config = dist_config_with_artifact("https://dl.kael.dev/releases", "1.2.3");
        let directory_result = build_feed(&config, std::slice::from_ref(&app_bundle), None, false);
        assert!(directory_result.is_err());

        let dmg = directory.path().join("Test.dmg");
        let zip = directory.path().join("Test-macos.zip");
        fs::write(&dmg, b"dmg").unwrap();
        fs::write(&zip, b"zip").unwrap();
        assert!(build_feed(&config, &[dmg, zip], None, false).is_err());
    }

    #[test]
    fn only_metadata_simulation_allows_missing_planned_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        let planned = directory.path().join("Test-macos.dmg");
        let config = dist_config_with_artifact("https://dl.kael.dev/releases", "1.2.3");

        assert!(build_feed(&config, std::slice::from_ref(&planned), None, false).is_err());
        let feed = build_feed(&config, std::slice::from_ref(&planned), None, true).unwrap();
        assert_eq!(feed.platforms[0].checksum, "0".repeat(64));
        assert_eq!(feed.platforms[0].size_bytes, 0);
    }

    #[test]
    fn strict_publish_readiness_requires_matching_configured_keys_and_real_bytes() {
        let artifact = temp_artifact(b"release payload", "Test-macos.dmg");
        let (private_hex, public_hex) = generate_keypair();
        let mut config = dist_config_with_artifact("https://dl.kael.dev/releases", "1.2.3");

        assert!(
            validate_publish_readiness(&config, std::slice::from_ref(&artifact), &private_hex)
                .is_err()
        );

        config.updater.as_mut().unwrap().public_key = Some(public_hex);
        assert!(
            validate_publish_readiness(&config, std::slice::from_ref(&artifact), &private_hex)
                .is_ok()
        );

        let (wrong_private_hex, _) = generate_keypair();
        assert!(
            validate_publish_readiness(
                &config,
                std::slice::from_ref(&artifact),
                &wrong_private_hex
            )
            .is_err()
        );
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }

    #[test]
    fn strict_publish_readiness_rejects_empty_artifacts() {
        let artifact = temp_artifact(b"", "Test-macos.dmg");
        let (private_hex, public_hex) = generate_keypair();
        let mut config = dist_config_with_artifact("https://dl.kael.dev/releases", "1.2.3");
        config.updater.as_mut().unwrap().public_key = Some(public_hex);

        assert!(
            validate_publish_readiness(&config, std::slice::from_ref(&artifact), &private_hex)
                .is_err()
        );
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }
}
