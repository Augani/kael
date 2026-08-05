#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

mod error;
mod subscription;

/// Database services.
pub mod database;
/// Key-value storage services.
pub mod kv;
/// Database migration services.
pub mod migration;
/// Platform paths and backend descriptions for storage services.
pub mod platform;

pub use database::{Database, FromRow, Transaction};
pub use error::{Error, Result};
pub use kv::{JsonKvStore, KvStore, PlatformKvStore, SqliteKvStore};
pub use migration::Migration;
pub use platform::StoragePaths;
pub use subscription::Subscription;

/// Returns the current storage backend label for the active target.
pub const fn backend_name() -> &'static str {
    platform::backend_name()
}

#[cfg(test)]
mod tests {
    use super::backend_name;

    #[test]
    fn backend_name_is_available() {
        assert!(!backend_name().is_empty());
    }
}
