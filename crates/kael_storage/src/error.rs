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
    /// A SQLite operation failed.
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),
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
    /// The configured migrations were not in strictly increasing order.
    #[error("migrations must be sorted in strictly increasing version order")]
    InvalidMigrationOrder,
    /// The configured migrations contained a duplicate version.
    #[error("duplicate migration version {0}")]
    DuplicateMigrationVersion(u32),
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
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

/// The result type used by the storage crate.
pub type Result<T> = std::result::Result<T, Error>;