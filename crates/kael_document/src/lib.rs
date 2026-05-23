//! Document lifecycle services for Kael.

#![deny(missing_docs)]

/// Autosave configuration and helpers.
pub mod autosave;
/// Document controller and handle types.
pub mod document;
/// File type matching.
pub mod file_type;
/// Platform integration metadata.
pub mod platform;
/// Recent document storage.
pub mod recent;
/// Document version storage.
pub mod versions;

pub use anyhow::Result;
pub use autosave::{AutosaveConfig, AutosaveLocation};
pub use document::{Document, DocumentController, DocumentHandle};
pub use file_type::FileType;
pub use recent::RecentDocument;
pub use versions::DocumentVersion;
