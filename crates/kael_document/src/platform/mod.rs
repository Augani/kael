//! Platform metadata for document services.

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