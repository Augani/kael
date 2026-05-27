use crate::AttentionType;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};
use objc2_foundation::NSString;

pub fn request_user_attention(attention_type: AttentionType) -> isize {
    let mtm = MainThreadMarker::new().expect("request_user_attention must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let request_type = match attention_type {
        AttentionType::Informational => NSRequestUserAttentionType::InformationalRequest,
        AttentionType::Critical => NSRequestUserAttentionType::CriticalRequest,
    };
    app.requestUserAttention(request_type)
}

pub fn cancel_user_attention(request_id: isize) {
    let mtm = MainThreadMarker::new().expect("cancel_user_attention must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.cancelUserAttentionRequest(request_id);
}

pub fn set_dock_badge(label: Option<&str>) {
    let mtm = MainThreadMarker::new().expect("set_dock_badge must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let dock_tile = app.dockTile();
    let ns_label = label.map(NSString::from_str);
    dock_tile.setBadgeLabel(ns_label.as_deref());
}
