#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

#[cfg(all(test, target_arch = "wasm32", target_os = "unknown"))]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

/// Autosave configuration and helpers.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod autosave;
/// Browser autosave configuration.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "autosave_web.rs"]
pub mod autosave;
/// Document controller and handle types.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod document;
/// Browser document controller and byte-oriented handle types.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "document_web.rs"]
pub mod document;
mod error;
/// File type matching.
pub mod file_type;
/// Platform integration metadata.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod platform;
/// Browser platform integration metadata.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "platform_web.rs"]
pub mod platform;
mod portable;
/// Recent document storage.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod recent;
/// Browser recent-document compatibility types.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "recent_web.rs"]
pub mod recent;
mod subscription;
/// Document version storage.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod versions;
/// Browser document-version types.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[path = "versions_web.rs"]
pub mod versions;

pub use anyhow::Result;
pub use autosave::{AutosaveConfig, AutosaveLocation};
pub use document::{Document, DocumentController, DocumentHandle};
pub use error::DocumentPlatformError;
pub use file_type::FileType;
pub use portable::{DocumentExport, StoredDocument};
pub use recent::RecentDocument;
pub use subscription::Subscription;
pub use versions::DocumentVersion;
