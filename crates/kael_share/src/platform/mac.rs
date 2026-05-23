use anyhow::{Result, anyhow};
use cocoa::{
    appkit::NSApplication,
    base::{BOOL, YES, id, nil},
    foundation::{NSAutoreleasePool, NSString},
};
use objc::{class, msg_send, sel, sel_impl};

use crate::{
    ReceiverCallback, ShareFileType, ShareResult, ShareSheet, ShareType,
    platform::PlatformShareReceiver,
};

pub(crate) async fn show(sheet: &ShareSheet) -> Result<ShareResult> {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let _: id = NSApplication::sharedApplication(nil);
        let items = share_objects(sheet)?;

        for (share_type, service_names) in preferred_services(sheet) {
            for service_name in service_names {
                let service: id = msg_send![class!(NSSharingService), sharingServiceNamed: ns_string(service_name)];
                if service == nil {
                    continue;
                }

                let can_perform: BOOL = msg_send![service, canPerformWithItems: items];
                if can_perform == YES {
                    let _: () = msg_send![service, performWithItems: items];
                    return Ok(ShareResult::Completed {
                        activity_type: share_type.activity_name().to_string(),
                    });
                }
            }
        }

        if !sheet.is_excluded(ShareType::Clipboard) {
            let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
            let _: isize = msg_send![pasteboard, clearContents];
            let wrote: BOOL = msg_send![pasteboard, writeObjects: items];
            if wrote == YES {
                return Ok(ShareResult::Completed {
                    activity_type: ShareType::Clipboard.activity_name().to_string(),
                });
            }
        }

        Ok(ShareResult::Cancelled)
    }
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
        social: true,
        print: true,
        receiver_registration: false,
    }
}

unsafe fn share_objects(sheet: &ShareSheet) -> Result<id> {
    let objects: id = msg_send![class!(NSMutableArray), array];

    for item in sheet.items() {
        if let Some(text) = item.text.as_deref().filter(|text| !text.is_empty()) {
            let _: () = msg_send![objects, addObject: ns_string(text)];
        }

        if let Some(url) = item.url.as_deref().filter(|url| !url.is_empty()) {
            let url_object: id = msg_send![class!(NSURL), URLWithString: ns_string(url)];
            if url_object != nil {
                let _: () = msg_send![objects, addObject: url_object];
            }
        }

        if let Some(image) = item.image.as_ref() {
            let data: id = msg_send![class!(NSData), dataWithBytes: image.bytes().as_ptr() length: image.bytes().len()];
            let image_object: id = msg_send![class!(NSImage), alloc];
            let image_object: id = msg_send![image_object, initWithData: data];
            if image_object != nil {
                let _: () = msg_send![objects, addObject: image_object];
            }
        }

        for file in &item.files {
            let path = file.to_string_lossy();
            let file_url: id = msg_send![class!(NSURL), fileURLWithPath: ns_string(&path)];
            if file_url != nil {
                let _: () = msg_send![objects, addObject: file_url];
            }
        }
    }

    Ok(objects)
}

fn ns_string(value: &str) -> id {
    unsafe { NSString::alloc(nil).init_str(value) }
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
    if !sheet.is_excluded(ShareType::Social) {
        services.push((
            ShareType::Social,
            &[
                "NSSharingServiceNamePostOnTwitter",
                "NSSharingServiceNamePostOnFacebook",
                "NSSharingServiceNamePostOnLinkedIn",
            ][..],
        ));
    }
    if !sheet.is_excluded(ShareType::Print) {
        services.push((ShareType::Print, &["NSSharingServiceNamePrint"][..]));
    }
    services
}
