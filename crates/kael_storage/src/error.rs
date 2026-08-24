//! Error types for platform storage services.

use std::path::PathBuf;

use thiserror::Error;

/// The error type used by the storage crate.
#[derive(Debug, Error)]
pub enum Error {
    /// An I/O operation failed.
    #[error("i/o error at {path}: {source}")]
    Io {
        /// The path associated with the failed operation.
        path: PathBuf,
        /// The source I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A file replacement completed, but syncing its parent directory failed.
    ///
    /// The new value is visible to this process, but may not survive a system
    /// crash because the directory entry could not be durably flushed.
    #[error("data was committed at {path}, but syncing its parent directory failed: {source}")]
    DurabilityUncertain {
        /// The path that was atomically replaced.
        path: PathBuf,
        /// The source directory-sync error.
        #[source]
        source: std::io::Error,
    },
    /// A SQLite operation failed.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),
    /// A browser storage API was unavailable or rejected an operation.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    #[error("browser storage operation `{operation}` failed: {message}")]
    BrowserStorage {
        /// The browser API operation that failed.
        operation: &'static str,
        /// The browser-provided diagnostic, if one was available.
        message: String,
    },
    /// SQLite is not part of the browser storage backend.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    #[error(
        "SQLite operation `{operation}` is unavailable in browser WebAssembly; use PlatformKvStore or BlobStore"
    )]
    BrowserSqlUnsupported {
        /// The requested SQLite operation.
        operation: &'static str,
    },
    /// A native file path was requested from an origin-scoped browser store.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    #[error("native storage paths are unavailable in browser WebAssembly")]
    BrowserPathUnsupported,
    /// Serializing a JSON payload failed.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    /// Serializing a specific key-value entry failed.
    #[error("failed to serialize key `{key}`: {source}")]
    SerializeValue {
        /// The key that failed to serialize.
        key: String,
        /// The source serialization error.
        #[source]
        source: serde_json::Error,
    },
    /// Deserializing a specific key-value entry failed.
    #[error("failed to deserialize key `{key}`: {source}")]
    DeserializeValue {
        /// The key that failed to deserialize.
        key: String,
        /// The source serialization error.
        #[source]
        source: serde_json::Error,
    },
    /// A required environment variable for path resolution was missing.
    #[error("required environment variable was not set: {0}")]
    MissingEnvironmentVariable(&'static str),
    /// An application or database identifier was not a portable file name.
    #[error("invalid storage identifier: {0:?}")]
    InvalidStorageIdentifier(String),
    /// A key was empty, excessive, or contained control characters.
    #[error("storage key must be non-empty, at most 4096 bytes, and free of control characters")]
    InvalidStorageKey,
    /// A JSON store exceeded its bounded on-disk size.
    #[error("JSON store is {actual} bytes, exceeding the {limit} byte limit")]
    JsonStoreTooLarge {
        /// Observed or serialized size.
        actual: u64,
        /// Maximum accepted size.
        limit: u64,
    },
    /// A binary value exceeded the bounded per-record size.
    #[error("blob is {actual} bytes, exceeding the {limit} byte limit")]
    BlobTooLarge {
        /// Observed byte length.
        actual: u64,
        /// Maximum accepted byte length.
        limit: u64,
    },
    /// Migration version zero is reserved for an unmigrated database.
    #[error("migration version zero is not valid")]
    InvalidMigrationVersion,
    /// The configured migrations were not in strictly increasing order.
    #[error("migrations must be sorted in strictly increasing version order")]
    InvalidMigrationOrder,
    /// The configured migrations contained a duplicate version.
    #[error("duplicate migration version {0}")]
    DuplicateMigrationVersion(u32),
    /// The database schema is newer than the newest supplied migration.
    #[error(
        "database schema version {current} is newer than the latest configured migration {latest}"
    )]
    DatabaseVersionNewer {
        /// Version recorded in the database.
        current: u32,
        /// Highest version in the configured migration list.
        latest: u32,
    },
    /// A rollback was requested for a migration without a `down` script.
    #[error("migration {0} cannot be rolled back because it does not define a down script")]
    MissingDownMigration(u32),
    /// A query that expected one row returned none.
    #[error("query returned no rows")]
    RowNotFound,
    /// A query that expected one row returned more than one.
    #[error("query returned {0} rows, expected exactly one")]
    UnexpectedRowCount(usize),
    /// A rollback target was above the current migration version.
    #[error("cannot roll back to version {target} because the current version is {current}")]
    InvalidRollbackTarget {
        /// The requested rollback target.
        target: u32,
        /// The current database version.
        current: u32,
    },
}

impl Error {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// The result type used by the storage crate.
pub type Result<T> = std::result::Result<T, Error>;
