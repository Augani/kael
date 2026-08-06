use super::PlatformDocumentSupport;
use std::path::PathBuf;

use anyhow::{Result, anyhow};

pub(crate) const SUPPORT: PlatformDocumentSupport = PlatformDocumentSupport {
    recent_documents_backend: "json",
    file_association_backend: "not-implemented",
    autosave_backend: "configured-path",
};

pub(crate) fn base_data_dir() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .ok_or_else(|| anyhow!("LOCALAPPDATA and APPDATA are not set"))
}
