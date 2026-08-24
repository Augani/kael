//! Platform-specific storage path resolution.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

const MAX_IDENTIFIER_BYTES: usize = 200;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use web as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// The resolved storage paths for an application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePaths {
    /// The directory used for configuration-style files.
    pub config_dir: PathBuf,
    /// The directory used for data files.
    pub data_dir: PathBuf,
    /// The legacy JSON preferences file used by `JsonKvStore` and as a migration source for the platform store.
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
    validate_identifier(app_id)?;
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
    Ok(paths.databases_dir.join(database_file_name(name)?))
}

fn create_dir_all(path: &Path) -> Result<()> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let _ = path;
        return Ok(());
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    std::fs::create_dir_all(path).map_err(|source| Error::io(path.to_path_buf(), source))
}

fn database_file_name(name: &str) -> Result<String> {
    validate_identifier(name)?;
    let has_sqlite_extension = name.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("sqlite") || extension.eq_ignore_ascii_case("sqlite3")
    });

    if has_sqlite_extension {
        Ok(name.to_string())
    } else {
        Ok(format!("{name}.sqlite3"))
    }
}

fn validate_identifier(identifier: &str) -> Result<()> {
    let bytes = identifier.as_bytes();
    let valid = !bytes.is_empty()
        && bytes.len() <= MAX_IDENTIFIER_BYTES
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'_' | b'-'))
        && !matches!(bytes.first(), Some(b' ' | b'.'))
        && !matches!(bytes.last(), Some(b' ' | b'.'))
        && !is_windows_reserved_identifier(identifier);
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidStorageIdentifier(identifier.to_string()))
    }
}

pub(crate) fn validate_storage_identifier(identifier: &str) -> Result<()> {
    validate_identifier(identifier)
}

fn is_windows_reserved_identifier(identifier: &str) -> bool {
    let base = identifier
        .split('.')
        .next()
        .unwrap_or(identifier)
        .trim_end_matches([' ', '.']);
    if ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|reserved| base.eq_ignore_ascii_case(reserved))
    {
        return true;
    }

    let bytes = base.as_bytes();
    bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

#[cfg(test)]
mod tests {
    use super::{MAX_IDENTIFIER_BYTES, database_file_name, validate_identifier};

    #[test]
    fn database_names_cannot_escape_the_storage_directory() {
        assert!(database_file_name("").is_err());
        assert!(database_file_name(".").is_err());
        assert!(database_file_name("..").is_err());
        assert!(database_file_name("../outside").is_err());
        assert!(database_file_name("nested/database").is_err());
        assert!(database_file_name(r"nested\database").is_err());
        assert_eq!(database_file_name("main db").unwrap(), "main db.sqlite3");
        assert_eq!(database_file_name("main.SQLITE").unwrap(), "main.SQLITE");
        assert!(database_file_name("main?db").is_err());
        assert!(validate_identifier("../com.example.app").is_err());
        assert!(validate_identifier("/tmp/com.example.app").is_err());
        assert!(validate_identifier("com.example.app").is_ok());
    }

    #[test]
    fn identifiers_are_portable_to_windows() {
        for identifier in [
            "CON",
            "con.json",
            "CON .json",
            "PRN",
            "AUX.settings",
            "NUL",
            "COM1",
            "com9.sqlite3",
            "LPT1",
            "lpt9.data",
            "trailing.",
            "trailing ",
            "drive:name",
        ] {
            assert!(
                validate_identifier(identifier).is_err(),
                "accepted {identifier:?}"
            );
        }

        for identifier in ["com.example.product", "main db", "COM10", "LPT0"] {
            assert!(
                validate_identifier(identifier).is_ok(),
                "rejected {identifier:?}"
            );
        }
    }

    #[test]
    fn identifiers_have_a_bounded_path_component() {
        assert!(validate_identifier(&"a".repeat(MAX_IDENTIFIER_BYTES)).is_ok());
        assert!(validate_identifier(&"a".repeat(MAX_IDENTIFIER_BYTES + 1)).is_err());
    }
}
