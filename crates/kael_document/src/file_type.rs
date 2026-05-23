//! File type matching for document formats.

use std::path::Path;

/// A supported document file type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileType {
    /// The human-readable file type name.
    pub name: &'static str,
    /// The supported filename extensions, without leading dots.
    pub extensions: &'static [&'static str],
    /// The macOS uniform type identifier if one exists.
    pub uti: Option<&'static str>,
    /// The MIME type if one exists.
    pub mime: Option<&'static str>,
}

/// Returns the matching file type index for a path.
pub fn file_type_index_for_path(path: &Path, file_types: &[FileType]) -> Option<usize> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    file_types.iter().position(|file_type| {
        file_type
            .extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&extension))
    })
}

/// Returns the default file type index when any file types are configured.
pub fn default_file_type_index(file_types: &[FileType]) -> Option<usize> {
    (!file_types.is_empty()).then_some(0)
}
