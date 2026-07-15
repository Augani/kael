use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use kael_release::update::{
    UpdateChannel, UpdateManifest, sign_manifest, signature_to_base64, signing_key_from_hex,
    verify_manifest,
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
    let feed = build_feed(config, artifacts, options.signing_key.as_deref())?;

    let json = serde_json::to_string_pretty(&feed)?;

    if options.dry_run {
        println!("dry-run: would write update feed to {}", output.display());
        println!("{}", json);
    } else {
        atomic_write(output, json.as_bytes())
            .with_context(|| format!("failed to write update feed: {}", output.display()))?;
    }

    println!("update feed generated: {}", output.display());
    Ok(())
}

pub fn verify(feed_path: &Path, signing_key_hex: &str) -> Result<()> {
    use base64::Engine as _;

    let signing_key = signing_key_from_hex(signing_key_hex)
        .context("KAEL_UPDATE_SIGNING_KEY is not a valid ed25519 key")?;
    let verifying_key = signing_key.verifying_key();

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

    let platforms = artifacts
        .iter()
        .map(|artifact| {
            let platform = detect_platform(artifact);
            let url = format!(
                "{}/{}",
                updater.feed_url.trim_end_matches('/'),
                artifact.file_name().unwrap_or_default().to_string_lossy()
            );
            let (checksum, size_bytes) = if artifact.exists() {
                let size = fs::metadata(artifact)
                    .with_context(|| format!("failed to stat artifact: {}", artifact.display()))?
                    .len();
                (sha256_file(artifact)?, size)
            } else {
                ("0".repeat(64), 0)
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

    fn dist_config_with_artifact(artifact_url: &str, version: &str) -> DistConfig {
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
                feed_url: artifact_url.to_string(),
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
        let feed = build_feed(&config, std::slice::from_ref(&artifact), None).unwrap();
        assert_eq!(feed.version, "1.2.3");
        assert_eq!(feed.channel, "stable");
        assert_eq!(feed.platforms.len(), 1);
        assert_eq!(feed.platforms[0].platform, "macos");
        assert_eq!(feed.platforms[0].size_bytes, 7);
        assert!(feed.platforms[0].signature.is_none());
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }

    #[test]
    fn signed_feed_roundtrips_through_verify_manifest() {
        let artifact = temp_artifact(b"some real bytes here", "Test-macos.zip");
        let config = dist_config_with_artifact("https://dl.kael.dev/feed", "2.0.0");
        let (private_hex, public_hex) = generate_keypair();

        let feed =
            build_feed(&config, std::slice::from_ref(&artifact), Some(&private_hex)).unwrap();
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
        let (private_hex, _public_hex) = generate_keypair();

        let feed =
            build_feed(&config, std::slice::from_ref(&artifact), Some(&private_hex)).unwrap();
        let feed_path = dir.join("update-feed.json");
        fs::write(&feed_path, serde_json::to_string_pretty(&feed).unwrap()).unwrap();

        verify(&feed_path, &private_hex).unwrap();

        // A feed signed by a different key must be rejected.
        let (other_private, _) = generate_keypair();
        assert!(verify(&feed_path, &other_private).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_signing_key_is_an_error() {
        let artifact = temp_artifact(b"x", "Test-macos.zip");
        let config = dist_config_with_artifact("https://dl.kael.dev/feed", "1.0.0");
        let result = build_feed(&config, std::slice::from_ref(&artifact), Some("not-hex"));
        assert!(result.is_err());
        let _ = fs::remove_dir_all(artifact.parent().unwrap());
    }
}
