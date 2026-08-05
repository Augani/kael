//! Autosave configuration and persistence helpers.

use std::{
    fs::OpenOptions,
    io::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context as _, Result};
use sha2::{Digest, Sha256};

pub(crate) const MAX_DOCUMENT_BYTES: u64 = 256 * 1024 * 1024;

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
                let autosave_name = format!(".{}.autosave", sanitize_name(file_name));
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

    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect autosave snapshot at {}", path.display()))?;
    anyhow::ensure!(
        metadata.file_type().is_file(),
        "autosave snapshot {} is not a regular file",
        path.display()
    );
    ensure_size(metadata.len())?;
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read autosave snapshot from {}", path.display()))?;
    ensure_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
    Ok(Some(bytes))
}

pub(crate) fn write_autosave(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
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
    ensure_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory {}", parent.display()))?;
    }

    let existing_permissions = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file(),
                "atomic write target {} is not a regular file",
                path.display()
            );
            Some(metadata.permissions())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect write target {}", path.display()));
        }
    };

    let (temp_path, mut file) = create_temporary_file(path)?;
    if let Some(permissions) = existing_permissions
        && let Err(error) = file.set_permissions(permissions)
    {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to preserve permissions for temporary file {}",
                temp_path.display()
            )
        });
    }
    let result = (|| {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        Ok::<_, std::io::Error>(())
    })();
    drop(file);
    if let Err(error) = result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error)
            .with_context(|| format!("failed to write temporary file {}", temp_path.display()));
    }

    if let Err(error) = replace_file(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error).with_context(|| {
            format!(
                "failed to finalize temporary file from {} to {}",
                temp_path.display(),
                path.display()
            )
        });
    }
    sync_parent_directory(path)?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    std::fs::rename(temp_path, path)
}

#[cfg(windows)]
fn replace_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return std::fs::rename(temp_path, path);
    }

    let backup_path = temp_path.with_extension("replace-backup");
    std::fs::rename(path, &backup_path)?;
    if let Err(error) = std::fs::rename(temp_path, path) {
        let _ = std::fs::rename(&backup_path, path);
        return Err(error);
    }
    let _ = std::fs::remove_file(backup_path);
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .with_context(|| format!("failed to sync parent directory {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn create_temporary_file(path: &Path) -> Result<(PathBuf, std::fs::File)> {
    static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("autosave");
    loop {
        let unique_suffix = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp_path = path.with_file_name(format!(
            "{file_name}.{}.{}.tmp",
            std::process::id(),
            unique_suffix
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
        {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create temporary file {}", temp_path.display())
                });
            }
        }
    }
}

fn system_temp_root(app_id: &str) -> PathBuf {
    let sanitized = sanitize_name(app_id);
    let sanitized = if sanitized.is_empty() {
        "kael".to_string()
    } else {
        sanitized
    };
    let digest = digest_hex(app_id.as_bytes());
    std::env::temp_dir()
        .join(format!("{sanitized}-{}", &digest[..12]))
        .join("autosave")
}

fn system_autosave_name(file_path: Option<&Path>, document_name: &str) -> String {
    let basis = file_path
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| document_name.to_string());
    let digest = digest_hex(basis.as_bytes());
    format!(
        "{}-{}.autosave",
        sanitize_name(document_name),
        &digest[..12]
    )
}

fn sanitize_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => character,
            _ => '_',
        })
        .take(64)
        .collect::<String>();
    if sanitized.is_empty() {
        "document".to_string()
    } else {
        sanitized
    }
}

fn digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub(crate) fn ensure_size(size: u64) -> Result<()> {
    anyhow::ensure!(
        size <= MAX_DOCUMENT_BYTES,
        "document payload is {size} bytes, exceeding the {MAX_DOCUMENT_BYTES} byte limit"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_payload_lengths_without_allocating() {
        assert!(ensure_size(MAX_DOCUMENT_BYTES).is_ok());
        assert!(ensure_size(MAX_DOCUMENT_BYTES + 1).is_err());
        assert!(ensure_size(u64::MAX).is_err());
    }

    #[test]
    fn refuses_to_replace_a_directory() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        std::fs::create_dir(&target).unwrap();

        assert!(write_bytes_atomically(&target, b"data").is_err());
        assert!(target.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_to_replace_a_symbolic_link() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        std::fs::write(&target, b"original").unwrap();
        symlink(&target, &link).unwrap();

        assert!(write_bytes_atomically(&link, b"replacement").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        std::fs::write(&target, b"old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();

        write_bytes_atomically(&target, b"new").unwrap();

        assert_eq!(
            std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
