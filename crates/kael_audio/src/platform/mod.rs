//! Platform metadata for audio services.

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

/// Platform metadata for audio support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformAudioSupport {
    /// The audio playback backend.
    pub playback_backend: &'static str,
    /// The session-management backend.
    pub session_backend: &'static str,
    /// The spatial-audio backend.
    pub spatial_backend: &'static str,
}

/// Returns the platform metadata for audio support.
pub const fn support() -> PlatformAudioSupport {
    imp::SUPPORT
}