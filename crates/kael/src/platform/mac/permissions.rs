use crate::platform::PermissionStatus;
use block2::RcBlock;
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFMutableDictionary;
use core_foundation::string::CFString;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicPtr, Ordering},
    },
};

use super::dispatcher::{dispatch_get_main_queue, dispatch_sys::dispatch_async_f};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: *mut AnyObject;
    static AVMediaTypeVideo: *mut AnyObject;
}

const AV_AUTHORIZATION_STATUS_NOT_DETERMINED: isize = 0;
const AV_AUTHORIZATION_STATUS_DENIED: isize = 2;
const AV_AUTHORIZATION_STATUS_AUTHORIZED: isize = 3;

pub fn accessibility_status() -> PermissionStatus {
    if unsafe { AXIsProcessTrusted() } {
        PermissionStatus::Granted
    } else {
        PermissionStatus::Denied
    }
}

pub fn request_accessibility_permission() {
    unsafe {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = CFBoolean::true_value();
        let mut options = CFMutableDictionary::new();
        options.set(key.as_CFTypeRef(), value.as_CFTypeRef());
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void);
    }
}

pub fn microphone_status() -> PermissionStatus {
    authorization_status(unsafe { AVMediaTypeAudio })
}

pub fn request_microphone_permission(callback: Box<dyn FnOnce(bool)>) {
    request_media_permission(unsafe { AVMediaTypeAudio }, callback);
}

pub fn camera_status() -> PermissionStatus {
    authorization_status(unsafe { AVMediaTypeVideo })
}

pub fn request_camera_permission(callback: Box<dyn FnOnce(bool)>) {
    request_media_permission(unsafe { AVMediaTypeVideo }, callback);
}

fn av_capture_device_class() -> Option<&'static AnyClass> {
    AnyClass::get(c"AVCaptureDevice")
}

fn authorization_status(media_type: *mut AnyObject) -> PermissionStatus {
    let Some(class) = av_capture_device_class() else {
        return PermissionStatus::Denied;
    };
    let status: isize = unsafe { msg_send![class, authorizationStatusForMediaType: media_type] };
    match status {
        AV_AUTHORIZATION_STATUS_AUTHORIZED => PermissionStatus::Granted,
        AV_AUTHORIZATION_STATUS_NOT_DETERMINED => PermissionStatus::NotDetermined,
        AV_AUTHORIZATION_STATUS_DENIED => PermissionStatus::Denied,
        _ => PermissionStatus::Denied,
    }
}

fn request_media_permission(media_type: *mut AnyObject, callback: Box<dyn FnOnce(bool)>) {
    let Some(class) = av_capture_device_class() else {
        return;
    };

    let callback_ptr = Arc::new(AtomicPtr::new(
        Box::into_raw(Box::new(callback)) as *mut c_void
    ));

    let block = RcBlock::new({
        let callback_ptr = Arc::clone(&callback_ptr);
        move |granted: Bool| {
            let callback = callback_ptr.swap(ptr::null_mut(), Ordering::AcqRel);
            if callback.is_null() {
                return;
            }

            let context = Box::new(PermissionRequestContext {
                callback,
                granted: granted.as_bool(),
            });

            unsafe {
                dispatch_async_f(
                    dispatch_get_main_queue(),
                    Box::into_raw(context) as *mut c_void,
                    Some(invoke_permission_callback),
                );
            }
        }
    });

    unsafe {
        let _: () = msg_send![
            class,
            requestAccessForMediaType: media_type,
            completionHandler: &*block
        ];
    }
}

struct PermissionRequestContext {
    callback: *mut c_void,
    granted: bool,
}

extern "C" fn invoke_permission_callback(context: *mut c_void) {
    unsafe {
        let context = Box::from_raw(context as *mut PermissionRequestContext);
        let callback = Box::from_raw(context.callback as *mut Box<dyn FnOnce(bool)>);
        callback(context.granted);
    }
}
