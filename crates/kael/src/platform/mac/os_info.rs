use crate::OsInfo;
use objc2_foundation::{NSLocale, NSProcessInfo, NSString};
use std::ffi::CStr;

pub fn get_os_info() -> OsInfo {
    let version = bounded_string(
        &NSProcessInfo::processInfo().operatingSystemVersionString(),
        4_096,
    );
    let locale = bounded_string(&NSLocale::currentLocale().localeIdentifier(), 1_024);

    let mut hostname_buf = [0u8; 256];
    let hostname = unsafe {
        if libc::gethostname(
            hostname_buf.as_mut_ptr() as *mut libc::c_char,
            hostname_buf.len(),
        ) == 0
        {
            hostname_buf[hostname_buf.len() - 1] = 0;
            CStr::from_ptr(hostname_buf.as_ptr() as *const libc::c_char)
                .to_string_lossy()
                .to_string()
        } else {
            String::new()
        }
    };

    OsInfo {
        name: "macOS".into(),
        version: version.into(),
        arch: std::env::consts::ARCH.into(),
        locale: locale.into(),
        hostname: hostname.into(),
    }
}

fn bounded_string(value: &NSString, max_utf16: usize) -> String {
    if value.length() <= max_utf16 {
        value.to_string()
    } else {
        String::new()
    }
}
