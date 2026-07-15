#![allow(dead_code)]

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSRange;

#[allow(non_camel_case_types)]
type id = *mut AnyObject;

/// The `cocoa` crate does not define NSAttributedString (and related Cocoa classes),
/// which are needed for copying rich text (that is, text intermingled with images)
/// to the clipboard. This adds access to those APIs.
#[allow(non_snake_case)]
pub trait NSAttributedString: Sized {
    unsafe fn alloc(_: Self) -> id {
        let Some(class) = AnyClass::get(c"NSAttributedString") else {
            return std::ptr::null_mut();
        };
        let obj: *mut AnyObject = unsafe { msg_send![class, alloc] };
        obj as id
    }

    unsafe fn init_attributed_string(self, string: id) -> id;
    unsafe fn appendAttributedString_(self, attr_string: id);
    unsafe fn RTFDFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id;
    unsafe fn RTFFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id;
    unsafe fn string(self) -> id;
}

impl NSAttributedString for id {
    unsafe fn init_attributed_string(self, string: id) -> id {
        let receiver = self as *mut AnyObject;
        let string = string as *mut AnyObject;
        let out: *mut AnyObject = unsafe { msg_send![receiver, initWithString: string] };
        out as id
    }

    unsafe fn appendAttributedString_(self, attr_string: id) {
        let receiver = self as *mut AnyObject;
        let attr_string = attr_string as *mut AnyObject;
        unsafe {
            let _: () = msg_send![receiver, appendAttributedString: attr_string];
        }
    }

    unsafe fn RTFDFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id {
        let receiver = self as *mut AnyObject;
        let attrs = attrs as *mut AnyObject;
        let out: *mut AnyObject =
            unsafe { msg_send![receiver, RTFDFromRange: range, documentAttributes: attrs] };
        out as id
    }

    unsafe fn RTFFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id {
        let receiver = self as *mut AnyObject;
        let attrs = attrs as *mut AnyObject;
        let out: *mut AnyObject =
            unsafe { msg_send![receiver, RTFFromRange: range, documentAttributes: attrs] };
        out as id
    }

    unsafe fn string(self) -> id {
        let receiver = self as *mut AnyObject;
        let out: *mut AnyObject = unsafe { msg_send![receiver, string] };
        out as id
    }
}

pub trait NSMutableAttributedString: NSAttributedString {
    unsafe fn alloc(_: Self) -> id {
        let Some(class) = AnyClass::get(c"NSMutableAttributedString") else {
            return std::ptr::null_mut();
        };
        let obj: *mut AnyObject = unsafe { msg_send![class, alloc] };
        obj as id
    }
}

impl NSMutableAttributedString for id {}
