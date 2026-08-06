//! Platform metadata for PDF services.

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

/// Platform metadata for the current PDF backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformPdfSupport {
    /// The parsing backend.
    pub parser_backend: &'static str,
    /// The schematic preview backend.
    pub schematic_preview_backend: &'static str,
    /// The annotation backend.
    pub annotation_backend: &'static str,
}

/// Returns the platform PDF support metadata.
pub const fn support() -> PlatformPdfSupport {
    imp::SUPPORT
}
