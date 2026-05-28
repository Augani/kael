//! Macos screen have a y axis that goings up from the bottom of the screen and
//! an origin at the bottom left of the main display.
mod dispatcher;
mod display;
mod display_link;
mod events;
mod keyboard;

#[cfg(feature = "screen-capture")]
mod screen_capture;

#[cfg(feature = "screen-capture")]
pub(crate) use screen_capture::*;

mod media_capture;
pub(crate) use media_capture::*;

#[cfg(not(feature = "macos-blade"))]
mod metal_atlas;
#[cfg(not(feature = "macos-blade"))]
pub mod metal_renderer;

use core_video::image_buffer::CVImageBuffer;
#[cfg(not(feature = "macos-blade"))]
use metal_renderer as renderer;

#[cfg(feature = "macos-blade")]
use crate::platform::blade as renderer;

mod attributed_string;

#[cfg(feature = "font-kit")]
mod open_type;

#[cfg(feature = "font-kit")]
mod text_system;

mod accessibility;
mod active_window;
mod auto_launch;
mod biometric;
mod dialog;
mod dock;
mod global_hotkey;
mod ipc;
mod network;
mod os_info;
mod permissions;
mod platform;
mod power;
mod tray;
mod window;
mod window_appearance;

use crate::{DevicePixels, Pixels, Size, px, size};
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_foundation::{NSRect, NSSize, NSString, NSUInteger};
use std::{
    ffi::{CStr, c_char},
    ops::Range,
};

#[cfg(test)]
static MAC_APPKIT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn mac_appkit_test_lock() -> &'static std::sync::Mutex<()> {
    &MAC_APPKIT_TEST_LOCK
}

pub(crate) use dispatcher::*;
pub(crate) use display::*;
pub(crate) use display_link::*;
pub(crate) use ipc::resolve_socket_path;
pub(crate) use keyboard::*;
pub(crate) use platform::*;
pub(crate) use window::*;

#[cfg(feature = "font-kit")]
pub(crate) use text_system::*;

/// A frame of video captured from a screen.
pub(crate) type PlatformScreenCaptureFrame = CVImageBuffer;

trait BoolExt {
    fn to_objc(self) -> Bool;
}

impl BoolExt for bool {
    fn to_objc(self) -> Bool {
        if self { Bool::YES } else { Bool::NO }
    }
}

trait NSStringExt {
    unsafe fn to_str(&self) -> &str;
}

impl NSStringExt for *mut AnyObject {
    unsafe fn to_str(&self) -> &str {
        unsafe {
            let cstr: *const c_char = msg_send![*self, UTF8String];
            if cstr.is_null() {
                ""
            } else {
                CStr::from_ptr(cstr).to_str().unwrap()
            }
        }
    }
}

pub(crate) use objc2_foundation::NSRange;

trait NSRangeExt {
    fn invalid() -> Self;
    fn is_valid(&self) -> bool;
    fn to_range(self) -> Option<Range<usize>>;
}

impl NSRangeExt for NSRange {
    fn invalid() -> Self {
        Self {
            location: NSUInteger::MAX,
            length: 0,
        }
    }

    fn is_valid(&self) -> bool {
        self.location != NSUInteger::MAX
    }

    fn to_range(self) -> Option<Range<usize>> {
        if self.is_valid() {
            let start = self.location as usize;
            let end = start + self.length as usize;
            Some(start..end)
        } else {
            None
        }
    }
}

unsafe fn ns_string(string: &str) -> *mut AnyObject {
    let string = NSString::from_str(string);
    let string = Retained::into_raw(string);
    unsafe { msg_send![string, autorelease] }
}

impl From<NSSize> for Size<Pixels> {
    fn from(value: NSSize) -> Self {
        Size {
            width: px(value.width as f32),
            height: px(value.height as f32),
        }
    }
}

impl From<NSRect> for Size<Pixels> {
    fn from(rect: NSRect) -> Self {
        let NSSize { width, height } = rect.size;
        size(width.into(), height.into())
    }
}

impl From<NSRect> for Size<DevicePixels> {
    fn from(rect: NSRect) -> Self {
        let NSSize { width, height } = rect.size;
        size(DevicePixels(width as i32), DevicePixels(height as i32))
    }
}
