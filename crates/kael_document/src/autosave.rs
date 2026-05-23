//! Autosave configuration and persistence helpers.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result};
use sha2::{Digest, Sha256};

/// Configuration for document autosave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutosaveConfig {
    /// The nominal autosave interval.
    pub interval: Duration,
    /// Where autosave snapshots are written.
    pub location: AutosaveLocation,
}

impl Default for AutosaveConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            location: AutosaveLocation::SystemTemp,
        }
    }
}

/// The storage location used for autosave snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutosaveLocation {
    /// Stores autosaves next to the target file using a hidden sibling file.
    AdjacentToFile,
    /// Stores autosaves in the system temporary directory.
    SystemTemp,
    /// Stores autosaves under a custom directory.
    Custom(PathBuf),
}

pub(crate) fn autosave_path(
    app_id: &str,
    location: &AutosaveLocation,
    file_path: Option<&Path>,
    document_name: &str,
) -> PathBuf {
    match location {
        AutosaveLocation::AdjacentToFile => {
            if let Some(path) = file_path {
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(document_name);
                let autosave_name = format!(".{file_name}.autosave");
                return path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(autosave_name);
            }

            system_temp_root(app_id).join(format!("{}.autosave", sanitize_name(document_name)))
        }
        AutosaveLocation::SystemTemp => {
            system_temp_root(app_id).join(system_autosave_name(file_path, document_name))
        }
        AutosaveLocation::Custom(root) => root.join(system_autosave_name(file_path, document_name)),
    }
}

pub(crate) fn load_autosave(path: &Path) -> Result<Option<Vec<u8>>> {
    if !path.exists() {
        return Ok(None);
    }

    std::fs::read(path)
        .map(Some)
        .with_context(|| format!("failed to read autosave snapshot from {}", path.display()))
}

pub(crate) fn write_autosave(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create autosave directory {}", parent.display()))?;
    }

    write_bytes_atomically(path, bytes)
}

pub(crate) fn clear_autosave(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to remove autosave snapshot {}", path.display())),
    }
}

pub(crate) fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, bytes)
        .with_context(|| format!("failed to write temporary file {}", temp_path.display()))?;
    std::fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to finalize temporary file from {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn system_temp_root(app_id: &str) -> PathBuf {
    std::env::temp_dir().join(app_id).join("autosave")
}

fn system_autosave_name(file_path: Option<&Path>, document_name: &str) -> String {
    let basis = file_path
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| document_name.to_string());
    let digest = digest_hex(basis.as_bytes());
    format!("{}-{}.autosave", sanitize_name(document_name), &digest[..12])
}

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => character,
            _ => '_',
        })
        .collect()
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}