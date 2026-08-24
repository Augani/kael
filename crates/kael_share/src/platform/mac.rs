use anyhow::{Result, anyhow};
use std::ffi::CStr;

use objc2::{
    msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::NSString;

use crate::{
    ReceiverCallback, ShareFileType, ShareResult, ShareSheet, ShareType,
    platform::PlatformShareReceiver,
};

pub(crate) async fn show(sheet: &ShareSheet) -> Result<ShareResult> {
    autoreleasepool(|_| unsafe {
        let _: *mut AnyObject = msg_send![lookup_class(c"NSApplication"), sharedApplication];
        let items = share_objects(sheet)?;

        for (share_type, service_names) in preferred_services(sheet) {
            for service_name in service_names {
                let service_name = ns_string(service_name);
                let service: *mut AnyObject = msg_send![lookup_class(c"NSSharingService"), sharingServiceNamed: &*service_name];
                if service.is_null() {
                    continue;
                }

                let can_perform: Bool = msg_send![service, canPerformWithItems: items];
                if can_perform.as_bool() {
                    let _: () = msg_send![service, performWithItems: items];
                    return Ok(ShareResult::Completed {
                        activity_type: share_type.activity_name().to_string(),
                    });
                }
            }
        }

        if !sheet.is_excluded(ShareType::Clipboard) {
            let pasteboard: *mut AnyObject =
                msg_send![lookup_class(c"NSPasteboard"), generalPasteboard];
            let _: isize = msg_send![pasteboard, clearContents];
            let wrote: Bool = msg_send![pasteboard, writeObjects: items];
            if wrote.as_bool() {
                return Ok(ShareResult::Completed {
                    activity_type: ShareType::Clipboard.activity_name().to_string(),
                });
            }
        }

        Ok(ShareResult::Cancelled)
    })
}

pub(crate) fn register_receiver(
    _file_types: &[ShareFileType],
    _callback: ReceiverCallback,
) -> Result<PlatformShareReceiver> {
    Err(anyhow!(
        "share receiver registration is not implemented yet on macOS"
    ))
}

pub(crate) fn support() -> crate::PlatformShareSupport {
    crate::PlatformShareSupport {
        mail: true,
        messages: true,
        airdrop: true,
        clipboard: true,
        social: false,
        print: true,
        receiver_registration: false,
        system_picker: false,
        memory_files: true,
        requires_user_activation: false,
    }
}

unsafe fn share_objects(sheet: &ShareSheet) -> Result<*mut AnyObject> {
    let objects: *mut AnyObject = unsafe { msg_send![lookup_class(c"NSMutableArray"), array] };
    let memory_attachments = sheet.memory_file_paths()?;

    for item in sheet.items() {
        if let Some(text) = item.text.as_deref().filter(|text| !text.is_empty()) {
            let text = ns_string(text);
            let _: () = unsafe { msg_send![objects, addObject: &*text] };
        }

        if let Some(url) = item.url.as_deref().filter(|url| !url.is_empty()) {
            let url = ns_string(url);
            let url_object: *mut AnyObject =
                unsafe { msg_send![lookup_class(c"NSURL"), URLWithString: &*url] };
            if !url_object.is_null() {
                let _: () = unsafe { msg_send![objects, addObject: url_object] };
            }
        }

        if let Some(image) = item.image.as_ref() {
            let data: *mut AnyObject = unsafe {
                msg_send![lookup_class(c"NSData"), dataWithBytes: image.bytes().as_ptr(), length: image.bytes().len()]
            };
            let image_object: *mut AnyObject =
                unsafe { msg_send![lookup_class(c"NSImage"), alloc] };
            let image_object: *mut AnyObject =
                unsafe { msg_send![image_object, initWithData: data] };
            if !image_object.is_null() {
                let _: () = unsafe { msg_send![objects, addObject: image_object] };
                let _: () = unsafe { msg_send![image_object, release] };
            }
        }

        for file in &item.files {
            unsafe { append_file_url(objects, file)? };
        }
    }

    for file in &memory_attachments {
        unsafe { append_file_url(objects, file)? };
    }

    Ok(objects)
}

unsafe fn append_file_url(objects: *mut AnyObject, file: &std::path::Path) -> Result<()> {
    let path = file
        .to_str()
        .ok_or_else(|| anyhow!("share file path is not valid Unicode: {}", file.display()))?;
    let path = ns_string(path);
    let file_url: *mut AnyObject =
        unsafe { msg_send![lookup_class(c"NSURL"), fileURLWithPath: &*path] };
    if !file_url.is_null() {
        let _: () = unsafe { msg_send![objects, addObject: file_url] };
    }
    Ok(())
}

fn ns_string(value: &str) -> Retained<NSString> {
    NSString::from_str(value)
}

fn lookup_class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing Objective-C class {name:?}"))
}

fn preferred_services(sheet: &ShareSheet) -> Vec<(ShareType, &'static [&'static str])> {
    let mut services = Vec::new();
    if !sheet.is_excluded(ShareType::Mail) {
        services.push((ShareType::Mail, &["NSSharingServiceNameComposeEmail"][..]));
    }
    if !sheet.is_excluded(ShareType::Messages) {
        services.push((
            ShareType::Messages,
            &["NSSharingServiceNameComposeMessage"][..],
        ));
    }
    if !sheet.is_excluded(ShareType::AirDrop) {
        services.push((
            ShareType::AirDrop,
            &["NSSharingServiceNameSendViaAirDrop"][..],
        ));
    }
    if !sheet.is_excluded(ShareType::Print) {
        services.push((ShareType::Print, &["NSSharingServiceNamePrint"][..]));
    }
    services
}
