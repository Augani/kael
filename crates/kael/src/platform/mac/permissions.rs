use crate::platform::PermissionStatus;
use block::ConcreteBlock;
use cocoa::{
    base::{BOOL, YES, id},
    foundation::NSInteger,
};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFMutableDictionary;
use core_foundation::string::CFString;
use objc::{class, msg_send, sel, sel_impl};
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
    static AVMediaTypeAudio: id;
    static AVMediaTypeVideo: id;
}

const AV_AUTHORIZATION_STATUS_NOT_DETERMINED: NSInteger = 0;
const AV_AUTHORIZATION_STATUS_DENIED: NSInteger = 2;
const AV_AUTHORIZATION_STATUS_AUTHORIZED: NSInteger = 3;

pub fn accessibility_status() -> PermissionStatus {
    unsafe {
        if AXIsProcessTrusted() {
            PermissionStatus::Granted
        } else {
            PermissionStatus::Denied
        }
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

fn authorization_status(media_type: id) -> PermissionStatus {
    unsafe {
        let status: NSInteger =
            msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: media_type];
        match status {
            AV_AUTHORIZATION_STATUS_AUTHORIZED => PermissionStatus::Granted,
            AV_AUTHORIZATION_STATUS_NOT_DETERMINED => PermissionStatus::NotDetermined,
            AV_AUTHORIZATION_STATUS_DENIED => PermissionStatus::Denied,
            _ => PermissionStatus::Denied,
        }
    }
}

fn request_media_permission(media_type: id, callback: Box<dyn FnOnce(bool)>) {
    let callback_ptr = Arc::new(AtomicPtr::new(
        Box::into_raw(Box::new(callback)) as *mut c_void
    ));
    let block = ConcreteBlock::new({
        let callback_ptr = Arc::clone(&callback_ptr);
        move |granted: BOOL| {
            let callback = callback_ptr.swap(ptr::null_mut(), Ordering::AcqRel);
            if callback.is_null() {
                return;
            }

            let context = Box::new(PermissionRequestContext {
                callback,
                granted: granted == YES,
            });

            unsafe {
                dispatch_async_f(
                    dispatch_get_main_queue(),
                    Box::into_raw(context) as *mut c_void,
                    Some(invoke_permission_callback),
                );
            }
        }
    })
    .copy();

    unsafe {
        let _: () = msg_send![
            class!(AVCaptureDevice),
            requestAccessForMediaType: media_type
            completionHandler: block
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
