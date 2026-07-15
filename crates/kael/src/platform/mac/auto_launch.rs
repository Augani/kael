use anyhow::Result;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

fn sm_app_service_main_app() -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"SMAppService")?;
    let service: *mut AnyObject = unsafe { msg_send![class, mainApp] };
    (!service.is_null()).then_some(service)
}

pub fn set_auto_launch(app_id: &str, enabled: bool) -> Result<()> {
    anyhow::ensure!(valid_app_id(app_id), "invalid auto-launch app identifier");
    let Some(service) = sm_app_service_main_app() else {
        return Err(anyhow::anyhow!(
            "SMAppService not available (requires macOS 13+)"
        ));
    };

    let mut error: *mut AnyObject = std::ptr::null_mut();
    let success: bool = if enabled {
        unsafe { msg_send![service, registerAndReturnError: &mut error] }
    } else {
        unsafe { msg_send![service, unregisterAndReturnError: &mut error] }
    };

    if !success {
        return Err(anyhow::anyhow!(
            "Failed to {}register auto-launch",
            if enabled { "" } else { "un" }
        ));
    }
    Ok(())
}

pub fn is_auto_launch_enabled(app_id: &str) -> bool {
    if !valid_app_id(app_id) {
        return false;
    }
    let Some(service) = sm_app_service_main_app() else {
        return false;
    };
    let status: isize = unsafe { msg_send![service, status] };
    status == 1
}

fn valid_app_id(app_id: &str) -> bool {
    !app_id.is_empty()
        && app_id.len() <= 255
        && app_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_launch_identifiers_are_bounded_path_safe_tokens() {
        assert!(valid_app_id("com.example.App"));
        assert!(!valid_app_id(""));
        assert!(!valid_app_id("../app"));
        assert!(!valid_app_id(&"x".repeat(256)));
    }
}
