//! Browser document-version metadata.

use serde::{Deserialize, Serialize};

/// A stored document version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentVersion {
    /// The unique version identifier.
    pub id: String,
    /// When the version was created.
    pub created_at_millis: u64,
    /// The SHA-256 digest of the stored bytes.
    pub digest: String,
    /// The serialized byte length for the version.
    pub size_bytes: usize,
}
