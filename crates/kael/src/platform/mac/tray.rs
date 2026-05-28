use crate::platform::TrayMenuItem;
use crate::{Bounds, Pixels, point, px, size};
use cocoa::base::{id, nil};
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2_foundation::{NSRect, NSSize, NSString};
use std::cell::Cell;
use std::ffi::c_void;

pub(crate) struct MacTray {
    status_item: *mut AnyObject,
    panel_mode: Cell<bool>,
    stored_menu: Cell<*mut AnyObject>,
}

unsafe fn class(name: &std::ffi::CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing class {name:?}"))
}

impl MacTray {
    pub fn new() -> Self {
        unsafe {
            let status_bar: *mut AnyObject = msg_send![class(c"NSStatusBar"), systemStatusBar];
            let length: f64 = -1.0;
            let status_item: *mut AnyObject = msg_send![status_bar, statusItemWithLength: length];
            let _: *mut AnyObject = msg_send![status_item, retain];
            let _: () = msg_send![status_item, setVisible: true];

            let button: *mut AnyObject = msg_send![status_item, button];
            if !button.is_null() {
                let default_title = NSString::from_str("App");
                let _: () = msg_send![button, setTitle: &*default_title];
            }

            Self {
                status_item,
                panel_mode: Cell::new(false),
                stored_menu: Cell::new(std::ptr::null_mut()),
            }
        }
    }

    pub fn set_icon(&self, icon_data: Option<&[u8]>) {
        unsafe {
            let button: *mut AnyObject = msg_send![self.status_item, button];
            if button.is_null() {
                return;
            }
            match icon_data {
                Some(data) => {
                    let ns_data: *mut AnyObject = msg_send![
                        class(c"NSData"),
                        dataWithBytes: data.as_ptr() as *const c_void,
                        length: data.len() as u64
                    ];
                    let image_alloc: *mut AnyObject = msg_send![class(c"NSImage"), alloc];
                    let image: *mut AnyObject = msg_send![image_alloc, initWithData: ns_data];
                    if !image.is_null() {
                        let target_size = NSSize::new(18.0, 18.0);
                        let _: () = msg_send![image, setSize: target_size];
                        let _: () = msg_send![image, setTemplate: true];
                        let _: () = msg_send![button, setImage: image];
                        let empty = NSString::from_str("");
                        let _: () = msg_send![button, setTitle: &*empty];
                    }
                }
                None => {
                    let _: () = msg_send![button, setImage: std::ptr::null_mut::<AnyObject>()];
                }
            }
        }
    }

    #[allow(dead_code)]
    pub fn set_title(&self, title: &str) {
        unsafe {
            let button: *mut AnyObject = msg_send![self.status_item, button];
            if button.is_null() {
                return;
            }
            let ns_title = NSString::from_str(title);
            let _: () = msg_send![button, setTitle: &*ns_title];
        }
    }

    pub fn set_tooltip(&self, tooltip: &str) {
        unsafe {
            let button: *mut AnyObject = msg_send![self.status_item, button];
            if button.is_null() {
                return;
            }
            let ns_tooltip = NSString::from_str(tooltip);
            let _: () = msg_send![button, setToolTip: &*ns_tooltip];
        }
    }

    pub fn set_menu(&self, items: Vec<TrayMenuItem>) {
        unsafe {
            let old_menu = self.stored_menu.get();
            if !old_menu.is_null() {
                let _: () = msg_send![old_menu, release];
            }

            let menu: *mut AnyObject = msg_send![class(c"NSMenu"), new];
            let _: () = msg_send![menu, setAutoenablesItems: false];
            let selector = Sel::register(c"handleTrayMenuItem:");
            build_menu_with_selector(menu as id, &items, selector);

            self.stored_menu.set(menu);

            if !self.panel_mode.get() {
                let _: () = msg_send![self.status_item, setMenu: menu];
            }
        }
    }

    pub fn set_panel_mode(&self, enabled: bool) {
        self.panel_mode.set(enabled);
        unsafe {
            if enabled {
                let _: () = msg_send![self.status_item, setMenu: std::ptr::null_mut::<AnyObject>()];

                let button: *mut AnyObject = msg_send![self.status_item, button];
                if !button.is_null() {
                    let delegate = get_app_delegate();
                    if !delegate.is_null() {
                        let _: () = msg_send![button, setTarget: delegate];
                        let panel_sel = Sel::register(c"handleTrayPanelClick:");
                        let _: () = msg_send![button, setAction: panel_sel];
                    }
                }
            } else {
                let button: *mut AnyObject = msg_send![self.status_item, button];
                if !button.is_null() {
                    let _: () = msg_send![button, setTarget: std::ptr::null_mut::<AnyObject>()];
                    let null_sel: *const std::ffi::c_void = std::ptr::null();
                    let _: () = msg_send![button, setAction: null_sel];
                }

                let stored = self.stored_menu.get();
                if !stored.is_null() {
                    let _: () = msg_send![self.status_item, setMenu: stored];
                }
            }
        }
    }

    pub fn get_icon_bounds(&self) -> Option<Bounds<Pixels>> {
        unsafe {
            let button: *mut AnyObject = msg_send![self.status_item, button];
            if button.is_null() {
                return None;
            }

            let button_window: *mut AnyObject = msg_send![button, window];
            if button_window.is_null() {
                return None;
            }

            let frame: NSRect = msg_send![button_window, frame];

            let main_screen: *mut AnyObject = msg_send![class(c"NSScreen"), mainScreen];
            if main_screen.is_null() {
                return None;
            }
            let screen_frame: NSRect = msg_send![main_screen, frame];

            let flipped_y = screen_frame.size.height - frame.origin.y - frame.size.height;

            Some(Bounds::new(
                point(px(frame.origin.x as f32), px(flipped_y as f32)),
                size(px(frame.size.width as f32), px(frame.size.height as f32)),
            ))
        }
    }
}

impl Drop for MacTray {
    fn drop(&mut self) {
        unsafe {
            let stored = self.stored_menu.get();
            if !stored.is_null() {
                let _: () = msg_send![stored, release];
            }
            let status_bar: *mut AnyObject = msg_send![class(c"NSStatusBar"), systemStatusBar];
            let _: () = msg_send![status_bar, removeStatusItem: self.status_item];
            let _: () = msg_send![self.status_item, release];
        }
    }
}

unsafe fn get_app_delegate() -> *mut AnyObject {
    unsafe {
        let app: *mut AnyObject = msg_send![class(c"NSApplication"), sharedApplication];
        msg_send![app, delegate]
    }
}

pub(crate) unsafe fn configure_actionable_item_with_selector(
    menu_item: id,
    item_id: &str,
    selector: Sel,
) {
    unsafe {
        let delegate = get_app_delegate();
        if !delegate.is_null() {
            let menu_item = menu_item as *mut AnyObject;
            let _: () = msg_send![menu_item, setTarget: delegate];
            let _: () = msg_send![menu_item, setAction: selector];
            let represented = NSString::from_str(item_id);
            let _: () = msg_send![menu_item, setRepresentedObject: &*represented];
            let _: () = msg_send![menu_item, setEnabled: true];
        }
    }
}

pub(crate) unsafe fn build_menu_with_selector(menu: id, items: &[TrayMenuItem], selector: Sel) {
    unsafe {
        let menu = menu as *mut AnyObject;
        for item in items {
            match item {
                TrayMenuItem::Action { label, id } => {
                    let title = NSString::from_str(label.as_ref());
                    let menu_item_alloc: *mut AnyObject = msg_send![class(c"NSMenuItem"), alloc];
                    let empty = NSString::from_str("");
                    let null_sel: *const std::ffi::c_void = std::ptr::null();
                    let menu_item: *mut AnyObject = msg_send![
                        menu_item_alloc,
                        initWithTitle: &*title,
                        action: null_sel,
                        keyEquivalent: &*empty
                    ];
                    configure_actionable_item_with_selector(menu_item as id, id.as_ref(), selector);
                    let _: () = msg_send![menu, addItem: menu_item];
                }
                TrayMenuItem::Separator => {
                    let separator: *mut AnyObject = msg_send![class(c"NSMenuItem"), separatorItem];
                    let _: () = msg_send![menu, addItem: separator];
                }
                TrayMenuItem::Submenu {
                    label,
                    items: sub_items,
                } => {
                    let title = NSString::from_str(label.as_ref());
                    let menu_item_alloc: *mut AnyObject = msg_send![class(c"NSMenuItem"), alloc];
                    let empty = NSString::from_str("");
                    let null_sel: *const std::ffi::c_void = std::ptr::null();
                    let menu_item: *mut AnyObject = msg_send![
                        menu_item_alloc,
                        initWithTitle: &*title,
                        action: null_sel,
                        keyEquivalent: &*empty
                    ];
                    let submenu: *mut AnyObject = msg_send![class(c"NSMenu"), new];
                    build_menu_with_selector(submenu as id, sub_items, selector);
                    let _: () = msg_send![menu_item, setSubmenu: submenu];
                    let _: () = msg_send![menu, addItem: menu_item];
                }
                TrayMenuItem::Toggle { label, checked, id } => {
                    let title = NSString::from_str(label.as_ref());
                    let menu_item_alloc: *mut AnyObject = msg_send![class(c"NSMenuItem"), alloc];
                    let empty = NSString::from_str("");
                    let null_sel: *const std::ffi::c_void = std::ptr::null();
                    let menu_item: *mut AnyObject = msg_send![
                        menu_item_alloc,
                        initWithTitle: &*title,
                        action: null_sel,
                        keyEquivalent: &*empty
                    ];
                    configure_actionable_item_with_selector(menu_item as id, id.as_ref(), selector);
                    let state: isize = if *checked { 1 } else { 0 };
                    let _: () = msg_send![menu_item, setState: state];
                    let _: () = msg_send![menu, addItem: menu_item];
                }
            }
        }
        let _ = nil;
    }
}
