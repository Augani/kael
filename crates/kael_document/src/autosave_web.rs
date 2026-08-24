//! Browser autosave configuration.

use std::path::PathBuf;

/// Configuration for document autosave.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutosaveConfig {
    /// Requested recovery placement.
    ///
    /// Browser controllers persist recovery bytes in their origin-scoped IndexedDB store because
    /// browsers do not expose arbitrary filesystem directories.
    pub location: AutosaveLocation,
}

impl AutosaveConfig {
    /// Creates autosave configuration for a recovery location.
    pub const fn new(location: AutosaveLocation) -> Self {
        Self { location }
    }
}

/// The requested storage location for autosave snapshots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AutosaveLocation {
    /// Uses IndexedDB beside the stored browser document.
    AdjacentToFile,
    /// Uses origin-scoped IndexedDB on browser WebAssembly.
    #[default]
    SystemTemp,
    /// Uses origin-scoped IndexedDB; the native path is retained only for shared configuration.
    Custom(PathBuf),
}
