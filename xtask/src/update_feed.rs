use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::DistConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFeed {
    pub version: String,
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
}

pub struct FeedOptions {
    pub dry_run: bool,
}

pub fn run(
    config: &DistConfig,
    output: &Path,
    artifacts: &[PathBuf],
    options: &FeedOptions,
) -> Result<()> {
    let feed = build_feed(config, artifacts)?;

    let json = serde_json::to_string_pretty(&feed)?;

    if options.dry_run {
        println!("dry-run: would write update feed to {}", output.display());
        println!("{}", json);
    } else {
        fs::create_dir_all(output.parent().unwrap_or(Path::new(".")))?;
        fs::write(output, json)
            .with_context(|| format!("failed to write update feed: {}", output.display()))?;
    }

    println!("update feed generated: {}", output.display());
    Ok(())
}

fn build_feed(config: &DistConfig, artifacts: &[PathBuf]) -> Result<UpdateFeed> {
    let updater = config
        .updater
        .as_ref()
        .context("updater config is required to generate update feed")?;

    let platforms = artifacts
        .iter()
        .map(|artifact| {
            let platform = detect_platform(artifact);
            let url = format!(
                "{}/{}",
                updater.feed_url.trim_end_matches('/'),
                artifact.file_name().unwrap_or_default().to_string_lossy()
            );
            let checksum = if artifact.exists() {
                sha256_file(artifact)?
            } else {
                "0".repeat(64)
            };

            Ok(PlatformUpdate {
                platform,
                url,
                signature: None,
                checksum,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(UpdateFeed {
        version: config.version.clone(),
        url: updater.feed_url.clone(),
        notes_url: None,
        pub_date: now_rfc3339(),
        platforms,
    })
}

fn detect_platform(artifact: &Path) -> String {
    let name = artifact.to_string_lossy().to_lowercase();
    if name.ends_with(".app") || name.contains("macos") || name.contains("darwin") {
        "macos".to_string()
    } else if name.ends_with(".exe") || name.contains("windows") || name.contains("win32") {
        "windows".to_string()
    } else if name.ends_with(".appimage") || name.ends_with(".appdir") || name.contains("linux") {
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
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
