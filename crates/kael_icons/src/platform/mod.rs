//! Platform bridges for icon services.

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos",
    target_os = "windows"
)))]
mod unsupported {
    pub const HAS_NATIVE_BRIDGE: bool = false;
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
use linux as imp;
#[cfg(target_os = "macos")]
use mac as imp;
#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "macos",
    target_os = "windows"
)))]
use unsupported as imp;
#[cfg(target_os = "windows")]
use windows as imp;

/// Returns whether the active target currently has an implemented native icon bridge.
pub const fn has_native_bridge() -> bool {
    imp::HAS_NATIVE_BRIDGE
}

/// Returns whether native icon lookup is available on this platform.
pub fn is_native_icon_available() -> bool {
    has_native_bridge()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_icon_availability_compiles() {
        assert_eq!(is_native_icon_available(), has_native_bridge());
    }
}
