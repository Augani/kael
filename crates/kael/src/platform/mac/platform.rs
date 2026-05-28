use super::tray::MacTray;
use super::{MacKeyboardLayout, MacKeyboardMapper, events::key_to_native, renderer};
use crate::{
    Action, AnyWindowHandle, BackgroundExecutor, ClipboardEntry, ClipboardItem, ClipboardString,
    CursorStyle, ForegroundExecutor, Image, ImageFormat, KeyContext, Keymap, MacDispatcher,
    MacDisplay, MacWindow, Menu, MenuItem, NotificationAction, OsMenu, OwnedMenu,
    PathPromptOptions, Platform, PlatformDisplay, PlatformKeyboardLayout, PlatformKeyboardMapper,
    PlatformTextSystem, PlatformWindow, Result, SemanticVersion, SharedString, SystemMenuType,
    Task, TrayIconEvent, TrayMenuItem, WindowAppearance, WindowParams, hash,
};
use anyhow::{Context as _, anyhow};
use block2::RcBlock;
use core_foundation::{
    base::{CFRelease, CFType, CFTypeRef, OSStatus, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::{CFDictionary, CFDictionaryRef, CFMutableDictionary},
    runloop::CFRunLoopRun,
    string::{CFString, CFStringRef},
};
use futures::channel::oneshot;
use itertools::Itertools;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{
    AnyThread, ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSEvent, NSEventModifierFlags, NSModalResponse,
    NSModalResponseOK, NSResponder,
};
use objc2_foundation::{
    NSInteger, NSObject, NSObjectProtocol, NSOperatingSystemVersion, NSPoint, NSRange, NSRect,
    NSSize, NSString, NSUInteger,
};
use parking_lot::Mutex;
use std::{
    cell::Cell,
    convert::TryInto,
    ffi::{CStr, OsStr, c_void},
    os::{raw::c_char, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
    process::Command,
    ptr,
    rc::Rc,
    slice, str,
    sync::{Arc, OnceLock},
};
use strum::IntoEnumIterator;
use util::ResultExt;

#[allow(non_upper_case_globals)]
const NSUTF8StringEncoding: NSUInteger = 4;

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    static NSPasteboardTypePNG: *mut AnyObject;
    static NSPasteboardTypeRTF: *mut AnyObject;
    static NSPasteboardTypeRTFD: *mut AnyObject;
    static NSPasteboardTypeString: *mut AnyObject;
    static NSPasteboardTypeTIFF: *mut AnyObject;
}

unsafe fn lookup_class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing class {name:?}"))
}

fn ns_string(string: &str) -> Retained<NSString> {
    NSString::from_str(string)
}

fn ns_string_object(string: &Retained<NSString>) -> *mut AnyObject {
    (&**string as *const NSString).cast_mut().cast()
}

unsafe fn ns_data_with_bytes(bytes: *const c_void, len: usize) -> *mut AnyObject {
    unsafe { msg_send![lookup_class(c"NSData"), dataWithBytes: bytes, length: len] }
}

#[derive(Debug)]
struct GPUIApplicationIvars;

define_class!(
    #[unsafe(super = NSApplication)]
    #[thread_kind = MainThreadOnly]
    #[ivars = GPUIApplicationIvars]
    #[name = "GPUIApplication"]
    struct GPUIApplication;

    unsafe impl NSObjectProtocol for GPUIApplication {}

    impl GPUIApplication {}
);

#[derive(Debug, Default)]
struct GPUIApplicationDelegateIvars {
    platform: Cell<*mut c_void>,
}

define_class!(
    #[unsafe(super = NSResponder)]
    #[thread_kind = MainThreadOnly]
    #[ivars = GPUIApplicationDelegateIvars]
    #[name = "GPUIApplicationDelegate"]
    struct GPUIApplicationDelegate;

    unsafe impl NSObjectProtocol for GPUIApplicationDelegate {}

    impl GPUIApplicationDelegate {
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn will_finish_launching(&self, _notification: *mut AnyObject) {
            unsafe {
                let user_defaults: *mut AnyObject =
                    msg_send![lookup_class(c"NSUserDefaults"), standardUserDefaults];

                let name = ns_string("NSAutoFillHeuristicControllerEnabled");
                let existing_value: *mut AnyObject =
                    msg_send![user_defaults, objectForKey: &*name];
                if existing_value.is_null() {
                    let false_value: *mut AnyObject =
                        msg_send![lookup_class(c"NSNumber"), numberWithBool: Bool::NO];
                    let _: () = msg_send![user_defaults, setObject: false_value, forKey: &*name];
                }
            }
        }

        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: *mut AnyObject) {
            unsafe {
                let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];

                let notification_center: *mut AnyObject =
                    msg_send![lookup_class(c"NSNotificationCenter"), defaultCenter];
                let observer = self as *const Self as *mut AnyObject;
                let name = ns_string("NSTextInputContextKeyboardSelectionDidChangeNotification");
                let keyboard_selector = Sel::register(c"onKeyboardLayoutChange:");
                let _: () = msg_send![notification_center, addObserver: observer,
                    selector: keyboard_selector,
                    name: &*name,
                    object: ptr::null_mut::<AnyObject>()
                ];

                let power_state_change = ns_string("NSProcessInfoPowerStateDidChangeNotification");
                let process_info: *mut AnyObject =
                    msg_send![lookup_class(c"NSProcessInfo"), processInfo];
                let power_selector = Sel::register(c"handleSystemPowerEvent:");
                let _: () = msg_send![notification_center, addObserver: observer,
                    selector: power_selector,
                    name: &*power_state_change,
                    object: process_info
                ];

                let workspace: *mut AnyObject =
                    msg_send![lookup_class(c"NSWorkspace"), sharedWorkspace];
                let ws_notification_center: *mut AnyObject = msg_send![workspace, notificationCenter];

                let power_notifications = [
                    "NSWorkspaceWillSleepNotification",
                    "NSWorkspaceDidWakeNotification",
                    "NSWorkspaceSessionDidResignActiveNotification",
                    "NSWorkspaceSessionDidBecomeActiveNotification",
                    "NSWorkspaceWillPowerOffNotification",
                ];
                for name in &power_notifications {
                    let ns_name = ns_string(name);
                    let _: () = msg_send![ws_notification_center, addObserver: observer,
                        selector: power_selector,
                        name: &*ns_name,
                        object: ptr::null_mut::<AnyObject>()
                    ];
                }

                let platform = self.mac_platform();
                let callback = platform.0.lock().finish_launching.take();
                if let Some(callback) = callback {
                    callback();
                }

                let keep_alive = platform.0.lock().keep_alive_without_windows;
                let policy = if keep_alive {
                    NSApplicationActivationPolicy::Accessory
                } else {
                    NSApplicationActivationPolicy::Regular
                };
                let _: Bool = msg_send![app, setActivationPolicy: policy];
            }
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: *mut AnyObject, has_open_windows: Bool) {
            if has_open_windows.as_bool() {
                return;
            }
            let platform = self.mac_platform();
            let mut lock = platform.0.lock();
            if let Some(mut callback) = lock.reopen.take() {
                drop(lock);
                callback();
                platform.0.lock().reopen.get_or_insert(callback);
            }
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: *mut AnyObject) {
            let platform = self.mac_platform();
            let mut lock = platform.0.lock();
            if let Some(mut callback) = lock.quit.take() {
                drop(lock);
                callback();
                platform.0.lock().quit.get_or_insert(callback);
            }
        }

        #[unsafe(method(handleGPUIMenuItem:))]
        fn handle_menu_item(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(handleTrayMenuItem:))]
        fn handle_tray_menu_item(&self, item: *mut AnyObject) {
            unsafe {
                let platform = self.mac_platform();
                let represented: *mut AnyObject = msg_send![item, representedObject];
                if represented.is_null() {
                    return;
                }
                let len: usize =
                    msg_send![represented, lengthOfBytesUsingEncoding: NSUTF8StringEncoding];
                let bytes: *const u8 = msg_send![represented, UTF8String];
                let id_str = std::str::from_utf8(slice::from_raw_parts(bytes, len)).unwrap_or("");
                let shared_id: SharedString = id_str.to_string().into();

                let platform_ptr = platform as *const MacPlatform;

                use super::dispatcher::{dispatch_get_main_queue, dispatch_sys::dispatch_async_f};

                struct TrayActionCtx {
                    platform: *const MacPlatform,
                    id: SharedString,
                }

                let ctx = Box::into_raw(Box::new(TrayActionCtx {
                    platform: platform_ptr,
                    id: shared_id,
                }));

                unsafe extern "C" fn invoke(ctx_ptr: *mut c_void) {
                    let ctx = unsafe { Box::from_raw(ctx_ptr as *mut TrayActionCtx) };
                    let platform = unsafe { &*ctx.platform };
                    let mut lock = platform.0.lock();
                    if let Some(mut callback) = lock.tray_menu_callback.take() {
                        drop(lock);
                        callback(ctx.id);
                        platform.0.lock().tray_menu_callback = Some(callback);
                    }
                }

                dispatch_async_f(dispatch_get_main_queue(), ctx as *mut c_void, Some(invoke));
            }
        }

        #[unsafe(method(handleTrayPanelClick:))]
        fn handle_tray_panel_click(&self, _sender: *mut AnyObject) {
            let platform = self.mac_platform();
            let platform_ptr = platform as *const MacPlatform;

            use super::dispatcher::{dispatch_get_main_queue, dispatch_sys::dispatch_async_f};

            unsafe extern "C" fn invoke(ctx_ptr: *mut c_void) {
                let platform = unsafe { &*(ctx_ptr as *const MacPlatform) };
                let mut lock = platform.0.lock();
                if let Some(mut callback) = lock.tray_icon_callback.take() {
                    drop(lock);
                    callback(TrayIconEvent::LeftClick);
                    platform.0.lock().tray_icon_callback = Some(callback);
                }
            }

            unsafe {
                dispatch_async_f(
                    dispatch_get_main_queue(),
                    platform_ptr as *mut c_void,
                    Some(invoke),
                );
            }
        }

        #[unsafe(method(cut:))]
        fn cut(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(copy:))]
        fn copy(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(paste:))]
        fn paste(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(selectAll:))]
        fn select_all(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(undo:))]
        fn undo(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(redo:))]
        fn redo(&self, item: *mut AnyObject) {
            self.handle_menu_item_inner(item);
        }

        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: *mut AnyObject) -> Bool {
            unsafe {
                let mut result = false;
                let platform = self.mac_platform();
                let mut lock = platform.0.lock();
                if let Some(mut callback) = lock.validate_menu_command.take() {
                    let tag: NSInteger = msg_send![item, tag];
                    let index = tag as usize;
                    if let Some(action) = lock.menu_actions.get(index) {
                        let action = action.boxed_clone();
                        drop(lock);
                        result = callback(action.as_ref());
                    }
                    platform
                        .0
                        .lock()
                        .validate_menu_command
                        .get_or_insert(callback);
                }
                Bool::new(result)
            }
        }

        #[unsafe(method(menuWillOpen:))]
        fn menu_will_open(&self, _menu: *mut AnyObject) {
            let platform = self.mac_platform();
            let mut lock = platform.0.lock();
            if let Some(mut callback) = lock.will_open_menu.take() {
                drop(lock);
                callback();
                platform.0.lock().will_open_menu.get_or_insert(callback);
            }
        }

        #[unsafe(method(applicationDockMenu:))]
        fn handle_dock_menu(&self, _app: *mut AnyObject) -> *mut AnyObject {
            let platform = self.mac_platform();
            let state = platform.0.lock();
            state.dock_menu.unwrap_or_else(ptr::null_mut)
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: *mut AnyObject, urls: *mut AnyObject) {
            let urls = unsafe {
                let count: usize = msg_send![urls, count];
                (0..count)
                    .filter_map(|i| {
                        let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
                        let absolute_string: *mut AnyObject = msg_send![url, absoluteString];
                        let string_ptr: *const c_char = msg_send![absolute_string, UTF8String];
                        match CStr::from_ptr(string_ptr).to_str() {
                            Ok(string) => Some(string.to_string()),
                            Err(err) => {
                                log::error!("error converting path to string: {}", err);
                                None
                            }
                        }
                    })
                    .collect::<Vec<_>>()
            };

            let platform = self.mac_platform();
            let mut lock = platform.0.lock();
            if let Some(mut callback) = lock.open_urls.take() {
                drop(lock);
                callback(urls);
                platform.0.lock().open_urls.get_or_insert(callback);
            }
        }

        #[unsafe(method(onKeyboardLayoutChange:))]
        fn on_keyboard_layout_change(&self, _notification: *mut AnyObject) {
            let platform = self.mac_platform();
            let mut lock = platform.0.lock();
            let keyboard_layout = MacKeyboardLayout::new();
            lock.keyboard_mapper = Rc::new(MacKeyboardMapper::new(keyboard_layout.id()));
            if let Some(mut callback) = lock.on_keyboard_layout_change.take() {
                drop(lock);
                callback();
                platform
                    .0
                    .lock()
                    .on_keyboard_layout_change
                    .get_or_insert(callback);
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _app: *mut AnyObject) -> Bool {
            let platform = self.mac_platform();
            let lock = platform.0.lock();
            Bool::new(!lock.keep_alive_without_windows)
        }

        #[unsafe(method(handleSystemPowerEvent:))]
        fn handle_system_power_event(&self, notification: *mut AnyObject) {
            unsafe {
                let name: *mut AnyObject = msg_send![notification, name];
                let name_str: *const c_char = msg_send![name, UTF8String];
                let name_cstr = CStr::from_ptr(name_str);
                let name_bytes = name_cstr.to_bytes();

                let event = match name_bytes {
                    b"NSWorkspaceWillSleepNotification" => crate::SystemPowerEvent::Suspend,
                    b"NSWorkspaceDidWakeNotification" => crate::SystemPowerEvent::Resume,
                    b"NSProcessInfoPowerStateDidChangeNotification" => {
                        crate::SystemPowerEvent::PowerModeChanged
                    }
                    b"NSWorkspaceSessionDidResignActiveNotification" => {
                        crate::SystemPowerEvent::LockScreen
                    }
                    b"NSWorkspaceSessionDidBecomeActiveNotification" => {
                        crate::SystemPowerEvent::UnlockScreen
                    }
                    b"NSWorkspaceWillPowerOffNotification" => crate::SystemPowerEvent::Shutdown,
                    _ => return,
                };

                let platform = self.mac_platform();
                let mut lock = platform.0.lock();
                if let Some(mut callback) = lock.system_power_callback.take() {
                    drop(lock);
                    callback(event);
                    platform.0.lock().system_power_callback = Some(callback);
                }
            }
        }

        #[unsafe(method(handleContextMenuItem:))]
        fn handle_context_menu_item(&self, item: *mut AnyObject) {
            unsafe {
                let platform = self.mac_platform();
                let represented: *mut AnyObject = msg_send![item, representedObject];
                if represented.is_null() {
                    return;
                }
                let len: usize =
                    msg_send![represented, lengthOfBytesUsingEncoding: NSUTF8StringEncoding];
                let bytes: *const u8 = msg_send![represented, UTF8String];
                let id_str = std::str::from_utf8(slice::from_raw_parts(bytes, len)).unwrap_or("");
                let shared_id: SharedString = id_str.to_string().into();

                let mut lock = platform.0.lock();
                if let Some(mut callback) = lock.context_menu_callback.take() {
                    drop(lock);
                    callback(shared_id);
                    platform.0.lock().context_menu_callback = Some(callback);
                }
            }
        }

    }
);

impl GPUIApplicationDelegate {
    fn mac_platform(&self) -> &MacPlatform {
        let platform_ptr = self.ivars().platform.get();
        assert!(!platform_ptr.is_null());
        unsafe { &*(platform_ptr as *const MacPlatform) }
    }

    fn handle_menu_item_inner(&self, item: *mut AnyObject) {
        unsafe {
            let platform = self.mac_platform();
            let mut lock = platform.0.lock();
            if let Some(mut callback) = lock.menu_command.take() {
                let tag: NSInteger = msg_send![item, tag];
                let index = tag as usize;
                if let Some(action) = lock.menu_actions.get(index) {
                    let action = action.boxed_clone();
                    drop(lock);
                    callback(&*action);
                }
                platform.0.lock().menu_command.get_or_insert(callback);
            }
        }
    }
}

impl GPUIApplicationDelegate {
    fn new(platform: *mut c_void, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(GPUIApplicationDelegateIvars {
            platform: Cell::new(platform),
        });
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Debug, Default)]
struct GPUINotificationDelegateIvars {
    platform: Cell<*mut c_void>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = GPUINotificationDelegateIvars]
    #[name = "GPUINotificationDelegate"]
    struct GPUINotificationDelegate;

    unsafe impl NSObjectProtocol for GPUINotificationDelegate {}

    impl GPUINotificationDelegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn handle_notification_response(
            &self,
            _center: *mut AnyObject,
            response: *mut AnyObject,
            completion_handler: *mut AnyObject,
        ) {
            unsafe {
                let platform_ptr = self.ivars().platform.get();
                if !platform_ptr.is_null() {
                    let platform = &*(platform_ptr as *const MacPlatform);

                    let action_id: *mut AnyObject = msg_send![response, actionIdentifier];
                    let len: usize =
                        msg_send![action_id, lengthOfBytesUsingEncoding: NSUTF8StringEncoding];
                    let bytes: *const u8 = msg_send![action_id, UTF8String];
                    let action_str =
                        std::str::from_utf8(slice::from_raw_parts(bytes, len)).unwrap_or("");

                    let default_action_id = "com.apple.UNNotificationDefaultActionIdentifier";
                    let dismiss_action_id = "com.apple.UNNotificationDismissActionIdentifier";
                    if action_str != default_action_id && action_str != dismiss_action_id {
                        let mut lock = platform.0.lock();
                        if let Some(mut callback) = lock.notification_action_callback.take() {
                            drop(lock);
                            callback(action_str.to_string());
                            platform.0.lock().notification_action_callback = Some(callback);
                        }
                    }
                }

                #[repr(C)]
                struct BlockLiteral {
                    isa: *const c_void,
                    flags: i32,
                    reserved: i32,
                    invoke: unsafe extern "C" fn(*const BlockLiteral),
                }
                let block_ptr = completion_handler as *const BlockLiteral;
                ((*block_ptr).invoke)(block_ptr);
            }
        }

        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn handle_will_present_notification(
            &self,
            _center: *mut AnyObject,
            _notification: *mut AnyObject,
            completion_handler: *mut AnyObject,
        ) {
            unsafe {
                #[repr(C)]
                struct BlockLiteral {
                    isa: *const c_void,
                    flags: i32,
                    reserved: i32,
                    invoke: unsafe extern "C" fn(*const BlockLiteral, u64),
                }
                let block_ptr = completion_handler as *const BlockLiteral;
                let options: u64 = 16 | 2;
                ((*block_ptr).invoke)(block_ptr, options);
            }
        }
    }
);

impl GPUINotificationDelegate {
    fn new(platform: *mut c_void) -> Retained<Self> {
        let this = Self::alloc().set_ivars(GPUINotificationDelegateIvars {
            platform: Cell::new(platform),
        });
        unsafe { msg_send![super(this), init] }
    }
}

pub(crate) struct MacPlatform(Mutex<MacPlatformState>);

pub(crate) struct MacPlatformState {
    background_executor: BackgroundExecutor,
    foreground_executor: ForegroundExecutor,
    text_system: Arc<dyn PlatformTextSystem>,
    renderer_context: renderer::Context,
    headless: bool,
    pasteboard: *mut AnyObject,
    text_hash_pasteboard_type: Retained<NSString>,
    metadata_pasteboard_type: Retained<NSString>,
    reopen: Option<Box<dyn FnMut()>>,
    on_keyboard_layout_change: Option<Box<dyn FnMut()>>,
    quit: Option<Box<dyn FnMut()>>,
    menu_command: Option<Box<dyn FnMut(&dyn Action)>>,
    validate_menu_command: Option<Box<dyn FnMut(&dyn Action) -> bool>>,
    will_open_menu: Option<Box<dyn FnMut()>>,
    menu_actions: Vec<Box<dyn Action>>,
    open_urls: Option<Box<dyn FnMut(Vec<String>)>>,
    finish_launching: Option<Box<dyn FnOnce()>>,
    dock_menu: Option<*mut AnyObject>,
    menus: Option<Vec<OwnedMenu>>,
    keyboard_mapper: Rc<MacKeyboardMapper>,
    keep_alive_without_windows: bool,
    tray: Option<MacTray>,
    tray_icon_callback: Option<Box<dyn FnMut(TrayIconEvent)>>,
    tray_menu_callback: Option<Box<dyn FnMut(SharedString)>>,
    global_hotkey_callback: Option<Box<dyn FnMut(u32)>>,
    global_hotkey_up_callback: Option<Box<dyn FnMut(u32)>>,
    global_hotkey_monitors: Vec<*mut AnyObject>,
    global_hotkey_registrations: std::collections::HashMap<u32, crate::Keystroke>,
    active_hotkey: Option<u32>,
    system_power_callback: Option<Box<dyn FnMut(crate::SystemPowerEvent)>>,
    network_change_callback: Option<Box<dyn FnMut(crate::NetworkStatus)>>,
    media_key_callback: Option<Box<dyn FnMut(crate::MediaKeyEvent)>>,
    media_key_monitor: Option<*mut AnyObject>,
    network_monitor: Option<*const c_void>,
    attention_request_id: isize,
    context_menu_callback: Option<Box<dyn FnMut(crate::SharedString)>>,
    notification_action_callback: Option<Box<dyn FnMut(String)>>,
    notification_delegate: Option<Retained<GPUINotificationDelegate>>,
}

impl Default for MacPlatform {
    fn default() -> Self {
        Self::new(false)
    }
}

impl MacPlatform {
    pub(crate) fn new(headless: bool) -> Self {
        let dispatcher = Arc::new(MacDispatcher::new());

        #[cfg(feature = "font-kit")]
        let text_system = Arc::new(crate::MacTextSystem::new());

        #[cfg(not(feature = "font-kit"))]
        let text_system = Arc::new(crate::NoopTextSystem::new());

        let keyboard_layout = MacKeyboardLayout::new();
        let keyboard_mapper = Rc::new(MacKeyboardMapper::new(keyboard_layout.id()));

        Self(Mutex::new(MacPlatformState {
            headless,
            text_system,
            background_executor: BackgroundExecutor::new(dispatcher.clone()),
            foreground_executor: ForegroundExecutor::new(dispatcher),
            renderer_context: renderer::Context::default(),
            pasteboard: unsafe { msg_send![lookup_class(c"NSPasteboard"), generalPasteboard] },
            text_hash_pasteboard_type: ns_string("kael-text-hash"),
            metadata_pasteboard_type: ns_string("kael-metadata"),
            reopen: None,
            quit: None,
            menu_command: None,
            validate_menu_command: None,
            will_open_menu: None,
            menu_actions: Default::default(),
            open_urls: None,
            finish_launching: None,
            dock_menu: None,
            on_keyboard_layout_change: None,
            menus: None,
            keyboard_mapper,
            keep_alive_without_windows: false,
            tray: None,
            tray_icon_callback: None,
            tray_menu_callback: None,
            global_hotkey_callback: None,
            global_hotkey_up_callback: None,
            global_hotkey_monitors: Vec::new(),
            global_hotkey_registrations: std::collections::HashMap::new(),
            active_hotkey: None,
            system_power_callback: None,
            network_change_callback: None,
            media_key_callback: None,
            media_key_monitor: None,
            network_monitor: None,
            attention_request_id: 0,
            context_menu_callback: None,
            notification_action_callback: None,
            notification_delegate: None,
        }))
    }

    unsafe fn read_from_pasteboard(
        &self,
        pasteboard: *mut AnyObject,
        kind: *mut AnyObject,
    ) -> Option<&[u8]> {
        unsafe {
            let data: *mut AnyObject = msg_send![pasteboard, dataForType: kind];
            if data.is_null() {
                None
            } else {
                let bytes: *const c_void = msg_send![data, bytes];
                let length: usize = msg_send![data, length];
                Some(slice::from_raw_parts(bytes.cast::<u8>(), length))
            }
        }
    }

    unsafe fn create_menu_bar(
        &self,
        menus: &Vec<Menu>,
        delegate: *mut AnyObject,
        actions: &mut Vec<Box<dyn Action>>,
        keymap: &Keymap,
    ) -> *mut AnyObject {
        unsafe {
            let application_menu: *mut AnyObject = msg_send![lookup_class(c"NSMenu"), new];
            let application_menu: *mut AnyObject = msg_send![application_menu, autorelease];
            let _: () = msg_send![application_menu, setDelegate: delegate];

            for menu_config in menus {
                let menu: *mut AnyObject = msg_send![lookup_class(c"NSMenu"), new];
                let menu: *mut AnyObject = msg_send![menu, autorelease];
                let menu_title = ns_string(&menu_config.name);
                let _: () = msg_send![menu, setTitle: &*menu_title];
                let _: () = msg_send![menu, setDelegate: delegate];

                for item_config in &menu_config.items {
                    let item = Self::create_menu_item(item_config, delegate, actions, keymap);
                    let _: () = msg_send![menu, addItem: item];
                }

                let menu_item: *mut AnyObject = msg_send![lookup_class(c"NSMenuItem"), new];
                let menu_item: *mut AnyObject = msg_send![menu_item, autorelease];
                let _: () = msg_send![menu_item, setTitle: &*menu_title];
                let _: () = msg_send![menu_item, setSubmenu: menu];

                if let Some(icon_bytes) = &menu_config.icon {
                    let ns_data: *mut AnyObject = msg_send![
                        lookup_class(c"NSData"),
                        dataWithBytes: icon_bytes.as_ptr() as *const c_void,
                        length: icon_bytes.len() as u64
                    ];
                    let image: *mut AnyObject = msg_send![lookup_class(c"NSImage"), alloc];
                    let image: *mut AnyObject = msg_send![image, initWithData: ns_data];
                    if !image.is_null() {
                        let image: *mut AnyObject = msg_send![image, autorelease];
                        let _: () = msg_send![image, setSize: NSSize::new(16.0, 16.0)];
                        let _: () = msg_send![image, setTemplate: Bool::YES];
                        let _: () = msg_send![menu_item, setImage: image];
                    }
                }

                let _: () = msg_send![application_menu, addItem: menu_item];

                if menu_config.name == "Window" {
                    let app: *mut AnyObject =
                        msg_send![GPUIApplication::class(), sharedApplication];
                    let _: () = msg_send![app, setWindowsMenu: menu];
                }
            }

            application_menu
        }
    }

    unsafe fn create_dock_menu(
        &self,
        menu_items: Vec<MenuItem>,
        delegate: *mut AnyObject,
        actions: &mut Vec<Box<dyn Action>>,
        keymap: &Keymap,
    ) -> *mut AnyObject {
        unsafe {
            let dock_menu: *mut AnyObject = msg_send![lookup_class(c"NSMenu"), new];
            let _: () = msg_send![dock_menu, setDelegate: delegate];
            for item_config in menu_items {
                let item = Self::create_menu_item(&item_config, delegate, actions, keymap);
                let _: () = msg_send![dock_menu, addItem: item];
            }

            dock_menu
        }
    }

    unsafe fn create_menu_item(
        item: &MenuItem,
        delegate: *mut AnyObject,
        actions: &mut Vec<Box<dyn Action>>,
        keymap: &Keymap,
    ) -> *mut AnyObject {
        static DEFAULT_CONTEXT: OnceLock<Vec<KeyContext>> = OnceLock::new();

        unsafe {
            match item {
                MenuItem::Separator => msg_send![lookup_class(c"NSMenuItem"), separatorItem],
                MenuItem::Action {
                    name,
                    action,
                    os_action,
                } => {
                    // Note that this is intentionally using earlier bindings, whereas typically
                    // later ones take display precedence. See the discussion on
                    // Use earlier bindings for display precedence in menus
                    let keystrokes = keymap
                        .bindings_for_action(action.as_ref())
                        .find_or_first(|binding| {
                            binding.predicate().is_none_or(|predicate| {
                                predicate.eval(DEFAULT_CONTEXT.get_or_init(|| {
                                    let mut workspace_context = KeyContext::new_with_defaults();
                                    workspace_context.add("Workspace");
                                    let mut pane_context = KeyContext::new_with_defaults();
                                    pane_context.add("Pane");
                                    let mut editor_context = KeyContext::new_with_defaults();
                                    editor_context.add("Editor");

                                    pane_context.extend(&editor_context);
                                    workspace_context.extend(&pane_context);
                                    vec![workspace_context]
                                }))
                            })
                        })
                        .map(|binding| binding.keystrokes());

                    let selector = match os_action {
                        Some(crate::OsAction::Cut) => Sel::register(c"cut:"),
                        Some(crate::OsAction::Copy) => Sel::register(c"copy:"),
                        Some(crate::OsAction::Paste) => Sel::register(c"paste:"),
                        Some(crate::OsAction::SelectAll) => Sel::register(c"selectAll:"),
                        // "undo:" and "redo:" are always disabled in our case, as
                        // we don't have a NSTextView/NSTextField to enable them on.
                        Some(crate::OsAction::Undo) => Sel::register(c"handleGPUIMenuItem:"),
                        Some(crate::OsAction::Redo) => Sel::register(c"handleGPUIMenuItem:"),
                        None => Sel::register(c"handleGPUIMenuItem:"),
                    };

                    let mut item: *mut AnyObject;
                    if let Some(keystrokes) = keystrokes {
                        if keystrokes.len() == 1 {
                            let keystroke = &keystrokes[0];
                            let mut mask = NSEventModifierFlags::empty();
                            for (modifier, flag) in &[
                                (
                                    keystroke.modifiers().platform,
                                    NSEventModifierFlags::Command,
                                ),
                                (keystroke.modifiers().control, NSEventModifierFlags::Control),
                                (keystroke.modifiers().alt, NSEventModifierFlags::Option),
                                (keystroke.modifiers().shift, NSEventModifierFlags::Shift),
                            ] {
                                if *modifier {
                                    mask |= *flag;
                                }
                            }

                            let title = ns_string(name);
                            let key_equivalent = ns_string(key_to_native(keystroke.key()).as_ref());
                            let item_alloc: *mut AnyObject =
                                msg_send![lookup_class(c"NSMenuItem"), alloc];
                            item = msg_send![
                                item_alloc,
                                initWithTitle: &*title,
                                action: selector,
                                keyEquivalent: &*key_equivalent
                            ];
                            item = msg_send![item, autorelease];
                            if Self::os_version() >= SemanticVersion::new(12, 0, 0) {
                                let _: () = msg_send![item, setAllowsAutomaticKeyEquivalentLocalization: Bool::NO];
                            }
                            let _: () = msg_send![item, setKeyEquivalentModifierMask: mask];
                        } else {
                            let title = ns_string(name);
                            let empty = ns_string("");
                            let item_alloc: *mut AnyObject =
                                msg_send![lookup_class(c"NSMenuItem"), alloc];
                            item = msg_send![
                                item_alloc,
                                initWithTitle: &*title,
                                action: selector,
                                keyEquivalent: &*empty
                            ];
                            item = msg_send![item, autorelease];
                        }
                    } else {
                        let title = ns_string(name);
                        let empty = ns_string("");
                        let item_alloc: *mut AnyObject =
                            msg_send![lookup_class(c"NSMenuItem"), alloc];
                        item = msg_send![
                            item_alloc,
                            initWithTitle: &*title,
                            action: selector,
                            keyEquivalent: &*empty
                        ];
                        item = msg_send![item, autorelease];
                    }

                    let tag = actions.len() as NSInteger;
                    let _: () = msg_send![item, setTag: tag];
                    actions.push(action.boxed_clone());
                    item
                }
                MenuItem::Submenu(Menu { name, icon, items }) => {
                    let item: *mut AnyObject = msg_send![lookup_class(c"NSMenuItem"), new];
                    let item: *mut AnyObject = msg_send![item, autorelease];
                    let submenu: *mut AnyObject = msg_send![lookup_class(c"NSMenu"), new];
                    let submenu: *mut AnyObject = msg_send![submenu, autorelease];
                    let _: () = msg_send![submenu, setDelegate: delegate];
                    for item in items {
                        let menu_item = Self::create_menu_item(item, delegate, actions, keymap);
                        let _: () = msg_send![submenu, addItem: menu_item];
                    }
                    let title = ns_string(name);
                    let _: () = msg_send![item, setSubmenu: submenu];
                    let _: () = msg_send![item, setTitle: &*title];

                    if let Some(icon_bytes) = icon {
                        let ns_data: *mut AnyObject = msg_send![
                            lookup_class(c"NSData"),
                            dataWithBytes: icon_bytes.as_ptr() as *const c_void,
                            length: icon_bytes.len() as u64
                        ];
                        let image: *mut AnyObject = msg_send![lookup_class(c"NSImage"), alloc];
                        let image: *mut AnyObject = msg_send![image, initWithData: ns_data];
                        if !image.is_null() {
                            let image: *mut AnyObject = msg_send![image, autorelease];
                            let _: () = msg_send![image, setSize: NSSize::new(16.0, 16.0)];
                            let _: () = msg_send![image, setTemplate: Bool::YES];
                            let _: () = msg_send![item, setImage: image];
                        }
                    }

                    item
                }
                MenuItem::SystemMenu(OsMenu { name, menu_type }) => {
                    let item: *mut AnyObject = msg_send![lookup_class(c"NSMenuItem"), new];
                    let item: *mut AnyObject = msg_send![item, autorelease];
                    let submenu: *mut AnyObject = msg_send![lookup_class(c"NSMenu"), new];
                    let submenu: *mut AnyObject = msg_send![submenu, autorelease];
                    let _: () = msg_send![submenu, setDelegate: delegate];
                    let title = ns_string(name);
                    let _: () = msg_send![item, setSubmenu: submenu];
                    let _: () = msg_send![item, setTitle: &*title];

                    match menu_type {
                        SystemMenuType::Services => {
                            let app: *mut AnyObject =
                                msg_send![GPUIApplication::class(), sharedApplication];
                            let _: () = msg_send![app, setServicesMenu: item];
                        }
                    }

                    item
                }
            }
        }
    }

    fn os_version() -> SemanticVersion {
        let version = unsafe {
            let process_info: *mut AnyObject =
                msg_send![lookup_class(c"NSProcessInfo"), processInfo];
            let version: NSOperatingSystemVersion = msg_send![process_info, operatingSystemVersion];
            version
        };
        SemanticVersion::new(
            version.majorVersion as usize,
            version.minorVersion as usize,
            version.patchVersion as usize,
        )
    }
}

impl Platform for MacPlatform {
    fn background_executor(&self) -> BackgroundExecutor {
        self.0.lock().background_executor.clone()
    }

    fn foreground_executor(&self) -> crate::ForegroundExecutor {
        self.0.lock().foreground_executor.clone()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.0.lock().text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn FnOnce()>) {
        let mut state = self.0.lock();
        if state.headless {
            drop(state);
            on_finish_launching();
            unsafe { CFRunLoopRun() };
        } else {
            state.finish_launching = Some(on_finish_launching);
            drop(state);
        }

        unsafe {
            let mtm = MainThreadMarker::new().expect("MacPlatform::run must run on main thread");
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let self_ptr = self as *const Self as *mut c_void;
            let app_delegate = GPUIApplicationDelegate::new(self_ptr, mtm);
            let _: () = msg_send![app, setDelegate: &*app_delegate];

            autoreleasepool(|_| {
                let _: () = msg_send![app, run];
            });

            app_delegate.ivars().platform.set(ptr::null_mut());
            let _: () = msg_send![app, setDelegate: ptr::null_mut::<AnyObject>()];
        }
    }

    fn quit(&self) {
        // Quitting the app causes us to close windows, which invokes `Window::on_close` callbacks
        // synchronously before this method terminates. If we call `Platform::quit` while holding a
        // borrow of the app state (which most of the time we will do), we will end up
        // double-borrowing the app state in the `on_close` callbacks for our open windows. To solve
        // this, we make quitting the application asynchronous so that we aren't holding borrows to
        // the app state on the stack when we actually terminate the app.

        use super::dispatcher::{dispatch_get_main_queue, dispatch_sys::dispatch_async_f};

        unsafe {
            dispatch_async_f(dispatch_get_main_queue(), ptr::null_mut(), Some(quit));
        }

        unsafe extern "C" fn quit(_: *mut c_void) {
            unsafe {
                let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
                let _: () = msg_send![app, terminate: ptr::null_mut::<AnyObject>()];
            }
        }
    }

    fn restart(&self, _binary_path: Option<PathBuf>) {
        use std::os::unix::process::CommandExt as _;

        let app_pid = std::process::id().to_string();
        let app_path = self
            .app_path()
            .ok()
            // When the app is not bundled, `app_path` returns the
            // directory containing the executable. Disregard this
            // and get the path to the executable itself.
            .and_then(|path| (path.extension()?.to_str()? == "app").then_some(path))
            .unwrap_or_else(|| std::env::current_exe().unwrap());

        // Wait until this process has exited and then re-open this path.
        let script = r#"
            while kill -0 $0 2> /dev/null; do
                sleep 0.1
            done
            open "$1"
        "#;

        #[allow(
            clippy::disallowed_methods,
            reason = "We are restarting ourselves, using std command thus is fine"
        )]
        let restart_process = Command::new("/bin/bash")
            .arg("-c")
            .arg(script)
            .arg(app_pid)
            .arg(app_path)
            .process_group(0)
            .spawn();

        match restart_process {
            Ok(_) => self.quit(),
            Err(e) => log::error!("failed to spawn restart script: {:?}", e),
        }
    }

    fn activate(&self, ignoring_other_apps: bool) {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let _: () = msg_send![app, activateIgnoringOtherApps: Bool::new(ignoring_other_apps)];
        }
    }

    fn hide(&self) {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let _: () = msg_send![app, hide: ptr::null_mut::<AnyObject>()];
        }
    }

    fn hide_other_apps(&self) {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let _: () = msg_send![app, hideOtherApplications: ptr::null_mut::<AnyObject>()];
        }
    }

    fn unhide_other_apps(&self) {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let _: () = msg_send![app, unhideAllApplications: ptr::null_mut::<AnyObject>()];
        }
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(MacDisplay::primary()))
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        MacDisplay::all()
            .map(|screen| Rc::new(screen) as Rc<_>)
            .collect()
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        let min_version = NSOperatingSystemVersion {
            majorVersion: 12,
            minorVersion: 3,
            patchVersion: 0,
        };
        super::is_macos_version_at_least(min_version)
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>> {
        super::screen_capture::get_sources()
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        MacWindow::active_window()
    }

    #[allow(unsafe_op_in_unsafe_fn)]
    fn cursor_position(&self) -> Option<crate::Point<crate::Pixels>> {
        unsafe {
            let screens: *mut AnyObject = msg_send![lookup_class(c"NSScreen"), screens];
            let count: usize = if screens.is_null() {
                0
            } else {
                msg_send![screens, count]
            };
            if count == 0 {
                return None;
            }
            let primary: *mut AnyObject = msg_send![screens, objectAtIndex: 0usize];
            let primary_frame: NSRect = msg_send![primary, frame];
            let location: NSPoint = msg_send![lookup_class(c"NSEvent"), mouseLocation];
            Some(crate::point(
                crate::px(location.x as f32),
                crate::px((primary_frame.size.height - location.y) as f32),
            ))
        }
    }

    // Returns the windows ordered front-to-back, meaning that the active
    // window is the first one in the returned vec.
    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        Some(MacWindow::ordered_windows())
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> Result<Box<dyn PlatformWindow>> {
        let renderer_context = self.0.lock().renderer_context.clone();
        Ok(Box::new(MacWindow::open(
            handle,
            options,
            self.foreground_executor(),
            renderer_context,
        )))
    }

    fn window_appearance(&self) -> WindowAppearance {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let appearance: *mut AnyObject = msg_send![app, effectiveAppearance];
            WindowAppearance::from_native(appearance)
        }
    }

    fn open_url(&self, url: &str) {
        unsafe {
            let url_string = ns_string(url);
            let url_alloc: *mut AnyObject = msg_send![lookup_class(c"NSURL"), alloc];
            let url: *mut AnyObject = msg_send![url_alloc, initWithString: &*url_string];
            let url: *mut AnyObject = msg_send![url, autorelease];
            let workspace: *mut AnyObject =
                msg_send![lookup_class(c"NSWorkspace"), sharedWorkspace];
            let _: Bool = msg_send![workspace, openURL: url];
        }
    }

    fn register_url_scheme(&self, scheme: &str) -> Task<anyhow::Result<()>> {
        // API only available post Monterey
        // https://developer.apple.com/documentation/appkit/nsworkspace/3753004-setdefaultapplicationaturl
        let (done_tx, done_rx) = oneshot::channel();
        if Self::os_version() < SemanticVersion::new(12, 0, 0) {
            return Task::ready(Err(anyhow!(
                "macOS 12.0 or later is required to register URL schemes"
            )));
        }

        let bundle_id = unsafe {
            let bundle: *mut AnyObject = msg_send![lookup_class(c"NSBundle"), mainBundle];
            let bundle_id: *mut AnyObject = msg_send![bundle, bundleIdentifier];
            if bundle_id.is_null() {
                return Task::ready(Err(anyhow!("Can only register URL scheme in bundled apps")));
            }
            bundle_id
        };

        unsafe {
            let workspace: *mut AnyObject =
                msg_send![lookup_class(c"NSWorkspace"), sharedWorkspace];
            let scheme = ns_string(scheme);
            let app: *mut AnyObject =
                msg_send![workspace, URLForApplicationWithBundleIdentifier: bundle_id];
            if app.is_null() {
                return Task::ready(Err(anyhow!(
                    "Cannot register URL scheme until app is installed"
                )));
            }
            let done_tx = Cell::new(Some(done_tx));
            let block = RcBlock::new(move |error: *mut AnyObject| {
                let result = if error.is_null() {
                    Ok(())
                } else {
                    let msg: *mut AnyObject = msg_send![error, localizedDescription];
                    Err(anyhow!("Failed to register: {msg:?}"))
                };

                if let Some(done_tx) = done_tx.take() {
                    let _ = done_tx.send(result);
                }
            });
            let _: () = msg_send![workspace, setDefaultApplicationAtURL: app, toOpenURLsWithScheme: &*scheme, completionHandler: &*block];
        }

        self.background_executor()
            .spawn(async { crate::Flatten::flatten(done_rx.await.map_err(|e| anyhow!(e))) })
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.0.lock().open_urls = Some(callback);
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        let (done_tx, done_rx) = oneshot::channel();
        self.foreground_executor()
            .spawn(async move {
                unsafe {
                    let panel: *mut AnyObject = msg_send![lookup_class(c"NSOpenPanel"), openPanel];
                    let _: () =
                        msg_send![panel, setCanChooseDirectories: Bool::new(options.directories)];
                    let _: () = msg_send![panel, setCanChooseFiles: Bool::new(options.files)];
                    let _: () =
                        msg_send![panel, setAllowsMultipleSelection: Bool::new(options.multiple)];

                    let _: () = msg_send![panel, setCanCreateDirectories: Bool::YES];
                    let _: () = msg_send![panel, setResolvesAliases: Bool::NO];
                    let done_tx = Cell::new(Some(done_tx));
                    let block = RcBlock::new(move |response: NSModalResponse| {
                        let result = if response == NSModalResponseOK {
                            let mut result = Vec::new();
                            let urls: *mut AnyObject = msg_send![panel, URLs];
                            let count: usize = msg_send![urls, count];
                            for i in 0..count {
                                let url: *mut AnyObject = msg_send![urls, objectAtIndex: i];
                                let is_file_url: Bool = msg_send![url, isFileURL];
                                if is_file_url.as_bool()
                                    && let Ok(path) = ns_url_to_path(url)
                                {
                                    result.push(path)
                                }
                            }
                            Some(result)
                        } else {
                            None
                        };

                        if let Some(done_tx) = done_tx.take() {
                            let _ = done_tx.send(Ok(result));
                        }
                    });

                    if let Some(prompt) = options.prompt {
                        let prompt = ns_string(&prompt);
                        let _: () = msg_send![panel, setPrompt: &*prompt];
                    }

                    let _: () = msg_send![panel, beginWithCompletionHandler: &*block];
                }
            })
            .detach();
        done_rx
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        let directory = directory.to_owned();
        let suggested_name = suggested_name.map(|s| s.to_owned());
        let (done_tx, done_rx) = oneshot::channel();
        self.foreground_executor()
            .spawn(async move {
                unsafe {
                    let panel: *mut AnyObject = msg_send![lookup_class(c"NSSavePanel"), savePanel];
                    let path = ns_string(directory.to_string_lossy().as_ref());
                    let url: *mut AnyObject = msg_send![
                        lookup_class(c"NSURL"),
                        fileURLWithPath: &*path,
                        isDirectory: Bool::YES
                    ];
                    let _: () = msg_send![panel, setDirectoryURL: url];

                    if let Some(suggested_name) = suggested_name {
                        let name_string = ns_string(&suggested_name);
                        let _: () = msg_send![panel, setNameFieldStringValue: &*name_string];
                    }

                    let done_tx = Cell::new(Some(done_tx));
                    let block = RcBlock::new(move |response: NSModalResponse| {
                        let mut result = None;
                        if response == NSModalResponseOK {
                            let url: *mut AnyObject = msg_send![panel, URL];
                            let is_file_url: Bool = msg_send![url, isFileURL];
                            if is_file_url.as_bool() {
                                result = ns_url_to_path(url).ok().map(|mut result| {
                                    let Some(filename) = result.file_name() else {
                                        return result;
                                    };
                                    let chunks = filename
                                        .as_bytes()
                                        .split(|&b| b == b'.')
                                        .collect::<Vec<_>>();

                                    // Workaround for macOS Sequoia file extension duplication bug
                                    // Workaround a bug in macOS Sequoia that adds an extra file-extension
                                    // sometimes. e.g. `a.sql` becomes `a.sql.s` or `a.txtx` becomes `a.txtx.txt`
                                    //
                                    // This is conditional on OS version because I'd like to get rid of it, so that
                                    // you can manually create a file called `a.sql.s`. That said it seems better
                                    // to break that use-case than breaking `a.sql`.
                                    if chunks.len() == 3
                                        && chunks[1].starts_with(chunks[2])
                                        && Self::os_version() >= SemanticVersion::new(15, 0, 0)
                                    {
                                        let new_filename = OsStr::from_bytes(
                                            &filename.as_bytes()
                                                [..chunks[0].len() + 1 + chunks[1].len()],
                                        )
                                        .to_owned();
                                        result.set_file_name(&new_filename);
                                    }
                                    result
                                })
                            }
                        }

                        if let Some(done_tx) = done_tx.take() {
                            let _ = done_tx.send(Ok(result));
                        }
                    });
                    let _: () = msg_send![panel, beginWithCompletionHandler: &*block];
                }
            })
            .detach();

        done_rx
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        true
    }

    fn reveal_path(&self, path: &Path) {
        unsafe {
            let path = path.to_path_buf();
            self.0
                .lock()
                .background_executor
                .spawn(async move {
                    let full_path = ns_string(path.to_str().unwrap_or(""));
                    let root_full_path = ns_string("");
                    let workspace: *mut AnyObject =
                        msg_send![lookup_class(c"NSWorkspace"), sharedWorkspace];
                    let _: Bool = msg_send![
                        workspace,
                        selectFile: &*full_path,
                        inFileViewerRootedAtPath: &*root_full_path
                    ];
                })
                .detach();
        }
    }

    fn open_with_system(&self, path: &Path) {
        let path = path.to_owned();
        self.0
            .lock()
            .background_executor
            .spawn(async move {
                if let Some(mut child) = smol::process::Command::new("open")
                    .arg(path)
                    .spawn()
                    .context("invoking open command")
                    .log_err()
                {
                    child.status().await.log_err();
                }
            })
            .detach();
    }

    fn on_quit(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().quit = Some(callback);
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().reopen = Some(callback);
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().on_keyboard_layout_change = Some(callback);
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.0.lock().menu_command = Some(callback);
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().will_open_menu = Some(callback);
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.0.lock().validate_menu_command = Some(callback);
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(MacKeyboardLayout::new())
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.0.lock().keyboard_mapper.clone()
    }

    fn app_path(&self) -> Result<PathBuf> {
        unsafe {
            let bundle: *mut AnyObject = msg_send![lookup_class(c"NSBundle"), mainBundle];
            anyhow::ensure!(!bundle.is_null(), "app is not running inside a bundle");
            Ok(path_from_objc(msg_send![bundle, bundlePath]))
        }
    }

    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let delegate: *mut AnyObject = msg_send![app, delegate];
            let mut state = self.0.lock();
            let actions = &mut state.menu_actions;
            let menu = self.create_menu_bar(&menus, delegate, actions, keymap);
            drop(state);
            let _: () = msg_send![app, setMainMenu: menu];
        }
        self.0.lock().menus = Some(menus.into_iter().map(|menu| menu.owned()).collect());
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.0.lock().menus.clone()
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        unsafe {
            let app: *mut AnyObject = msg_send![GPUIApplication::class(), sharedApplication];
            let delegate: *mut AnyObject = msg_send![app, delegate];
            let mut state = self.0.lock();
            let actions = &mut state.menu_actions;
            let new = self.create_dock_menu(menu, delegate, actions, keymap);
            if let Some(old) = state.dock_menu.replace(new) {
                CFRelease(old as _)
            }
        }
    }

    fn add_recent_document(&self, path: &Path) {
        if let Some(path_str) = path.to_str() {
            unsafe {
                let document_controller: *mut AnyObject = msg_send![
                    lookup_class(c"NSDocumentController"),
                    sharedDocumentController
                ];
                let path = ns_string(path_str);
                let url: *mut AnyObject =
                    msg_send![lookup_class(c"NSURL"), fileURLWithPath: &*path];
                let _: () = msg_send![document_controller, noteNewRecentDocumentURL:url];
            }
        }
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        unsafe {
            let bundle: *mut AnyObject = msg_send![lookup_class(c"NSBundle"), mainBundle];
            anyhow::ensure!(!bundle.is_null(), "app is not running inside a bundle");
            let name = ns_string(name);
            let url: *mut AnyObject = msg_send![bundle, URLForAuxiliaryExecutable: &*name];
            anyhow::ensure!(!url.is_null(), "resource not found");
            ns_url_to_path(url)
        }
    }

    /// Match cursor style to one of the styles available
    /// in macOS's [NSCursor](https://developer.apple.com/documentation/appkit/nscursor).
    fn set_cursor_style(&self, style: CursorStyle) {
        unsafe {
            if style == CursorStyle::None {
                let _: () =
                    msg_send![lookup_class(c"NSCursor"), setHiddenUntilMouseMoves: Bool::YES];
                return;
            }

            let new_cursor: *mut AnyObject = match style {
                CursorStyle::Arrow => msg_send![lookup_class(c"NSCursor"), arrowCursor],
                CursorStyle::IBeam => msg_send![lookup_class(c"NSCursor"), IBeamCursor],
                CursorStyle::Crosshair => msg_send![lookup_class(c"NSCursor"), crosshairCursor],
                CursorStyle::ClosedHand => msg_send![lookup_class(c"NSCursor"), closedHandCursor],
                CursorStyle::OpenHand => msg_send![lookup_class(c"NSCursor"), openHandCursor],
                CursorStyle::PointingHand => {
                    msg_send![lookup_class(c"NSCursor"), pointingHandCursor]
                }
                CursorStyle::ResizeLeftRight | CursorStyle::ResizeColumn => {
                    msg_send![lookup_class(c"NSCursor"), resizeLeftRightCursor]
                }
                CursorStyle::ResizeUpDown | CursorStyle::ResizeRow => {
                    msg_send![lookup_class(c"NSCursor"), resizeUpDownCursor]
                }
                CursorStyle::ResizeLeft => msg_send![lookup_class(c"NSCursor"), resizeLeftCursor],
                CursorStyle::ResizeRight => {
                    msg_send![lookup_class(c"NSCursor"), resizeRightCursor]
                }
                CursorStyle::ResizeUp => msg_send![lookup_class(c"NSCursor"), resizeUpCursor],
                CursorStyle::ResizeDown => msg_send![lookup_class(c"NSCursor"), resizeDownCursor],

                // Undocumented, private class methods:
                // https://stackoverflow.com/questions/27242353/cocoa-predefined-resize-mouse-cursor
                CursorStyle::ResizeUpLeftDownRight => {
                    msg_send![
                        lookup_class(c"NSCursor"),
                        _windowResizeNorthWestSouthEastCursor
                    ]
                }
                CursorStyle::ResizeUpRightDownLeft => {
                    msg_send![
                        lookup_class(c"NSCursor"),
                        _windowResizeNorthEastSouthWestCursor
                    ]
                }

                CursorStyle::IBeamCursorForVerticalLayout => {
                    msg_send![lookup_class(c"NSCursor"), IBeamCursorForVerticalLayout]
                }
                CursorStyle::OperationNotAllowed => {
                    msg_send![lookup_class(c"NSCursor"), operationNotAllowedCursor]
                }
                CursorStyle::DragLink => msg_send![lookup_class(c"NSCursor"), dragLinkCursor],
                CursorStyle::DragCopy => msg_send![lookup_class(c"NSCursor"), dragCopyCursor],
                CursorStyle::ContextualMenu => {
                    msg_send![lookup_class(c"NSCursor"), contextualMenuCursor]
                }
                CursorStyle::None => unreachable!(),
            };

            let old_cursor: *mut AnyObject = msg_send![lookup_class(c"NSCursor"), currentCursor];
            if new_cursor != old_cursor {
                let _: () = msg_send![new_cursor, set];
            }
        }
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        #[allow(non_upper_case_globals)]
        const NSScrollerStyleOverlay: NSInteger = 1;

        unsafe {
            let style: NSInteger = msg_send![lookup_class(c"NSScroller"), preferredScrollerStyle];
            style == NSScrollerStyleOverlay
        }
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        use crate::ClipboardEntry;

        unsafe {
            // We only want to use NSAttributedString if there are multiple entries to write.
            if item.entries.len() <= 1 {
                match item.entries.first() {
                    Some(entry) => match entry {
                        ClipboardEntry::String(string) => {
                            self.write_plaintext_to_clipboard(string);
                        }
                        ClipboardEntry::Image(image) => {
                            self.write_image_to_clipboard(image);
                        }
                    },
                    None => {
                        // Writing an empty list of entries just clears the clipboard.
                        let state = self.0.lock();
                        let _: NSInteger = msg_send![state.pasteboard, clearContents];
                    }
                }
            } else {
                let mut any_images = false;
                let attributed_string = {
                    let empty = ns_string("");
                    let buf_alloc: *mut AnyObject =
                        msg_send![lookup_class(c"NSMutableAttributedString"), alloc];
                    let buf: *mut AnyObject = msg_send![buf_alloc, initWithString: &*empty];

                    for entry in item.entries {
                        if let ClipboardEntry::String(ClipboardString { text, metadata: _ }) = entry
                        {
                            let text = ns_string(&text);
                            let to_append_alloc: *mut AnyObject =
                                msg_send![lookup_class(c"NSAttributedString"), alloc];
                            let to_append: *mut AnyObject =
                                msg_send![to_append_alloc, initWithString: &*text];

                            let _: () = msg_send![buf, appendAttributedString: to_append];
                        }
                    }

                    buf
                };

                let state = self.0.lock();
                let _: NSInteger = msg_send![state.pasteboard, clearContents];

                // Only set rich text clipboard types if we actually have 1+ images to include.
                if any_images {
                    let length: usize = msg_send![attributed_string, length];
                    let range = NSRange::new(0, length);
                    let rtfd_data: *mut AnyObject = msg_send![
                        attributed_string,
                        RTFDFromRange: range,
                        documentAttributes: ptr::null_mut::<AnyObject>()
                    ];
                    if !rtfd_data.is_null() {
                        let _: Bool = msg_send![
                            state.pasteboard,
                            setData: rtfd_data,
                            forType: NSPasteboardTypeRTFD
                        ];
                    }

                    let rtf_data: *mut AnyObject = msg_send![
                        attributed_string,
                        RTFFromRange: range,
                        documentAttributes: ptr::null_mut::<AnyObject>()
                    ];
                    if !rtf_data.is_null() {
                        let _: Bool = msg_send![
                            state.pasteboard,
                            setData: rtf_data,
                            forType: NSPasteboardTypeRTF
                        ];
                    }
                }

                let plain_text: *mut AnyObject = msg_send![attributed_string, string];
                let _: Bool = msg_send![
                    state.pasteboard,
                    setString: plain_text,
                    forType: NSPasteboardTypeString
                ];
            }
        }
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        let state = self.0.lock();
        let pasteboard = state.pasteboard;

        // First, see if it's a string.
        unsafe {
            let types: *mut AnyObject = msg_send![pasteboard, types];
            let string_type = ns_string("public.utf8-plain-text");

            let contains_string: Bool = msg_send![types, containsObject: &*string_type];
            if contains_string.as_bool() {
                let data: *mut AnyObject = msg_send![pasteboard, dataForType: &*string_type];
                if data.is_null() {
                    return None;
                }
                let bytes: *const c_void = msg_send![data, bytes];
                if bytes.is_null() {
                    // https://developer.apple.com/documentation/foundation/nsdata/1410616-bytes?language=objc
                    // "If the length of the NSData object is 0, this property returns nil."
                    return Some(self.read_string_from_clipboard(&state, &[]));
                } else {
                    let length: usize = msg_send![data, length];
                    let bytes = slice::from_raw_parts(bytes.cast::<u8>(), length);

                    return Some(self.read_string_from_clipboard(&state, bytes));
                }
            }

            // If it wasn't a string, try the various supported image types.
            for format in ImageFormat::iter() {
                if let Some(item) = try_clipboard_image(pasteboard, format) {
                    return Some(item);
                }
            }
        }

        // If it wasn't a string or a supported image type, give up.
        None
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        let url = url.to_string();
        let username = username.to_string();
        let password = password.to_vec();
        self.background_executor().spawn(async move {
            unsafe {
                use security::*;

                let url = CFString::from(url.as_str());
                let username = CFString::from(username.as_str());
                let password = CFData::from_buffer(&password);

                // First, check if there are already credentials for the given server. If so, then
                // update the username and password.
                let mut verb = "updating";
                let mut query_attrs = CFMutableDictionary::with_capacity(2);
                query_attrs.set(kSecClass as *const _, kSecClassInternetPassword as *const _);
                query_attrs.set(kSecAttrServer as *const _, url.as_CFTypeRef());

                let mut attrs = CFMutableDictionary::with_capacity(4);
                attrs.set(kSecClass as *const _, kSecClassInternetPassword as *const _);
                attrs.set(kSecAttrServer as *const _, url.as_CFTypeRef());
                attrs.set(kSecAttrAccount as *const _, username.as_CFTypeRef());
                attrs.set(kSecValueData as *const _, password.as_CFTypeRef());

                let mut status = SecItemUpdate(
                    query_attrs.as_concrete_TypeRef(),
                    attrs.as_concrete_TypeRef(),
                );

                // If there were no existing credentials for the given server, then create them.
                if status == errSecItemNotFound {
                    verb = "creating";
                    status = SecItemAdd(attrs.as_concrete_TypeRef(), ptr::null_mut());
                }
                anyhow::ensure!(status == errSecSuccess, "{verb} password failed: {status}");
            }
            Ok(())
        })
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        let url = url.to_string();
        self.background_executor().spawn(async move {
            let url = CFString::from(url.as_str());
            let cf_true = CFBoolean::true_value().as_CFTypeRef();

            unsafe {
                use security::*;

                // Find any credentials for the given server URL.
                let mut attrs = CFMutableDictionary::with_capacity(5);
                attrs.set(kSecClass as *const _, kSecClassInternetPassword as *const _);
                attrs.set(kSecAttrServer as *const _, url.as_CFTypeRef());
                attrs.set(kSecReturnAttributes as *const _, cf_true);
                attrs.set(kSecReturnData as *const _, cf_true);

                let mut result = CFTypeRef::from(ptr::null());
                let status = SecItemCopyMatching(attrs.as_concrete_TypeRef(), &mut result);
                match status {
                    security::errSecSuccess => {}
                    security::errSecItemNotFound | security::errSecUserCanceled => return Ok(None),
                    _ => anyhow::bail!("reading password failed: {status}"),
                }

                let result = CFType::wrap_under_create_rule(result)
                    .downcast::<CFDictionary>()
                    .context("keychain item was not a dictionary")?;
                let username = result
                    .find(kSecAttrAccount as *const _)
                    .context("account was missing from keychain item")?;
                let username = CFType::wrap_under_get_rule(*username)
                    .downcast::<CFString>()
                    .context("account was not a string")?;
                let password = result
                    .find(kSecValueData as *const _)
                    .context("password was missing from keychain item")?;
                let password = CFType::wrap_under_get_rule(*password)
                    .downcast::<CFData>()
                    .context("password was not a string")?;

                Ok(Some((username.to_string(), password.bytes().to_vec())))
            }
        })
    }

    fn set_keep_alive_without_windows(&self, keep_alive: bool) {
        self.0.lock().keep_alive_without_windows = keep_alive;
    }

    fn set_tray_icon(&self, icon: Option<&[u8]>) {
        let mut state = self.0.lock();
        if state.tray.is_none() {
            state.tray = Some(MacTray::new());
        }
        if let Some(tray) = &state.tray {
            tray.set_icon(icon);
        }
    }

    fn set_tray_menu(&self, menu: Vec<TrayMenuItem>) {
        let mut state = self.0.lock();
        if state.tray.is_none() {
            state.tray = Some(MacTray::new());
        }
        if let Some(tray) = &state.tray {
            tray.set_menu(menu);
        }
    }

    fn set_tray_tooltip(&self, tooltip: &str) {
        let mut state = self.0.lock();
        if state.tray.is_none() {
            state.tray = Some(MacTray::new());
        }
        if let Some(tray) = &state.tray {
            tray.set_tooltip(tooltip);
        }
    }

    fn set_tray_panel_mode(&self, enabled: bool) {
        let mut state = self.0.lock();
        if state.tray.is_none() {
            state.tray = Some(MacTray::new());
        }
        if let Some(tray) = &state.tray {
            tray.set_panel_mode(enabled);
        }
    }

    fn get_tray_icon_bounds(&self) -> Option<crate::Bounds<crate::Pixels>> {
        let state = self.0.lock();
        state.tray.as_ref().and_then(|tray| tray.get_icon_bounds())
    }

    fn on_tray_icon_event(&self, callback: Box<dyn FnMut(TrayIconEvent)>) {
        self.0.lock().tray_icon_callback = Some(callback);
    }

    fn on_tray_menu_action(&self, callback: Box<dyn FnMut(SharedString)>) {
        self.0.lock().tray_menu_callback = Some(callback);
    }

    fn register_global_hotkey(&self, id: u32, keystroke: &crate::Keystroke) -> Result<()> {
        let mut state = self.0.lock();
        state
            .global_hotkey_registrations
            .insert(id, keystroke.clone());

        if state.global_hotkey_monitors.is_empty() {
            let platform_ptr = &self.0 as *const Mutex<MacPlatformState> as *const c_void;

            unsafe {
                let mask: u64 = (1 << 10) | (1 << 11) | (1 << 12);

                let global_block = RcBlock::new(move |event: *mut AnyObject| {
                    let platform_state = &*(platform_ptr as *const Mutex<MacPlatformState>);
                    let event_ref = &*(event as *const NSEvent);
                    let event_type = event_ref.r#type();
                    log::trace!("global hotkey event: type={:?}", event_type);
                    let mut lock = platform_state.lock();
                    if let Some(hotkey_id) = super::global_hotkey::find_matching_hotkey(
                        &lock.global_hotkey_registrations,
                        event,
                    ) {
                        log::debug!("global hotkey DOWN: id={hotkey_id}");
                        lock.active_hotkey = Some(hotkey_id);
                        if let Some(mut callback) = lock.global_hotkey_callback.take() {
                            drop(lock);
                            callback(hotkey_id);
                            platform_state.lock().global_hotkey_callback = Some(callback);
                        }
                    } else if let Some(hotkey_id) =
                        super::global_hotkey::find_matching_hotkey_released(
                            lock.active_hotkey,
                            &lock.global_hotkey_registrations,
                            event,
                        )
                    {
                        log::debug!("global hotkey UP: id={hotkey_id}");
                        lock.active_hotkey = None;
                        if let Some(mut callback) = lock.global_hotkey_up_callback.take() {
                            drop(lock);
                            callback(hotkey_id);
                            platform_state.lock().global_hotkey_up_callback = Some(callback);
                        }
                    }
                });
                let global_monitor: *mut AnyObject = msg_send![
                    lookup_class(c"NSEvent"),
                    addGlobalMonitorForEventsMatchingMask: mask,
                    handler: &*global_block
                ];
                std::mem::forget(global_block);

                let local_block = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
                    let platform_state = &*(platform_ptr as *const Mutex<MacPlatformState>);
                    let event_ref = &*(event as *const NSEvent);
                    let event_type = event_ref.r#type();
                    log::trace!("local hotkey event: type={:?}", event_type);
                    let mut lock = platform_state.lock();
                    if let Some(hotkey_id) = super::global_hotkey::find_matching_hotkey(
                        &lock.global_hotkey_registrations,
                        event,
                    ) {
                        log::debug!("local hotkey DOWN: id={hotkey_id}");
                        lock.active_hotkey = Some(hotkey_id);
                        if let Some(mut callback) = lock.global_hotkey_callback.take() {
                            drop(lock);
                            callback(hotkey_id);
                            platform_state.lock().global_hotkey_callback = Some(callback);
                        }
                    } else if let Some(hotkey_id) =
                        super::global_hotkey::find_matching_hotkey_released(
                            lock.active_hotkey,
                            &lock.global_hotkey_registrations,
                            event,
                        )
                    {
                        log::debug!("local hotkey UP: id={hotkey_id}");
                        lock.active_hotkey = None;
                        if let Some(mut callback) = lock.global_hotkey_up_callback.take() {
                            drop(lock);
                            callback(hotkey_id);
                            platform_state.lock().global_hotkey_up_callback = Some(callback);
                        }
                    }
                    event
                });
                let local_monitor: *mut AnyObject = msg_send![
                    lookup_class(c"NSEvent"),
                    addLocalMonitorForEventsMatchingMask: mask,
                    handler: &*local_block
                ];
                std::mem::forget(local_block);

                state.global_hotkey_monitors.push(global_monitor);
                state.global_hotkey_monitors.push(local_monitor);
            }
        }

        Ok(())
    }

    fn unregister_global_hotkey(&self, id: u32) {
        let mut state = self.0.lock();
        state.global_hotkey_registrations.remove(&id);
    }

    fn on_global_hotkey(&self, callback: Box<dyn FnMut(u32)>) {
        self.0.lock().global_hotkey_callback = Some(callback);
    }

    fn on_global_hotkey_up(&self, callback: Box<dyn FnMut(u32)>) {
        self.0.lock().global_hotkey_up_callback = Some(callback);
    }

    fn focused_window_info(&self) -> Option<crate::FocusedWindowInfo> {
        super::active_window::get_focused_window_info()
    }

    fn accessibility_status(&self) -> crate::PermissionStatus {
        super::permissions::accessibility_status()
    }

    fn request_accessibility_permission(&self) {
        super::permissions::request_accessibility_permission();
    }

    fn microphone_status(&self) -> crate::PermissionStatus {
        super::permissions::microphone_status()
    }

    fn request_microphone_permission(&self, callback: Box<dyn FnOnce(bool)>) {
        super::permissions::request_microphone_permission(callback);
    }

    fn camera_status(&self) -> crate::PermissionStatus {
        super::permissions::camera_status()
    }

    fn request_camera_permission(&self, callback: Box<dyn FnOnce(bool)>) {
        super::permissions::request_camera_permission(callback);
    }

    fn set_auto_launch(&self, app_id: &str, enabled: bool) -> Result<()> {
        super::auto_launch::set_auto_launch(app_id, enabled)
    }

    fn is_auto_launch_enabled(&self, app_id: &str) -> bool {
        super::auto_launch::is_auto_launch_enabled(app_id)
    }

    fn show_notification(&self, title: &str, body: &str) -> Result<()> {
        unsafe {
            let bundle: *mut AnyObject = msg_send![lookup_class(c"NSBundle"), mainBundle];
            let bundle_id: *mut AnyObject = msg_send![bundle, bundleIdentifier];
            if bundle_id.is_null() {
                return Err(anyhow!(
                    "Notifications require an app bundle (bundleIdentifier is nil)"
                ));
            }

            let center: *mut AnyObject = msg_send![
                lookup_class(c"UNUserNotificationCenter"),
                currentNotificationCenter
            ];
            if center.is_null() {
                return Err(anyhow!("UNUserNotificationCenter not available"));
            }
            let content: *mut AnyObject =
                msg_send![lookup_class(c"UNMutableNotificationContent"), new];
            let ns_title = ns_string(title);
            let _: () = msg_send![content, setTitle: &*ns_title];
            let ns_body = ns_string(body);
            let _: () = msg_send![content, setBody: &*ns_body];

            let uuid_str = uuid::Uuid::new_v4().to_string();
            let ns_id = ns_string(&uuid_str);
            let request: *mut AnyObject = msg_send![
                lookup_class(c"UNNotificationRequest"),
                requestWithIdentifier: &*ns_id,
                content: content,
                trigger: ptr::null_mut::<AnyObject>()
            ];
            let _: () = msg_send![center, addNotificationRequest: request, withCompletionHandler: ptr::null_mut::<AnyObject>()];
        }
        Ok(())
    }

    fn show_notification_with_actions(
        &self,
        title: &str,
        body: &str,
        actions: &[NotificationAction],
        callback: Box<dyn FnMut(String)>,
    ) -> Result<()> {
        unsafe {
            let bundle: *mut AnyObject = msg_send![lookup_class(c"NSBundle"), mainBundle];
            let bundle_id: *mut AnyObject = msg_send![bundle, bundleIdentifier];
            if bundle_id.is_null() {
                return Err(anyhow!(
                    "Notifications require an app bundle (bundleIdentifier is nil)"
                ));
            }

            let center: *mut AnyObject = msg_send![
                lookup_class(c"UNUserNotificationCenter"),
                currentNotificationCenter
            ];
            if center.is_null() {
                return Err(anyhow!("UNUserNotificationCenter not available"));
            }

            let ns_actions: *mut AnyObject =
                msg_send![lookup_class(c"NSMutableArray"), arrayWithCapacity: actions.len()];
            for action in actions {
                let ns_id = ns_string(&action.id);
                let ns_label = ns_string(&action.label);
                let un_action: *mut AnyObject = msg_send![
                    lookup_class(c"UNNotificationAction"),
                    actionWithIdentifier: &*ns_id,
                    title: &*ns_label,
                    options: 4u64
                ];
                let _: () = msg_send![ns_actions, addObject: un_action];
            }

            let category_id_str = format!("gpui-actions-{}", uuid::Uuid::new_v4());
            let ns_category_id = ns_string(&category_id_str);
            let empty_array: *mut AnyObject = msg_send![lookup_class(c"NSArray"), array];
            let category: *mut AnyObject = msg_send![
                lookup_class(c"UNNotificationCategory"),
                categoryWithIdentifier: &*ns_category_id,
                actions: ns_actions,
                intentIdentifiers: empty_array,
                options: 0u64
            ];

            let category_set: *mut AnyObject =
                msg_send![lookup_class(c"NSSet"), setWithObject: category];
            let _: () = msg_send![center, setNotificationCategories: category_set];

            let self_ptr = self as *const Self as *const c_void;
            let delegate = GPUINotificationDelegate::new(self_ptr as *mut c_void);
            let _: () = msg_send![center, setDelegate: &*delegate];

            {
                let mut state = self.0.lock();
                state.notification_action_callback = Some(callback);
                state.notification_delegate = Some(delegate);
            }

            let content: *mut AnyObject =
                msg_send![lookup_class(c"UNMutableNotificationContent"), new];
            let ns_title = ns_string(title);
            let _: () = msg_send![content, setTitle: &*ns_title];
            let ns_body = ns_string(body);
            let _: () = msg_send![content, setBody: &*ns_body];
            let _: () = msg_send![content, setCategoryIdentifier: &*ns_category_id];

            let uuid_str = uuid::Uuid::new_v4().to_string();
            let ns_id = ns_string(&uuid_str);
            let request: *mut AnyObject = msg_send![
                lookup_class(c"UNNotificationRequest"),
                requestWithIdentifier: &*ns_id,
                content: content,
                trigger: ptr::null_mut::<AnyObject>()
            ];
            let _: () = msg_send![center, addNotificationRequest: request, withCompletionHandler: ptr::null_mut::<AnyObject>()];
        }
        Ok(())
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        let url = url.to_string();

        self.background_executor().spawn(async move {
            unsafe {
                use security::*;

                let url = CFString::from(url.as_str());
                let mut query_attrs = CFMutableDictionary::with_capacity(2);
                query_attrs.set(kSecClass as *const _, kSecClassInternetPassword as *const _);
                query_attrs.set(kSecAttrServer as *const _, url.as_CFTypeRef());

                let status = SecItemDelete(query_attrs.as_concrete_TypeRef());
                anyhow::ensure!(status == errSecSuccess, "delete password failed: {status}");
            }
            Ok(())
        })
    }

    fn on_system_power_event(&self, callback: Box<dyn FnMut(crate::SystemPowerEvent)>) {
        self.0.lock().system_power_callback = Some(callback);
    }

    fn start_power_save_blocker(&self, kind: crate::PowerSaveBlockerKind) -> Option<u32> {
        super::power::start_power_save_blocker(kind)
    }

    fn stop_power_save_blocker(&self, id: u32) {
        super::power::stop_power_save_blocker(id);
    }

    fn power_mode(&self) -> crate::PowerMode {
        super::power::power_mode()
    }

    fn system_idle_time(&self) -> Option<std::time::Duration> {
        super::power::system_idle_time()
    }

    fn network_status(&self) -> crate::NetworkStatus {
        super::network::network_status()
    }

    fn on_network_status_change(&self, callback: Box<dyn FnMut(crate::NetworkStatus)>) {
        let mut state = self.0.lock();

        if let Some(old_monitor) = state.network_monitor.take() {
            unsafe { super::network::cancel_path_monitor(old_monitor) };
        }

        state.network_change_callback = Some(callback);

        let platform_ptr = &self.0 as *const Mutex<MacPlatformState> as *const c_void;

        unsafe {
            let monitor = super::network::create_path_monitor();
            if monitor.is_null() {
                return;
            }

            let block = RcBlock::new(move |path: *const c_void| {
                let status = super::network::path_status_to_network_status(path);

                struct NetworkChangeCtx {
                    platform: *const c_void,
                    status: crate::NetworkStatus,
                }

                let ctx = Box::into_raw(Box::new(NetworkChangeCtx {
                    platform: platform_ptr,
                    status,
                }));

                use super::dispatcher::{dispatch_get_main_queue, dispatch_sys::dispatch_async_f};

                unsafe extern "C" fn invoke(ctx_ptr: *mut c_void) {
                    let ctx = unsafe { Box::from_raw(ctx_ptr as *mut NetworkChangeCtx) };
                    let platform_state =
                        unsafe { &*(ctx.platform as *const Mutex<MacPlatformState>) };
                    let mut lock = platform_state.lock();
                    if let Some(mut callback) = lock.network_change_callback.take() {
                        drop(lock);
                        callback(ctx.status);
                        platform_state.lock().network_change_callback = Some(callback);
                    }
                }

                dispatch_async_f(dispatch_get_main_queue(), ctx as *mut c_void, Some(invoke));
            });
            let queue = super::dispatcher::dispatch_get_main_queue();
            super::network::start_path_monitor(
                monitor,
                &*block as *const _ as *const c_void,
                queue as *const c_void,
            );
            std::mem::forget(block);

            state.network_monitor = Some(monitor);
        }
    }

    fn on_media_key_event(&self, callback: Box<dyn FnMut(crate::MediaKeyEvent)>) {
        let mut state = self.0.lock();
        state.media_key_callback = Some(callback);

        if state.media_key_monitor.is_some() {
            return;
        }

        let platform_ptr = &self.0 as *const Mutex<MacPlatformState> as *const c_void;

        unsafe {
            let mask: u64 = 1 << 14; // NSSystemDefinedMask

            let block = RcBlock::new(move |event: *mut AnyObject| {
                let subtype: i16 = msg_send![event, subtype];
                if subtype != 8 {
                    return;
                }

                let data1: isize = msg_send![event, data1];
                let key_code = (data1 >> 16) & 0xFF;
                let flags = (data1 >> 8) & 0xFF;
                let is_down = (flags & 0x1) == 0;

                if !is_down {
                    return;
                }

                let media_event = match key_code {
                    16 => crate::MediaKeyEvent::PlayPause,
                    17 => crate::MediaKeyEvent::NextTrack,
                    18 => crate::MediaKeyEvent::PreviousTrack,
                    19 => crate::MediaKeyEvent::Stop,
                    20 => crate::MediaKeyEvent::Play,
                    _ => return,
                };

                let platform_state = &*(platform_ptr as *const Mutex<MacPlatformState>);
                let mut lock = platform_state.lock();
                if let Some(mut callback) = lock.media_key_callback.take() {
                    drop(lock);
                    callback(media_event);
                    platform_state.lock().media_key_callback = Some(callback);
                }
            });
            let monitor: *mut AnyObject = msg_send![
                lookup_class(c"NSEvent"),
                addGlobalMonitorForEventsMatchingMask: mask,
                handler: &*block
            ];
            std::mem::forget(block);

            state.media_key_monitor = Some(monitor);
        }
    }

    fn request_user_attention(&self, attention_type: crate::AttentionType) {
        let id = super::dock::request_user_attention(attention_type);
        self.0.lock().attention_request_id = id;
    }

    fn cancel_user_attention(&self) {
        let id = self.0.lock().attention_request_id;
        super::dock::cancel_user_attention(id);
    }

    fn set_dock_badge(&self, label: Option<&str>) {
        super::dock::set_dock_badge(label);
    }

    fn show_context_menu(
        &self,
        position: crate::Point<crate::Pixels>,
        items: Vec<crate::TrayMenuItem>,
        callback: Box<dyn FnMut(crate::SharedString)>,
    ) {
        self.0.lock().context_menu_callback = Some(callback);

        unsafe {
            let menu: *mut AnyObject = msg_send![lookup_class(c"NSMenu"), new];
            let _: () = msg_send![menu, setAutoenablesItems: Bool::NO];
            super::tray::build_menu_with_selector(
                menu,
                &items,
                objc2::runtime::Sel::register(c"handleContextMenuItem:"),
            );

            let main_screen: *mut AnyObject = msg_send![lookup_class(c"NSScreen"), mainScreen];
            let screen_height = if main_screen.is_null() {
                0.0
            } else {
                let frame: NSRect = msg_send![main_screen, frame];
                frame.size.height
            };

            let point = NSPoint::new(position.x.0 as f64, screen_height - position.y.0 as f64);

            let _: () = msg_send![menu, popUpMenuPositioningItem: ptr::null_mut::<AnyObject>(), atLocation: point, inView: ptr::null_mut::<AnyObject>()];
            let _: () = msg_send![menu, release];
        }
    }

    fn show_dialog(
        &self,
        options: crate::DialogOptions,
    ) -> futures::channel::oneshot::Receiver<usize> {
        super::dialog::show_dialog(options)
    }

    fn os_info(&self) -> crate::OsInfo {
        super::os_info::get_os_info()
    }

    fn biometric_status(&self) -> crate::BiometricStatus {
        super::biometric::biometric_status()
    }

    fn authenticate_biometric(&self, reason: &str, callback: Box<dyn FnOnce(bool) + Send>) {
        super::biometric::authenticate_biometric(reason, callback);
    }
}

impl MacPlatform {
    unsafe fn read_string_from_clipboard(
        &self,
        state: &MacPlatformState,
        text_bytes: &[u8],
    ) -> ClipboardItem {
        unsafe {
            let text = String::from_utf8_lossy(text_bytes).to_string();
            let metadata = self
                .read_from_pasteboard(
                    state.pasteboard,
                    ns_string_object(&state.text_hash_pasteboard_type),
                )
                .and_then(|hash_bytes| {
                    let hash_bytes = hash_bytes.try_into().ok()?;
                    let hash = u64::from_be_bytes(hash_bytes);
                    let metadata = self.read_from_pasteboard(
                        state.pasteboard,
                        ns_string_object(&state.metadata_pasteboard_type),
                    )?;

                    if hash == ClipboardString::text_hash(&text) {
                        String::from_utf8(metadata.to_vec()).ok()
                    } else {
                        None
                    }
                });

            ClipboardItem {
                entries: vec![ClipboardEntry::String(ClipboardString { text, metadata })],
            }
        }
    }

    unsafe fn write_plaintext_to_clipboard(&self, string: &ClipboardString) {
        unsafe {
            let state = self.0.lock();
            let _: NSInteger = msg_send![state.pasteboard, clearContents];

            let text_bytes =
                ns_data_with_bytes(string.text.as_ptr() as *const c_void, string.text.len());
            let _: Bool =
                msg_send![state.pasteboard, setData: text_bytes, forType: NSPasteboardTypeString];

            if let Some(metadata) = string.metadata.as_ref() {
                let hash_bytes = ClipboardString::text_hash(&string.text).to_be_bytes();
                let hash_bytes =
                    ns_data_with_bytes(hash_bytes.as_ptr() as *const c_void, hash_bytes.len());
                let _: Bool = msg_send![
                    state.pasteboard,
                    setData: hash_bytes,
                    forType: ns_string_object(&state.text_hash_pasteboard_type)
                ];

                let metadata_bytes =
                    ns_data_with_bytes(metadata.as_ptr() as *const c_void, metadata.len());
                let _: Bool = msg_send![
                    state.pasteboard,
                    setData: metadata_bytes,
                    forType: ns_string_object(&state.metadata_pasteboard_type)
                ];
            }
        }
    }

    unsafe fn write_image_to_clipboard(&self, image: &Image) {
        unsafe {
            let state = self.0.lock();
            let _: NSInteger = msg_send![state.pasteboard, clearContents];

            let bytes =
                ns_data_with_bytes(image.bytes.as_ptr() as *const c_void, image.bytes.len());

            let _: Bool = msg_send![
                state.pasteboard,
                setData: bytes,
                forType: Into::<UTType>::into(image.format).inner()
            ];
        }
    }
}

fn try_clipboard_image(pasteboard: *mut AnyObject, format: ImageFormat) -> Option<ClipboardItem> {
    let ut_type: UTType = format.into();

    unsafe {
        let types: *mut AnyObject = msg_send![pasteboard, types];
        let contains: Bool = msg_send![types, containsObject: ut_type.inner()];
        if contains.as_bool() {
            let data: *mut AnyObject = msg_send![pasteboard, dataForType: ut_type.inner()];
            if data.is_null() {
                None
            } else {
                let bytes_ptr: *const c_void = msg_send![data, bytes];
                let len: usize = msg_send![data, length];
                let bytes = Vec::from(slice::from_raw_parts(bytes_ptr.cast::<u8>(), len));
                let id = hash(&bytes);

                Some(ClipboardItem {
                    entries: vec![ClipboardEntry::Image(Image { format, bytes, id })],
                })
            }
        } else {
            None
        }
    }
}

unsafe fn path_from_objc(path: *mut AnyObject) -> PathBuf {
    let len = msg_send![path, lengthOfBytesUsingEncoding: NSUTF8StringEncoding];
    let bytes: *const u8 = unsafe { msg_send![path, UTF8String] };
    let path = str::from_utf8(unsafe { slice::from_raw_parts(bytes, len) }).unwrap();
    PathBuf::from(path)
}

unsafe fn ns_url_to_path(url: *mut AnyObject) -> Result<PathBuf> {
    let path: *mut c_char = msg_send![url, fileSystemRepresentation];
    let absolute_string: *mut AnyObject = msg_send![url, absoluteString];
    let absolute_string_ptr: *const c_char = msg_send![absolute_string, UTF8String];
    anyhow::ensure!(!path.is_null(), "url is not a file path: {}", unsafe {
        CStr::from_ptr(absolute_string_ptr).to_string_lossy()
    });
    Ok(PathBuf::from(OsStr::from_bytes(unsafe {
        CStr::from_ptr(path).to_bytes()
    })))
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    pub(super) fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    pub(super) fn TISGetInputSourceProperty(
        inputSource: *mut c_void,
        propertyKey: *const c_void,
    ) -> *mut c_void;

    pub(super) fn UCKeyTranslate(
        keyLayoutPtr: *const ::std::os::raw::c_void,
        virtualKeyCode: u16,
        keyAction: u16,
        modifierKeyState: u32,
        keyboardType: u32,
        keyTranslateOptions: u32,
        deadKeyState: *mut u32,
        maxStringLength: usize,
        actualStringLength: *mut usize,
        unicodeString: *mut u16,
    ) -> u32;
    pub(super) fn LMGetKbdType() -> u16;
    pub(super) static kTISPropertyUnicodeKeyLayoutData: CFStringRef;
    pub(super) static kTISPropertyInputSourceID: CFStringRef;
    pub(super) static kTISPropertyLocalizedName: CFStringRef;
}

mod security {
    #![allow(non_upper_case_globals)]
    use super::*;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        pub static kSecClass: CFStringRef;
        pub static kSecClassInternetPassword: CFStringRef;
        pub static kSecAttrServer: CFStringRef;
        pub static kSecAttrAccount: CFStringRef;
        pub static kSecValueData: CFStringRef;
        pub static kSecReturnAttributes: CFStringRef;
        pub static kSecReturnData: CFStringRef;

        pub fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
        pub fn SecItemUpdate(query: CFDictionaryRef, attributes: CFDictionaryRef) -> OSStatus;
        pub fn SecItemDelete(query: CFDictionaryRef) -> OSStatus;
        pub fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
    }

    pub const errSecSuccess: OSStatus = 0;
    pub const errSecUserCanceled: OSStatus = -128;
    pub const errSecItemNotFound: OSStatus = -25300;
}

impl From<ImageFormat> for UTType {
    fn from(value: ImageFormat) -> Self {
        match value {
            ImageFormat::Png => Self::png(),
            ImageFormat::Jpeg => Self::jpeg(),
            ImageFormat::Tiff => Self::tiff(),
            ImageFormat::Webp => Self::webp(),
            ImageFormat::Gif => Self::gif(),
            ImageFormat::Bmp => Self::bmp(),
            ImageFormat::Svg => Self::svg(),
        }
    }
}

// See https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/
struct UTType(Retained<NSString>);

impl UTType {
    pub fn png() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/png
        Self(unsafe {
            Retained::retain(NSPasteboardTypePNG.cast::<NSString>())
                .expect("NSPasteboardTypePNG must be available")
        })
    }

    pub fn jpeg() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/jpeg
        Self(ns_string("public.jpeg"))
    }

    pub fn gif() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/gif
        Self(ns_string("com.compuserve.gif"))
    }

    pub fn webp() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/webp
        Self(ns_string("org.webmproject.webp"))
    }

    pub fn bmp() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/bmp
        Self(ns_string("com.microsoft.bmp"))
    }

    pub fn svg() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/svg
        Self(ns_string("public.svg-image"))
    }

    pub fn tiff() -> Self {
        // https://developer.apple.com/documentation/uniformtypeidentifiers/uttype-swift.struct/tiff
        Self(unsafe {
            Retained::retain(NSPasteboardTypeTIFF.cast::<NSString>())
                .expect("NSPasteboardTypeTIFF must be available")
        })
    }

    fn inner(&self) -> *mut AnyObject {
        (&*self.0 as *const NSString).cast_mut().cast()
    }
}

#[cfg(test)]
mod tests {
    use crate::ClipboardItem;

    use super::*;

    #[test]
    fn test_clipboard() {
        let _guard = crate::platform::mac::mac_appkit_test_lock().lock().unwrap();

        let platform = build_platform();
        assert_eq!(platform.read_from_clipboard(), None);

        let item = ClipboardItem::new_string("1".to_string());
        platform.write_to_clipboard(item.clone());
        assert_eq!(platform.read_from_clipboard(), Some(item));

        let item = ClipboardItem {
            entries: vec![ClipboardEntry::String(
                ClipboardString::new("2".to_string()).with_json_metadata(vec![3, 4]),
            )],
        };
        platform.write_to_clipboard(item.clone());
        assert_eq!(platform.read_from_clipboard(), Some(item));

        let text_from_other_app = "text from other app";
        unsafe {
            let bytes = ns_data_with_bytes(
                text_from_other_app.as_ptr() as *const c_void,
                text_from_other_app.len(),
            );
            let pasteboard = platform.0.lock().pasteboard;
            let _: Bool = msg_send![pasteboard, setData: bytes, forType: NSPasteboardTypeString];
        }
        assert_eq!(
            platform.read_from_clipboard(),
            Some(ClipboardItem::new_string(text_from_other_app.to_string()))
        );
    }

    fn build_platform() -> MacPlatform {
        let platform = MacPlatform::new(false);
        platform.0.lock().pasteboard =
            unsafe { msg_send![lookup_class(c"NSPasteboard"), pasteboardWithUniqueName] };
        platform
    }
}
