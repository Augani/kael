//! Platform metadata for document services.

use std::{
    ffi::OsStr,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context as _, Result};

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(target_os = "windows")]
use windows as imp;

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

/// Returns the platform integration metadata for the active target.
pub const fn support() -> PlatformDocumentSupport {
    imp::SUPPORT
}

pub(crate) fn document_storage_root(app_id: &str) -> Result<PathBuf> {
    let mut components = Path::new(app_id).components();
    anyhow::ensure!(
        matches!(components.next(), Some(Component::Normal(component)) if component == OsStr::new(app_id))
            && components.next().is_none(),
        "invalid application identifier {app_id:?}"
    );

    let root = imp::base_data_dir()?.join(app_id).join("documents");
    std::fs::create_dir_all(&root)
        .with_context(|| format!("failed to create document storage root {}", root.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).with_context(
            || format!("failed to secure document storage root {}", root.display()),
        )?;
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::document_storage_root;

    #[test]
    fn application_identifiers_cannot_escape_the_data_directory() {
        assert!(document_storage_root("").is_err());
        assert!(document_storage_root(".").is_err());
        assert!(document_storage_root("..").is_err());
        assert!(document_storage_root("../outside").is_err());
        assert!(document_storage_root("nested/app").is_err());
    }
}
