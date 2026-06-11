//! Unit tests for the Linux AT-SPI2 accessibility provider.
//!
//! The provider is a thin wrapper over `accesskit_unix::Adapter`; tree
//! conversion is covered by the shared accessibility tests. Live D-Bus
//! integration requires a Linux session bus and is exercised on-device.

#[cfg(target_os = "linux")]
mod linux_tests {
    use crate::PermissionStatus;
    use crate::platform::linux::accessibility::*;

    #[test]
    fn test_accessibility_status_is_granted() {
        assert_eq!(accessibility_status(), PermissionStatus::Granted);
    }
}
