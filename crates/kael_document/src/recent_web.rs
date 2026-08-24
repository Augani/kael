//! Browser recent-document compatibility type.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A recently opened native-path document.
///
/// Browser controllers expose [`crate::StoredDocument`] instead because an origin-scoped
/// document identifier must not masquerade as a native path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentDocument {
    /// The normalized native document path.
    pub path: PathBuf,
    /// When the document was last opened or saved.
    pub last_opened_at_millis: u64,
}
