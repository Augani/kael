use crate::platform::FocusedWindowInfo;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use objc2_app_kit::NSWorkspace;
use std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: core_foundation::string::CFStringRef,
        value: *mut *mut c_void,
    ) -> i32;
}

pub fn get_focused_window_info() -> Option<FocusedWindowInfo> {
    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost_app = workspace.frontmostApplication()?;
    let app_name = frontmost_app.localizedName()?.to_string();
    let bundle_id = frontmost_app.bundleIdentifier().map(|id| id.to_string());
    let pid = frontmost_app.processIdentifier();
    let window_title = get_window_title_via_accessibility(pid).unwrap_or_default();

    Some(FocusedWindowInfo {
        app_name,
        window_title,
        bundle_id,
        pid: Some(pid as u32),
    })
}

fn get_window_title_via_accessibility(pid: i32) -> Option<String> {
    unsafe {
        let app_element = AXUIElementCreateApplication(pid);
        if app_element.is_null() {
            return None;
        }

        let focused_window_attr = CFString::new("AXFocusedWindow");
        let mut window_value: *mut c_void = std::ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            app_element,
            focused_window_attr.as_concrete_TypeRef(),
            &mut window_value,
        );
        core_foundation::base::CFRelease(app_element as _);

        if result != 0 || window_value.is_null() {
            return None;
        }

        let title_attr = CFString::new("AXTitle");
        let mut title_value: *mut c_void = std::ptr::null_mut();
        let result = AXUIElementCopyAttributeValue(
            window_value,
            title_attr.as_concrete_TypeRef(),
            &mut title_value,
        );
        core_foundation::base::CFRelease(window_value as _);

        if result != 0 || title_value.is_null() {
            return None;
        }

        let cf_title = core_foundation::string::CFString::wrap_under_create_rule(
            title_value as core_foundation::string::CFStringRef,
        );
        Some(cf_title.to_string())
    }
}
