//! Autosave configuration and persistence helpers.

use std::{
    fs::OpenOptions,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context as _, Result};
use sha2::{Digest, Sha256};

pub(crate) const MAX_DOCUMENT_BYTES: u64 = 256 * 1024 * 1024;
const AUTOSAVE_MAGIC: &[u8] = b"KAEL-AUTOSAVE\0";
const AUTOSAVE_VERSION: u8 = 1;
const DIGEST_BYTES: usize = 32;
const MAX_AUTOSAVE_BYTES: u64 =
    MAX_DOCUMENT_BYTES + AUTOSAVE_MAGIC.len() as u64 + 2 + DIGEST_BYTES as u64;

pub(crate) type ContentDigest = [u8; DIGEST_BYTES];

pub(crate) struct LoadedAutosave {
    pub(crate) bytes: Vec<u8>,
    pub(crate) baseline_digest: Option<ContentDigest>,
    pub(crate) legacy: bool,
}

/// Configuration for document autosave.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutosaveConfig {
    /// Where autosave snapshots are written.
    pub location: AutosaveLocation,
}

impl AutosaveConfig {
    /// Creates autosave configuration for a recovery location.
    pub const fn new(location: AutosaveLocation) -> Self {
        Self { location }
    }
}

impl Default for AutosaveConfig {
    fn default() -> Self {
        Self::new(AutosaveLocation::SystemTemp)
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
                let autosave_name = format!(
                    ".{}-{}.autosave",
                    sanitize_name(file_name),
                    path_digest_hex(path)
                );
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

pub(crate) fn load_autosave(path: &Path) -> Result<Option<LoadedAutosave>> {
    if !path.exists() {
        return Ok(None);
    }

    let mut bytes = read_regular_file_bounded(path, MAX_AUTOSAVE_BYTES, "autosave snapshot")?;
    let Some(remainder) = bytes.strip_prefix(AUTOSAVE_MAGIC) else {
        ensure_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
        return Ok(Some(LoadedAutosave {
            bytes,
            baseline_digest: None,
            legacy: true,
        }));
    };
    anyhow::ensure!(
        remainder.len() >= 2,
        "autosave snapshot {} has a truncated header",
        path.display()
    );
    anyhow::ensure!(
        remainder[0] == AUTOSAVE_VERSION,
        "autosave snapshot {} uses unsupported format version {}",
        path.display(),
        remainder[0]
    );
    let (baseline_digest, payload_offset) = match remainder[1] {
        0 => (None, 2),
        1 => {
            anyhow::ensure!(
                remainder.len() >= 2 + DIGEST_BYTES,
                "autosave snapshot {} has a truncated baseline digest",
                path.display()
            );
            let mut digest = [0; DIGEST_BYTES];
            digest.copy_from_slice(&remainder[2..2 + DIGEST_BYTES]);
            (Some(digest), 2 + DIGEST_BYTES)
        }
        flag => anyhow::bail!(
            "autosave snapshot {} has invalid baseline flag {flag}",
            path.display()
        ),
    };
    let payload_start = AUTOSAVE_MAGIC.len() + payload_offset;
    bytes.drain(..payload_start);
    ensure_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
    Ok(Some(LoadedAutosave {
        bytes,
        baseline_digest,
        legacy: false,
    }))
}

pub(crate) fn write_autosave(
    path: &Path,
    bytes: &[u8],
    baseline_digest: Option<&ContentDigest>,
) -> Result<()> {
    ensure_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_real_directory(parent, "autosave directory")?;
    }

    let mut header = Vec::with_capacity(AUTOSAVE_MAGIC.len() + 2 + DIGEST_BYTES);
    header.extend_from_slice(AUTOSAVE_MAGIC);
    header.push(AUTOSAVE_VERSION);
    if let Some(digest) = baseline_digest {
        header.push(1);
        header.extend_from_slice(digest);
    } else {
        header.push(0);
    }
    write_chunks_atomically_with_privacy(path, &[&header, bytes], true, MAX_AUTOSAVE_BYTES)
}

pub(crate) fn clear_autosave(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to remove autosave snapshot {}", path.display())),
    }
}

pub(crate) fn legacy_snapshot_is_newer(snapshot: &Path, document: &Path) -> Result<bool> {
    let snapshot_modified = std::fs::symlink_metadata(snapshot)
        .with_context(|| format!("failed to inspect legacy autosave {}", snapshot.display()))?
        .modified()
        .with_context(|| {
            format!(
                "legacy autosave {} has no modification time",
                snapshot.display()
            )
        })?;
    let document_modified = std::fs::symlink_metadata(document)
        .with_context(|| format!("failed to inspect document {}", document.display()))?
        .modified()
        .with_context(|| format!("document {} has no modification time", document.display()))?;
    Ok(snapshot_modified > document_modified)
}

pub(crate) fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    write_chunks_atomically_with_privacy(path, &[bytes], false, MAX_DOCUMENT_BYTES)
}

pub(crate) fn write_private_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    write_chunks_atomically_with_privacy(path, &[bytes], true, MAX_DOCUMENT_BYTES)
}

fn write_chunks_atomically_with_privacy(
    path: &Path,
    chunks: &[&[u8]],
    private: bool,
    max_bytes: u64,
) -> Result<()> {
    let total_bytes = chunks.iter().try_fold(0u64, |total, chunk| {
        total.checked_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX))
    });
    anyhow::ensure!(
        total_bytes.is_some_and(|total| total <= max_bytes),
        "atomic write payload exceeds the {max_bytes} byte limit"
    );
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_real_directory(parent, "write parent directory")?;
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
    if let Err(error) = configure_temporary_permissions(&file, existing_permissions, private) {
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
        for chunk in chunks {
            file.write_all(chunk)?;
        }
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

pub(crate) fn ensure_real_directory(path: &Path, kind: &str) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create {kind} {}", path.display()))?;
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {kind} {}", path.display()))?;
    anyhow::ensure!(
        metadata.file_type().is_dir(),
        "{kind} {} is not a real directory",
        path.display()
    );
    Ok(())
}

fn configure_temporary_permissions(
    file: &std::fs::File,
    existing_permissions: Option<std::fs::Permissions>,
    private: bool,
) -> std::io::Result<()> {
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt as _;
        return file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }

    #[cfg(not(unix))]
    let _ = private;

    if let Some(permissions) = existing_permissions {
        file.set_permissions(permissions)?;
    }
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

    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_WRITE_THROUGH, ReplaceFileW};

    let replaced = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replacement = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are owned, NUL-terminated UTF-16 buffers that remain alive for the call;
    // the optional backup and reserved pointers are intentionally null.
    let replaced_ok = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_WRITE_THROUGH,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced_ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
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
        .join(format!("{sanitized}-{digest}"))
        .join("autosave")
}

fn system_autosave_name(file_path: Option<&Path>, document_name: &str) -> String {
    let digest = file_path.map_or_else(
        || digest_hex(format!("{}:{document_name}", std::process::id()).as_bytes()),
        path_digest_hex,
    );
    format!("{}-{digest}.autosave", sanitize_name(document_name))
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

pub(crate) fn content_digest(bytes: &[u8]) -> ContentDigest {
    Sha256::digest(bytes).into()
}

pub(crate) fn path_digest_hex(path: &Path) -> String {
    let mut digest = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        digest.update(path.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        for unit in path.as_os_str().encode_wide() {
            digest.update(unit.to_le_bytes());
        }
    }
    #[cfg(not(any(unix, windows)))]
    digest.update(path.to_string_lossy().as_bytes());
    digest_hex_output(digest.finalize().into())
}

fn digest_hex(bytes: &[u8]) -> String {
    digest_hex_output(content_digest(bytes))
}

fn digest_hex_output(digest: ContentDigest) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(DIGEST_BYTES * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn read_regular_file_bounded(
    path: &Path,
    max_bytes: u64,
    kind: &str,
) -> Result<Vec<u8>> {
    let path_metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {kind} at {}", path.display()))?;
    anyhow::ensure!(
        path_metadata.file_type().is_file(),
        "{kind} {} is not a regular file",
        path.display()
    );
    anyhow::ensure!(
        path_metadata.len() <= max_bytes,
        "{kind} exceeds the {max_bytes} byte limit"
    );

    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .with_context(|| format!("failed to open {kind} at {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect opened {kind} at {}", path.display()))?;
    anyhow::ensure!(
        metadata.is_file(),
        "{kind} {} is not a regular file",
        path.display()
    );
    anyhow::ensure!(
        metadata.len() <= max_bytes,
        "{kind} exceeds the {max_bytes} byte limit"
    );

    let initial_capacity = usize::try_from(metadata.len())
        .with_context(|| format!("{kind} is too large for this platform's address space"))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(initial_capacity)
        .with_context(|| format!("failed to reserve {kind} buffer"))?;
    let mut chunk = [0u8; 8 * 1024];
    loop {
        let bytes_read = file
            .read(&mut chunk)
            .with_context(|| format!("failed to read {kind} from {}", path.display()))?;
        if bytes_read == 0 {
            break;
        }
        let next_len = bytes
            .len()
            .checked_add(bytes_read)
            .context("bounded file length overflowed")?;
        anyhow::ensure!(
            u64::try_from(next_len).unwrap_or(u64::MAX) <= max_bytes,
            "{kind} exceeds the {max_bytes} byte limit"
        );
        bytes
            .try_reserve_exact(bytes_read)
            .with_context(|| format!("failed to grow {kind} buffer"))?;
        bytes.extend_from_slice(&chunk[..bytes_read]);
    }
    Ok(bytes)
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

    #[cfg(unix)]
    #[test]
    fn private_writes_use_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("recovery");

        write_private_bytes_atomically(&target, b"private").unwrap();

        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn autosave_envelopes_bind_recovery_to_the_saved_baseline() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("recovery");
        let baseline = content_digest(b"saved");

        write_autosave(&target, b"recovery", Some(&baseline)).unwrap();
        let loaded = load_autosave(&target).unwrap().unwrap();

        assert_eq!(loaded.bytes, b"recovery");
        assert_eq!(loaded.baseline_digest, Some(baseline));
        assert!(!loaded.legacy);
    }

    #[test]
    fn recovery_names_use_full_path_and_application_digests() {
        let first = autosave_path(
            "dev.kael.documents",
            &AutosaveLocation::SystemTemp,
            Some(Path::new("/tmp/first")),
            "document",
        );
        let second = autosave_path(
            "dev.kael.documents",
            &AutosaveLocation::SystemTemp,
            Some(Path::new("/tmp/second")),
            "document",
        );

        assert_ne!(first, second);
        let first_name = first.file_name().unwrap().to_string_lossy();
        let digest = first_name
            .strip_prefix("document-")
            .and_then(|name| name.strip_suffix(".autosave"))
            .unwrap();
        assert_eq!(digest.len(), 64);
    }
}
