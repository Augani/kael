//! Browser integration metadata for document services.

/// Platform integration metadata for document services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformDocumentSupport {
    /// The backend used for recent-document integration.
    pub recent_documents_backend: &'static str,
    /// The backend used for file-association registration.
    pub file_association_backend: &'static str,
    /// The backend used for autosave placement.
    pub autosave_backend: &'static str,
}

/// Returns browser document integration metadata.
pub const fn support() -> PlatformDocumentSupport {
    PlatformDocumentSupport {
        recent_documents_backend: "indexeddb-document-identifiers",
        file_association_backend: "file-picker-byte-bridge",
        autosave_backend: "indexeddb",
    }
}
