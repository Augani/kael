//! Platform-specific storage path resolution.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// The resolved storage paths for an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePaths {
    /// The directory used for configuration-style files.
    pub config_dir: PathBuf,
    /// The directory used for data files.
    pub data_dir: PathBuf,
    /// The JSON file used by the default key-value store.
    pub preferences_path: PathBuf,
    /// The directory that stores SQLite database files.
    pub databases_dir: PathBuf,
}

/// Returns the backend label for the active target.
pub const fn backend_name() -> &'static str {
    imp::BACKEND_NAME
}

/// Resolves storage paths for the given application identifier.
pub fn storage_paths(app_id: &str) -> Result<StoragePaths> {
    let config_dir = imp::base_config_dir()?.join(app_id);
    let data_dir = imp::base_data_dir()?.join(app_id);

    Ok(StoragePaths {
        preferences_path: config_dir.join("preferences.json"),
        databases_dir: data_dir.join("databases"),
        config_dir,
        data_dir,
    })
}

/// Ensures the storage directories for the given application identifier exist.
pub fn ensure_storage_paths(app_id: &str) -> Result<StoragePaths> {
    let paths = storage_paths(app_id)?;

    create_dir_all(&paths.config_dir)?;
    create_dir_all(&paths.data_dir)?;
    create_dir_all(&paths.databases_dir)?;

    Ok(paths)
}

/// Resolves a database file path for the given application identifier and database name.
pub fn database_path(app_id: &str, name: &str) -> Result<PathBuf> {
    let paths = ensure_storage_paths(app_id)?;
    Ok(paths.databases_dir.join(database_file_name(name)))
}

fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|source| Error::io(path.to_path_buf(), source))
}

fn database_file_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' => character,
            _ => '_',
        })
        .collect::<String>();

    if sanitized.ends_with(".sqlite") || sanitized.ends_with(".sqlite3") {
        sanitized
    } else {
        format!("{sanitized}.sqlite3")
    }
}
