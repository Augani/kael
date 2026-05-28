use anyhow::Result;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};

fn sm_app_service_main_app() -> Option<*mut AnyObject> {
    let class = AnyClass::get(c"SMAppService")?;
    let service: *mut AnyObject = unsafe { msg_send![class, mainApp] };
    (!service.is_null()).then_some(service)
}

pub fn set_auto_launch(_app_id: &str, enabled: bool) -> Result<()> {
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

pub fn is_auto_launch_enabled(_app_id: &str) -> bool {
    let Some(service) = sm_app_service_main_app() else {
        return false;
    };
    let status: isize = unsafe { msg_send![service, status] };
    status == 1
}
