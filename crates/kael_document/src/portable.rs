//! Byte-oriented document exchange types shared by desktop and web adapters.

/// Serialized document bytes ready for a native file writer or browser download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentExport {
    /// Suggested file name, including an extension when the format defines one.
    pub file_name: String,
    /// MIME type reported by the selected [`crate::FileType`], when configured.
    pub mime_type: Option<&'static str>,
    /// Serialized bytes produced by the application's [`crate::Document`] implementation.
    pub bytes: Vec<u8>,
}

/// Metadata for a document stored in the cross-platform durable document store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDocument {
    /// Stable application-provided identifier used for later opens and saves.
    pub id: String,
    /// User-facing document name.
    pub name: String,
    /// Index into [`crate::Document::file_types`].
    pub file_type_index: usize,
    /// Serialized document size.
    pub size_bytes: usize,
    /// Last successful persistent save time, in Unix milliseconds.
    pub modified_at_millis: u64,
}
