//! Typed platform-boundary errors for document workflows.

use thiserror::Error;

/// A document operation that cannot be represented on the active platform.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DocumentPlatformError {
    /// Browsers do not grant arbitrary native-path access.
    #[error(
        "document operation `{operation}` requires a native path; import/export bytes or use the persistent browser store"
    )]
    NativePathUnavailable {
        /// The path-oriented operation that was requested.
        operation: &'static str,
    },
    /// The controller was created without opening its browser IndexedDB store.
    #[error(
        "browser document persistence is not configured; create the controller with DocumentController::new_persistent"
    )]
    PersistenceNotConfigured,
    /// A document has not yet been assigned a persistent browser identifier.
    #[error(
        "document has no persistent browser identifier; call DocumentHandle::save_stored first"
    )]
    MissingPersistentIdentity,
    /// A browser document identifier was empty, excessive, or contained control characters.
    #[error("invalid browser document identifier: {0:?}")]
    InvalidDocumentIdentifier(String),
    /// No persisted browser document exists for an identifier.
    #[error("unknown persisted browser document: {0:?}")]
    UnknownStoredDocument(String),
    /// Persisted browser data did not match Kael's bounded envelope format.
    #[error("persisted browser document is corrupt or uses an unsupported envelope")]
    InvalidStoredDocument,
}
