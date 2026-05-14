use cocoa::base::id;
use cocoa::foundation::NSRange;
use objc::{class, msg_send, sel, sel_impl};

/// The `cocoa` crate does not define NSAttributedString (and related Cocoa classes),
/// which are needed for copying rich text (that is, text intermingled with images)
/// to the clipboard. This adds access to those APIs.
#[allow(non_snake_case)]
pub trait NSAttributedString: Sized {
    unsafe fn alloc(_: Self) -> id {
        msg_send![class!(NSAttributedString), alloc]
    }

    unsafe fn init_attributed_string(self, string: id) -> id;
    unsafe fn appendAttributedString_(self, attr_string: id);
    unsafe fn RTFDFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id;
    unsafe fn RTFFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id;
    unsafe fn string(self) -> id;
}

impl NSAttributedString for id {
    unsafe fn init_attributed_string(self, string: id) -> id {
        msg_send![self, initWithString: string]
    }

    unsafe fn appendAttributedString_(self, attr_string: id) {
        let _: () = msg_send![self, appendAttributedString: attr_string];
    }

    unsafe fn RTFDFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id {
        msg_send![self, RTFDFromRange: range documentAttributes: attrs]
    }

    unsafe fn RTFFromRange_documentAttributes_(self, range: NSRange, attrs: id) -> id {
        msg_send![self, RTFFromRange: range documentAttributes: attrs]
    }

    unsafe fn string(self) -> id {
        msg_send![self, string]
    }
}

pub trait NSMutableAttributedString: NSAttributedString {
    unsafe fn alloc(_: Self) -> id {
        msg_send![class!(NSMutableAttributedString), alloc]
    }
}

impl NSMutableAttributedString for id {}

#[cfg(test)]
mod tests {
    use super::*;
    use cocoa::base::nil;
    use cocoa::foundation::NSAutoreleasePool;
    use cocoa::foundation::{NSSize, NSString};
    #[test]
    fn test_nsattributed_string() {
        let _guard = crate::platform::mac::mac_appkit_test_lock().lock().unwrap();

        #[allow(non_snake_case)]
        pub trait NSTextAttachment: Sized {
            unsafe fn alloc(_: Self) -> id {
                msg_send![class!(NSTextAttachment), alloc]
            }
        }

        impl NSTextAttachment for id {}

        unsafe {
            let pool = NSAutoreleasePool::new(nil);

            let image: id = msg_send![class!(NSImage), alloc];
            let image: id = msg_send![image, initWithSize: NSSize::new(1.0, 1.0)];
            assert_ne!(image, nil);

            let string = NSString::alloc(nil).init_str("Test String");
            let attr_string = NSMutableAttributedString::alloc(nil).init_attributed_string(string);
            assert_ne!(attr_string, nil);

            let hello_string = NSString::alloc(nil).init_str("Hello World");
            let hello_attr_string =
                NSAttributedString::alloc(nil).init_attributed_string(hello_string);
            assert_ne!(hello_attr_string, nil);
            attr_string.appendAttributedString_(hello_attr_string);

            let attachment = NSTextAttachment::alloc(nil);
            let attachment: id = msg_send![attachment, init];
            assert_ne!(attachment, nil);
            let _: () = msg_send![attachment, setImage: image];
            let image_attr_string: id =
                msg_send![class!(NSAttributedString), attributedStringWithAttachment: attachment];
            assert_ne!(image_attr_string, nil);
            attr_string.appendAttributedString_(image_attr_string);

            let another_string = NSString::alloc(nil).init_str("Another String");
            let another_attr_string =
                NSAttributedString::alloc(nil).init_attributed_string(another_string);
            assert_ne!(another_attr_string, nil);
            attr_string.appendAttributedString_(another_attr_string);

            let _len: cocoa::foundation::NSUInteger = msg_send![attr_string, length];

            let rtfd_data = attr_string.RTFDFromRange_documentAttributes_(
                NSRange::new(0, msg_send![attr_string, length]),
                nil,
            );
            assert_ne!(rtfd_data, nil);

            let _: () = msg_send![pool, drain];
        }
    }
}
