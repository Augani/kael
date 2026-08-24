mod accessibility_tests;
mod atlas_policy_properties;
mod auxiliary_exec_properties;
mod clipboard_properties;
mod crash_report_properties;
mod event_dispatch_properties;
mod file_watcher_properties;
mod font_feature_properties;
#[cfg(target_os = "linux")]
mod linux_accessibility_tests;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod linux_dialog_tests;
mod security_tests;
mod session_state_properties;
mod smoke_tests;
mod tab_management_properties;
mod text_layout_properties;
mod trace_event_properties;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn explicit_linux_backend_selection_is_deterministic() {
    assert_eq!(super::explicit_linux_backend("x11"), Some("X11"));
    assert_eq!(super::explicit_linux_backend(" WAYLAND "), Some("Wayland"));
    assert_eq!(super::explicit_linux_backend("headless"), Some("Headless"));
    assert_eq!(super::explicit_linux_backend("auto"), None);
    assert_eq!(
        super::explicit_linux_backend("unknown"),
        Some("Invalid KAEL_LINUX_BACKEND value")
    );
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn linux_backend_prefers_native_wayland_when_available() {
    assert_eq!(super::select_linux_backend(None, true, true), "Wayland");
    assert_eq!(
        super::select_linux_backend(Some("auto"), true, true),
        "Wayland"
    );
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn explicit_linux_backend_overrides_native_preference() {
    assert_eq!(
        super::select_linux_backend(Some("wayland"), true, true),
        "Wayland"
    );
    assert_eq!(super::select_linux_backend(Some("x11"), true, true), "X11");
    assert_eq!(
        super::select_linux_backend(Some("headless"), true, true),
        "Headless"
    );
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
#[test]
fn linux_backend_never_invents_a_display() {
    assert_eq!(super::select_linux_backend(None, true, false), "Wayland");
    assert_eq!(super::select_linux_backend(None, false, true), "X11");
    assert_eq!(super::select_linux_backend(None, false, false), "Headless");
}
