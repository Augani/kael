use crate::platform::FocusedWindowInfo;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use objc2_app_kit::NSWorkspace;
use std::ffi::c_void;

const MAX_APP_NAME_UTF16: usize = 1_024;
const MAX_BUNDLE_ID_UTF16: usize = 1_024;
const MAX_WINDOW_TITLE_UTF16: usize = 4_096;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementGetTypeID() -> core_foundation_sys::base::CFTypeID;
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: core_foundation::string::CFStringRef,
        value: *mut *mut c_void,
    ) -> i32;
}

pub fn get_focused_window_info() -> Option<FocusedWindowInfo> {
    let workspace = NSWorkspace::sharedWorkspace();
    let frontmost_app = workspace.frontmostApplication()?;
    let native_app_name = frontmost_app.localizedName()?;
    if native_app_name.length() > MAX_APP_NAME_UTF16 {
        return None;
    }
    let app_name = native_app_name.to_string();
    let bundle_id = frontmost_app
        .bundleIdentifier()
        .and_then(|id| (id.length() <= MAX_BUNDLE_ID_UTF16).then(|| id.to_string()));
    let pid = frontmost_app.processIdentifier();
    let pid = u32::try_from(pid).ok()?;
    let window_title = get_window_title_via_accessibility(pid).unwrap_or_default();

    Some(FocusedWindowInfo {
        app_name,
        window_title,
        bundle_id,
        pid: Some(pid),
    })
}

fn get_window_title_via_accessibility(pid: u32) -> Option<String> {
    let pid = i32::try_from(pid).ok()?;
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
            if !window_value.is_null() {
                core_foundation::base::CFRelease(window_value as _);
            }
            return None;
        }
        if core_foundation_sys::base::CFGetTypeID(window_value.cast()) != AXUIElementGetTypeID() {
            core_foundation::base::CFRelease(window_value as _);
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
            if !title_value.is_null() {
                core_foundation::base::CFRelease(title_value as _);
            }
            return None;
        }
        let title_ref = title_value as core_foundation::string::CFStringRef;
        if core_foundation_sys::base::CFGetTypeID(title_value.cast())
            != core_foundation_sys::string::CFStringGetTypeID()
            || core_foundation_sys::string::CFStringGetLength(title_ref) < 0
            || usize::try_from(core_foundation_sys::string::CFStringGetLength(title_ref)).ok()?
                > MAX_WINDOW_TITLE_UTF16
        {
            core_foundation::base::CFRelease(title_value as _);
            return None;
        }

        let cf_title = core_foundation::string::CFString::wrap_under_create_rule(title_ref);
        Some(cf_title.to_string())
    }
}
