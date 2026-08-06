use super::PlatformDocumentSupport;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

pub(crate) const SUPPORT: PlatformDocumentSupport = PlatformDocumentSupport {
    recent_documents_backend: "json",
    file_association_backend: "not-implemented",
    autosave_backend: "configured-path",
};

pub(crate) fn base_data_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support"))
}
