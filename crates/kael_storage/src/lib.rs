#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

#[cfg(all(test, target_arch = "wasm32", target_os = "unknown"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

mod blob;
mod error;
mod subscription;

/// Database services.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod database;
/// Database compatibility surface for browser WebAssembly.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "database_web.rs"]
pub mod database;
/// Key-value storage services.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod kv;
/// Key-value storage services backed by browser `localStorage`.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "kv_web.rs"]
pub mod kv;
/// Database migration services.
pub mod migration;
/// Platform paths and backend descriptions for storage services.
pub mod platform;

pub use blob::BlobStore;
pub use database::{Database, FromRow, Transaction};
pub use error::{Error, Result};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use kv::{BrowserKvStore, KvStore, PlatformKvStore};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
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
