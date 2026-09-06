use super::{BoolExt, MacDisplay, NSRange, NSRangeExt, NSStringExt, ns_string, renderer};
#[cfg(feature = "macos-blade")]
use crate::platform::encode_bgra_png;
#[cfg(not(feature = "macos-blade"))]
use crate::platform::encode_premultiplied_bgra_png;
use crate::{
    AnyWindowHandle, AsyncWindowContext, Bounds, Capslock, DisplayLink, Edges, ExternalDropData,
    FileDropEvent, ForegroundExecutor, GameInputAvailability, GameInputCapabilities,
    GameInputError, GameInputErrorKind, KeyDownEvent, Keystroke, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformWindow, Point, PointerInputEvent, PointerLockStatus,
    PromptButton, PromptLevel, RenderImage, RequestFrameOptions, Rgba, SharedString, Size,
    SystemWindowTab, Timer, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
    WindowControlArea, WindowKind, WindowParams, dispatch_get_main_queue,
    dispatch_sys::dispatch_async_f,
    platform::PlatformInputHandler,
    point,
    print::{PlatformPrintJob, PlatformPrintPage, PrintCommand, PrintImageFit},
    px, size,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand, WebViewCookie,
        WebViewDocumentTitleChangedHandler, WebViewDownloadCompleted,
        WebViewDownloadCompletedHandler, WebViewDownloadPolicy, WebViewDownloadStartedHandler,
        WebViewDragDropEvent, WebViewDragDropHandler, WebViewDragDropPolicy, WebViewMessageHandler,
        WebViewNativePermissionRequest, WebViewNavigationHandler, WebViewNewWindowHandler,
        WebViewNewWindowPolicy, WebViewPageLoadEvent, WebViewPageLoadHandler,
        WebViewPermissionDecision, WebViewPermissionFrame, WebViewPermissionHandler,
        WebViewPermissionKind,
    },
};
use anyhow::Context as _;
use block2::{Block, RcBlock};

use core_graphics::{
    display::{CGDirectDisplayID, CGDisplay},
    event::CGEvent,
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::CGPoint,
};
use ctor::ctor;
use futures::channel::oneshot;
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba as ImageRgba};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::*;
use objc2_foundation::{
    NSInteger, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize, NSString,
    NSUInteger, NSUUID,
};
use parking_lot::Mutex;
use raw_window_handle as rwh;
use smallvec::SmallVec;
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    ffi::{CStr, CString, c_void},
    io::Cursor,
    mem,
    ops::Range,
    path::PathBuf,
    ptr::{self, NonNull},
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use util::ResultExt;

const WINDOW_STATE_IVAR: &str = "windowState";
const WEBVIEW_STATE_IVAR: &str = "webViewState";
const PRINT_VIEW_STATE_IVAR: &str = "printViewState";
const WEBVIEW_MESSAGE_HANDLER_NAME: &str = "gpui";
const WEBVIEW_CLIPBOARD_BRIDGE_KIND: &str = "__kaelClipboard";

#[allow(non_camel_case_types)]
type id = *mut AnyObject;
type Object = AnyObject;
type Class = AnyClass;
type BOOL = Bool;
type Method0<R> = extern "C" fn(id, Sel) -> R;
type Method1<A, R> = extern "C" fn(id, Sel, A) -> R;
type Method2<A, B, R> = extern "C" fn(id, Sel, A, B) -> R;
type Method3<A, B, C, R> = extern "C" fn(id, Sel, A, B, C) -> R;
type Method4<A, B, C, D, R> = extern "C" fn(id, Sel, A, B, C, D) -> R;
type Method5<A, B, C, D, E, R> = extern "C" fn(id, Sel, A, B, C, D, E) -> R;

fn catch_platform_callback<T>(name: &'static str, fallback: T, callback: impl FnOnce() -> T) -> T {
    crate::platform::catch_platform_callback("macOS", name, fallback, callback)
}

fn dispatch_webview_completion(
    async_window: AsyncWindowContext,
    name: &'static str,
    completion: impl FnOnce() + 'static,
) {
    catch_platform_callback("webview completion dispatch", (), || {
        async_window
            .spawn(async move |_| {
                catch_platform_callback(name, (), completion);
            })
            .detach();
    });
}

const YES: Bool = Bool::YES;
const NO: Bool = Bool::NO;
#[allow(non_upper_case_globals)]
const nil: id = ptr::null_mut();
const NS_KEY_VALUE_OBSERVING_OPTION_NEW: NSUInteger = 1;
const WK_MEDIA_PLAYBACK_TYPE_NONE: NSUInteger = 0;
const WK_MEDIA_PLAYBACK_TYPE_ALL: NSUInteger = NSUInteger::MAX;
const NS_ALERT_FIRST_BUTTON_RETURN: NSInteger = 1_000;

static mut WINDOW_CLASS: *const Class = ptr::null();
static mut PANEL_CLASS: *const Class = ptr::null();
static mut VIEW_CLASS: *const Class = ptr::null();
static mut BLURRED_VIEW_CLASS: *const Class = ptr::null();
static mut WEBVIEW_CLASS: *const Class = ptr::null();
static mut WEBVIEW_DELEGATE_CLASS: *const Class = ptr::null();
static mut PRINT_VIEW_CLASS: *const Class = ptr::null();

/// CoreGraphics cursor association and AppKit's cursor-hide count are process
/// global. Serialize ownership so two Kael windows can never leak or unbalance
/// another window's lock.
static MAC_POINTER_LOCK_OWNER: Mutex<Option<usize>> = Mutex::new(None);

unsafe fn lookup_class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing class {name:?}"))
}

unsafe fn ivar_ptr<T: objc2::encode::Encode>(object: id, name: &str) -> *mut T {
    assert!(!object.is_null(), "cannot access `{name}` on a null object");
    let name = CString::new(name).expect("ivar names cannot contain nul bytes");
    let object = unsafe { &*object };
    let ivar = object
        .class()
        .instance_variable(name.as_c_str())
        .unwrap_or_else(|| panic!("missing ivar {name:?}"));
    unsafe { ivar.load_ptr::<T>(object) }
}

unsafe fn load_ivar<T: objc2::encode::Encode + Copy>(object: id, name: &str) -> T {
    unsafe { *ivar_ptr::<T>(object, name) }
}

unsafe fn store_ivar<T: objc2::encode::Encode>(object: id, name: &str, value: T) {
    unsafe { ivar_ptr::<T>(object, name).write(value) };
}

#[allow(non_snake_case)]
trait ObjcObjectExt {
    unsafe fn screen(self) -> id;
    unsafe fn styleMask(self) -> NSWindowStyleMask;
    unsafe fn contentView(self) -> id;
    unsafe fn frame(self) -> NSRect;
    unsafe fn bounds(self) -> NSRect;
    unsafe fn initWithFrame_(self, frame: NSRect) -> id;
    unsafe fn removeFromSuperview(self);
    unsafe fn occlusionState(self) -> NSWindowOcclusionState;
    unsafe fn visibleFrame(self) -> NSRect;
    unsafe fn windowNumber(self) -> NSInteger;
    unsafe fn mouseLocationOutsideOfEventStream(self) -> NSPoint;
    unsafe fn autorelease(self) -> id;
    unsafe fn isKeyWindow(self) -> BOOL;
    unsafe fn setDelegate_(self, delegate: id);
    unsafe fn setMovable_(self, movable: BOOL);
    unsafe fn setContentMinSize_(self, size: NSSize);
    unsafe fn setTitlebarAppearsTransparent_(self, transparent: BOOL);
    unsafe fn setTitleVisibility_(self, visibility: NSWindowTitleVisibility);
    unsafe fn setAutoresizingMask_(self, mask: NSAutoresizingMaskOptions);
    unsafe fn setWantsLayer(self, enabled: BOOL);
    unsafe fn setWantsBestResolutionOpenGLSurface_(self, enabled: BOOL);
    unsafe fn addSubview_(self, view: id);
    unsafe fn makeFirstResponder_(self, responder: id) -> BOOL;
    unsafe fn setLevel_(self, level: NSInteger);
    unsafe fn setAcceptsMouseMovedEvents_(self, accepts: BOOL);
    unsafe fn setCollectionBehavior_(self, behavior: NSWindowCollectionBehavior);
    unsafe fn setContentSize_(self, size: NSSize);
    unsafe fn setOpaque_(self, opaque: BOOL);
    unsafe fn setBackgroundColor_(self, color: id);
    unsafe fn toggleFullScreen_(self, sender: id);
    unsafe fn zoom_(self, sender: id);
}

#[allow(non_snake_case)]
impl ObjcObjectExt for id {
    unsafe fn screen(self) -> id {
        unsafe { msg_send![self, screen] }
    }

    unsafe fn styleMask(self) -> NSWindowStyleMask {
        unsafe { msg_send![self, styleMask] }
    }

    unsafe fn contentView(self) -> id {
        unsafe { msg_send![self, contentView] }
    }

    unsafe fn frame(self) -> NSRect {
        unsafe { msg_send![self, frame] }
    }

    unsafe fn bounds(self) -> NSRect {
        unsafe { msg_send![self, bounds] }
    }

    unsafe fn initWithFrame_(self, frame: NSRect) -> id {
        unsafe { msg_send![self, initWithFrame: frame] }
    }

    unsafe fn removeFromSuperview(self) {
        unsafe { msg_send![self, removeFromSuperview] }
    }

    unsafe fn occlusionState(self) -> NSWindowOcclusionState {
        unsafe { msg_send![self, occlusionState] }
    }

    unsafe fn visibleFrame(self) -> NSRect {
        unsafe { msg_send![self, visibleFrame] }
    }

    unsafe fn windowNumber(self) -> NSInteger {
        unsafe { msg_send![self, windowNumber] }
    }

    unsafe fn mouseLocationOutsideOfEventStream(self) -> NSPoint {
        unsafe { msg_send![self, mouseLocationOutsideOfEventStream] }
    }

    unsafe fn autorelease(self) -> id {
        unsafe { msg_send![self, autorelease] }
    }

    unsafe fn isKeyWindow(self) -> BOOL {
        unsafe { msg_send![self, isKeyWindow] }
    }

    unsafe fn setDelegate_(self, delegate: id) {
        unsafe { msg_send![self, setDelegate: delegate] }
    }

    unsafe fn setMovable_(self, movable: BOOL) {
        unsafe { msg_send![self, setMovable: movable] }
    }

    unsafe fn setContentMinSize_(self, size: NSSize) {
        unsafe { msg_send![self, setContentMinSize: size] }
    }

    unsafe fn setTitlebarAppearsTransparent_(self, transparent: BOOL) {
        unsafe { msg_send![self, setTitlebarAppearsTransparent: transparent] }
    }

    unsafe fn setTitleVisibility_(self, visibility: NSWindowTitleVisibility) {
        unsafe { msg_send![self, setTitleVisibility: visibility] }
    }

    unsafe fn setAutoresizingMask_(self, mask: NSAutoresizingMaskOptions) {
        unsafe { msg_send![self, setAutoresizingMask: mask] }
    }

    unsafe fn setWantsLayer(self, enabled: BOOL) {
        unsafe { msg_send![self, setWantsLayer: enabled] }
    }

    unsafe fn setWantsBestResolutionOpenGLSurface_(self, enabled: BOOL) {
        unsafe { msg_send![self, setWantsBestResolutionOpenGLSurface: enabled] }
    }

    unsafe fn addSubview_(self, view: id) {
        unsafe { msg_send![self, addSubview: view] }
    }

    unsafe fn makeFirstResponder_(self, responder: id) -> BOOL {
        unsafe { msg_send![self, makeFirstResponder: responder] }
    }

    unsafe fn setLevel_(self, level: NSInteger) {
        unsafe { msg_send![self, setLevel: level] }
    }

    unsafe fn setAcceptsMouseMovedEvents_(self, accepts: BOOL) {
        unsafe { msg_send![self, setAcceptsMouseMovedEvents: accepts] }
    }

    unsafe fn setCollectionBehavior_(self, behavior: NSWindowCollectionBehavior) {
        unsafe { msg_send![self, setCollectionBehavior: behavior] }
    }

    unsafe fn setContentSize_(self, size: NSSize) {
        unsafe { msg_send![self, setContentSize: size] }
    }

    unsafe fn setOpaque_(self, opaque: BOOL) {
        unsafe { msg_send![self, setOpaque: opaque] }
    }

    unsafe fn setBackgroundColor_(self, color: id) {
        unsafe { msg_send![self, setBackgroundColor: color] }
    }

    unsafe fn toggleFullScreen_(self, sender: id) {
        unsafe { msg_send![self, toggleFullScreen: sender] }
    }

    unsafe fn zoom_(self, sender: id) {
        unsafe { msg_send![self, zoom: sender] }
    }
}

#[allow(non_upper_case_globals)]
const NSWindowStyleMaskNonactivatingPanel: NSWindowStyleMask =
    NSWindowStyleMask::from_bits_retain(1 << 7);
#[allow(non_upper_case_globals)]
const NSBackingStoreBuffered: NSBackingStoreType = NSBackingStoreType::Buffered;
#[allow(non_upper_case_globals)]
const NSViewWidthSizable: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions::ViewWidthSizable;
#[allow(non_upper_case_globals)]
const NSViewHeightSizable: NSAutoresizingMaskOptions = NSAutoresizingMaskOptions::ViewHeightSizable;
#[allow(non_upper_case_globals)]
const NSNormalWindowLevel: NSInteger = 0;
#[allow(non_upper_case_globals)]
const NSPopUpWindowLevel: NSInteger = 101;
/// `kCGDesktopWindowLevel` - the lowest level, behind desktop icons. macOS's nearest
/// equivalent to wlr-layer-shell's `Wallpaper`/background layer.
#[allow(non_upper_case_globals)]
const NSDesktopWindowLevel: NSInteger = -2147483648;
#[allow(non_upper_case_globals)]
const NSTrackingMouseEnteredAndExited: NSUInteger = 0x01;
#[allow(non_upper_case_globals)]
const NSTrackingMouseMoved: NSUInteger = 0x02;
#[allow(non_upper_case_globals)]
const NSTrackingActiveAlways: NSUInteger = 0x80;
#[allow(non_upper_case_globals)]
const NSTrackingInVisibleRect: NSUInteger = 0x200;
#[allow(non_upper_case_globals)]
const NSWindowAnimationBehaviorUtilityWindow: NSInteger = 4;
#[allow(non_upper_case_globals)]
const NSViewLayerContentsRedrawDuringViewResize: NSInteger = 2;
#[allow(non_upper_case_globals)]
const WKNavigationActionPolicyCancel: NSInteger = 0;
#[allow(non_upper_case_globals)]
const WKNavigationActionPolicyAllow: NSInteger = 1;
#[allow(non_upper_case_globals)]
const WKNavigationActionPolicyDownload: NSInteger = 2;
#[allow(non_upper_case_globals)]
const WKNavigationResponsePolicyAllow: NSInteger = 1;
#[allow(non_upper_case_globals)]
const WKNavigationResponsePolicyDownload: NSInteger = 2;
#[allow(non_upper_case_globals)]
const WKUserScriptInjectionTimeAtDocumentStart: NSInteger = 0;
#[allow(non_upper_case_globals)]
const WKUserScriptInjectionTimeAtDocumentEnd: NSInteger = 1;
#[allow(non_upper_case_globals)]
const NSPaperOrientationPortrait: NSInteger = 0;
#[allow(non_upper_case_globals)]
const NSPaperOrientationLandscape: NSInteger = 1;
#[allow(non_upper_case_globals)]
const NSStringDrawingUsesLineFragmentOrigin: NSUInteger = 1 << 0;
#[allow(non_upper_case_globals)]
const NSStringDrawingUsesFontLeading: NSUInteger = 1 << 1;
#[allow(non_upper_case_globals)]
const NSCompositingOperationSourceOver: NSInteger = 2;
// https://developer.apple.com/documentation/appkit/nsdragoperation
type NSDragOperation = NSUInteger;
#[allow(non_upper_case_globals)]
const NSDragOperationNone: NSDragOperation = 0;
#[allow(non_upper_case_globals)]
const NSDragOperationCopy: NSDragOperation = 1;

#[repr(transparent)]
struct NSRangePointer(*mut NSRange);

unsafe impl objc2::encode::Encode for NSRangePointer {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(&NSRange::ENCODING);
}

#[derive(PartialEq)]
pub enum UserTabbingPreference {
    Never,
    Always,
    InFullScreen,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    // Widely used private APIs; Apple uses them for their Terminal.app.
    fn CGSMainConnectionID() -> id;
    fn CGSSetWindowBackgroundBlurRadius(
        connection_id: id,
        window_id: NSInteger,
        radius: i64,
    ) -> i32;
}

#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

#[ctor(unsafe)]
unsafe fn build_classes() {
    unsafe {
        WINDOW_CLASS = build_window_class(c"GPUIWindow", lookup_class(c"NSWindow"));
        PANEL_CLASS = build_window_class(c"GPUIPanel", lookup_class(c"NSPanel"));
        VIEW_CLASS = {
            let mut decl = ClassBuilder::new(c"GPUIView", lookup_class(c"NSView")).unwrap();
            decl.add_ivar::<*mut c_void>(c"windowState");
            {
                let dealloc_view = dealloc_view as Method0<()>;
                let handle_key_equivalent = handle_key_equivalent as Method1<id, BOOL>;
                let handle_key_down = handle_key_down as Method1<id, ()>;
                let handle_key_up = handle_key_up as Method1<id, ()>;
                let handle_view_event = handle_view_event as Method1<id, ()>;
                let make_backing_layer = make_backing_layer as Method0<id>;
                let view_did_change_backing_properties =
                    view_did_change_backing_properties as Method0<()>;
                let set_frame_size = set_frame_size as Method1<NSSize, ()>;
                let display_layer = display_layer as Method1<id, ()>;
                let valid_attributes_for_marked_text =
                    valid_attributes_for_marked_text as Method0<id>;
                let has_marked_text = has_marked_text as Method0<BOOL>;
                let marked_range = marked_range as Method0<NSRange>;
                let selected_range = selected_range as Method0<NSRange>;
                let first_rect_for_character_range =
                    first_rect_for_character_range as Method2<NSRange, id, NSRect>;
                let insert_text = insert_text as Method2<id, NSRange, ()>;
                let set_marked_text = set_marked_text as Method3<id, NSRange, NSRange, ()>;
                let unmark_text = unmark_text as Method0<()>;
                let attributed_substring_for_proposed_range =
                    attributed_substring_for_proposed_range as Method2<NSRange, *mut c_void, id>;
                let view_did_change_effective_appearance =
                    view_did_change_effective_appearance as Method0<()>;
                let do_command_by_selector = do_command_by_selector as Method1<Sel, ()>;
                let accepts_first_mouse = accepts_first_mouse as Method1<id, BOOL>;
                let character_index_for_point = character_index_for_point as Method1<NSPoint, u64>;

                decl.add_method(sel!(dealloc), dealloc_view);

                decl.add_method(sel!(performKeyEquivalent:), handle_key_equivalent);
                decl.add_method(sel!(keyDown:), handle_key_down);
                decl.add_method(sel!(keyUp:), handle_key_up);
                decl.add_method(sel!(mouseDown:), handle_view_event);
                decl.add_method(sel!(mouseUp:), handle_view_event);
                decl.add_method(sel!(rightMouseDown:), handle_view_event);
                decl.add_method(sel!(rightMouseUp:), handle_view_event);
                decl.add_method(sel!(otherMouseDown:), handle_view_event);
                decl.add_method(sel!(otherMouseUp:), handle_view_event);
                decl.add_method(sel!(mouseMoved:), handle_view_event);
                decl.add_method(sel!(mouseExited:), handle_view_event);
                decl.add_method(sel!(mouseDragged:), handle_view_event);
                decl.add_method(sel!(tabletPoint:), handle_view_event);
                decl.add_method(sel!(tabletProximity:), handle_view_event);
                decl.add_method(sel!(scrollWheel:), handle_view_event);
                decl.add_method(sel!(swipeWithEvent:), handle_view_event);
                decl.add_method(sel!(magnifyWithEvent:), handle_view_event);
                decl.add_method(sel!(flagsChanged:), handle_view_event);

                decl.add_method(sel!(makeBackingLayer), make_backing_layer);

                decl.add_protocol(AnyProtocol::get(c"CALayerDelegate").unwrap());
                decl.add_method(
                    sel!(viewDidChangeBackingProperties),
                    view_did_change_backing_properties,
                );
                decl.add_method(sel!(setFrameSize:), set_frame_size);
                decl.add_method(sel!(displayLayer:), display_layer);

                decl.add_protocol(AnyProtocol::get(c"NSTextInputClient").unwrap());
                decl.add_method(
                    sel!(validAttributesForMarkedText),
                    valid_attributes_for_marked_text,
                );
                decl.add_method(sel!(hasMarkedText), has_marked_text);
                decl.add_method(sel!(markedRange), marked_range);
                decl.add_method(sel!(selectedRange), selected_range);
                decl.add_method(
                    sel!(firstRectForCharacterRange:actualRange:),
                    first_rect_for_character_range,
                );
                decl.add_method(sel!(insertText:replacementRange:), insert_text);
                decl.add_method(
                    sel!(setMarkedText:selectedRange:replacementRange:),
                    set_marked_text,
                );
                decl.add_method(sel!(unmarkText), unmark_text);
                decl.add_method(
                    sel!(attributedSubstringForProposedRange:actualRange:),
                    attributed_substring_for_proposed_range,
                );
                decl.add_method(
                    sel!(viewDidChangeEffectiveAppearance),
                    view_did_change_effective_appearance,
                );

                // Suppress beep on keystrokes with modifier keys.
                decl.add_method(sel!(doCommandBySelector:), do_command_by_selector);

                decl.add_method(sel!(acceptsFirstMouse:), accepts_first_mouse);

                decl.add_method(sel!(characterIndexForPoint:), character_index_for_point);
            }
            decl.register()
        };
        BLURRED_VIEW_CLASS = {
            let mut decl =
                ClassBuilder::new(c"BlurredView", lookup_class(c"NSVisualEffectView")).unwrap();
            {
                let blurred_view_init_with_frame =
                    blurred_view_init_with_frame as Method1<NSRect, id>;
                let blurred_view_update_layer = blurred_view_update_layer as Method0<()>;

                decl.add_method(sel!(initWithFrame:), blurred_view_init_with_frame);
                decl.add_method(sel!(updateLayer), blurred_view_update_layer);
                decl.register()
            }
        };
        WEBVIEW_CLASS = {
            let mut decl = ClassBuilder::new(c"GPUIWebView", lookup_class(c"WKWebView")).unwrap();
            decl.add_ivar::<*mut c_void>(c"webViewState");
            {
                let webview_dragging_entered =
                    webview_dragging_entered as Method1<id, NSDragOperation>;
                let webview_dragging_updated =
                    webview_dragging_updated as Method1<id, NSDragOperation>;
                let webview_dragging_exited = webview_dragging_exited as Method1<id, ()>;
                let webview_dragging_ended = webview_dragging_ended as Method1<id, ()>;
                let webview_perform_drag_operation =
                    webview_perform_drag_operation as Method1<id, BOOL>;
                let webview_key_down = webview_key_down as Method1<id, ()>;
                let webview_magnify = webview_magnify as Method1<id, ()>;

                decl.add_method(sel!(keyDown:), webview_key_down);
                decl.add_method(sel!(magnifyWithEvent:), webview_magnify);
                decl.add_method(sel!(draggingEntered:), webview_dragging_entered);
                decl.add_method(sel!(draggingUpdated:), webview_dragging_updated);
                decl.add_method(sel!(draggingExited:), webview_dragging_exited);
                decl.add_method(sel!(draggingEnded:), webview_dragging_ended);
                decl.add_method(sel!(performDragOperation:), webview_perform_drag_operation);
                decl.register()
            }
        };
        WEBVIEW_DELEGATE_CLASS = {
            let mut decl =
                ClassBuilder::new(c"GPUIWebViewDelegate", lookup_class(c"NSObject")).unwrap();
            decl.add_ivar::<*mut c_void>(c"webViewState");
            {
                let dealloc_webview_delegate = dealloc_webview_delegate as Method0<()>;
                let webview_did_receive_script_message =
                    webview_did_receive_script_message as Method2<id, id, ()>;
                let webview_decide_policy_for_navigation_action =
                    webview_decide_policy_for_navigation_action as Method3<id, id, id, ()>;
                let webview_decide_policy_for_navigation_response =
                    webview_decide_policy_for_navigation_response as Method3<id, id, id, ()>;
                let webview_navigation_action_did_become_download =
                    webview_navigation_action_did_become_download as Method3<id, id, id, ()>;
                let webview_navigation_response_did_become_download =
                    webview_navigation_response_did_become_download as Method3<id, id, id, ()>;
                let webview_create_webview_with_configuration =
                    webview_create_webview_with_configuration as Method4<id, id, id, id, id>;
                let webview_request_media_capture_permission =
                    webview_request_media_capture_permission
                        as Method5<id, id, id, NSInteger, id, ()>;
                let webview_did_close = webview_did_close as Method1<id, ()>;
                let webview_download_decide_destination =
                    webview_download_decide_destination as Method4<id, id, id, id, ()>;
                let webview_download_did_finish = webview_download_did_finish as Method1<id, ()>;
                let webview_download_did_fail =
                    webview_download_did_fail as Method3<id, id, id, ()>;
                let webview_download_did_receive_final_url =
                    webview_download_did_receive_final_url as Method2<id, id, ()>;
                let webview_did_start_provisional_navigation =
                    webview_did_start_provisional_navigation as Method2<id, id, ()>;
                let webview_did_finish_navigation =
                    webview_did_finish_navigation as Method2<id, id, ()>;
                let webview_observe_value_for_key_path =
                    webview_observe_value_for_key_path as Method4<id, id, id, *mut c_void, ()>;
                let webview_start_url_scheme_task =
                    webview_start_url_scheme_task as Method2<id, id, ()>;
                let webview_stop_url_scheme_task =
                    webview_stop_url_scheme_task as Method2<id, id, ()>;

                decl.add_method(sel!(dealloc), dealloc_webview_delegate);
                if let Some(protocol) = AnyProtocol::get(c"WKNavigationDelegate") {
                    decl.add_protocol(protocol);
                }
                if let Some(protocol) = AnyProtocol::get(c"WKDownloadDelegate") {
                    decl.add_protocol(protocol);
                }
                if let Some(protocol) = AnyProtocol::get(c"WKUIDelegate") {
                    decl.add_protocol(protocol);
                }
                if let Some(protocol) = AnyProtocol::get(c"WKScriptMessageHandler") {
                    decl.add_protocol(protocol);
                }
                if let Some(protocol) = AnyProtocol::get(c"WKURLSchemeHandler") {
                    decl.add_protocol(protocol);
                }
                decl.add_method(
                    sel!(userContentController:didReceiveScriptMessage:),
                    webview_did_receive_script_message,
                );
                decl.add_method(
                    sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
                    webview_decide_policy_for_navigation_action,
                );
                decl.add_method(
                    sel!(webView:decidePolicyForNavigationResponse:decisionHandler:),
                    webview_decide_policy_for_navigation_response,
                );
                decl.add_method(
                    sel!(webView:navigationAction:didBecomeDownload:),
                    webview_navigation_action_did_become_download,
                );
                decl.add_method(
                    sel!(webView:navigationResponse:didBecomeDownload:),
                    webview_navigation_response_did_become_download,
                );
                decl.add_method(
                    sel!(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:),
                    webview_create_webview_with_configuration,
                );
                decl.add_method(
                    sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:),
                    webview_request_media_capture_permission,
                );
                decl.add_method(sel!(webViewDidClose:), webview_did_close);
                decl.add_method(
                    sel!(download:decideDestinationUsingResponse:suggestedFilename:completionHandler:),
                    webview_download_decide_destination,
                );
                decl.add_method(sel!(downloadDidFinish:), webview_download_did_finish);
                decl.add_method(
                    sel!(download:didFailWithError:resumeData:),
                    webview_download_did_fail,
                );
                decl.add_method(
                    sel!(download:didReceiveFinalURL:),
                    webview_download_did_receive_final_url,
                );
                decl.add_method(
                    sel!(webView:didStartProvisionalNavigation:),
                    webview_did_start_provisional_navigation,
                );
                decl.add_method(
                    sel!(webView:didFinishNavigation:),
                    webview_did_finish_navigation,
                );
                decl.add_method(
                    sel!(observeValueForKeyPath:ofObject:change:context:),
                    webview_observe_value_for_key_path,
                );
                decl.add_method(
                    sel!(webView:startURLSchemeTask:),
                    webview_start_url_scheme_task,
                );
                decl.add_method(
                    sel!(webView:stopURLSchemeTask:),
                    webview_stop_url_scheme_task,
                );
                decl.register()
            }
        };
        PRINT_VIEW_CLASS = {
            let mut decl = ClassBuilder::new(c"GPUIPrintView", lookup_class(c"NSView")).unwrap();
            decl.add_ivar::<*mut c_void>(c"printViewState");
            {
                let dealloc_print_view = dealloc_print_view as Method0<()>;
                let yes = yes as Method0<BOOL>;
                let print_view_knows_page_range =
                    print_view_knows_page_range as Method1<NSRangePointer, BOOL>;
                let print_view_rect_for_page =
                    print_view_rect_for_page as Method1<NSInteger, NSRect>;
                let draw_print_view = draw_print_view as Method1<NSRect, ()>;

                decl.add_method(sel!(dealloc), dealloc_print_view);
                decl.add_method(sel!(isFlipped), yes);
                decl.add_method(sel!(knowsPageRange:), print_view_knows_page_range);
                decl.add_method(sel!(rectForPage:), print_view_rect_for_page);
                decl.add_method(sel!(drawRect:), draw_print_view);
                decl.register()
            }
        };
    }
}

pub(crate) fn convert_mouse_position(position: NSPoint, window_height: Pixels) -> Point<Pixels> {
    point(
        px(position.x as f32),
        // macOS screen coordinates are relative to bottom left
        window_height - px(position.y as f32),
    )
}

fn webview_command_id(command: &PlatformWebViewCommand) -> SharedString {
    match command {
        PlatformWebViewCommand::Navigate { id, .. }
        | PlatformWebViewCommand::NavigateWithHeaders { id, .. }
        | PlatformWebViewCommand::LoadHtml { id, .. }
        | PlatformWebViewCommand::EvaluateJavaScript { id, .. }
        | PlatformWebViewCommand::EvaluateJavaScriptWithResult { id, .. }
        | PlatformWebViewCommand::PostMessage { id, .. }
        | PlatformWebViewCommand::Reload { id }
        | PlatformWebViewCommand::GoBack { id }
        | PlatformWebViewCommand::GoForward { id }
        | PlatformWebViewCommand::OpenDevTools { id }
        | PlatformWebViewCommand::CloseDevTools { id }
        | PlatformWebViewCommand::IsDevToolsOpen { id, .. }
        | PlatformWebViewCommand::Print { id }
        | PlatformWebViewCommand::SetZoomFactor { id, .. }
        | PlatformWebViewCommand::Focus { id }
        | PlatformWebViewCommand::FocusParent { id }
        | PlatformWebViewCommand::ClearBrowsingData { id }
        | PlatformWebViewCommand::ReadUrl { id, .. }
        | PlatformWebViewCommand::ReadCookies { id, .. }
        | PlatformWebViewCommand::SetCookie { id, .. }
        | PlatformWebViewCommand::DeleteCookie { id, .. } => id.clone(),
    }
}

unsafe fn ns_rect_from_bounds(bounds: Bounds<Pixels>, content_height: Pixels) -> NSRect {
    NSRect::new(
        NSPoint::new(
            bounds.origin.x.0 as f64,
            (content_height - bounds.origin.y - bounds.size.height).0 as f64,
        ),
        NSSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64),
    )
}

unsafe fn add_webview_user_script(controller: id, source: &str, injection_time: NSInteger) {
    unsafe {
        let user_script: id = msg_send![lookup_class(c"WKUserScript"), alloc];
        let user_script: id = msg_send![
            user_script,
            initWithSource: ns_string(source),
            injectionTime: injection_time,
            forMainFrameOnly: YES
        ];
        let _: () = msg_send![controller, addUserScript: user_script];
        let _: id = msg_send![user_script, autorelease];
    }
}

fn json_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn webview_bridge_script(storage_key: Option<&SharedString>, nonce: &str) -> String {
    let storage_key = storage_key
        .map(|storage_key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string_literal(storage_key.as_ref())
            )
        })
        .unwrap_or_default();

    let nonce = json_string_literal(nonce);
    format!(
        "(() => {{ {storage_key} const nonce = {nonce}; if (!window.external) {{ window.external = {{}}; }} window.external.invoke = function(message) {{ const body = typeof message === 'string' ? message : JSON.stringify(message); window.webkit.messageHandlers.{WEBVIEW_MESSAGE_HANDLER_NAME}.postMessage(JSON.stringify({{ __kaelIpcNonce: nonce, body }})); }}; if (!window.gpui) {{ window.gpui = {{}}; }} window.gpui.postMessage = function(message) {{ window.external.invoke(message); }}; }})();"
    )
}

fn decode_mac_webview_bridge_message(
    payload: &serde_json::Value,
    nonce: &str,
) -> Option<serde_json::Value> {
    if payload.get("__kaelIpcNonce")?.as_str()? != nonce {
        return None;
    }
    let body = payload.get("body")?.as_str()?;
    Some(serde_json::from_str(body).unwrap_or_else(|_| serde_json::Value::String(body.to_owned())))
}

fn webview_css_script(css: &str) -> String {
    format!(
        "(() => {{ const mount = () => {{ if (!document.head) {{ return; }} const style = document.createElement('style'); style.setAttribute('data-gpui-webview-style', 'true'); style.textContent = {}; document.head.appendChild(style); }}; if (document.head) {{ mount(); }} else {{ document.addEventListener('DOMContentLoaded', mount, {{ once: true }}); }} }})();",
        json_string_literal(css)
    )
}

fn webview_clipboard_script(nonce: &str) -> String {
    let nonce = json_string_literal(nonce);
    format!(
        "(() => {{
            if (!window.webkit || !window.webkit.messageHandlers || !window.webkit.messageHandlers.{WEBVIEW_MESSAGE_HANDLER_NAME}) {{ return; }}
            const handler = window.webkit.messageHandlers.{WEBVIEW_MESSAGE_HANDLER_NAME};
            const nonce = {nonce};
            const pending = new Map();
            let nextId = 1;
            const send = (op, value) => new Promise((resolve, reject) => {{
                const id = String(nextId++);
                pending.set(id, {{ resolve, reject }});
                handler.postMessage(JSON.stringify({{ __kaelIpcNonce: nonce, __kaelClipboard: true, id, op, value }}));
            }});
            const bridge = {{
                readText: () => send('readText'),
                writeText: value => send('writeText', String(value ?? '')),
            }};
            const makeTextItem = text => {{
                const blob = typeof Blob === 'function' ? new Blob([text], {{ type: 'text/plain' }}) : null;
                if (typeof ClipboardItem === 'function' && blob) {{
                    return new ClipboardItem({{ 'text/plain': blob }});
                }}
                return {{
                    types: ['text/plain'],
                    getType(type) {{
                        if (type !== 'text/plain' || !blob) {{ return Promise.reject(new DOMException('Clipboard type unavailable', 'NotFoundError')); }}
                        return Promise.resolve(blob);
                    }}
                }};
            }};
            bridge.read = () => bridge.readText().then(text => [makeTextItem(text)]);
            bridge.write = async items => {{
                for (const item of Array.from(items || [])) {{
                    if (!item || !Array.from(item.types || []).includes('text/plain')) {{ continue; }}
                    const value = await item.getType('text/plain');
                    const text = typeof value === 'string' ? value : await value.text();
                    await bridge.writeText(text);
                    return;
                }}
                throw new DOMException('Only text/plain clipboard items are supported', 'NotAllowedError');
            }};
            const selectedText = () => {{
                const active = document.activeElement;
                if (active && typeof active.value === 'string' && typeof active.selectionStart === 'number' && typeof active.selectionEnd === 'number') {{
                    return active.value.slice(active.selectionStart, active.selectionEnd);
                }}
                const selection = window.getSelection && window.getSelection();
                return selection ? String(selection) : '';
            }};
            Object.defineProperty(window, '__kaelClipboardBridge', {{
                configurable: true,
                value: {{
                    resolve(id, ok, value) {{
                        const entry = pending.get(String(id));
                        if (!entry) {{ return; }}
                        pending.delete(String(id));
                        if (ok) {{
                            entry.resolve(value ?? '');
                        }} else {{
                            entry.reject(new DOMException(value || 'Clipboard request failed', 'NotAllowedError'));
                        }}
                    }}
                }}
            }});
            try {{
                Object.defineProperty(navigator, 'clipboard', {{ configurable: true, value: bridge }});
            }} catch (_) {{
                if (!navigator.clipboard) {{ navigator.clipboard = bridge; }}
            }}
            const originalExecCommand = document.execCommand && document.execCommand.bind(document);
            if (originalExecCommand) {{
                document.execCommand = function(command, showUI, value) {{
                    const normalized = String(command || '').toLowerCase();
                    if (normalized === 'copy' || normalized === 'cut') {{
                        bridge.writeText(selectedText());
                        if (normalized === 'cut') {{
                            originalExecCommand('delete', false, null);
                        }}
                        return true;
                    }}
                    return originalExecCommand(command, showUI, value);
                }};
            }}
        }})();"
    )
}

unsafe fn get_webview_delegate_state(this: id) -> Option<&'static mut MacWebViewDelegateState> {
    unsafe {
        let raw: *mut c_void = load_ivar(this, WEBVIEW_STATE_IVAR);
        if raw.is_null() {
            None
        } else {
            Some(&mut *(raw as *mut MacWebViewDelegateState))
        }
    }
}

unsafe fn get_print_view_state(this: id) -> Option<&'static mut MacPrintViewState> {
    unsafe {
        let raw: *mut c_void = load_ivar(this, PRINT_VIEW_STATE_IVAR);
        if raw.is_null() {
            None
        } else {
            Some(&mut *(raw as *mut MacPrintViewState))
        }
    }
}

unsafe fn build_print_view(job: PlatformPrintJob) -> id {
    let view: id = unsafe { msg_send![PRINT_VIEW_CLASS, alloc] };
    let rendered_page_size = crate::print::oriented_print_page_size(job.page_size, job.orientation);
    let frame = NSRect::new(
        NSPoint::new(0., 0.),
        NSSize::new(
            rendered_page_size.width.0 as f64,
            rendered_page_size.height.0 as f64,
        ),
    );
    let view: id = unsafe { msg_send![view, initWithFrame: frame] };
    let state = Box::new(MacPrintViewState {
        page_size: rendered_page_size,
        margins: job.margins,
        pages: job.pages,
    });
    unsafe {
        store_ivar(
            view,
            PRINT_VIEW_STATE_IVAR,
            Box::into_raw(state) as *mut c_void,
        );
    }
    view
}

unsafe fn run_print_job(
    native_window: id,
    job: PlatformPrintJob,
    show_dialog: bool,
) -> anyhow::Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let print_info: id = unsafe {
            let shared: id = msg_send![lookup_class(c"NSPrintInfo"), sharedPrintInfo];
            msg_send![shared, copy]
        };
        unsafe {
            let _: () = msg_send![print_info, setTopMargin: job.margins.top.0 as f64];
            let _: () = msg_send![print_info, setRightMargin: job.margins.right.0 as f64];
            let _: () = msg_send![print_info, setBottomMargin: job.margins.bottom.0 as f64];
            let _: () = msg_send![print_info, setLeftMargin: job.margins.left.0 as f64];
            let _: () = msg_send![
                print_info,
                setPaperSize: NSSize::new(job.page_size.width.0 as f64, job.page_size.height.0 as f64)
            ];
            let orientation = if matches!(job.orientation, crate::PrintOrientation::Landscape) {
                NSPaperOrientationLandscape
            } else {
                NSPaperOrientationPortrait
            };
            let _: () = msg_send![print_info, setOrientation: orientation];
        }

        let title = unsafe { ns_string(job.title.as_ref()) };
        let view = unsafe { build_print_view(job) };
        let operation: id = unsafe {
            msg_send![lookup_class(c"NSPrintOperation"), printOperationWithView: view, printInfo: print_info]
        };

        unsafe {
            let _: () =
                msg_send![operation, setShowsPrintPanel: if show_dialog { YES } else { NO }];
            let _: () =
                msg_send![operation, setShowsProgressPanel: if show_dialog { YES } else { NO }];
            let _: () = msg_send![operation, setJobTitle: title];
            let _: () = msg_send![operation, setCanSpawnSeparateThread: NO];
        }

        let success: BOOL = unsafe {
            if show_dialog {
                msg_send![
                    operation,
                    runOperationModalForWindow: native_window,
                    delegate: nil,
                    didRunSelector: ptr::null::<c_void>(),
                    contextInfo: ptr::null_mut::<c_void>()
                ]
            } else {
                msg_send![operation, runOperation]
            }
        };

        if success == YES {
            Ok(())
        } else {
            Err(anyhow::anyhow!("print operation failed"))
        }
    })
}

unsafe fn ns_color(color: Rgba) -> id {
    unsafe {
        msg_send![
            lookup_class(c"NSColor"),
            colorWithSRGBRed: color.r as f64,
            green: color.g as f64,
            blue: color.b as f64,
            alpha: color.a as f64
        ]
    }
}

fn apply_webview_background_color(webview: id, background_color: Option<Rgba>) {
    unsafe {
        let (opaque, color): (BOOL, id) = if let Some(color) = background_color {
            (Bool::new(color.a >= 1.0), ns_color(color))
        } else {
            (
                YES,
                msg_send![
                    lookup_class(c"NSColor"),
                    colorWithSRGBRed: 1.0f64,
                    green: 1.0f64,
                    blue: 1.0f64,
                    alpha: 1.0f64
                ],
            )
        };

        let _: () = msg_send![webview, setOpaque: opaque];
        let _: () = msg_send![webview, setBackgroundColor: color];

        let has_scroll_view: BOOL = msg_send![webview, respondsToSelector: sel!(scrollView)];
        let scroll_view: id = if has_scroll_view.as_bool() {
            msg_send![webview, scrollView]
        } else {
            nil
        };
        if !scroll_view.is_null() {
            if opaque.as_bool() {
                let _: () = msg_send![scroll_view, setDrawsBackground: YES];
                let _: () = msg_send![scroll_view, setBackgroundColor: color];
            } else {
                let clear_color: id = msg_send![lookup_class(c"NSColor"), clearColor];
                let _: () = msg_send![scroll_view, setDrawsBackground: NO];
                let _: () = msg_send![scroll_view, setBackgroundColor: clear_color];
            }
        }
    }
}

fn set_webview_inspectable(webview: id, inspectable: bool) -> bool {
    unsafe {
        if webview.is_null() {
            return false;
        }
        let responds: BOOL = msg_send![webview, respondsToSelector: sel!(setInspectable:)];
        if responds.as_bool() {
            let _: () = msg_send![webview, setInspectable: Bool::new(inspectable)];
            true
        } else {
            false
        }
    }
}

unsafe fn register_dragged_types(target: id) {
    unsafe {
        let dragged_types: id = msg_send![lookup_class(c"NSMutableArray"), array];
        let () = msg_send![dragged_types, addObject: NSPasteboardTypeFileURL];
        let () = msg_send![dragged_types, addObject: NSPasteboardTypeURL];
        let () = msg_send![dragged_types, addObject: NSPasteboardTypeString];
        let () = msg_send![target, registerForDraggedTypes: dragged_types];
    }
}

unsafe fn ns_rect_for_print_bounds(bounds: Bounds<Pixels>, margins: Edges<Pixels>) -> NSRect {
    NSRect::new(
        NSPoint::new(
            (margins.left + bounds.origin.x).0 as f64,
            (margins.top + bounds.origin.y).0 as f64,
        ),
        NSSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64),
    )
}

unsafe fn ns_bezier_path_for_rect(
    bounds: Bounds<Pixels>,
    margins: Edges<Pixels>,
    radius: Option<Pixels>,
) -> id {
    let rect = unsafe { ns_rect_for_print_bounds(bounds, margins) };
    match radius {
        Some(radius) if radius.0 > 0.0 => unsafe {
            msg_send![
                lookup_class(c"NSBezierPath"),
                bezierPathWithRoundedRect: rect,
                xRadius: radius.0 as f64,
                yRadius: radius.0 as f64
            ]
        },
        _ => unsafe { msg_send![lookup_class(c"NSBezierPath"), bezierPathWithRect: rect] },
    }
}

unsafe fn ns_font(style: &crate::PrintTextStyle) -> id {
    unsafe {
        if let Some(font_family) = style.font_family_ref() {
            let font_name = ns_string(font_family.as_ref());
            let font: id = msg_send![
                lookup_class(c"NSFont"),
                fontWithName: font_name,
                size: style.font_size().0 as f64
            ];
            if font != nil {
                return font;
            }
        }

        msg_send![lookup_class(c"NSFont"), systemFontOfSize: style.font_size().0 as f64]
    }
}

unsafe fn ns_text_attributes(style: &crate::PrintTextStyle) -> id {
    let font = unsafe { ns_font(style) };
    let color = unsafe { ns_color(style.color_ref()) };
    let keys = [unsafe { ns_string("NSFont") }, unsafe {
        ns_string("NSColor")
    }];
    let values = [font, color];
    unsafe {
        msg_send![
            lookup_class(c"NSDictionary"),
            dictionaryWithObjects: values.as_ptr(),
            forKeys: keys.as_ptr(),
            count: 2usize
        ]
    }
}

fn fitted_print_image_bounds(
    bounds: Bounds<Pixels>,
    image: &RenderImage,
    fit: PrintImageFit,
    frame_index: usize,
) -> Bounds<Pixels> {
    let image_size = image
        .size(frame_index)
        .map(|dimension| Pixels::from(u32::from(dimension)));
    let image_ratio = image_size.width / image_size.height;
    let bounds_ratio = bounds.size.width / bounds.size.height;

    match fit {
        PrintImageFit::Fill => bounds,
        PrintImageFit::Contain => {
            let new_size = if bounds_ratio > image_ratio {
                size(
                    image_size.width * (bounds.size.height / image_size.height),
                    bounds.size.height,
                )
            } else {
                size(
                    bounds.size.width,
                    image_size.height * (bounds.size.width / image_size.width),
                )
            };

            Bounds::new(
                point(
                    bounds.origin.x + (bounds.size.width - new_size.width) / 2.0,
                    bounds.origin.y + (bounds.size.height - new_size.height) / 2.0,
                ),
                new_size,
            )
        }
        PrintImageFit::ScaleDown => {
            if image_size.width > bounds.size.width || image_size.height > bounds.size.height {
                let new_size = if bounds_ratio > image_ratio {
                    size(
                        image_size.width * (bounds.size.height / image_size.height),
                        bounds.size.height,
                    )
                } else {
                    size(
                        bounds.size.width,
                        image_size.height * (bounds.size.width / image_size.width),
                    )
                };

                Bounds::new(
                    point(
                        bounds.origin.x + (bounds.size.width - new_size.width) / 2.0,
                        bounds.origin.y + (bounds.size.height - new_size.height) / 2.0,
                    ),
                    new_size,
                )
            } else {
                let original_size = size(image_size.width, image_size.height);
                Bounds::new(
                    point(
                        bounds.origin.x + (bounds.size.width - original_size.width) / 2.0,
                        bounds.origin.y + (bounds.size.height - original_size.height) / 2.0,
                    ),
                    original_size,
                )
            }
        }
        PrintImageFit::Cover => {
            let new_size = if bounds_ratio > image_ratio {
                size(
                    bounds.size.width,
                    image_size.height * (bounds.size.width / image_size.width),
                )
            } else {
                size(
                    image_size.width * (bounds.size.height / image_size.height),
                    bounds.size.height,
                )
            };

            Bounds::new(
                point(
                    bounds.origin.x + (bounds.size.width - new_size.width) / 2.0,
                    bounds.origin.y + (bounds.size.height - new_size.height) / 2.0,
                ),
                new_size,
            )
        }
        PrintImageFit::None => Bounds::new(bounds.origin, image_size),
    }
}

fn ns_image_from_render_image(image: &RenderImage, frame_index: usize) -> id {
    let Some(bytes) = image.as_bytes(frame_index) else {
        return nil;
    };

    let image_size = image.size(frame_index);
    let width = u32::from(image_size.width);
    let height = u32::from(image_size.height);

    let mut rgba_bytes = bytes.to_vec();
    for pixel in rgba_bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    let Some(buffer) = ImageBuffer::<ImageRgba<u8>, Vec<u8>>::from_raw(width, height, rgba_bytes)
    else {
        return nil;
    };

    let mut cursor = Cursor::new(Vec::new());
    if DynamicImage::ImageRgba8(buffer)
        .write_to(&mut cursor, ImageFormat::Png)
        .is_err()
    {
        return nil;
    }

    unsafe {
        let encoded = cursor.into_inner();
        let data: id = msg_send![
            lookup_class(c"NSData"),
            dataWithBytes: encoded.as_ptr() as *const c_void,
            length: encoded.len()
        ];
        let image: id = msg_send![lookup_class(c"NSImage"), alloc];
        msg_send![image, initWithData: data]
    }
}

unsafe fn draw_print_command(command: &PrintCommand, margins: Edges<Pixels>) {
    match command {
        PrintCommand::FillRect { bounds, color } => unsafe {
            let path = ns_bezier_path_for_rect(*bounds, margins, None);
            let fill = ns_color(*color);
            let _: () = msg_send![fill, setFill];
            let _: () = msg_send![path, fill];
        },
        PrintCommand::FillRoundedRect {
            bounds,
            radius,
            color,
        } => unsafe {
            let path = ns_bezier_path_for_rect(*bounds, margins, Some(*radius));
            let fill = ns_color(*color);
            let _: () = msg_send![fill, setFill];
            let _: () = msg_send![path, fill];
        },
        PrintCommand::StrokeRect { bounds, stroke } => unsafe {
            let path = ns_bezier_path_for_rect(*bounds, margins, None);
            let _: () = msg_send![path, setLineWidth: stroke.width().0 as f64];
            let stroke_color = ns_color(stroke.color_ref());
            let _: () = msg_send![stroke_color, setStroke];
            let _: () = msg_send![path, stroke];
        },
        PrintCommand::StrokeRoundedRect {
            bounds,
            radius,
            stroke,
        } => unsafe {
            let path = ns_bezier_path_for_rect(*bounds, margins, Some(*radius));
            let _: () = msg_send![path, setLineWidth: stroke.width().0 as f64];
            let stroke_color = ns_color(stroke.color_ref());
            let _: () = msg_send![stroke_color, setStroke];
            let _: () = msg_send![path, stroke];
        },
        PrintCommand::StrokeLine { from, to, stroke } => unsafe {
            let path: id = msg_send![lookup_class(c"NSBezierPath"), bezierPath];
            let _: () = msg_send![
                path,
                moveToPoint: NSPoint::new((margins.left + from.x).0 as f64, (margins.top + from.y).0 as f64)
            ];
            let _: () = msg_send![
                path,
                lineToPoint: NSPoint::new((margins.left + to.x).0 as f64, (margins.top + to.y).0 as f64)
            ];
            let _: () = msg_send![path, setLineWidth: stroke.width().0 as f64];
            let stroke_color = ns_color(stroke.color_ref());
            let _: () = msg_send![stroke_color, setStroke];
            let _: () = msg_send![path, stroke];
        },
        PrintCommand::Text {
            origin,
            text,
            style,
        } => unsafe {
            let attributes = ns_text_attributes(style);
            let string = ns_string(text.as_ref());
            let _: NSSize = msg_send![
                string,
                drawAtPoint: NSPoint::new((margins.left + origin.x).0 as f64, (margins.top + origin.y).0 as f64),
                withAttributes: attributes
            ];
        },
        PrintCommand::TextBlock {
            bounds,
            text,
            style,
        } => unsafe {
            let attributes = ns_text_attributes(style);
            let string = ns_string(text.as_ref());
            let rect = ns_rect_for_print_bounds(*bounds, margins);
            let options = NSStringDrawingUsesLineFragmentOrigin | NSStringDrawingUsesFontLeading;
            let _: NSRect = msg_send![
                string,
                drawWithRect: rect,
                options: options,
                attributes: attributes
            ];
        },
        PrintCommand::Image {
            bounds,
            image,
            style,
        } => unsafe {
            let frame_index = style.selected_frame_index();
            let ns_image = ns_image_from_render_image(image, frame_index);
            if ns_image == nil {
                return;
            }

            let fitted_bounds = fitted_print_image_bounds(
                *bounds,
                image.as_ref(),
                style.object_fit_ref(),
                frame_index,
            );
            let target_rect = ns_rect_for_print_bounds(fitted_bounds, margins);
            let source_size = image.as_ref().size(frame_index);
            let source_rect = NSRect::new(
                NSPoint::new(0., 0.),
                NSSize::new(
                    u32::from(source_size.width) as f64,
                    u32::from(source_size.height) as f64,
                ),
            );
            // `Cover` deliberately makes `target_rect` larger than the
            // requested image box. Keep the crop local to this command so it
            // cannot bleed into neighboring print content.
            let _: () = msg_send![lookup_class(c"NSGraphicsContext"), saveGraphicsState];
            let clip_rect = ns_rect_for_print_bounds(*bounds, margins);
            let _: () = msg_send![lookup_class(c"NSBezierPath"), clipRect: clip_rect];
            let _: () = msg_send![
                ns_image,
                drawInRect: target_rect,
                fromRect: source_rect,
                operation: NSCompositingOperationSourceOver,
                fraction: style.opacity_ref() as f64
            ];
            let _: () = msg_send![lookup_class(c"NSGraphicsContext"), restoreGraphicsState];
        },
    }
}

unsafe fn webview_message_value(body: id) -> serde_json::Value {
    unsafe {
        if body.is_null() {
            return serde_json::Value::Null;
        }

        let responds_to_utf8: BOOL = msg_send![body, respondsToSelector: sel!(UTF8String)];
        if responds_to_utf8 == YES {
            let text = body.to_str().to_string();
            return serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
        }

        let description: id = msg_send![body, description];
        if description.is_null() {
            serde_json::Value::Null
        } else {
            serde_json::Value::String(description.to_str().to_string())
        }
    }
}

fn ns_error_message(error: id) -> String {
    unsafe {
        if error.is_null() {
            return "WebView JavaScript evaluation failed".into();
        }

        let localized_description: id = msg_send![error, localizedDescription];
        if !localized_description.is_null() {
            return localized_description.to_str().to_string();
        }

        let description: id = msg_send![error, description];
        if !description.is_null() {
            return description.to_str().to_string();
        }
    }

    "WebView JavaScript evaluation failed".into()
}

fn finish_mac_custom_protocol_task(task: id, response: crate::CustomProtocolResponse) {
    unsafe {
        if task.is_null() {
            return;
        }
        let request: id = msg_send![task, request];
        let url: id = if request.is_null() {
            nil
        } else {
            msg_send![request, URL]
        };
        if url.is_null() {
            fail_mac_custom_protocol_task(task, 400, "custom protocol request URL is missing");
            return;
        }

        let headers: id = msg_send![lookup_class(c"NSMutableDictionary"), dictionary];
        let _: () = msg_send![
            headers,
            setObject: ns_string(&response.mime_type),
            forKey: ns_string("Content-Type")
        ];
        let _: () = msg_send![
            headers,
            setObject: ns_string(&response.body.len().to_string()),
            forKey: ns_string("Content-Length")
        ];
        for (name, value) in &response.headers {
            if !name.eq_ignore_ascii_case("content-type")
                && !name.eq_ignore_ascii_case("content-length")
            {
                let _: () = msg_send![
                    headers,
                    setObject: ns_string(value),
                    forKey: ns_string(name)
                ];
            }
        }

        let url_response: id = msg_send![lookup_class(c"NSHTTPURLResponse"), alloc];
        let url_response: id = msg_send![
            url_response,
            initWithURL: url,
            statusCode: response.status as NSInteger,
            HTTPVersion: ns_string("HTTP/1.1"),
            headerFields: headers
        ];
        if url_response.is_null() {
            fail_mac_custom_protocol_task(
                task,
                500,
                "could not construct custom protocol response",
            );
            return;
        }

        let data: id = msg_send![
            lookup_class(c"NSData"),
            dataWithBytes: response.body.as_ptr() as *const c_void,
            length: response.body.len()
        ];
        let _: () = msg_send![task, didReceiveResponse: url_response];
        let _: () = msg_send![task, didReceiveData: data];
        let _: () = msg_send![task, didFinish];
        let _: () = msg_send![url_response, release];
    }
}

unsafe fn fail_mac_custom_protocol_task(task: id, code: NSInteger, message: &str) {
    unsafe {
        if task.is_null() {
            return;
        }
        let user_info: id = msg_send![lookup_class(c"NSMutableDictionary"), dictionary];
        let _: () = msg_send![
            user_info,
            setObject: ns_string(message),
            forKey: ns_string("NSLocalizedDescription")
        ];
        let error: id = msg_send![
            lookup_class(c"NSError"),
            errorWithDomain: ns_string("KaelCustomProtocolError"),
            code: code,
            userInfo: user_info
        ];
        let _: () = msg_send![task, didFailWithError: error];
    }
}

extern "C" fn webview_start_url_scheme_task(handler: id, _: Sel, _webview: id, task: id) {
    catch_platform_callback("webview custom protocol", (), || unsafe {
        let is_main_thread: BOOL = msg_send![lookup_class(c"NSThread"), isMainThread];
        if !is_main_thread.as_bool() {
            fail_mac_custom_protocol_task(
                task,
                500,
                "custom protocol callback was invoked outside the WebView UI thread",
            );
            return;
        }

        let request: id = msg_send![task, request];
        let url: id = if request.is_null() {
            nil
        } else {
            msg_send![request, URL]
        };
        let raw_url = ns_url_absolute_string(url);
        if raw_url.is_empty() {
            fail_mac_custom_protocol_task(task, 400, "custom protocol request URL is missing");
            return;
        }

        let Some(state) = get_webview_delegate_state(handler) else {
            fail_mac_custom_protocol_task(task, 500, "custom protocol host is unavailable");
            return;
        };
        let mut context = state.async_window.clone();
        match context.update(|_, cx| cx.handle_custom_protocol_url(raw_url.to_string())) {
            Ok(Ok(Some(response))) => finish_mac_custom_protocol_task(task, response),
            Ok(Ok(None)) => {
                finish_mac_custom_protocol_task(task, crate::CustomProtocolResponse::not_found())
            }
            Ok(Err(error)) | Err(error) => {
                log::warn!("serving WKWebView custom protocol failed: {error:#}");
                fail_mac_custom_protocol_task(task, 500, "custom protocol handler failed");
            }
        }
    });
}

extern "C" fn webview_stop_url_scheme_task(_handler: id, _: Sel, _webview: id, _task: id) {
    // Responses are completed synchronously on WebKit's UI-thread callback.
    // There is no outstanding body producer to cancel after this method runs.
}

fn mac_webview_url(webview: id) -> SharedString {
    unsafe {
        if webview.is_null() {
            return SharedString::default();
        }

        let url: id = msg_send![webview, URL];
        let url = ns_url_absolute_string(url);
        if !url.is_empty() {
            return url;
        }
    }

    SharedString::default()
}

fn ns_url_absolute_string(url: id) -> SharedString {
    unsafe {
        if url.is_null() {
            return SharedString::default();
        }
        let absolute_string: id = msg_send![url, absoluteString];
        if absolute_string.is_null() {
            SharedString::default()
        } else {
            absolute_string.to_str().to_string().into()
        }
    }
}

fn ns_url_path(url: id) -> Option<PathBuf> {
    unsafe {
        if url.is_null() {
            return None;
        }
        let path: id = msg_send![url, path];
        if path.is_null() {
            None
        } else {
            Some(PathBuf::from(path.to_str().to_string()))
        }
    }
}

fn ns_request_url_string(request: id) -> SharedString {
    unsafe {
        if request.is_null() {
            return SharedString::default();
        }
        let url: id = msg_send![request, URL];
        ns_url_absolute_string(url)
    }
}

fn ns_response_url_string(response: id) -> SharedString {
    unsafe {
        if response.is_null() {
            return SharedString::default();
        }
        let url: id = msg_send![response, URL];
        ns_url_absolute_string(url)
    }
}

fn default_download_path(suggested_filename: &str) -> PathBuf {
    let sanitized = sanitize_download_filename(suggested_filename);
    let downloads_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("Downloads"))
        .unwrap_or_else(std::env::temp_dir);
    downloads_dir.join(sanitized)
}

fn sanitize_download_filename(suggested_filename: &str) -> String {
    let sanitized = suggested_filename
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    let sanitized = sanitized.trim();
    if sanitized.is_empty() {
        "download.bin".into()
    } else {
        sanitized.into()
    }
}

fn mac_webview_title(webview: id) -> SharedString {
    unsafe {
        if webview.is_null() {
            return SharedString::default();
        }

        let title: id = msg_send![webview, title];
        if !title.is_null() {
            return title.to_str().to_string().into();
        }
    }

    SharedString::default()
}

#[derive(Clone)]
struct WebViewCookieUrlFilter {
    host: String,
    path: String,
    secure: bool,
}

impl WebViewCookieUrlFilter {
    fn parse(url: &str) -> Option<Self> {
        let scheme_end = url.find("://")?;
        let scheme = &url[..scheme_end];
        let rest = &url[scheme_end + 3..];
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        if authority.is_empty() {
            return None;
        }
        let host_port = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = if host_port.starts_with('[') {
            host_port
                .find(']')
                .map(|end| &host_port[1..end])
                .unwrap_or(host_port)
        } else {
            host_port
                .split_once(':')
                .map_or(host_port, |(host, _)| host)
        };
        if host.is_empty() {
            return None;
        }

        let path = if let Some(path_start) = rest.find('/') {
            let path = &rest[path_start..];
            let path_end = path.find(['?', '#']).unwrap_or(path.len());
            &path[..path_end]
        } else {
            "/"
        };

        Some(Self {
            host: host.to_ascii_lowercase(),
            path: if path.is_empty() { "/" } else { path }.to_string(),
            secure: scheme.eq_ignore_ascii_case("https"),
        })
    }

    fn matches(&self, cookie: &WebViewCookie) -> bool {
        if cookie.secure && !self.secure {
            return false;
        }

        if let Some(domain) = cookie.domain.as_ref() {
            let domain = domain.as_ref().trim_start_matches('.').to_ascii_lowercase();
            if !domain.is_empty()
                && self.host != domain
                && !self.host.ends_with(&format!(".{domain}"))
            {
                return false;
            }
        }

        if let Some(path) = cookie.path.as_ref() {
            let path = path.as_ref();
            if !path.is_empty() && !self.path.starts_with(path) {
                return false;
            }
        }

        true
    }
}

unsafe fn mac_webview_cookies_from_array(
    cookies: id,
    filter: Option<&WebViewCookieUrlFilter>,
) -> Vec<WebViewCookie> {
    if cookies.is_null() {
        return Vec::new();
    }

    let count: NSUInteger = unsafe { msg_send![cookies, count] };
    let mut result = Vec::with_capacity(count as usize);
    for index in 0..count {
        let cookie: id = unsafe { msg_send![cookies, objectAtIndex: index] };
        if cookie.is_null() {
            continue;
        }
        let cookie = unsafe { mac_webview_cookie_from_ns(cookie) };
        if filter.is_none_or(|filter| filter.matches(&cookie)) {
            result.push(cookie);
        }
    }
    result
}

unsafe fn mac_webview_cookie_from_ns(cookie: id) -> WebViewCookie {
    let name: id = unsafe { msg_send![cookie, name] };
    let value: id = unsafe { msg_send![cookie, value] };
    let domain: id = unsafe { msg_send![cookie, domain] };
    let path: id = unsafe { msg_send![cookie, path] };
    let secure: BOOL = unsafe { msg_send![cookie, isSecure] };
    let http_only: BOOL = unsafe { msg_send![cookie, isHTTPOnly] };

    WebViewCookie {
        name: ns_string_to_shared_string(name),
        value: ns_string_to_shared_string(value),
        domain: ns_optional_shared_string(domain),
        path: ns_optional_shared_string(path),
        secure: secure.as_bool(),
        http_only: http_only.as_bool(),
    }
}

fn mac_webview_cookie_to_ns(cookie: WebViewCookie, origin_url: Option<&str>) -> Option<id> {
    let domain = cookie.domain.as_ref().map(|domain| domain.as_ref());
    let origin_url = origin_url.filter(|url| !url.is_empty());
    if domain.is_none() && origin_url.is_none() {
        return None;
    }

    unsafe {
        let properties: id = msg_send![lookup_class(c"NSMutableDictionary"), dictionary];
        let _: () = msg_send![
            properties,
            setObject: ns_string(cookie.name.as_ref()),
            forKey: ns_string("Name")
        ];
        let _: () = msg_send![
            properties,
            setObject: ns_string(cookie.value.as_ref()),
            forKey: ns_string("Value")
        ];
        if let Some(domain) = domain {
            let _: () = msg_send![
                properties,
                setObject: ns_string(domain),
                forKey: ns_string("Domain")
            ];
        } else if let Some(origin_url) = origin_url {
            let _: () = msg_send![
                properties,
                setObject: ns_string(origin_url),
                forKey: ns_string("OriginURL")
            ];
        }
        if let Some(path) = cookie.path.as_ref() {
            let _: () = msg_send![
                properties,
                setObject: ns_string(path.as_ref()),
                forKey: ns_string("Path")
            ];
        } else {
            let _: () = msg_send![
                properties,
                setObject: ns_string("/"),
                forKey: ns_string("Path")
            ];
        }
        if cookie.secure {
            let _: () = msg_send![
                properties,
                setObject: ns_string("TRUE"),
                forKey: ns_string("Secure")
            ];
        }
        if cookie.http_only {
            let _: () = msg_send![
                properties,
                setObject: ns_string("TRUE"),
                forKey: ns_string("HttpOnly")
            ];
        }

        let cookie: id = msg_send![lookup_class(c"NSHTTPCookie"), cookieWithProperties: properties];
        if cookie.is_null() { None } else { Some(cookie) }
    }
}

fn ns_string_to_shared_string(value: id) -> SharedString {
    if value.is_null() {
        SharedString::default()
    } else {
        unsafe { value.to_str().to_string().into() }
    }
}

fn ns_optional_shared_string(value: id) -> Option<SharedString> {
    if value.is_null() {
        return None;
    }
    let value = unsafe { value.to_str() };
    if value.is_empty() {
        None
    } else {
        Some(value.to_string().into())
    }
}

fn webkit_security_origin_string(origin: id) -> Option<SharedString> {
    if origin.is_null() {
        return None;
    }
    let scheme: id = unsafe { msg_send![origin, protocol] };
    let host: id = unsafe { msg_send![origin, host] };
    let scheme = ns_optional_shared_string(scheme)?;
    let host = ns_optional_shared_string(host)?;
    let port: NSInteger = unsafe { msg_send![origin, port] };
    let default_port = matches!(scheme.as_ref(), "http" | "ws") && port == 80
        || matches!(scheme.as_ref(), "https" | "wss") && port == 443;
    if port > 0 && !default_port {
        Some(format!("{scheme}://{host}:{port}").into())
    } else {
        Some(format!("{scheme}://{host}").into())
    }
}

fn alert_response_index(response: NSInteger, button_order: &[usize]) -> Option<usize> {
    let ordinal = response.checked_sub(NS_ALERT_FIRST_BUTTON_RETURN)?;
    let ordinal = usize::try_from(ordinal).ok()?;
    button_order.get(ordinal).copied()
}

unsafe fn call_navigation_decision_handler(decision_handler: id, policy: NSInteger) {
    if decision_handler.is_null() {
        log::error!("WebKit provided no navigation decision handler");
        return;
    }
    unsafe {
        let decision_handler = &*(decision_handler as *const Block<dyn Fn(NSInteger)>);
        decision_handler.call((policy,));
    }
}

unsafe fn call_webview_permission_decision_handler(decision_handler: id, decision: NSInteger) {
    if decision_handler.is_null() {
        return;
    }

    unsafe {
        let decision_handler = &*(decision_handler as *const Block<dyn Fn(NSInteger)>);
        decision_handler.call((decision,));
    }
}

extern "C" fn webview_request_media_capture_permission(
    delegate: id,
    _: Sel,
    _: id,
    origin: id,
    frame: id,
    capture_type: NSInteger,
    decision_handler: id,
) {
    const PROMPT: NSInteger = 0;
    const GRANT: NSInteger = 1;
    const DENY: NSInteger = 2;

    let is_main_frame = if frame.is_null() {
        None
    } else {
        let is_main_frame: BOOL = unsafe { msg_send![frame, isMainFrame] };
        Some(is_main_frame.as_bool())
    };

    let Some(state) = (unsafe { get_webview_delegate_state(delegate) }) else {
        unsafe { call_webview_permission_decision_handler(decision_handler, PROMPT) };
        return;
    };
    let Some(handler) = state.permission_handler.clone() else {
        unsafe { call_webview_permission_decision_handler(decision_handler, PROMPT) };
        return;
    };

    let decide = |kind| {
        let request = WebViewNativePermissionRequest::with_requesting_origin(
            kind,
            webkit_security_origin_string(origin),
            match is_main_frame {
                Some(true) => WebViewPermissionFrame::Main,
                Some(false) => WebViewPermissionFrame::Subframe,
                None => WebViewPermissionFrame::Unknown,
            },
            None,
        );
        catch_platform_callback(
            "webview native permission policy",
            WebViewPermissionDecision::Deny,
            || handler(request),
        )
    };
    let decision = match capture_type {
        0 => decide(WebViewPermissionKind::Camera),
        1 => decide(WebViewPermissionKind::Microphone),
        2 => {
            let camera = decide(WebViewPermissionKind::Camera);
            let microphone = decide(WebViewPermissionKind::Microphone);
            match (camera, microphone) {
                (WebViewPermissionDecision::Allow, WebViewPermissionDecision::Allow) => {
                    WebViewPermissionDecision::Allow
                }
                (WebViewPermissionDecision::Deny, _) | (_, WebViewPermissionDecision::Deny) => {
                    WebViewPermissionDecision::Deny
                }
                _ => WebViewPermissionDecision::Default,
            }
        }
        _ => decide(WebViewPermissionKind::Other),
    };
    let decision = match decision {
        WebViewPermissionDecision::Allow => GRANT,
        WebViewPermissionDecision::Deny => DENY,
        WebViewPermissionDecision::Default => PROMPT,
    };
    unsafe { call_webview_permission_decision_handler(decision_handler, decision) };
}

unsafe fn call_download_destination_handler(decision_handler: id, destination: id) {
    if decision_handler.is_null() {
        log::error!("WebKit provided no download destination handler");
        return;
    }
    unsafe {
        let decision_handler = &*(decision_handler as *const Block<dyn Fn(id)>);
        decision_handler.call((destination,));
    }
}

fn resolve_mac_download_started(
    url: SharedString,
    suggested_path: PathBuf,
    state: &MacWebViewDelegateState,
) -> Option<PathBuf> {
    let Some(handler) = state.download_started_handler.clone() else {
        return Some(suggested_path);
    };

    let mut async_window = state.async_window.clone();
    match catch_platform_callback(
        "webview download started",
        WebViewDownloadPolicy::Deny,
        || {
            async_window
                .update(|window, cx| handler(url, Some(suggested_path.clone()), window, cx))
                .unwrap_or(WebViewDownloadPolicy::Deny)
        },
    ) {
        WebViewDownloadPolicy::Allow => Some(suggested_path),
        WebViewDownloadPolicy::Deny => None,
        WebViewDownloadPolicy::SaveTo(destination) => {
            if destination.is_absolute() {
                Some(destination)
            } else {
                log::warn!(
                    "WebView download destination must be absolute: {}",
                    destination.display()
                );
                None
            }
        }
    }
}

fn dispatch_mac_download_completed(
    state: &MacWebViewDelegateState,
    download_state: MacWebViewDownloadState,
    success: bool,
) {
    let Some(handler) = state.download_completed_handler.clone() else {
        return;
    };
    let event = WebViewDownloadCompleted {
        url: download_state.url,
        path: download_state.path,
        success,
    };
    let mut async_window = state.async_window.clone();
    catch_platform_callback("webview download completed", (), || {
        let _ = async_window.update(|window, cx| handler(event, window, cx));
    });
}

fn register_mac_download(delegate: id, download: id, url: SharedString) {
    let Some(state) = (unsafe { get_webview_delegate_state(delegate) }) else {
        return;
    };
    unsafe {
        let _: () = msg_send![download, setDelegate: delegate];
    }
    state.downloads.insert(
        download as usize,
        MacWebViewDownloadState { url, path: None },
    );
}

enum MacWebViewDragEvent {
    Enter,
    Over,
    Drop,
    Leave,
}

fn dispatch_mac_webview_drag_drop(
    webview: id,
    dragging_info: id,
    event: MacWebViewDragEvent,
) -> Option<WebViewDragDropPolicy> {
    let Some(state) = (unsafe { get_webview_delegate_state(webview) }) else {
        return None;
    };
    let Some(handler) = state.drag_drop_handler.clone() else {
        return None;
    };

    let position = webview_drag_position(webview, dragging_info);
    let event = match event {
        MacWebViewDragEvent::Enter => WebViewDragDropEvent::Enter {
            paths: webview_drag_paths(dragging_info),
            position,
        },
        MacWebViewDragEvent::Over => WebViewDragDropEvent::Over { position },
        MacWebViewDragEvent::Drop => WebViewDragDropEvent::Drop {
            paths: webview_drag_paths(dragging_info),
            position,
        },
        MacWebViewDragEvent::Leave => WebViewDragDropEvent::Leave,
    };

    let mut async_window = state.async_window.clone();
    Some(catch_platform_callback(
        "webview drag-and-drop",
        WebViewDragDropPolicy::BlockBrowserDefault,
        || {
            async_window
                .update(|window, cx| handler(event, window, cx))
                .unwrap_or(WebViewDragDropPolicy::BlockBrowserDefault)
        },
    ))
}

fn webview_drag_paths(dragging_info: id) -> Vec<PathBuf> {
    external_drop_data_from_event(dragging_info)
        .map(|data| data.paths().paths().to_vec())
        .unwrap_or_default()
}

fn webview_drag_position(webview: id, dragging_info: id) -> (i32, i32) {
    unsafe {
        let window_point: NSPoint = msg_send![dragging_info, draggingLocation];
        let local_point: NSPoint = msg_send![webview, convertPoint: window_point, fromView: nil];
        let bounds: NSRect = msg_send![webview, bounds];
        let window: id = msg_send![webview, window];
        let scale = if window.is_null() {
            1.0
        } else {
            let scale: f64 = msg_send![window, backingScaleFactor];
            scale
        };
        (
            (local_point.x * scale).round() as i32,
            ((bounds.size.height - local_point.y) * scale).round() as i32,
        )
    }
}

fn mac_webview_zoom_key(event: id) -> Option<MacWebViewZoomKey> {
    if event.is_null() {
        return None;
    }
    let event = unsafe { &*(event as *const NSEvent) };
    let modifiers = event.modifierFlags();
    if !modifiers.contains(NSEventModifierFlags::Command)
        || modifiers.contains(NSEventModifierFlags::Control)
        || modifiers.contains(NSEventModifierFlags::Option)
    {
        return None;
    }

    let chars = event.charactersIgnoringModifiers()?;
    let chars = chars.to_string();

    match chars.as_str() {
        "=" | "+" => Some(MacWebViewZoomKey::In),
        "-" => Some(MacWebViewZoomKey::Out),
        "0" => Some(MacWebViewZoomKey::Reset),
        _ => None,
    }
}

enum MacWebViewZoomKey {
    In,
    Out,
    Reset,
}

fn apply_mac_webview_zoom_key(webview: id, key: MacWebViewZoomKey) {
    let current: f64 = unsafe { msg_send![webview, pageZoom] };
    let current = if current.is_finite() && current > 0.0 {
        current
    } else {
        1.0
    };
    let next = match key {
        MacWebViewZoomKey::In => current * 1.1,
        MacWebViewZoomKey::Out => current / 1.1,
        MacWebViewZoomKey::Reset => 1.0,
    }
    .clamp(0.25, 5.0);

    unsafe {
        let _: () = msg_send![webview, setPageZoom: next];
    }
}

fn apply_mac_webview_magnification(webview: id, event: id) {
    if webview.is_null() || event.is_null() {
        return;
    }
    let event = unsafe { &*(event as *const NSEvent) };
    let delta = event.magnification();
    if !delta.is_finite() || delta.abs() <= f64::EPSILON {
        return;
    }

    let current: f64 = unsafe { msg_send![webview, pageZoom] };
    let current = if current.is_finite() && current > 0.0 {
        current
    } else {
        1.0
    };
    let next = (current * (1.0 + delta).max(0.1)).clamp(0.25, 5.0);

    unsafe {
        let _: () = msg_send![webview, setPageZoom: next];
    }
}

fn handle_mac_webview_clipboard_message(
    state: &MacWebViewDelegateState,
    script_message: id,
    payload: &serde_json::Value,
) -> bool {
    if !state.clipboard_access
        || payload
            .get(WEBVIEW_CLIPBOARD_BRIDGE_KIND)
            .and_then(|value| value.as_bool())
            != Some(true)
    {
        return false;
    }

    if payload
        .get("__kaelIpcNonce")
        .and_then(|value| value.as_str())
        != Some(state.ipc_nonce.as_ref())
    {
        warn_rejected_mac_webview_ipc_once(
            state,
            "clipboard message had a missing or invalid authentication nonce",
        );
        return true;
    }

    let webview = mac_webview_from_script_message(script_message);
    if webview.is_null() {
        warn_rejected_mac_webview_ipc_once(
            state,
            "clipboard message source WebView was unavailable",
        );
        return true;
    }

    let id = payload
        .get("id")
        .and_then(|id| id.as_str())
        .unwrap_or_default();
    let result = match payload.get("op").and_then(|op| op.as_str()) {
        Some("readText") => Ok(mac_clipboard_read_text()),
        Some("writeText") => {
            let value = payload
                .get("value")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            if mac_clipboard_write_text(value) {
                Ok(String::new())
            } else {
                Err("Clipboard write failed".to_string())
            }
        }
        _ => Err("Unsupported clipboard operation".to_string()),
    };

    resolve_mac_webview_clipboard_request(webview, id, result);
    true
}

fn mac_webview_from_script_message(message: id) -> id {
    unsafe {
        let responds_to_webview: BOOL = msg_send![message, respondsToSelector: sel!(webView)];
        if responds_to_webview.as_bool() {
            let webview: id = msg_send![message, webView];
            if !webview.is_null() {
                return webview;
            }
        }

        let frame_info: id = msg_send![message, frameInfo];
        if frame_info.is_null() {
            return nil;
        }
        msg_send![frame_info, webView]
    }
}

fn mac_clipboard_read_text() -> String {
    unsafe {
        let pasteboard: id = msg_send![lookup_class(c"NSPasteboard"), generalPasteboard];
        if pasteboard.is_null() {
            return String::new();
        }
        let value: id = msg_send![pasteboard, stringForType: NSPasteboardTypeString];
        if value.is_null() {
            String::new()
        } else {
            value.to_str().to_string()
        }
    }
}

fn mac_clipboard_write_text(value: &str) -> bool {
    unsafe {
        let pasteboard: id = msg_send![lookup_class(c"NSPasteboard"), generalPasteboard];
        if pasteboard.is_null() {
            return false;
        }
        let _: NSInteger = msg_send![pasteboard, clearContents];
        let ok: BOOL = msg_send![
            pasteboard,
            setString: ns_string(value),
            forType: NSPasteboardTypeString
        ];
        ok.as_bool()
    }
}

fn resolve_mac_webview_clipboard_request(webview: id, id: &str, result: Result<String, String>) {
    let (ok, value) = match result {
        Ok(value) => ("true", value),
        Err(error) => ("false", error),
    };
    let script = format!(
        "window.__kaelClipboardBridge && window.__kaelClipboardBridge.resolve({}, {ok}, {});",
        json_string_literal(id),
        json_string_literal(&value)
    );
    unsafe {
        let _: () = msg_send![
            webview,
            evaluateJavaScript: ns_string(&script),
            completionHandler: nil
        ];
    }
}

unsafe fn build_window_class(name: &'static CStr, superclass: &Class) -> *const Class {
    unsafe {
        let mut decl = ClassBuilder::new(name, superclass).unwrap();
        decl.add_ivar::<*mut c_void>(c"windowState");

        let dealloc_window = dealloc_window as Method0<()>;
        let yes = yes as Method0<BOOL>;
        let window_did_resize = window_did_resize as Method1<id, ()>;
        let window_did_change_occlusion_state =
            window_did_change_occlusion_state as Method1<id, ()>;
        let window_will_enter_fullscreen = window_will_enter_fullscreen as Method1<id, ()>;
        let window_will_exit_fullscreen = window_will_exit_fullscreen as Method1<id, ()>;
        let window_did_move = window_did_move as Method1<id, ()>;
        let window_did_change_screen = window_did_change_screen as Method1<id, ()>;
        let window_did_change_key_status = window_did_change_key_status as Method1<id, ()>;
        let window_should_close = window_should_close as Method1<id, BOOL>;
        let close_window = close_window as Method0<()>;
        let dragging_entered = dragging_entered as Method1<id, NSDragOperation>;
        let dragging_updated = dragging_updated as Method1<id, NSDragOperation>;
        let dragging_exited = dragging_exited as Method1<id, ()>;
        let perform_drag_operation = perform_drag_operation as Method1<id, BOOL>;
        let conclude_drag_operation = conclude_drag_operation as Method1<id, ()>;
        let add_titlebar_accessory_view_controller =
            add_titlebar_accessory_view_controller as Method1<id, ()>;
        let move_tab_to_new_window = move_tab_to_new_window as Method1<id, ()>;
        let merge_all_windows = merge_all_windows as Method1<id, ()>;
        let select_next_tab = select_next_tab as Method1<id, ()>;
        let select_previous_tab = select_previous_tab as Method1<id, ()>;
        let toggle_tab_bar = toggle_tab_bar as Method1<id, ()>;

        decl.add_method(sel!(dealloc), dealloc_window);

        decl.add_method(sel!(canBecomeMainWindow), yes);
        decl.add_method(sel!(canBecomeKeyWindow), yes);
        decl.add_method(sel!(windowDidResize:), window_did_resize);
        decl.add_method(
            sel!(windowDidChangeOcclusionState:),
            window_did_change_occlusion_state,
        );
        decl.add_method(
            sel!(windowWillEnterFullScreen:),
            window_will_enter_fullscreen,
        );
        decl.add_method(sel!(windowWillExitFullScreen:), window_will_exit_fullscreen);
        decl.add_method(sel!(windowDidMove:), window_did_move);
        decl.add_method(sel!(windowDidChangeScreen:), window_did_change_screen);
        decl.add_method(sel!(windowDidBecomeKey:), window_did_change_key_status);
        decl.add_method(sel!(windowDidResignKey:), window_did_change_key_status);
        decl.add_method(sel!(windowShouldClose:), window_should_close);

        decl.add_method(sel!(close), close_window);

        decl.add_method(sel!(draggingEntered:), dragging_entered);
        decl.add_method(sel!(draggingUpdated:), dragging_updated);
        decl.add_method(sel!(draggingExited:), dragging_exited);
        decl.add_method(sel!(performDragOperation:), perform_drag_operation);
        decl.add_method(sel!(concludeDragOperation:), conclude_drag_operation);

        decl.add_method(
            sel!(addTitlebarAccessoryViewController:),
            add_titlebar_accessory_view_controller,
        );

        decl.add_method(sel!(moveTabToNewWindow:), move_tab_to_new_window);

        decl.add_method(sel!(mergeAllWindows:), merge_all_windows);

        decl.add_method(sel!(selectNextTab:), select_next_tab);

        decl.add_method(sel!(selectPreviousTab:), select_previous_tab);

        decl.add_method(sel!(toggleTabBar:), toggle_tab_bar);

        decl.register()
    }
}

struct MacWindowState {
    handle: AnyWindowHandle,
    executor: ForegroundExecutor,
    native_window: id,
    native_view: NonNull<Object>,
    blurred_view: Option<id>,
    display_link: Option<DisplayLink>,
    frame_polling_active: bool,
    renderer: renderer::Renderer,
    request_frame_callback: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    event_callback: Option<Box<dyn FnMut(PlatformInput) -> crate::DispatchEventResult>>,
    activate_callback: Option<Box<dyn FnMut(bool)>>,
    resize_callback: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    resize_sync_scheduled: bool,
    moved_callback: Option<Box<dyn FnMut()>>,
    should_close_callback: Option<Box<dyn FnMut() -> bool>>,
    close_callback: Option<Box<dyn FnOnce()>>,
    appearance_changed_callback: Option<Box<dyn FnMut()>>,
    input_handler: Option<PlatformInputHandler>,
    last_key_equivalent: Option<KeyDownEvent>,
    synthetic_drag_counter: usize,
    traffic_light_position: Option<Point<Pixels>>,
    transparent_titlebar: bool,
    previous_modifiers_changed_event: Option<PlatformInput>,
    keystroke_for_do_command: Option<Keystroke>,
    do_command_handled: Option<bool>,
    external_files_dragged: bool,
    // Whether the next left-mouse click is also the focusing click.
    first_mouse: bool,
    fullscreen_restore_bounds: Bounds<Pixels>,
    move_tab_to_new_window_callback: Option<Box<dyn FnMut()>>,
    merge_all_windows_callback: Option<Box<dyn FnMut()>>,
    select_next_tab_callback: Option<Box<dyn FnMut()>>,
    select_previous_tab_callback: Option<Box<dyn FnMut()>>,
    toggle_tab_bar_callback: Option<Box<dyn FnMut()>>,
    activated_least_once: bool,
    pointer_lock: crate::game_input::NativePointerLockState,
    pointer_lock_saved_position: Option<CGPoint>,
    pointer_lock_cursor_hidden: bool,
    accessibility_provider: super::accessibility::MacAccessibilityProvider,
    webviews: HashMap<SharedString, MacWebViewHost>,
    pending_webview_commands: HashMap<SharedString, Vec<PlatformWebViewCommand>>,
}

struct MacWebViewDelegateState {
    async_window: AsyncWindowContext,
    message_handler: Option<WebViewMessageHandler>,
    navigation_handler: Option<WebViewNavigationHandler>,
    new_window_handler: Option<WebViewNewWindowHandler>,
    download_started_handler: Option<WebViewDownloadStartedHandler>,
    download_completed_handler: Option<WebViewDownloadCompletedHandler>,
    document_title_changed_handler: Option<WebViewDocumentTitleChangedHandler>,
    page_load_handler: Option<WebViewPageLoadHandler>,
    drag_drop_handler: Option<WebViewDragDropHandler>,
    permission_handler: Option<WebViewPermissionHandler>,
    ipc_nonce: SharedString,
    ipc_rejection_reported: AtomicBool,
    popup_downgrade_reported: AtomicBool,
    zoom_hotkeys_enabled: bool,
    clipboard_access: bool,
    downloads: HashMap<usize, MacWebViewDownloadState>,
}

#[derive(Clone)]
struct MacWebViewDownloadState {
    url: SharedString,
    path: Option<PathBuf>,
}

struct MacPrintViewState {
    page_size: Size<Pixels>,
    margins: Edges<Pixels>,
    pages: Vec<PlatformPrintPage>,
}

struct MacWebViewHost {
    command_id: SharedString,
    webview: id,
    controller: id,
    delegate: id,
    native_window: id,
    native_view: id,
    state: Box<MacWebViewDelegateState>,
    // The declarative source last accepted from `PlatformWebView`. Controller commands do not
    // modify this baseline, otherwise the next unchanged render would navigate backward.
    declared_url: SharedString,
    declared_html: Option<SharedString>,
    request_headers: Option<http_client::http::HeaderMap>,
    current_url_hint: SharedString,
    user_agent: Option<SharedString>,
    storage_key: Option<SharedString>,
    javascript_disabled: bool,
    media_autoplay: Option<bool>,
    custom_protocol_schemes: Vec<SharedString>,
    background_color: Option<Rgba>,
    devtools: bool,
    user_scripts_initialized: bool,
    clipboard_access: bool,
    injected_css: Vec<SharedString>,
    injected_javascript: Vec<SharedString>,
    focused: Option<bool>,
    observing_title: bool,
    visible: bool,
    opacity: f32,
}

impl MacWindowState {
    fn pointer_lock_owner_id(&self) -> usize {
        self.native_window as usize
    }

    fn current_global_pointer_position() -> Result<CGPoint, GameInputError> {
        let source =
            CGEventSource::new(CGEventSourceStateID::CombinedSessionState).map_err(|()| {
                GameInputError::new(
                    GameInputErrorKind::InitializationFailed,
                    "macOS could not create a CoreGraphics event source for pointer lock",
                )
            })?;
        CGEvent::new(source)
            .map(|event| event.location())
            .map_err(|()| {
                GameInputError::new(
                    GameInputErrorKind::InitializationFailed,
                    "macOS could not query the global cursor position for pointer lock",
                )
            })
    }

    fn request_pointer_lock(&mut self) -> Result<(), GameInputError> {
        if !self.pointer_lock.begin_request()? {
            return Ok(());
        }

        if unsafe { self.native_window.isKeyWindow() != YES } {
            let error = GameInputError::new(
                GameInputErrorKind::Rejected,
                "macOS pointer lock requires the Kael window to be active",
            );
            return Err(self.pointer_lock.fail(error));
        }

        let owner_id = self.pointer_lock_owner_id();
        {
            let mut owner = MAC_POINTER_LOCK_OWNER.lock();
            if owner.is_some_and(|owner| owner != owner_id) {
                let error = GameInputError::new(
                    GameInputErrorKind::Rejected,
                    "another Kael window currently owns the macOS pointer lock",
                );
                return Err(self.pointer_lock.fail(error));
            }
            *owner = Some(owner_id);
        }

        let saved_position = match Self::current_global_pointer_position() {
            Ok(position) => position,
            Err(error) => {
                *MAC_POINTER_LOCK_OWNER.lock() = None;
                return Err(self.pointer_lock.fail(error));
            }
        };

        if let Err(code) = CGDisplay::associate_mouse_and_mouse_cursor_position(false) {
            *MAC_POINTER_LOCK_OWNER.lock() = None;
            let error = GameInputError::new(
                GameInputErrorKind::Platform,
                format!("macOS failed to disassociate the mouse cursor (CGError {code})"),
            );
            return Err(self.pointer_lock.fail(error));
        }

        NSCursor::hide();
        self.pointer_lock_saved_position = Some(saved_position);
        self.pointer_lock_cursor_hidden = true;
        self.pointer_lock.lock();
        Ok(())
    }

    /// Release only resources owned by this window. This is safe to call from
    /// focus-loss and drop paths and remains balanced after partial failures.
    fn release_pointer_lock(&mut self) -> Result<(), GameInputError> {
        let owner_id = self.pointer_lock_owner_id();
        let owns_global_lock = MAC_POINTER_LOCK_OWNER
            .lock()
            .is_some_and(|id| id == owner_id);

        let mut first_error = None;
        if owns_global_lock {
            if let Some(position) = self.pointer_lock_saved_position.take()
                && let Err(code) = CGDisplay::warp_mouse_cursor_position(position)
            {
                first_error = Some(GameInputError::new(
                    GameInputErrorKind::Platform,
                    format!("macOS failed to restore the cursor position (CGError {code})"),
                ));
            }

            if let Err(code) = CGDisplay::associate_mouse_and_mouse_cursor_position(true)
                && first_error.is_none()
            {
                first_error = Some(GameInputError::new(
                    GameInputErrorKind::Platform,
                    format!("macOS failed to reassociate the mouse cursor (CGError {code})"),
                ));
            }

            if self.pointer_lock_cursor_hidden {
                NSCursor::unhide();
                self.pointer_lock_cursor_hidden = false;
            }
            *MAC_POINTER_LOCK_OWNER.lock() = None;
        } else {
            self.pointer_lock_saved_position = None;
            self.pointer_lock_cursor_hidden = false;
        }

        if let Some(error) = first_error {
            return Err(self.pointer_lock.fail(error));
        }
        self.pointer_lock.unlock();
        Ok(())
    }

    fn move_traffic_light(&self) {
        if let Some(traffic_light_position) = self.traffic_light_position {
            if self.is_fullscreen() {
                // Moving traffic lights while fullscreen doesn't work,
                // Moving traffic lights while fullscreen is a known macOS limitation
                return;
            }

            let titlebar_height = self.titlebar_height();

            unsafe {
                let close_button: id = msg_send![
                    self.native_window,
                    standardWindowButton: NSWindowButton::CloseButton
                ];
                let min_button: id = msg_send![
                    self.native_window,
                    standardWindowButton: NSWindowButton::MiniaturizeButton
                ];
                let zoom_button: id = msg_send![
                    self.native_window,
                    standardWindowButton: NSWindowButton::ZoomButton
                ];

                let mut close_button_frame: NSRect = msg_send![close_button, frame];
                let mut min_button_frame: NSRect = msg_send![min_button, frame];
                let mut zoom_button_frame: NSRect = msg_send![zoom_button, frame];
                let mut origin = point(
                    traffic_light_position.x,
                    titlebar_height
                        - traffic_light_position.y
                        - px(close_button_frame.size.height as f32),
                );
                let button_spacing =
                    px((min_button_frame.origin.x - close_button_frame.origin.x) as f32);

                close_button_frame.origin = NSPoint::new(origin.x.into(), origin.y.into());
                let _: () = msg_send![close_button, setFrame: close_button_frame];
                origin.x += button_spacing;

                min_button_frame.origin = NSPoint::new(origin.x.into(), origin.y.into());
                let _: () = msg_send![min_button, setFrame: min_button_frame];
                origin.x += button_spacing;

                zoom_button_frame.origin = NSPoint::new(origin.x.into(), origin.y.into());
                let _: () = msg_send![zoom_button, setFrame: zoom_button_frame];
                origin.x += button_spacing;
            }
        }
    }

    fn start_display_link(&mut self) {
        self.stop_display_link();
        if !self.frame_polling_active {
            return;
        }
        unsafe {
            if !self
                .native_window
                .occlusionState()
                .contains(NSWindowOcclusionState::Visible)
            {
                return;
            }
        }
        let display_id = unsafe { display_id_for_screen(self.native_window.screen()) };
        if let Some(mut display_link) =
            DisplayLink::new(display_id, self.native_view.as_ptr() as *mut c_void, step).log_err()
        {
            display_link.start().log_err();
            self.display_link = Some(display_link);
        }
    }

    fn stop_display_link(&mut self) {
        self.display_link = None;
    }

    fn is_maximized(&self) -> bool {
        unsafe {
            let bounds = self.bounds();
            let screen_size = self.native_window.screen().visibleFrame().into();
            bounds.size == screen_size
        }
    }

    fn is_fullscreen(&self) -> bool {
        unsafe {
            let style_mask: NSWindowStyleMask = msg_send![self.native_window, styleMask];
            style_mask.contains(NSWindowStyleMask::FullScreen)
        }
    }

    fn bounds(&self) -> Bounds<Pixels> {
        let mut window_frame: NSRect = unsafe { msg_send![self.native_window, frame] };
        let screen: id = unsafe { msg_send![self.native_window, screen] };
        if screen == nil {
            return Bounds::new(point(px(0.), px(0.)), crate::DEFAULT_WINDOW_SIZE);
        }
        let screen_frame: NSRect = unsafe { msg_send![screen, frame] };

        // Flip the y coordinate to be top-left origin
        window_frame.origin.y =
            screen_frame.size.height - window_frame.origin.y - window_frame.size.height;

        Bounds::new(
            point(
                px((window_frame.origin.x - screen_frame.origin.x) as f32),
                px((window_frame.origin.y + screen_frame.origin.y) as f32),
            ),
            size(
                px(window_frame.size.width as f32),
                px(window_frame.size.height as f32),
            ),
        )
    }

    fn content_size(&self) -> Size<Pixels> {
        let NSSize { width, height, .. } = unsafe { self.native_window.contentView().frame() }.size;
        size(px(width as f32), px(height as f32))
    }

    fn scale_factor(&self) -> f32 {
        get_scale_factor(self.native_window)
    }

    fn titlebar_height(&self) -> Pixels {
        unsafe {
            let frame = self.native_window.frame();
            let content_layout_rect: NSRect = msg_send![self.native_window, contentLayoutRect];
            px((frame.size.height - content_layout_rect.size.height) as f32)
        }
    }

    fn sync_webviews(&mut self, webviews: &[PlatformWebView]) {
        let mut active_ids: HashSet<SharedString> = HashSet::default();
        let mut command_id_counts: HashMap<SharedString, usize> = HashMap::default();
        for webview in webviews {
            *command_id_counts.entry(webview.id.clone()).or_default() += 1;
        }

        for webview in webviews {
            let webview_id = webview.instance_id.clone();
            active_ids.insert(webview_id.clone());

            let needs_rebuild = self.webviews.get(&webview_id).is_some_and(|host| {
                host.storage_key != webview.storage_key
                    || host.javascript_disabled != webview.javascript_disabled
                    || host.media_autoplay != webview.media_autoplay
                    || host.custom_protocol_schemes != webview.custom_protocol_schemes
            });
            if needs_rebuild {
                self.webviews.remove(&webview_id);
            }

            if let Some(host) = self.webviews.get_mut(&webview_id) {
                host.sync(webview, self.native_window, self.native_view.as_ptr() as id);
            } else {
                let mut host = unsafe {
                    MacWebViewHost::new(
                        webview,
                        self.native_window,
                        self.native_view.as_ptr() as id,
                    )
                };
                if command_id_counts.get(&webview.id) == Some(&1)
                    && let Some(commands) = self.pending_webview_commands.remove(&webview.id)
                {
                    for command in commands {
                        if let Err(error) = host.apply_command(command) {
                            log::error!(
                                "failed to apply queued macOS WebView command for {}: {error:#}",
                                webview.id
                            );
                        }
                    }
                }
                self.webviews.insert(webview_id, host);
            }
        }

        let stale_ids = self
            .webviews
            .keys()
            .filter(|webview_id| !active_ids.contains(*webview_id))
            .cloned()
            .collect::<Vec<_>>();
        for webview_id in stale_ids {
            self.webviews.remove(&webview_id);
        }

        let content_view = unsafe { self.native_window.contentView() };
        let mut previous_view = self.native_view.as_ptr() as id;
        for webview in webviews {
            if let Some(host) = self.webviews.get(&webview.instance_id) {
                unsafe {
                    let _: () = msg_send![
                        content_view,
                        addSubview: host.webview,
                        positioned: NSWindowOrderingMode::Above,
                        relativeTo: previous_view
                    ];
                }
                previous_view = host.webview;
            }
        }
    }

    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        let webview_id = webview_command_id(&command);
        let mut matches = self
            .webviews
            .values_mut()
            .filter(|host| host.command_id == webview_id);
        if let Some(host) = matches.next() {
            if matches.next().is_some() {
                anyhow::bail!(
                    "ambiguous webview id `{}`; WebView command ids must be unique within a window",
                    webview_id
                );
            }
            host.apply_command(command)?;
        } else {
            self.pending_webview_commands
                .entry(webview_id)
                .or_default()
                .push(command);
        }
        Ok(())
    }

    fn window_bounds(&self) -> WindowBounds {
        if self.is_fullscreen() {
            WindowBounds::Fullscreen(self.fullscreen_restore_bounds)
        } else {
            WindowBounds::Windowed(self.bounds())
        }
    }
}

impl MacWebViewHost {
    unsafe fn new(webview: &PlatformWebView, native_window: id, native_view: id) -> Self {
        if let Some(general_autofill) = webview.general_autofill {
            log::warn!(
                "WebView general autofill override {general_autofill} is not supported by Kael's macOS WebView backend"
            );
        }
        let content_view = unsafe { native_window.contentView() };
        let frame = unsafe {
            ns_rect_from_bounds(webview.bounds, px(content_view.bounds().size.height as f32))
        };

        let config: id = unsafe { msg_send![lookup_class(c"WKWebViewConfiguration"), alloc] };
        let config: id = unsafe { msg_send![config, init] };
        if let Some(autoplay) = webview.media_autoplay {
            let media_policy = if autoplay {
                WK_MEDIA_PLAYBACK_TYPE_NONE
            } else {
                WK_MEDIA_PLAYBACK_TYPE_ALL
            };
            let _: () = unsafe {
                msg_send![
                    config,
                    setMediaTypesRequiringUserActionForPlayback: media_policy
                ]
            };
        }
        let preferences: id = unsafe { msg_send![config, preferences] };
        if !preferences.is_null() {
            let _: () = unsafe {
                msg_send![
                    preferences,
                    setJavaScriptEnabled: Bool::new(!webview.javascript_disabled)
                ]
            };
        }
        let controller: id = unsafe { msg_send![lookup_class(c"WKUserContentController"), alloc] };
        let controller: id = unsafe { msg_send![controller, init] };
        let data_store: id = unsafe { mac_webview_data_store(webview.storage_key.as_ref()) };
        let _: () = unsafe { msg_send![config, setWebsiteDataStore: data_store] };
        let _: () = unsafe { msg_send![config, setUserContentController: controller] };

        let mut state = Box::new(MacWebViewDelegateState {
            async_window: webview.async_window.clone(),
            message_handler: webview.message_handler.clone(),
            navigation_handler: webview.navigation_handler.clone(),
            new_window_handler: webview.new_window_handler.clone(),
            download_started_handler: webview.download_started_handler.clone(),
            download_completed_handler: webview.download_completed_handler.clone(),
            document_title_changed_handler: webview.document_title_changed_handler.clone(),
            page_load_handler: webview.page_load_handler.clone(),
            drag_drop_handler: webview.drag_drop_handler.clone(),
            permission_handler: webview.permission_handler.clone(),
            ipc_nonce: uuid::Uuid::new_v4().simple().to_string().into(),
            ipc_rejection_reported: AtomicBool::new(false),
            popup_downgrade_reported: AtomicBool::new(false),
            zoom_hotkeys_enabled: webview.zoom_hotkeys_enabled,
            clipboard_access: webview.clipboard_access,
            downloads: HashMap::default(),
        });

        let delegate: id = unsafe { msg_send![WEBVIEW_DELEGATE_CLASS, alloc] };
        let delegate: id = unsafe { msg_send![delegate, init] };
        unsafe {
            store_ivar(
                delegate,
                WEBVIEW_STATE_IVAR,
                state.as_mut() as *mut MacWebViewDelegateState as *mut c_void,
            );
        }
        let _: () = unsafe {
            msg_send![
                controller,
                addScriptMessageHandler: delegate,
                name: ns_string(WEBVIEW_MESSAGE_HANDLER_NAME)
            ]
        };
        for scheme in &webview.custom_protocol_schemes {
            let _: () = unsafe {
                msg_send![
                    config,
                    setURLSchemeHandler: delegate,
                    forURLScheme: ns_string(scheme.as_ref())
                ]
            };
        }

        let webview_view: id = unsafe { msg_send![WEBVIEW_CLASS, alloc] };
        let webview_view: id =
            unsafe { msg_send![webview_view, initWithFrame: frame, configuration: config] };
        let _: () = unsafe { msg_send![config, release] };
        unsafe {
            store_ivar(
                webview_view,
                WEBVIEW_STATE_IVAR,
                state.as_mut() as *mut MacWebViewDelegateState as *mut c_void,
            );
            register_dragged_types(webview_view);
        }
        if webview.devtools && !set_webview_inspectable(webview_view, true) {
            log::warn!(
                "WebView devtools requested, but this macOS WebView backend does not expose WKWebView inspectability"
            );
        }
        apply_webview_background_color(webview_view, webview.background_color);
        let _: () = unsafe { msg_send![webview_view, setNavigationDelegate: delegate] };
        let _: () = unsafe { msg_send![webview_view, setUIDelegate: delegate] };
        let _: () = unsafe { msg_send![webview_view, setHidden: Bool::new(!webview.visible)] };
        let _: () = unsafe {
            msg_send![
                content_view,
                addSubview: webview_view,
                positioned: NSWindowOrderingMode::Above,
                relativeTo: native_view
            ]
        };

        let mut host = Self {
            command_id: webview.id.clone(),
            webview: unsafe { webview_view.autorelease() },
            controller: unsafe { controller.autorelease() },
            delegate: unsafe { delegate.autorelease() },
            native_window,
            native_view,
            state,
            declared_url: SharedString::default(),
            declared_html: None,
            request_headers: None,
            current_url_hint: SharedString::default(),
            user_agent: None,
            storage_key: webview.storage_key.clone(),
            javascript_disabled: webview.javascript_disabled,
            media_autoplay: webview.media_autoplay,
            custom_protocol_schemes: webview.custom_protocol_schemes.clone(),
            background_color: webview.background_color,
            devtools: webview.devtools,
            user_scripts_initialized: false,
            clipboard_access: false,
            injected_css: Vec::new(),
            injected_javascript: Vec::new(),
            focused: None,
            observing_title: false,
            visible: webview.visible,
            opacity: -1.0,
        };
        host.sync(webview, native_window, native_view);
        host
    }

    fn sync(&mut self, webview: &PlatformWebView, native_window: id, native_view: id) {
        self.command_id = webview.id.clone();
        self.native_window = native_window;
        self.native_view = native_view;
        self.state.async_window = webview.async_window.clone();
        self.state.message_handler = webview.message_handler.clone();
        self.state.navigation_handler = webview.navigation_handler.clone();
        self.state.new_window_handler = webview.new_window_handler.clone();
        self.state.download_started_handler = webview.download_started_handler.clone();
        self.state.download_completed_handler = webview.download_completed_handler.clone();
        self.state.document_title_changed_handler = webview.document_title_changed_handler.clone();
        self.state.page_load_handler = webview.page_load_handler.clone();
        self.state.drag_drop_handler = webview.drag_drop_handler.clone();
        self.state.permission_handler = webview.permission_handler.clone();
        self.state.zoom_hotkeys_enabled = webview.zoom_hotkeys_enabled;
        self.state.clipboard_access = webview.clipboard_access;

        unsafe {
            let content_height = px(native_window.contentView().bounds().size.height as f32);
            let frame = ns_rect_from_bounds(webview.bounds, content_height);
            let _: () = msg_send![self.webview, setFrame: frame];
        }

        if self.visible != webview.visible {
            unsafe {
                let _: () = msg_send![self.webview, setHidden: Bool::new(!webview.visible)];
            }
            self.visible = webview.visible;
        }

        if self.opacity != webview.opacity {
            unsafe {
                let _: () = msg_send![self.webview, setAlphaValue: webview.opacity as f64];
            }
            self.opacity = webview.opacity;
        }

        if self.background_color != webview.background_color {
            apply_webview_background_color(self.webview, webview.background_color);
            self.background_color = webview.background_color;
        }

        if self.devtools != webview.devtools {
            if !set_webview_inspectable(self.webview, webview.devtools) && webview.devtools {
                log::warn!(
                    "WebView devtools requested, but this macOS WebView backend does not expose WKWebView inspectability"
                );
            }
            self.devtools = webview.devtools;
        }

        if self.focused != webview.focused {
            if webview.focused == Some(true) {
                if let Err(error) = self.focus() {
                    log::error!("failed to focus macOS WebView {}: {error:#}", webview.id);
                }
            }
            self.focused = webview.focused;
        }

        self.sync_title_observer(webview.document_title_changed_handler.is_some());

        let scripts_changed = self.sync_user_scripts(webview);
        let user_agent_changed = if self.user_agent != webview.user_agent {
            unsafe {
                if let Some(user_agent) = webview.user_agent.as_ref() {
                    let _: () = msg_send![
                        self.webview,
                        setCustomUserAgent: ns_string(user_agent.as_ref())
                    ];
                } else {
                    let _: () = msg_send![self.webview, setCustomUserAgent: nil];
                }
            }
            self.user_agent = webview.user_agent.clone();
            true
        } else {
            false
        };

        if !webview.url.as_ref().is_empty()
            && (self.declared_url != webview.url || self.request_headers != webview.request_headers)
        {
            match self.load_url_with_headers(webview.url.as_ref(), webview.request_headers.as_ref())
            {
                Ok(()) => {
                    self.declared_url = webview.url.clone();
                    self.declared_html = None;
                    self.request_headers = webview.request_headers.clone();
                    self.current_url_hint = webview.url.clone();
                }
                Err(error) => {
                    log::error!("failed to navigate macOS WebView {}: {error:#}", webview.id)
                }
            }
        } else if webview.url.as_ref().is_empty() && self.declared_html != webview.html {
            if let Some(html) = webview.html.as_ref() {
                match self.load_html(html.as_ref()) {
                    Ok(()) => {
                        self.declared_url = SharedString::default();
                        self.declared_html = webview.html.clone();
                        self.request_headers = None;
                        self.current_url_hint = SharedString::default();
                    }
                    Err(error) => {
                        log::error!(
                            "failed to load macOS WebView {} HTML: {error:#}",
                            webview.id
                        )
                    }
                }
            } else {
                self.declared_url = SharedString::default();
                self.declared_html = None;
                self.request_headers = None;
                self.current_url_hint = SharedString::default();
            }
        } else if scripts_changed || user_agent_changed {
            // Reload the document that is actually visible. Re-loading the declarative HTML here
            // would undo an imperative controller navigation on the next render.
            self.reload();
        }
    }

    fn sync_user_scripts(&mut self, webview: &PlatformWebView) -> bool {
        if self.user_scripts_initialized
            && self.storage_key == webview.storage_key
            && self.clipboard_access == webview.clipboard_access
            && self.injected_css == webview.injected_css
            && self.injected_javascript == webview.injected_javascript
        {
            return false;
        }

        unsafe {
            let _: () = msg_send![self.controller, removeAllUserScripts];
        }

        unsafe {
            add_webview_user_script(
                self.controller,
                &webview_bridge_script(webview.storage_key.as_ref(), self.state.ipc_nonce.as_ref()),
                WKUserScriptInjectionTimeAtDocumentStart,
            );
        }

        if webview.clipboard_access {
            unsafe {
                add_webview_user_script(
                    self.controller,
                    &webview_clipboard_script(self.state.ipc_nonce.as_ref()),
                    WKUserScriptInjectionTimeAtDocumentStart,
                );
            }
        }

        for css in &webview.injected_css {
            unsafe {
                add_webview_user_script(
                    self.controller,
                    &webview_css_script(css.as_ref()),
                    WKUserScriptInjectionTimeAtDocumentEnd,
                );
            }
        }

        for javascript in &webview.injected_javascript {
            unsafe {
                add_webview_user_script(
                    self.controller,
                    javascript.as_ref(),
                    WKUserScriptInjectionTimeAtDocumentEnd,
                );
            }
        }

        self.storage_key = webview.storage_key.clone();
        self.clipboard_access = webview.clipboard_access;
        self.injected_css = webview.injected_css.clone();
        self.injected_javascript = webview.injected_javascript.clone();
        self.user_scripts_initialized = true;
        true
    }

    fn sync_title_observer(&mut self, should_observe: bool) {
        if self.observing_title == should_observe {
            return;
        }

        unsafe {
            if should_observe {
                let _: () = msg_send![
                    self.webview,
                    addObserver: self.delegate,
                    forKeyPath: ns_string("title"),
                    options: NS_KEY_VALUE_OBSERVING_OPTION_NEW,
                    context: ptr::null_mut::<c_void>()
                ];
            } else {
                let _: () = msg_send![
                    self.webview,
                    removeObserver: self.delegate,
                    forKeyPath: ns_string("title")
                ];
            }
        }
        self.observing_title = should_observe;
    }

    fn apply_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => {
                self.load_url(url.as_ref())?;
                self.current_url_hint = url;
            }
            PlatformWebViewCommand::NavigateWithHeaders { url, headers, .. } => {
                self.load_url_with_headers(url.as_ref(), Some(&headers))?;
                self.current_url_hint = url;
            }
            PlatformWebViewCommand::LoadHtml { html, .. } => {
                self.load_html(html.as_ref())?;
                self.current_url_hint = SharedString::default();
            }
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.evaluate_javascript(script.as_ref())
            }
            PlatformWebViewCommand::EvaluateJavaScriptWithResult {
                script, callback, ..
            } => {
                self.evaluate_javascript_with_result(script.as_ref(), callback);
            }
            PlatformWebViewCommand::PostMessage { message, .. } => {
                let payload =
                    serde_json::to_string(&message).context("serializing macOS WebView message")?;
                let script = format!(
                    "(() => {{ const payload = {payload}; if (window.dispatchEvent) {{ window.dispatchEvent(new MessageEvent('message', {{ data: payload }})); }} if (typeof window.onmessage === 'function') {{ window.onmessage({{ data: payload }}); }} }})();"
                );
                self.evaluate_javascript(&script);
            }
            PlatformWebViewCommand::Reload { .. } => self.reload(),
            PlatformWebViewCommand::GoBack { .. } => unsafe {
                let _: () = msg_send![self.webview, goBack];
            },
            PlatformWebViewCommand::GoForward { .. } => unsafe {
                let _: () = msg_send![self.webview, goForward];
            },
            PlatformWebViewCommand::OpenDevTools { .. } => {
                anyhow::bail!(
                    "opening WebView devtools is unsupported by WKWebView; enable WebView inspectability and open the inspector from Safari"
                );
            }
            PlatformWebViewCommand::CloseDevTools { .. } => {
                anyhow::bail!(
                    "closing WebView devtools is unavailable because WKWebView has no public close-inspector API"
                );
            }
            PlatformWebViewCommand::IsDevToolsOpen { callback, .. } => {
                callback(Err(
                    "WebView devtools open-state is not supported by Kael's macOS WebView backend"
                        .into(),
                ));
            }
            PlatformWebViewCommand::Print { .. } => self.print()?,
            PlatformWebViewCommand::SetZoomFactor { factor, .. } => self.set_zoom_factor(factor),
            PlatformWebViewCommand::Focus { .. } => self.focus()?,
            PlatformWebViewCommand::FocusParent { .. } => self.focus_parent()?,
            PlatformWebViewCommand::ClearBrowsingData { .. } => {
                self.clear_browsing_data()?;
            }
            PlatformWebViewCommand::ReadUrl { callback, .. } => {
                callback(Ok(self.current_url()));
            }
            PlatformWebViewCommand::ReadCookies { url, callback, .. } => {
                self.read_cookies(url, callback);
            }
            PlatformWebViewCommand::SetCookie {
                cookie, callback, ..
            } => {
                self.set_cookie(cookie, callback);
            }
            PlatformWebViewCommand::DeleteCookie {
                cookie, callback, ..
            } => {
                self.delete_cookie(cookie, callback);
            }
        }
        Ok(())
    }

    fn load_url(&mut self, url: &str) -> anyhow::Result<()> {
        self.load_url_with_headers(url, None)
    }

    fn load_url_with_headers(
        &mut self,
        url: &str,
        headers: Option<&http_client::http::HeaderMap>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!url.is_empty(), "macOS WebView URL cannot be empty");

        unsafe {
            let url: id = msg_send![lookup_class(c"NSURL"), URLWithString: ns_string(url)];
            anyhow::ensure!(!url.is_null(), "invalid macOS WebView URL");
            let request: id = msg_send![lookup_class(c"NSMutableURLRequest"), requestWithURL: url];
            anyhow::ensure!(
                !request.is_null(),
                "could not create macOS WebView URL request"
            );
            if let Some(headers) = headers {
                for (name, value) in headers {
                    let value = value.to_str().with_context(|| {
                        format!("WebView request header `{}` is not UTF-8", name.as_str())
                    })?;
                    let _: () = msg_send![
                        request,
                        setValue: ns_string(value),
                        forHTTPHeaderField: ns_string(name.as_str())
                    ];
                }
            }
            let navigation: id = msg_send![self.webview, loadRequest: request];
            anyhow::ensure!(
                !navigation.is_null(),
                "macOS WebView rejected the URL request"
            );
        }
        Ok(())
    }

    fn load_html(&mut self, html: &str) -> anyhow::Result<()> {
        unsafe {
            let navigation: id = msg_send![
                self.webview,
                loadHTMLString: ns_string(html),
                baseURL: nil
            ];
            anyhow::ensure!(
                !navigation.is_null(),
                "macOS WebView rejected the HTML document"
            );
        }
        Ok(())
    }

    fn current_url(&self) -> SharedString {
        let url = mac_webview_url(self.webview);
        if !url.as_ref().is_empty() {
            return url;
        }

        self.current_url_hint.clone()
    }

    fn cookie_store(&self) -> id {
        unsafe {
            let config: id = msg_send![self.webview, configuration];
            if config.is_null() {
                return nil;
            }
            let data_store: id = msg_send![config, websiteDataStore];
            if data_store.is_null() {
                return nil;
            }
            msg_send![data_store, httpCookieStore]
        }
    }

    fn read_cookies(
        &self,
        url: Option<SharedString>,
        callback: crate::webview::WebViewCookieCallback,
    ) {
        let async_window = self.state.async_window.clone();
        let cookie_store = self.cookie_store();
        if cookie_store.is_null() {
            dispatch_webview_completion(async_window, "webview cookie read", move || {
                callback(Err("WebView cookie store is unavailable".into()));
            });
            return;
        }

        let filter = url
            .as_ref()
            .and_then(|url| WebViewCookieUrlFilter::parse(url.as_ref()));
        let block = RcBlock::new(move |cookies: id| {
            let result = catch_platform_callback(
                "webview cookie decode",
                Err("WebView cookie response could not be decoded".into()),
                || Ok(unsafe { mac_webview_cookies_from_array(cookies, filter.as_ref()) }),
            );
            dispatch_webview_completion(async_window.clone(), "webview cookie read", {
                let callback = callback.clone();
                move || callback(result)
            });
        });

        unsafe {
            let _: () = msg_send![cookie_store, getAllCookies: &*block];
        }
    }

    fn set_cookie(
        &self,
        cookie: WebViewCookie,
        callback: crate::webview::WebViewCookieMutationCallback,
    ) {
        let async_window = self.state.async_window.clone();
        let cookie_store = self.cookie_store();
        if cookie_store.is_null() {
            dispatch_webview_completion(async_window, "webview cookie set", move || {
                callback(Err("WebView cookie store is unavailable".into()));
            });
            return;
        }
        let current_url = self.current_url();
        let Some(cookie) = mac_webview_cookie_to_ns(cookie, Some(current_url.as_ref())) else {
            dispatch_webview_completion(async_window, "webview cookie set", move || {
                callback(Err(
                    "WebView cookie mutation requires a domain or committed WebView URL".into(),
                ));
            });
            return;
        };

        let block = RcBlock::new(move || {
            dispatch_webview_completion(async_window.clone(), "webview cookie set", {
                let callback = callback.clone();
                move || callback(Ok(()))
            });
        });
        unsafe {
            let _: () = msg_send![
                cookie_store,
                setCookie: cookie,
                completionHandler: &*block
            ];
        }
    }

    fn delete_cookie(
        &self,
        cookie: WebViewCookie,
        callback: crate::webview::WebViewCookieMutationCallback,
    ) {
        let async_window = self.state.async_window.clone();
        let cookie_store = self.cookie_store();
        if cookie_store.is_null() {
            dispatch_webview_completion(async_window, "webview cookie delete", move || {
                callback(Err("WebView cookie store is unavailable".into()));
            });
            return;
        }
        let current_url = self.current_url();
        let Some(cookie) = mac_webview_cookie_to_ns(cookie, Some(current_url.as_ref())) else {
            dispatch_webview_completion(async_window, "webview cookie delete", move || {
                callback(Err(
                    "WebView cookie mutation requires a domain or committed WebView URL".into(),
                ));
            });
            return;
        };

        let block = RcBlock::new(move || {
            dispatch_webview_completion(async_window.clone(), "webview cookie delete", {
                let callback = callback.clone();
                move || callback(Ok(()))
            });
        });
        unsafe {
            let _: () = msg_send![
                cookie_store,
                deleteCookie: cookie,
                completionHandler: &*block
            ];
        }
    }

    fn evaluate_javascript(&mut self, script: &str) {
        unsafe {
            let _: () = msg_send![
                self.webview,
                evaluateJavaScript: ns_string(script),
                completionHandler: nil
            ];
        }
    }

    fn evaluate_javascript_with_result(
        &mut self,
        script: &str,
        callback: crate::webview::WebViewJavaScriptResultCallback,
    ) {
        let async_window = self.state.async_window.clone();
        let script = serde_json::to_string(script).unwrap_or_else(|_| "\"\"".into());
        let script = format!(
            "(() => {{ const value = (0, eval)({script}); const serialized = JSON.stringify(value); return serialized === undefined ? 'null' : serialized; }})()"
        );
        let callback_for_block = callback.clone();
        let block = RcBlock::new(move |result: id, error: id| {
            let result = catch_platform_callback(
                "webview JavaScript result decode",
                Err("WebView JavaScript result could not be decoded".into()),
                || {
                    if !error.is_null() {
                        Err(ns_error_message(error).into())
                    } else if result.is_null() {
                        Ok("null".into())
                    } else {
                        Ok(unsafe { result.to_str() }.to_string().into())
                    }
                },
            );
            dispatch_webview_completion(async_window.clone(), "webview JavaScript result", {
                let callback = callback_for_block.clone();
                move || callback(result)
            });
        });

        unsafe {
            let _: () = msg_send![
                self.webview,
                evaluateJavaScript: ns_string(&script),
                completionHandler: &*block
            ];
        }
    }

    fn clear_browsing_data(&mut self) -> anyhow::Result<()> {
        unsafe {
            let config: id = msg_send![self.webview, configuration];
            anyhow::ensure!(
                !config.is_null(),
                "macOS WebView configuration is unavailable"
            );

            let store: id = msg_send![config, websiteDataStore];
            anyhow::ensure!(!store.is_null(), "macOS WebView data store is unavailable");

            let data_types: id =
                msg_send![lookup_class(c"WKWebsiteDataStore"), allWebsiteDataTypes];
            let since: id =
                msg_send![lookup_class(c"NSDate"), dateWithTimeIntervalSince1970: 0.0f64];
            let block = RcBlock::new(|| {});
            let _: () = msg_send![
                store,
                removeDataOfTypes: data_types,
                modifiedSince: since,
                completionHandler: &*block
            ];
        }
        Ok(())
    }

    fn focus(&mut self) -> anyhow::Result<()> {
        unsafe {
            let focused: BOOL = msg_send![self.native_window, makeFirstResponder: self.webview];
            anyhow::ensure!(focused == YES, "macOS refused to focus the WebView");
        }
        Ok(())
    }

    fn focus_parent(&mut self) -> anyhow::Result<()> {
        unsafe {
            let focused: BOOL = msg_send![self.native_window, makeFirstResponder: self.native_view];
            anyhow::ensure!(focused == YES, "macOS refused to focus the WebView parent");
        }
        Ok(())
    }

    fn print(&mut self) -> anyhow::Result<()> {
        unsafe {
            let print_info: id = {
                let shared: id = msg_send![lookup_class(c"NSPrintInfo"), sharedPrintInfo];
                msg_send![shared, copy]
            };
            let operation: id = msg_send![
                lookup_class(c"NSPrintOperation"),
                printOperationWithView: self.webview,
                printInfo: print_info
            ];
            anyhow::ensure!(
                !operation.is_null(),
                "WebView print operation could not be created"
            );

            let _: () = msg_send![operation, setShowsPrintPanel: YES];
            let _: () = msg_send![operation, setShowsProgressPanel: YES];
            let _: () = msg_send![operation, setCanSpawnSeparateThread: NO];
            let success: BOOL = msg_send![
                operation,
                runOperationModalForWindow: self.native_window,
                delegate: nil,
                didRunSelector: ptr::null::<c_void>(),
                contextInfo: ptr::null_mut::<c_void>()
            ];
            anyhow::ensure!(
                success == YES,
                "WebView print operation failed or was cancelled"
            );
        }
        Ok(())
    }

    fn set_zoom_factor(&mut self, factor: f64) {
        let factor = if factor.is_finite() {
            factor.clamp(0.25, 5.0)
        } else {
            1.0
        };
        unsafe {
            let _: () = msg_send![self.webview, setPageZoom: factor];
        }
    }

    fn reload(&mut self) {
        unsafe {
            let _: () = msg_send![self.webview, reload];
        }
    }
}

unsafe fn mac_webview_data_store(storage_key: Option<&SharedString>) -> id {
    let data_store_class = unsafe { lookup_class(c"WKWebsiteDataStore") };
    let Some(storage_key) = storage_key else {
        return unsafe { msg_send![data_store_class, nonPersistentDataStore] };
    };

    let custom_stores_available = is_macos_version_at_least(NSOperatingSystemVersion {
        majorVersion: 14,
        minorVersion: 0,
        patchVersion: 0,
    });
    if custom_stores_available {
        let identifier = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("kael:webview-profile:{storage_key}").as_bytes(),
        );
        let identifier = NSUUID::from_bytes(identifier.into_bytes());
        return unsafe { msg_send![data_store_class, dataStoreForIdentifier: &*identifier] };
    }

    // WKWebsiteDataStore did not expose named persistent stores before macOS
    // 14. Retain persistence on older systems, but callers should not assume
    // profile-key isolation there.
    unsafe { msg_send![data_store_class, defaultDataStore] }
}

impl Drop for MacWebViewHost {
    fn drop(&mut self) {
        unsafe {
            if self.observing_title && !self.webview.is_null() && !self.delegate.is_null() {
                let _: () = msg_send![
                    self.webview,
                    removeObserver: self.delegate,
                    forKeyPath: ns_string("title")
                ];
                self.observing_title = false;
            }
            if !self.delegate.is_null() {
                store_ivar(self.delegate, WEBVIEW_STATE_IVAR, ptr::null_mut::<c_void>());
            }
            if !self.controller.is_null() {
                let _: () = msg_send![
                    self.controller,
                    removeScriptMessageHandlerForName: ns_string(WEBVIEW_MESSAGE_HANDLER_NAME)
                ];
            }
            if !self.webview.is_null() {
                store_ivar(self.webview, WEBVIEW_STATE_IVAR, ptr::null_mut::<c_void>());
                let _: () = msg_send![self.webview, setNavigationDelegate: nil];
                let _: () = msg_send![self.webview, setUIDelegate: nil];
                self.webview.removeFromSuperview();
            }
        }
    }
}

unsafe impl Send for MacWindowState {}

pub(crate) struct MacWindow(Arc<Mutex<MacWindowState>>);

impl MacWindow {
    pub fn open(
        handle: AnyWindowHandle,
        WindowParams {
            bounds,
            titlebar,
            kind,
            is_movable,
            is_resizable,
            is_minimizable,
            focus,
            show,
            display_id,
            window_min_size,
            tabbing_identifier,
            mouse_passthrough,
            parent: _parent,
        }: WindowParams,
        executor: ForegroundExecutor,
        renderer_context: renderer::Context,
    ) -> anyhow::Result<Self> {
        unsafe {
            let allows_automatic_window_tabbing = tabbing_identifier.is_some();
            if allows_automatic_window_tabbing {
                let () = msg_send![lookup_class(c"NSWindow"), setAllowsAutomaticWindowTabbing: YES];
            } else {
                let () = msg_send![lookup_class(c"NSWindow"), setAllowsAutomaticWindowTabbing: NO];
            }

            let mut style_mask;
            if let Some(titlebar) = titlebar.as_ref() {
                style_mask = NSWindowStyleMask::Closable | NSWindowStyleMask::Titled;

                if is_resizable {
                    style_mask |= NSWindowStyleMask::Resizable;
                }

                if is_minimizable {
                    style_mask |= NSWindowStyleMask::Miniaturizable;
                }

                if titlebar.appears_transparent {
                    style_mask |= NSWindowStyleMask::FullSizeContentView;
                }
            } else {
                style_mask = NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView;
            }

            let native_window: id = match kind {
                // Bottom/Wallpaper are wlr-layer-shell-only kinds with no macOS
                // equivalent; fall back to a regular window, matching what the wayland
                // backend does when the compositor itself lacks layer-shell support.
                WindowKind::Normal | WindowKind::Floating | WindowKind::Bottom(_)
                | WindowKind::Wallpaper(_) => {
                    msg_send![WINDOW_CLASS, alloc]
                }
                // Top has no real macOS equivalent either - unlike X11's struts or
                // Windows' AppBars, there's no unentitled way to reserve screen space
                // - so it's treated the same as Overlay rather than inventing a third,
                // equally non-reserving elevated level.
                WindowKind::PopUp | WindowKind::Overlay(_) | WindowKind::Top(_) => {
                    style_mask |= NSWindowStyleMaskNonactivatingPanel;
                    msg_send![PANEL_CLASS, alloc]
                }
            };

            let display = display_id
                .and_then(MacDisplay::find_by_id)
                .unwrap_or_else(MacDisplay::primary);

            let mut target_screen = nil;
            let mut screen_frame = None;

            let screens: id = msg_send![lookup_class(c"NSScreen"), screens];
            let count: NSUInteger = msg_send![screens, count];
            for i in 0..count {
                let screen: id = msg_send![screens, objectAtIndex: i];
                let frame: NSRect = msg_send![screen, frame];
                let display_id = display_id_for_screen(screen);
                if display_id == display.0 {
                    screen_frame = Some(frame);
                    target_screen = screen;
                }
            }

            let screen_frame = screen_frame.unwrap_or_else(|| {
                let screen: id = msg_send![lookup_class(c"NSScreen"), mainScreen];
                target_screen = screen;
                msg_send![screen, frame]
            });

            let window_rect = NSRect::new(
                NSPoint::new(
                    screen_frame.origin.x + bounds.origin.x.0 as f64,
                    screen_frame.origin.y
                        + (display.bounds().size.height - bounds.origin.y).0 as f64,
                ),
                NSSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64),
            );

            let native_window: id = msg_send![
                native_window,
                initWithContentRect: window_rect,
                styleMask: style_mask,
                backing: NSBackingStoreBuffered,
                defer: NO,
                screen: target_screen
            ];
            anyhow::ensure!(
                !native_window.is_null(),
                "AppKit failed to create a native window"
            );
            register_dragged_types(native_window);
            let () = msg_send![
                native_window,
                setReleasedWhenClosed: NO
            ];

            let content_view = native_window.contentView();
            let native_view: id = msg_send![VIEW_CLASS, alloc];
            let native_view = native_view.initWithFrame_(content_view.bounds());
            let native_view_ptr = NonNull::new(native_view)
                .ok_or_else(|| anyhow::anyhow!("AppKit failed to create a native view"))?;

            let mut window = Self(Arc::new(Mutex::new(MacWindowState {
                handle,
                executor,
                native_window,
                native_view: native_view_ptr,
                blurred_view: None,
                display_link: None,
                frame_polling_active: false,
                renderer: renderer::try_new_renderer(
                    renderer_context,
                    native_window as *mut _,
                    native_view as *mut _,
                    bounds.size.map(|pixels| pixels.0),
                    false,
                )?,
                request_frame_callback: None,
                event_callback: None,
                activate_callback: None,
                resize_callback: None,
                resize_sync_scheduled: false,
                moved_callback: None,
                should_close_callback: None,
                close_callback: None,
                appearance_changed_callback: None,
                input_handler: None,
                last_key_equivalent: None,
                synthetic_drag_counter: 0,
                traffic_light_position: titlebar
                    .as_ref()
                    .and_then(|titlebar| titlebar.traffic_light_position),
                transparent_titlebar: titlebar
                    .as_ref()
                    .is_none_or(|titlebar| titlebar.appears_transparent),
                previous_modifiers_changed_event: None,
                keystroke_for_do_command: None,
                do_command_handled: None,
                external_files_dragged: false,
                first_mouse: false,
                fullscreen_restore_bounds: Bounds::default(),
                move_tab_to_new_window_callback: None,
                merge_all_windows_callback: None,
                select_next_tab_callback: None,
                select_previous_tab_callback: None,
                toggle_tab_bar_callback: None,
                activated_least_once: false,
                pointer_lock: crate::game_input::NativePointerLockState::new(true),
                pointer_lock_saved_position: None,
                pointer_lock_cursor_hidden: false,
                accessibility_provider: super::accessibility::MacAccessibilityProvider::new(
                    native_view as *mut c_void,
                ),
                webviews: HashMap::default(),
                pending_webview_commands: HashMap::default(),
            })));

            store_ivar(
                native_window,
                WINDOW_STATE_IVAR,
                Arc::into_raw(window.0.clone()) as *const c_void,
            );
            native_window.setDelegate_(native_window);
            store_ivar(
                native_view,
                WINDOW_STATE_IVAR,
                Arc::into_raw(window.0.clone()) as *const c_void,
            );

            if let Some(title) = titlebar
                .as_ref()
                .and_then(|t| t.title.as_ref().map(AsRef::as_ref))
            {
                window.set_title(title);
            }

            native_window.setMovable_(Bool::new(is_movable));

            if let Some(window_min_size) = window_min_size {
                native_window.setContentMinSize_(NSSize {
                    width: window_min_size.width.to_f64(),
                    height: window_min_size.height.to_f64(),
                });
            }

            if titlebar.is_none_or(|titlebar| titlebar.appears_transparent) {
                native_window.setTitlebarAppearsTransparent_(YES);
                native_window.setTitleVisibility_(NSWindowTitleVisibility::Hidden);
            }

            native_view.setAutoresizingMask_(NSViewWidthSizable | NSViewHeightSizable);
            native_view.setWantsBestResolutionOpenGLSurface_(YES);

            // From winit crate: On Mojave, views automatically become layer-backed shortly after
            // being added to a native_window. Changing the layer-backedness of a view breaks the
            // association between the view and its associated OpenGL context. To work around this,
            // on we explicitly make the view layer-backed up front so that AppKit doesn't do it
            // itself and break the association with its context.
            native_view.setWantsLayer(YES);
            let _: () = msg_send![
            native_view,
            setLayerContentsRedrawPolicy: NSViewLayerContentsRedrawDuringViewResize
            ];

            content_view.addSubview_(native_view.autorelease());
            native_window.makeFirstResponder_(native_view);

            match kind {
                // Bottom/Wallpaper are wlr-layer-shell-only kinds with no direct
                // macOS equivalent; each gets the closest native window level instead.
                // Wallpaper -> kCGDesktopWindowLevel, behind desktop icons. Bottom
                // stays at the normal level - macOS has no level strictly between
                // desktop and normal - and relies on avoiding activation (focus:
                // false) plus an explicit orderBack below to stay out of the way.
                // (Top is handled in the Overlay arm below - see its comment.)
                WindowKind::Normal | WindowKind::Floating | WindowKind::Bottom(_)
                | WindowKind::Wallpaper(_) => {
                    let level = match kind {
                        WindowKind::Wallpaper(_) => NSDesktopWindowLevel,
                        _ => NSNormalWindowLevel,
                    };
                    native_window.setLevel_(level);
                    native_window.setAcceptsMouseMovedEvents_(YES);

                    if let Some(tabbing_identifier) = tabbing_identifier {
                        let tabbing_id = ns_string(tabbing_identifier.as_str());
                        let _: () = msg_send![native_window, setTabbingIdentifier: tabbing_id];
                    } else {
                        let _: () = msg_send![native_window, setTabbingIdentifier:nil];
                    }
                }
                WindowKind::PopUp => {
                    // Use a tracking area to allow receiving MouseMoved events even when
                    // the window or application aren't active, which is often the case
                    // e.g. for notification windows.
                    let tracking_area: id = msg_send![lookup_class(c"NSTrackingArea"), alloc];
                    let _: () = msg_send![
                        tracking_area,
                        initWithRect: NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
                        options: NSTrackingMouseEnteredAndExited | NSTrackingMouseMoved | NSTrackingActiveAlways | NSTrackingInVisibleRect,
                        owner: native_view,
                        userInfo: nil
                    ];
                    let _: () =
                        msg_send![native_view, addTrackingArea: tracking_area.autorelease()];

                    native_window.setLevel_(NSPopUpWindowLevel);
                    let _: () = msg_send![
                        native_window,
                        setAnimationBehavior: NSWindowAnimationBehaviorUtilityWindow
                    ];
                    native_window.setCollectionBehavior_(
                        NSWindowCollectionBehavior::CanJoinAllSpaces
                            | NSWindowCollectionBehavior::FullScreenAuxiliary,
                    );
                }
                // Top has no real macOS equivalent: unlike X11's struts or Windows'
                // AppBars, there's no unentitled API to reserve screen space, so
                // rather than invent a third elevated level that's equally unable to
                // push other windows out of the way, it just gets Overlay's treatment
                // directly.
                WindowKind::Overlay(_) | WindowKind::Top(_) => {
                    let tracking_area: id = msg_send![lookup_class(c"NSTrackingArea"), alloc];
                    let _: () = msg_send![
                        tracking_area,
                        initWithRect: NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
                        options: NSTrackingMouseEnteredAndExited | NSTrackingMouseMoved | NSTrackingActiveAlways | NSTrackingInVisibleRect,
                        owner: native_view,
                        userInfo: nil
                    ];
                    let _: () =
                        msg_send![native_view, addTrackingArea: tracking_area.autorelease()];

                    let _: () = msg_send![native_window, setLevel: 25_isize];
                    let _: () = msg_send![
                        native_window,
                        setAnimationBehavior: NSWindowAnimationBehaviorUtilityWindow
                    ];
                    let behavior = NSWindowCollectionBehavior::CanJoinAllSpaces
                        | NSWindowCollectionBehavior::Stationary
                        | NSWindowCollectionBehavior::FullScreenAuxiliary;
                    let _: () = msg_send![native_window, setCollectionBehavior: behavior];
                }
            }

            let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
            let main_window: id = msg_send![app, mainWindow];
            if allows_automatic_window_tabbing
                && !main_window.is_null()
                && main_window != native_window
            {
                let main_window_style_mask: NSWindowStyleMask = msg_send![main_window, styleMask];
                let main_window_is_fullscreen =
                    main_window_style_mask.contains(NSWindowStyleMask::FullScreen);
                let user_tabbing_preference = Self::get_user_tabbing_preference()
                    .unwrap_or(UserTabbingPreference::InFullScreen);
                let should_add_as_tab = user_tabbing_preference == UserTabbingPreference::Always
                    || user_tabbing_preference == UserTabbingPreference::InFullScreen
                        && main_window_is_fullscreen;

                if should_add_as_tab {
                    let main_window_can_tab: BOOL =
                        msg_send![main_window, respondsToSelector: sel!(addTabbedWindow:ordered:)];
                    let main_window_visible: BOOL = msg_send![main_window, isVisible];

                    if main_window_can_tab == YES && main_window_visible == YES {
                        let _: () = msg_send![main_window, addTabbedWindow: native_window, ordered: NSWindowOrderingMode::Above];

                        // Ensure the window is visible immediately after adding the tab, since the tab bar is updated with a new entry at this point.
                        // Note: Calling orderFront here can break fullscreen mode (makes fullscreen windows exit fullscreen), so only do this if the main window is not fullscreen.
                        if !main_window_is_fullscreen {
                            let _: () = msg_send![native_window, orderFront: nil];
                        }
                    }
                }
            }

            if mouse_passthrough {
                let _: () = msg_send![native_window, setIgnoresMouseEvents: YES];
            }

            if focus && show {
                let _: () = msg_send![native_window, makeKeyAndOrderFront: nil];
            } else if show {
                let _: () = msg_send![native_window, orderFront: nil];
            }

            if show && matches!(kind, WindowKind::Bottom(_)) {
                // No macOS level sits strictly between desktop and normal, so a
                // Bottom window shares NSNormalWindowLevel with ordinary windows;
                // ordering it to the back of that level is the closest approximation
                // to staying beneath them.
                let _: () = msg_send![native_window, orderBack: nil];
            }

            // Set the initial position of the window to the specified origin.
            // Although we already specified the position using `initWithContentRect_styleMask_backing_defer_screen_`,
            // the window position might be incorrect if the main screen (the screen that contains the window that has focus)
            //  is different from the primary screen.
            let _: () = msg_send![native_window, setFrameTopLeftPoint: window_rect.origin];
            window.0.lock().move_traffic_light();

            Ok(window)
        }
    }

    pub fn active_window() -> Option<AnyWindowHandle> {
        unsafe {
            let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
            let main_window: id = msg_send![app, mainWindow];
            if main_window.is_null() {
                return None;
            }

            if msg_send![main_window, isKindOfClass: WINDOW_CLASS] {
                let handle = get_window_state(main_window).lock().handle;
                Some(handle)
            } else {
                None
            }
        }
    }

    pub fn ordered_windows() -> Vec<AnyWindowHandle> {
        unsafe {
            let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
            let windows: id = msg_send![app, orderedWindows];
            let count: NSUInteger = msg_send![windows, count];

            let mut window_handles = Vec::new();
            for i in 0..count {
                let window: id = msg_send![windows, objectAtIndex:i];
                if msg_send![window, isKindOfClass: WINDOW_CLASS] {
                    let handle = get_window_state(window).lock().handle;
                    window_handles.push(handle);
                }
            }

            window_handles
        }
    }

    pub fn get_user_tabbing_preference() -> Option<UserTabbingPreference> {
        unsafe {
            let defaults: id = msg_send![lookup_class(c"NSUserDefaults"), standardUserDefaults];
            let domain = ns_string("NSGlobalDomain");
            let key = ns_string("AppleWindowTabbingMode");

            let dict: id = msg_send![defaults, persistentDomainForName: domain];
            let value: id = if !dict.is_null() {
                msg_send![dict, objectForKey: key]
            } else {
                nil
            };

            let value_str = if !value.is_null() {
                let value = &*(value as *const NSString);
                value.to_string()
            } else {
                String::new()
            };

            match value_str.as_str() {
                "manual" => Some(UserTabbingPreference::Never),
                "always" => Some(UserTabbingPreference::Always),
                _ => Some(UserTabbingPreference::InFullScreen),
            }
        }
    }
}

impl Drop for MacWindow {
    fn drop(&mut self) {
        let mut this = self.0.lock();
        if let Err(error) = this.release_pointer_lock() {
            log::error!("failed to release macOS pointer lock while dropping window: {error}");
        }
        this.renderer.destroy();
        let window = this.native_window;
        this.display_link.take();
        unsafe {
            let _: () = msg_send![this.native_window, setDelegate: nil];
        }
        this.input_handler.take();
        this.executor
            .spawn(async move {
                unsafe {
                    let _: () = msg_send![window, close];
                    let _: id = msg_send![window, autorelease];
                }
            })
            .detach();
    }
}

fn should_start_display_link(active: bool, was_active: bool, has_display_link: bool) -> bool {
    active && (!was_active || !has_display_link)
}

impl PlatformWindow for MacWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.as_ref().lock().bounds()
    }

    fn window_bounds(&self) -> WindowBounds {
        self.0.as_ref().lock().window_bounds()
    }

    fn is_maximized(&self) -> bool {
        self.0.as_ref().lock().is_maximized()
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.as_ref().lock().content_size()
    }

    fn resize(&mut self, size: Size<Pixels>) {
        let this = self.0.lock();
        let window = this.native_window;
        this.executor
            .spawn(async move {
                unsafe {
                    window.setContentSize_(NSSize {
                        width: size.width.0 as f64,
                        height: size.height.0 as f64,
                    });
                }
            })
            .detach();
    }

    fn merge_all_windows(&self) {
        let native_window = self.0.lock().native_window;
        unsafe extern "C" fn merge_windows_async(context: *mut std::ffi::c_void) {
            let native_window = context as id;
            let _: () = msg_send![native_window, mergeAllWindows:nil];
        }

        unsafe {
            dispatch_async_f(
                dispatch_get_main_queue(),
                native_window as *mut std::ffi::c_void,
                Some(merge_windows_async),
            );
        }
    }

    fn move_tab_to_new_window(&self) {
        let native_window = self.0.lock().native_window;
        unsafe extern "C" fn move_tab_async(context: *mut std::ffi::c_void) {
            let native_window = context as id;
            let _: () = msg_send![native_window, moveTabToNewWindow:nil];
            let _: () = msg_send![native_window, makeKeyAndOrderFront: nil];
        }

        unsafe {
            dispatch_async_f(
                dispatch_get_main_queue(),
                native_window as *mut std::ffi::c_void,
                Some(move_tab_async),
            );
        }
    }

    fn toggle_window_tab_overview(&self) {
        let native_window = self.0.lock().native_window;
        unsafe {
            let _: () = msg_send![native_window, toggleTabOverview:nil];
        }
    }

    fn set_tabbing_identifier(&self, tabbing_identifier: Option<String>) {
        let native_window = self.0.lock().native_window;
        unsafe {
            let allows_automatic_window_tabbing = tabbing_identifier.is_some();
            if allows_automatic_window_tabbing {
                let () = msg_send![lookup_class(c"NSWindow"), setAllowsAutomaticWindowTabbing: YES];
            } else {
                let () = msg_send![lookup_class(c"NSWindow"), setAllowsAutomaticWindowTabbing: NO];
            }

            if let Some(tabbing_identifier) = tabbing_identifier {
                let tabbing_id = ns_string(tabbing_identifier.as_str());
                let _: () = msg_send![native_window, setTabbingIdentifier: tabbing_id];
            } else {
                let _: () = msg_send![native_window, setTabbingIdentifier:nil];
            }
        }
    }

    fn scale_factor(&self) -> f32 {
        self.0.as_ref().lock().scale_factor()
    }

    fn appearance(&self) -> WindowAppearance {
        unsafe {
            let appearance: id = msg_send![self.0.lock().native_window, effectiveAppearance];
            WindowAppearance::from_native(appearance)
        }
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        unsafe {
            let screen: id = msg_send![self.0.lock().native_window, screen];
            if screen.is_null() {
                return None;
            }
            let device_description: id = msg_send![screen, deviceDescription];
            let screen_number_key = ns_string("NSScreenNumber");
            let screen_number: id = msg_send![device_description, valueForKey: screen_number_key];

            let screen_number: u32 = msg_send![screen_number, unsignedIntValue];

            Some(Rc::new(MacDisplay(screen_number)))
        }
    }

    fn display_refresh_rate(&self) -> Option<f32> {
        self.display().and_then(|display| display.refresh_rate())
    }

    fn mouse_position(&self) -> Point<Pixels> {
        let position = unsafe {
            self.0
                .lock()
                .native_window
                .mouseLocationOutsideOfEventStream()
        };
        convert_mouse_position(position, self.content_size().height)
    }

    fn modifiers(&self) -> Modifiers {
        unsafe {
            let modifiers: NSEventModifierFlags =
                msg_send![lookup_class(c"NSEvent"), modifierFlags];

            let control = modifiers.contains(NSEventModifierFlags::Control);
            let alt = modifiers.contains(NSEventModifierFlags::Option);
            let shift = modifiers.contains(NSEventModifierFlags::Shift);
            let command = modifiers.contains(NSEventModifierFlags::Command);
            let function = modifiers.contains(NSEventModifierFlags::Function);

            Modifiers {
                control,
                alt,
                shift,
                platform: command,
                function,
            }
        }
    }

    fn capslock(&self) -> Capslock {
        unsafe {
            let modifiers: NSEventModifierFlags =
                msg_send![lookup_class(c"NSEvent"), modifierFlags];

            Capslock {
                on: modifiers.contains(NSEventModifierFlags::CapsLock),
            }
        }
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.as_ref().lock().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.as_ref().lock().input_handler.take()
    }

    fn prompt(
        &self,
        level: PromptLevel,
        msg: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
    ) -> Option<oneshot::Receiver<usize>> {
        // macOs applies overrides to modal window buttons after they are added.
        // Two most important for this logic are:
        // * Buttons with "Cancel" title will be displayed as the last buttons in the modal
        // * Last button added to the modal via `addButtonWithTitle` stays focused
        // * Focused buttons react on "space"/" " keypresses
        // * Usage of `keyEquivalent`, `makeFirstResponder` or `setInitialFirstResponder` does not change the focus
        //
        // See also https://developer.apple.com/documentation/appkit/nsalert/1524532-addbuttonwithtitle#discussion
        // ```
        // By default, the first button has a key equivalent of Return,
        // any button with a title of “Cancel” has a key equivalent of Escape,
        // and any button with the title “Don’t Save” has a key equivalent of Command-D (but only if it’s not the first button).
        // ```
        //
        // To avoid situations when the last element added is "Cancel" and it gets the focus
        // (hence stealing both ESC and Space shortcuts), we find and add one non-Cancel button
        // last, so it gets focus and a Space shortcut.
        // This way, "Save this file? Yes/No/Cancel"-ish modals will get all three buttons mapped with a key.
        let latest_non_cancel_label = answers
            .iter()
            .enumerate()
            .rev()
            .find(|(_, label)| !label.is_cancel())
            .filter(|&(label_index, _)| label_index > 0);

        unsafe {
            let alert: id = msg_send![lookup_class(c"NSAlert"), alloc];
            let alert: id = msg_send![alert, init];
            let alert_style = match level {
                PromptLevel::Info => 1,
                PromptLevel::Warning => 0,
                PromptLevel::Critical => 2,
            };
            let _: () = msg_send![alert, setAlertStyle: alert_style];
            let _: () = msg_send![alert, setMessageText: ns_string(msg)];
            if let Some(detail) = detail {
                let _: () = msg_send![alert, setInformativeText: ns_string(detail)];
            }

            let mut button_order = Vec::with_capacity(answers.len());
            for (ix, answer) in answers
                .iter()
                .enumerate()
                .filter(|&(ix, _)| Some(ix) != latest_non_cancel_label.map(|(ix, _)| ix))
            {
                let button: id = msg_send![alert, addButtonWithTitle: ns_string(answer.label())];
                let _: () = msg_send![button, setTag: ix as NSInteger];
                button_order.push(ix);

                if answer.is_cancel() {
                    // Bind Escape Key to Cancel Button
                    if let Some(key) = std::char::from_u32(super::events::ESCAPE_KEY as u32) {
                        let _: () =
                            msg_send![button, setKeyEquivalent: ns_string(&key.to_string())];
                    }
                }
            }
            if let Some((ix, answer)) = latest_non_cancel_label {
                let button: id = msg_send![alert, addButtonWithTitle: ns_string(answer.label())];
                let _: () = msg_send![button, setTag: ix as NSInteger];
                button_order.push(ix);
            }

            let fallback_answer = answers
                .iter()
                .position(PromptButton::is_cancel)
                .unwrap_or(0);

            let (done_tx, done_rx) = oneshot::channel();
            let done_tx = Cell::new(Some(done_tx));
            let block = RcBlock::new(move |answer: NSInteger| {
                if let Some(done_tx) = done_tx.take() {
                    let answer = alert_response_index(answer, &button_order).unwrap_or_else(|| {
                        log::error!("macOS returned invalid alert response {answer}");
                        fallback_answer
                    });
                    let _ = done_tx.send(answer);
                }
            });
            let native_window = self.0.lock().native_window;
            let executor = self.0.lock().executor.clone();
            executor
                .spawn(async move {
                    let _: () = msg_send![
                        alert,
                        beginSheetModalForWindow: native_window,
                        completionHandler: &*block
                    ];
                })
                .detach();

            Some(done_rx)
        }
    }

    fn activate(&self) {
        let window = self.0.lock().native_window;
        let executor = self.0.lock().executor.clone();
        executor
            .spawn(async move {
                unsafe {
                    let _: () = msg_send![window, makeKeyAndOrderFront: nil];
                }
            })
            .detach();
    }

    fn is_active(&self) -> bool {
        unsafe { self.0.lock().native_window.isKeyWindow() == YES }
    }

    // is_hovered is unused on macOS. See Window::is_window_hovered.
    fn is_hovered(&self) -> bool {
        false
    }

    fn set_title(&mut self, title: &str) {
        unsafe {
            let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
            let window = self.0.lock().native_window;
            let title = ns_string(title);
            let _: () = msg_send![app, changeWindowsItem: window, title: title, filename: false];
            let _: () = msg_send![window, setTitle: title];
            self.0.lock().move_traffic_light();
        }
    }

    fn get_title(&self) -> String {
        unsafe {
            let title: id = msg_send![self.0.lock().native_window, title];
            if title.is_null() {
                "".to_string()
            } else {
                title.to_str().to_string()
            }
        }
    }

    fn set_app_id(&mut self, _app_id: &str) {}

    fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        let mut this = self.0.as_ref().lock();

        let opaque = background_appearance == WindowBackgroundAppearance::Opaque;
        this.renderer.update_transparency(!opaque);

        unsafe {
            this.native_window.setOpaque_(Bool::new(opaque));
            let background_color = if opaque {
                msg_send![lookup_class(c"NSColor"), colorWithSRGBRed: 0f64, green: 0f64, blue: 0f64, alpha: 1f64]
            } else {
                // Not using `+[NSColor clearColor]` to avoid broken shadow.
                msg_send![lookup_class(c"NSColor"), colorWithSRGBRed: 0f64, green: 0f64, blue: 0f64, alpha: 0.0001f64]
            };
            this.native_window.setBackgroundColor_(background_color);

            if NSAppKitVersionNumber < NSAppKitVersionNumber12_0 {
                // Whether `-[NSVisualEffectView respondsToSelector:@selector(_updateProxyLayer)]`.
                // On macOS Catalina/Big Sur `NSVisualEffectView` doesn’t own concrete sublayers
                // but uses a `CAProxyLayer`. Use the legacy WindowServer API.
                let blur_radius = if background_appearance == WindowBackgroundAppearance::Blurred {
                    80
                } else {
                    0
                };

                let window_number = this.native_window.windowNumber();
                CGSSetWindowBackgroundBlurRadius(CGSMainConnectionID(), window_number, blur_radius);
            } else {
                // On newer macOS `NSVisualEffectView` manages the effect layer directly. Using it
                // could have a better performance (it downsamples the backdrop) and more control
                // over the effect layer.
                if background_appearance != WindowBackgroundAppearance::Blurred {
                    if let Some(blur_view) = this.blurred_view {
                        blur_view.removeFromSuperview();
                        this.blurred_view = None;
                    }
                } else if this.blurred_view.is_none() {
                    let content_view = this.native_window.contentView();
                    let frame = content_view.bounds();
                    let mut blur_view: id = msg_send![BLURRED_VIEW_CLASS, alloc];
                    blur_view = blur_view.initWithFrame_(frame);
                    blur_view.setAutoresizingMask_(NSViewWidthSizable | NSViewHeightSizable);

                    let _: () = msg_send![
                        content_view,
                        addSubview: blur_view,
                        positioned: NSWindowOrderingMode::Below,
                        relativeTo: nil
                    ];
                    this.blurred_view = Some(blur_view.autorelease());
                }
            }
        }
    }

    fn set_opacity(&self, opacity: f32) {
        unsafe {
            let window = self.0.lock().native_window;
            let _: () = msg_send![window, setAlphaValue: opacity as f64];
        }
    }

    fn set_always_on_top(&self, always_on_top: bool) {
        let level = if always_on_top {
            NSPopUpWindowLevel
        } else {
            NSNormalWindowLevel
        };
        unsafe {
            self.0.lock().native_window.setLevel_(level);
        }
    }

    fn set_frame_polling(&self, active: bool) {
        let mut this = self.0.as_ref().lock();
        let was_active = this.frame_polling_active;
        this.frame_polling_active = active;
        // Polling can first be enabled while AppKit still reports the window as
        // occluded, in which case `start_display_link` deliberately leaves no
        // link behind. A transient CoreVideo start failure has the same state.
        // Treat a later active refresh as a retry instead of requiring an
        // unrelated false -> true transition to make the window render again.
        if should_start_display_link(active, was_active, this.display_link.is_some()) {
            this.start_display_link();
        } else if !active && was_active {
            this.stop_display_link();
        }
    }

    fn set_edited(&mut self, edited: bool) {
        unsafe {
            let window = self.0.lock().native_window;
            msg_send![window, setDocumentEdited: Bool::new(edited)]
        }

        // Changing the document edited state resets the traffic light position,
        // so we have to move it again.
        self.0.lock().move_traffic_light();
    }

    fn show_character_palette(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        this.executor
            .spawn(async move {
                unsafe {
                    let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
                    let _: () = msg_send![app, orderFrontCharacterPalette: window];
                }
            })
            .detach();
    }

    fn close(&self) {
        let window = self.0.lock().native_window;
        unsafe {
            let _: () = msg_send![window, performClose: nil];
        }
    }

    fn minimize(&self) {
        let window = self.0.lock().native_window;
        unsafe {
            let _: () = msg_send![window, miniaturize: nil];
        }
    }

    fn zoom(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        this.executor
            .spawn(async move {
                zoom_window_immediately(window);
            })
            .detach();
    }

    fn toggle_fullscreen(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        this.executor
            .spawn(async move {
                unsafe {
                    window.toggleFullScreen_(nil);
                }
            })
            .detach();
    }

    fn is_fullscreen(&self) -> bool {
        let this = self.0.lock();
        let window = this.native_window;

        unsafe { window.styleMask().contains(NSWindowStyleMask::FullScreen) }
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.as_ref().lock().request_frame_callback = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> crate::DispatchEventResult>) {
        self.0.as_ref().lock().event_callback = Some(callback);
    }

    fn game_input_capabilities(&self) -> GameInputCapabilities {
        let gamepads = if cfg!(feature = "game-input") {
            GameInputAvailability::Available
        } else {
            GameInputAvailability::DisabledAtCompileTime
        };
        GameInputCapabilities::new(GameInputAvailability::Available, gamepads)
    }

    fn pointer_lock_status(&self) -> PointerLockStatus {
        self.0.lock().pointer_lock.status()
    }

    fn request_pointer_lock(&self) -> Result<(), GameInputError> {
        self.0.lock().request_pointer_lock()
    }

    fn exit_pointer_lock(&self) -> Result<(), GameInputError> {
        self.0.lock().release_pointer_lock()
    }

    fn pointer_lock_error(&self) -> Option<GameInputError> {
        self.0.lock().pointer_lock.error()
    }

    fn on_active_status_change(&self, callback: Box<dyn FnMut(bool)>) {
        self.0.as_ref().lock().activate_callback = Some(callback);
    }

    fn on_hover_status_change(&self, _: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.as_ref().lock().resize_callback = Some(callback);
    }

    fn on_moved(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().moved_callback = Some(callback);
    }

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.as_ref().lock().should_close_callback = Some(callback);
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.as_ref().lock().close_callback = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_appearance_changed(&self, callback: Box<dyn FnMut()>) {
        self.0.lock().appearance_changed_callback = Some(callback);
    }

    fn tabbed_windows(&self) -> Option<Vec<SystemWindowTab>> {
        unsafe {
            let windows: id = msg_send![self.0.lock().native_window, tabbedWindows];
            if windows.is_null() {
                return None;
            }

            let count: NSUInteger = msg_send![windows, count];
            let mut result = Vec::new();
            for i in 0..count {
                let window: id = msg_send![windows, objectAtIndex:i];
                if msg_send![window, isKindOfClass: WINDOW_CLASS] {
                    let handle = get_window_state(window).lock().handle;
                    let title: id = msg_send![window, title];
                    let title = SharedString::from(title.to_str().to_string());

                    result.push(SystemWindowTab::new(title, handle));
                }
            }

            Some(result)
        }
    }

    fn tab_bar_visible(&self) -> bool {
        unsafe {
            let tab_group: id = msg_send![self.0.lock().native_window, tabGroup];
            if tab_group.is_null() {
                false
            } else {
                let tab_bar_visible: BOOL = msg_send![tab_group, isTabBarVisible];
                tab_bar_visible == YES
            }
        }
    }

    fn on_move_tab_to_new_window(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().move_tab_to_new_window_callback = Some(callback);
    }

    fn on_merge_all_windows(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().merge_all_windows_callback = Some(callback);
    }

    fn on_select_next_tab(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().select_next_tab_callback = Some(callback);
    }

    fn on_select_previous_tab(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().select_previous_tab_callback = Some(callback);
    }

    fn on_toggle_tab_bar(&self, callback: Box<dyn FnMut()>) {
        self.0.as_ref().lock().toggle_tab_bar_callback = Some(callback);
    }

    fn sync_webviews(&mut self, webviews: &[PlatformWebView]) {
        if cfg!(feature = "webview") {
            self.0.lock().sync_webviews(webviews);
        } else if !webviews.is_empty() {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                log::warn!(
                    "WebView rendering is disabled; enable the `webview` feature for this target"
                );
            });
        }
    }

    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        if cfg!(feature = "webview") {
            self.0.lock().dispatch_webview_command(command)
        } else {
            Err(anyhow::anyhow!(
                "WebView support is disabled; enable the `webview` feature for this target"
            ))
        }
    }

    fn print(&mut self, job: PlatformPrintJob) -> anyhow::Result<()> {
        let native_window = self.0.lock().native_window;
        unsafe { run_print_job(native_window, job, false) }
    }

    fn show_print_dialog(&mut self, job: PlatformPrintJob) -> anyhow::Result<()> {
        let native_window = self.0.lock().native_window;
        unsafe { run_print_job(native_window, job, true) }
    }

    #[cfg(not(feature = "macos-blade"))]
    fn export_scene_png(
        &self,
        scene: &crate::Scene,
    ) -> std::result::Result<crate::Image, crate::WindowCaptureError> {
        let readback = {
            let mut this = self.0.lock();
            let content_size = this.content_size();
            let scale_factor = this.scale_factor();
            let device_dimension = |logical: Pixels| {
                let scaled = f64::from(logical.0) * f64::from(scale_factor);
                if !scaled.is_finite() || scaled < 1.0 || scaled > f64::from(i32::MAX) {
                    return Err(crate::WindowCaptureError::Backend(
                        "window capture dimensions are invalid".into(),
                    ));
                }
                Ok(crate::DevicePixels(scaled.round() as i32))
            };
            let viewport = size(
                device_dimension(content_size.width)?,
                device_dimension(content_size.height)?,
            );
            this.renderer
                .render_scene_to_bytes(scene, viewport)
                .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))?
        };
        encode_premultiplied_bgra_png(readback.width, readback.height, readback.bgra)
            .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))
    }

    #[cfg(feature = "macos-blade")]
    fn export_scene_png(
        &self,
        scene: &crate::Scene,
    ) -> std::result::Result<crate::Image, crate::WindowCaptureError> {
        let readback = self
            .0
            .lock()
            .renderer
            .render_scene_to_bgra(scene)
            .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))?;
        encode_bgra_png(
            readback.width,
            readback.height,
            readback.bgra,
            readback.premultiplied_alpha,
        )
        .map_err(|error| crate::WindowCaptureError::Backend(error.to_string()))
    }

    fn draw(&self, scene: &crate::Scene) {
        let mut this = self.0.lock();
        this.renderer.draw(scene);
    }

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.lock().renderer.sprite_atlas().clone()
    }

    fn gpu_specs(&self) -> Option<crate::GpuSpecs> {
        None
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {
        let executor = self.0.lock().executor.clone();
        executor
            .spawn(async move {
                unsafe {
                    let input_context: id =
                        msg_send![lookup_class(c"NSTextInputContext"), currentInputContext];
                    if input_context.is_null() {
                        return;
                    }
                    let _: () = msg_send![input_context, invalidateCharacterCoordinates];
                }
            })
            .detach()
    }

    fn show(&self) {
        unsafe {
            let _: () = msg_send![self.0.lock().native_window, makeKeyAndOrderFront: nil];
        }
    }

    fn hide(&self) {
        unsafe {
            let _: () = msg_send![self.0.lock().native_window, orderOut: nil];
        }
    }

    fn is_visible(&self) -> bool {
        unsafe { msg_send![self.0.lock().native_window, isVisible] }
    }

    fn set_mouse_passthrough(&self, passthrough: bool) {
        unsafe {
            let _: () = msg_send![self.0.lock().native_window, setIgnoresMouseEvents: Bool::new(passthrough)];
        }
    }

    fn set_atlas_byte_budget(&self, budget: Option<u64>) {
        self.0.lock().renderer.set_atlas_byte_budget(budget);
    }

    fn titlebar_double_click(&self) {
        let this = self.0.lock();
        let window = this.native_window;
        this.executor
            .spawn(async move {
                perform_titlebar_double_click_action(window);
            })
            .detach();
    }

    fn set_progress_bar(&self, state: crate::ProgressBarState) {
        unsafe {
            let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
            let dock_tile: id = msg_send![app, dockTile];
            if dock_tile == nil {
                return;
            }

            match state {
                crate::ProgressBarState::None => {
                    let _: () = msg_send![dock_tile, setContentView: nil];
                    let _: () = msg_send![dock_tile, setBadgeLabel: nil];
                    let _: () = msg_send![dock_tile, display];
                }
                crate::ProgressBarState::Indeterminate => {
                    let indicator: id = msg_send![lookup_class(c"NSProgressIndicator"), alloc];
                    let frame = NSRect {
                        origin: NSPoint::new(0.0, 0.0),
                        size: NSSize::new(140.0, 140.0),
                    };
                    let indicator: id = msg_send![indicator, initWithFrame: frame];
                    let _: () = msg_send![indicator, setStyle: 0i64]; // NSProgressIndicatorBarStyle
                    let _: () = msg_send![indicator, setIndeterminate: YES];
                    let _: () = msg_send![indicator, startAnimation: nil];
                    let _: () = msg_send![dock_tile, setContentView: indicator];
                    let _: () = msg_send![indicator, release];
                    let _: () = msg_send![dock_tile, display];
                }
                crate::ProgressBarState::Normal(pct)
                | crate::ProgressBarState::Error(pct)
                | crate::ProgressBarState::Paused(pct) => {
                    let indicator: id = msg_send![lookup_class(c"NSProgressIndicator"), alloc];
                    let frame = NSRect {
                        origin: NSPoint::new(0.0, 0.0),
                        size: NSSize::new(140.0, 140.0),
                    };
                    let indicator: id = msg_send![indicator, initWithFrame: frame];
                    let _: () = msg_send![indicator, setStyle: 0i64]; // NSProgressIndicatorBarStyle
                    let _: () = msg_send![indicator, setIndeterminate: NO];
                    let _: () = msg_send![indicator, setMinValue: 0.0f64];
                    let _: () = msg_send![indicator, setMaxValue: 100.0f64];
                    let _: () = msg_send![indicator, setDoubleValue: pct * 100.0];
                    let _: () = msg_send![dock_tile, setContentView: indicator];
                    let _: () = msg_send![indicator, release];
                    let _: () = msg_send![dock_tile, display];
                }
            }
        }
    }

    fn update_accessibility_tree(
        &mut self,
        tree: &crate::AccessibilityTree,
    ) -> Vec<crate::AccessibilityActionRequest> {
        let mut this = self.0.lock();
        this.accessibility_provider.update_tree(tree);
        this.accessibility_provider.drain_actions(tree)
    }
}

impl rwh::HasWindowHandle for MacWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        // SAFETY: The AppKitWindowHandle is a wrapper around a pointer to an NSView
        unsafe {
            Ok(rwh::WindowHandle::borrow_raw(rwh::RawWindowHandle::AppKit(
                rwh::AppKitWindowHandle::new(self.0.lock().native_view.cast()),
            )))
        }
    }
}

impl rwh::HasDisplayHandle for MacWindow {
    fn display_handle(&self) -> Result<rwh::DisplayHandle<'_>, rwh::HandleError> {
        // SAFETY: This is a no-op on macOS
        unsafe {
            Ok(rwh::DisplayHandle::borrow_raw(
                rwh::AppKitDisplayHandle::new().into(),
            ))
        }
    }
}

fn get_scale_factor(native_window: id) -> f32 {
    let factor = unsafe {
        let screen: id = msg_send![native_window, screen];
        if screen.is_null() {
            return 2.0;
        }
        let scale: f64 = msg_send![screen, backingScaleFactor];
        scale as f32
    };

    // We are not certain what triggers this, but it seems that sometimes
    // this method would return 0 (known macOS edge case with off-screen windows)
    // It seems most likely that this would happen if the window has no screen
    // (if it is off-screen), though we'd expect to see viewDidChangeBackingProperties before
    // it was rendered for real.
    // Regardless, attempt to avoid the issue here.
    if factor == 0.0 { 2. } else { factor }
}

unsafe fn get_window_state(object: id) -> Arc<Mutex<MacWindowState>> {
    unsafe {
        let raw: *mut c_void = load_ivar(object, WINDOW_STATE_IVAR);
        let rc1 = Arc::from_raw(raw as *mut Mutex<MacWindowState>);
        let rc2 = rc1.clone();
        mem::forget(rc1);
        rc2
    }
}

unsafe fn drop_window_state(object: id) {
    unsafe {
        let raw: *mut c_void = load_ivar(object, WINDOW_STATE_IVAR);
        Arc::from_raw(raw as *mut Mutex<MacWindowState>);
    }
}

extern "C" fn yes(_: id, _: Sel) -> BOOL {
    YES
}

extern "C" fn dealloc_window(this: id, _: Sel) {
    unsafe {
        drop_window_state(this);
        let _: () = msg_send![super(this, lookup_class(c"NSWindow")), dealloc];
    }
}

extern "C" fn dealloc_view(this: id, _: Sel) {
    unsafe {
        drop_window_state(this);
        let _: () = msg_send![super(this, lookup_class(c"NSView")), dealloc];
    }
}

extern "C" fn dealloc_webview_delegate(this: id, _: Sel) {
    unsafe {
        let _: () = msg_send![super(this, lookup_class(c"NSObject")), dealloc];
    }
}

extern "C" fn dealloc_print_view(this: id, _: Sel) {
    unsafe {
        let raw: *mut c_void = load_ivar(this, PRINT_VIEW_STATE_IVAR);
        if !raw.is_null() {
            drop(Box::from_raw(raw as *mut MacPrintViewState));
            store_ivar(this, PRINT_VIEW_STATE_IVAR, ptr::null_mut::<c_void>());
        }
        let _: () = msg_send![super(this, lookup_class(c"NSView")), dealloc];
    }
}

extern "C" fn print_view_knows_page_range(this: id, _: Sel, range: NSRangePointer) -> BOOL {
    let Some(state) = (unsafe { get_print_view_state(this) }) else {
        return NO;
    };

    if range.0.is_null() {
        return NO;
    }

    unsafe {
        *range.0 = NSRange {
            location: 1,
            length: state.pages.len() as NSUInteger,
        };
    }
    YES
}

extern "C" fn print_view_rect_for_page(this: id, _: Sel, _: NSInteger) -> NSRect {
    let Some(state) = (unsafe { get_print_view_state(this) }) else {
        return NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.));
    };

    NSRect::new(
        NSPoint::new(0., 0.),
        NSSize::new(
            state.page_size.width.0 as f64,
            state.page_size.height.0 as f64,
        ),
    )
}

extern "C" fn draw_print_view(this: id, _: Sel, _: NSRect) {
    let Some(state) = (unsafe { get_print_view_state(this) }) else {
        return;
    };

    let operation: id = unsafe { msg_send![lookup_class(c"NSPrintOperation"), currentOperation] };
    let current_page: NSInteger = unsafe { msg_send![operation, currentPage] };
    let page_index = current_page.max(1) as usize - 1;
    let Some(page) = state.pages.get(page_index) else {
        return;
    };

    for command in &page.commands {
        unsafe {
            draw_print_command(command, state.margins);
        }
    }
}

fn warn_rejected_mac_webview_ipc_once(state: &MacWebViewDelegateState, reason: &str) {
    if !state.ipc_rejection_reported.swap(true, Ordering::Relaxed) {
        log::warn!("rejected macOS WebView IPC message: {reason}");
    }
}

extern "C" fn webview_did_receive_script_message(this: id, _: Sel, _: id, message: id) {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        return;
    };
    let frame_info: id = unsafe { msg_send![message, frameInfo] };
    let is_main_frame = if frame_info.is_null() {
        false
    } else {
        let value: BOOL = unsafe { msg_send![frame_info, isMainFrame] };
        value.as_bool()
    };
    if !is_main_frame {
        warn_rejected_mac_webview_ipc_once(state, "message originated in a subframe");
        return;
    }

    let body: id = unsafe { msg_send![message, body] };
    let payload = unsafe { webview_message_value(body) };
    if handle_mac_webview_clipboard_message(state, message, &payload) {
        return;
    }

    let Some(payload) = decode_mac_webview_bridge_message(&payload, state.ipc_nonce.as_ref())
    else {
        warn_rejected_mac_webview_ipc_once(state, "missing or invalid authentication nonce");
        return;
    };

    let Some(handler) = state.message_handler.clone() else {
        return;
    };

    let mut async_window = state.async_window.clone();
    catch_platform_callback("webview message", (), || {
        let _ = async_window.update(|window, cx| {
            handler(payload, window, cx);
        });
    });
}

extern "C" fn webview_did_start_provisional_navigation(this: id, _: Sel, webview: id, _: id) {
    emit_webview_page_load(this, webview, WebViewPageLoadEvent::Started);
}

extern "C" fn webview_did_finish_navigation(this: id, _: Sel, webview: id, _: id) {
    emit_webview_page_load(this, webview, WebViewPageLoadEvent::Finished);
}

extern "C" fn webview_decide_policy_for_navigation_response(
    _: id,
    _: Sel,
    _: id,
    navigation_response: id,
    decision_handler: id,
) {
    let can_show_mime_type: BOOL = unsafe { msg_send![navigation_response, canShowMIMEType] };
    unsafe {
        call_navigation_decision_handler(
            decision_handler,
            if can_show_mime_type.as_bool() {
                WKNavigationResponsePolicyAllow
            } else {
                WKNavigationResponsePolicyDownload
            },
        );
    }
}

extern "C" fn webview_navigation_action_did_become_download(
    this: id,
    _: Sel,
    _: id,
    navigation_action: id,
    download: id,
) {
    let request: id = unsafe { msg_send![navigation_action, request] };
    let url = ns_request_url_string(request);
    register_mac_download(this, download, url);
}

extern "C" fn webview_navigation_response_did_become_download(
    this: id,
    _: Sel,
    _: id,
    navigation_response: id,
    download: id,
) {
    let response: id = unsafe { msg_send![navigation_response, response] };
    let url = ns_response_url_string(response);
    register_mac_download(this, download, url);
}

extern "C" fn webview_create_webview_with_configuration(
    this: id,
    _: Sel,
    webview: id,
    _: id,
    navigation_action: id,
    _: id,
) -> id {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        return nil;
    };

    let request: id = unsafe { msg_send![navigation_action, request] };
    let url = ns_request_url_string(request);
    if resolve_mac_new_window_policy(url.as_ref(), state) != WebViewNewWindowPolicy::Deny {
        // A WKWebView returned here may outlive the Kael element that owns the
        // delegate state. Until popup ownership and teardown are explicit,
        // preserve the navigation without creating a second native view.
        unsafe {
            let _: () = msg_send![webview, loadRequest: request];
        }
    }
    nil
}

extern "C" fn webview_did_close(_: id, _: Sel, webview: id) {
    unsafe {
        webview.removeFromSuperview();
    }
}

extern "C" fn webview_key_down(this: id, _: Sel, event: id) {
    let zoom_key = (unsafe { get_webview_delegate_state(this) })
        .filter(|state| state.zoom_hotkeys_enabled)
        .and_then(|_| mac_webview_zoom_key(event));

    if let Some(zoom_key) = zoom_key {
        apply_mac_webview_zoom_key(this, zoom_key);
    } else {
        unsafe {
            let _: () = msg_send![super(this, lookup_class(c"WKWebView")), keyDown: event];
        }
    }
}

extern "C" fn webview_magnify(this: id, _: Sel, event: id) {
    if (unsafe { get_webview_delegate_state(this) }).is_some_and(|state| state.zoom_hotkeys_enabled)
    {
        apply_mac_webview_magnification(this, event);
    } else {
        unsafe {
            let _: () = msg_send![super(this, lookup_class(c"WKWebView")), magnifyWithEvent: event];
        }
    }
}

extern "C" fn webview_dragging_entered(this: id, _: Sel, dragging_info: id) -> NSDragOperation {
    let Some(policy) =
        dispatch_mac_webview_drag_drop(this, dragging_info, MacWebViewDragEvent::Enter)
    else {
        return unsafe {
            msg_send![super(this, lookup_class(c"WKWebView")), draggingEntered: dragging_info]
        };
    };

    if policy == WebViewDragDropPolicy::BlockBrowserDefault {
        NSDragOperationNone
    } else {
        unsafe {
            msg_send![super(this, lookup_class(c"WKWebView")), draggingEntered: dragging_info]
        }
    }
}

extern "C" fn webview_dragging_updated(this: id, _: Sel, dragging_info: id) -> NSDragOperation {
    let Some(policy) =
        dispatch_mac_webview_drag_drop(this, dragging_info, MacWebViewDragEvent::Over)
    else {
        return unsafe {
            msg_send![super(this, lookup_class(c"WKWebView")), draggingUpdated: dragging_info]
        };
    };

    if policy == WebViewDragDropPolicy::BlockBrowserDefault {
        NSDragOperationNone
    } else {
        unsafe {
            msg_send![super(this, lookup_class(c"WKWebView")), draggingUpdated: dragging_info]
        }
    }
}

extern "C" fn webview_dragging_exited(this: id, _: Sel, dragging_info: id) {
    let _ = dispatch_mac_webview_drag_drop(this, dragging_info, MacWebViewDragEvent::Leave);
    unsafe {
        let _: () =
            msg_send![super(this, lookup_class(c"WKWebView")), draggingExited: dragging_info];
    }
}

extern "C" fn webview_dragging_ended(this: id, _: Sel, dragging_info: id) {
    let _ = dispatch_mac_webview_drag_drop(this, dragging_info, MacWebViewDragEvent::Leave);
    unsafe {
        let _: () =
            msg_send![super(this, lookup_class(c"WKWebView")), draggingEnded: dragging_info];
    }
}

extern "C" fn webview_perform_drag_operation(this: id, _: Sel, dragging_info: id) -> BOOL {
    let Some(policy) =
        dispatch_mac_webview_drag_drop(this, dragging_info, MacWebViewDragEvent::Drop)
    else {
        return unsafe {
            msg_send![super(this, lookup_class(c"WKWebView")), performDragOperation: dragging_info]
        };
    };

    if policy == WebViewDragDropPolicy::BlockBrowserDefault {
        YES
    } else {
        unsafe {
            msg_send![super(this, lookup_class(c"WKWebView")), performDragOperation: dragging_info]
        }
    }
}

extern "C" fn webview_download_decide_destination(
    this: id,
    _: Sel,
    download: id,
    response: id,
    suggested_filename: id,
    completion_handler: id,
) {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        unsafe {
            call_download_destination_handler(completion_handler, nil);
        }
        return;
    };

    let download_id = download as usize;
    let response_url = ns_response_url_string(response);
    let suggested_filename = if suggested_filename.is_null() {
        "download.bin"
    } else {
        unsafe { suggested_filename.to_str() }
    };
    let suggested_path = default_download_path(suggested_filename);
    let url = state
        .downloads
        .get(&download_id)
        .map(|download| download.url.clone())
        .filter(|url| !url.is_empty())
        .unwrap_or(response_url);

    let Some(destination) = resolve_mac_download_started(url.clone(), suggested_path, state) else {
        state.downloads.remove(&download_id);
        unsafe {
            call_download_destination_handler(completion_handler, nil);
        }
        return;
    };

    state.downloads.insert(
        download_id,
        MacWebViewDownloadState {
            url,
            path: Some(destination.clone()),
        },
    );

    unsafe {
        let destination_url: id = msg_send![lookup_class(c"NSURL"), fileURLWithPath: ns_string(&destination.to_string_lossy())];
        call_download_destination_handler(completion_handler, destination_url);
    }
}

extern "C" fn webview_download_did_finish(this: id, _: Sel, download: id) {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        return;
    };
    if let Some(download_state) = state.downloads.remove(&(download as usize)) {
        dispatch_mac_download_completed(state, download_state, true);
    }
}

extern "C" fn webview_download_did_fail(this: id, _: Sel, download: id, error: id, _: id) {
    if !error.is_null() {
        log::warn!("WebView download failed: {}", ns_error_message(error));
    }
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        return;
    };
    if let Some(download_state) = state.downloads.remove(&(download as usize)) {
        dispatch_mac_download_completed(state, download_state, false);
    }
}

extern "C" fn webview_download_did_receive_final_url(this: id, _: Sel, download: id, url: id) {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        return;
    };
    if let Some(download_state) = state.downloads.get_mut(&(download as usize)) {
        if let Some(path) = ns_url_path(url) {
            download_state.path = Some(path);
        }
    }
}

extern "C" fn webview_observe_value_for_key_path(
    this: id,
    _: Sel,
    key_path: id,
    object: id,
    _: id,
    _: *mut c_void,
) {
    if key_path.is_null() || unsafe { key_path.to_str() } != "title" {
        return;
    }
    emit_webview_document_title_changed(this, object);
}

fn emit_webview_page_load(delegate: id, webview: id, event: WebViewPageLoadEvent) {
    let Some(state) = (unsafe { get_webview_delegate_state(delegate) }) else {
        return;
    };
    let Some(handler) = state.page_load_handler.clone() else {
        return;
    };

    let url = mac_webview_url(webview);
    let mut async_window = state.async_window.clone();
    catch_platform_callback("webview page load", (), || {
        let _ = async_window.update(|window, cx| {
            handler(event, url, window, cx);
        });
    });
}

fn emit_webview_document_title_changed(delegate: id, webview: id) {
    let Some(state) = (unsafe { get_webview_delegate_state(delegate) }) else {
        return;
    };
    let Some(handler) = state.document_title_changed_handler.clone() else {
        return;
    };

    let title = mac_webview_title(webview);
    let mut async_window = state.async_window.clone();
    catch_platform_callback("webview title change", (), || {
        let _ = async_window.update(|window, cx| {
            handler(title, window, cx);
        });
    });
}

fn safe_mac_new_window_policy(policy: WebViewNewWindowPolicy) -> WebViewNewWindowPolicy {
    if policy == WebViewNewWindowPolicy::Allow {
        WebViewNewWindowPolicy::NavigateCurrent
    } else {
        policy
    }
}

fn resolve_mac_new_window_policy(
    url: &str,
    state: &MacWebViewDelegateState,
) -> WebViewNewWindowPolicy {
    if let Some(handler) = state.new_window_handler.clone() {
        let mut async_window = state.async_window.clone();
        let policy = catch_platform_callback(
            "webview new-window policy",
            WebViewNewWindowPolicy::Deny,
            || {
                async_window
                    .update(|window, cx| handler(url.to_string().into(), window, cx))
                    .unwrap_or(WebViewNewWindowPolicy::Deny)
            },
        );
        let safe_policy = safe_mac_new_window_policy(policy);
        if policy != safe_policy && !state.popup_downgrade_reported.swap(true, Ordering::Relaxed) {
            log::warn!(
                "macOS WebView popup Allow is downgraded to NavigateCurrent until popup lifetime ownership is safe"
            );
        }
        return safe_policy;
    }

    if let Some(handler) = state.navigation_handler.clone() {
        let mut async_window = state.async_window.clone();
        return if catch_platform_callback(
            "webview navigation policy",
            NavigationPolicy::Deny,
            || {
                async_window
                    .update(|window, cx| handler(url.to_string().into(), window, cx))
                    .unwrap_or(NavigationPolicy::Deny)
            },
        ) == NavigationPolicy::Allow
        {
            WebViewNewWindowPolicy::NavigateCurrent
        } else {
            WebViewNewWindowPolicy::Deny
        };
    }

    WebViewNewWindowPolicy::NavigateCurrent
}

extern "C" fn webview_decide_policy_for_navigation_action(
    this: id,
    _: Sel,
    webview: id,
    navigation_action: id,
    decision_handler: id,
) {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        unsafe {
            call_navigation_decision_handler(decision_handler, WKNavigationActionPolicyAllow);
        }
        return;
    };

    let request: id = unsafe { msg_send![navigation_action, request] };
    let url_string = ns_request_url_string(request);
    let should_perform_download: BOOL =
        unsafe { msg_send![navigation_action, shouldPerformDownload] };

    let default_policy = NavigationPolicy::Allow;
    let policy = if let Some(handler) = state.navigation_handler.clone() {
        let mut async_window = state.async_window.clone();
        catch_platform_callback("webview navigation policy", NavigationPolicy::Deny, || {
            async_window
                .update(|window, cx| handler(url_string.clone(), window, cx))
                .unwrap_or(default_policy)
        })
    } else {
        default_policy
    };

    if should_perform_download.as_bool() {
        unsafe {
            call_navigation_decision_handler(
                decision_handler,
                if policy == NavigationPolicy::Allow {
                    WKNavigationActionPolicyDownload
                } else {
                    WKNavigationActionPolicyCancel
                },
            );
        }
        return;
    }

    let target_frame: id = unsafe { msg_send![navigation_action, targetFrame] };

    if target_frame.is_null() {
        let policy = resolve_mac_new_window_policy(url_string.as_ref(), state);
        if policy == WebViewNewWindowPolicy::NavigateCurrent {
            unsafe {
                let _: () = msg_send![webview, loadRequest: request];
            }
        }
        unsafe {
            call_navigation_decision_handler(decision_handler, WKNavigationActionPolicyCancel);
        }
        return;
    }

    unsafe {
        call_navigation_decision_handler(
            decision_handler,
            if policy == NavigationPolicy::Allow {
                WKNavigationActionPolicyAllow
            } else {
                WKNavigationActionPolicyCancel
            },
        );
    }
}

extern "C" fn handle_key_equivalent(this: id, _: Sel, native_event: id) -> BOOL {
    handle_key_event(this, native_event, true)
}

extern "C" fn handle_key_down(this: id, _: Sel, native_event: id) {
    handle_key_event(this, native_event, false);
}

extern "C" fn handle_key_up(this: id, _: Sel, native_event: id) {
    handle_key_event(this, native_event, false);
}

// Things to test if you're modifying this method:
//  U.S. layout:
//   - The IME consumes characters like 'j' and 'k', which makes paging through `less` in
//     the terminal behave incorrectly by default. This behavior should be patched by our
//     IME integration
//   - `alt-t` should open the tasks menu
//   - In vim mode, this keybinding should work:
//     ```
//        {
//          "context": "Editor && vim_mode == insert",
//          "bindings": {"j j": "vim::NormalBefore"}
//        }
//     ```
//     and typing 'j k' in insert mode with this keybinding should insert the two characters
//  Brazilian layout:
//   - `" space` should create an unmarked quote
//   - `" backspace` should delete the marked quote
//   - `" "`should create an unmarked quote and a second marked quote
//   - `" up` should insert a quote, unmark it, and move up one line
//   - `" cmd-down` should insert a quote, unmark it, and move to the end of the file
//   - `cmd-ctrl-space` and clicking on an emoji should type it
//  Czech (QWERTY) layout:
//   - in vim mode `option-4`  should go to end of line (same as $)
//  Japanese (Romaji) layout:
//   - type `a i left down up enter enter` should create an unmarked text "愛"
extern "C" fn handle_key_event(this: id, native_event: id, key_equivalent: bool) -> BOOL {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();

    let window_height = lock.content_size().height;
    let event = unsafe { PlatformInput::from_native(native_event, Some(window_height)) };

    let Some(event) = event else {
        return NO;
    };

    let run_callback = |event: PlatformInput| -> BOOL {
        let mut callback = window_state.as_ref().lock().event_callback.take();
        let handled: BOOL = if let Some(callback) = callback.as_mut() {
            catch_platform_callback("input event", NO, || Bool::new(!callback(event).propagate))
        } else {
            NO
        };
        window_state.as_ref().lock().event_callback = callback;
        handled
    };

    match event {
        PlatformInput::KeyDown(mut key_down_event) => {
            // For certain keystrokes, macOS will first dispatch a "key equivalent" event.
            // If that event isn't handled, it will then dispatch a "key down" event. GPUI
            // makes no distinction between these two types of events, so we need to ignore
            // the "key down" event if we've already just processed its "key equivalent" version.
            if key_equivalent {
                lock.last_key_equivalent = Some(key_down_event.clone());
            } else if lock.last_key_equivalent.take().as_ref() == Some(&key_down_event) {
                return NO;
            }

            drop(lock);

            let is_composing =
                with_input_handler(this, |input_handler| input_handler.marked_text_range())
                    .flatten()
                    .is_some();

            // If we're composing, send the key to the input handler first;
            // otherwise we only send to the input handler if we don't have a matching binding.
            // The input handler may call `do_command_by_selector` if it doesn't know how to handle
            // a key. If it does so, it will return YES so we won't send the key twice.
            // We also do this for non-printing keys (like arrow keys and escape) as the IME menu
            // may need them even if there is no marked text;
            // however we skip keys with control or the input handler adds control-characters to the buffer.
            // and keys with function, as the input handler swallows them.
            if is_composing
                || (key_down_event.keystroke.key_char.is_none()
                    && !key_down_event.keystroke.modifiers.control
                    && !key_down_event.keystroke.modifiers.function)
            {
                {
                    let mut lock = window_state.as_ref().lock();
                    lock.keystroke_for_do_command = Some(key_down_event.keystroke.clone());
                    lock.do_command_handled.take();
                    drop(lock);
                }

                let handled: BOOL = unsafe {
                    let input_context: id = msg_send![this, inputContext];
                    msg_send![input_context, handleEvent: native_event]
                };
                window_state.as_ref().lock().keystroke_for_do_command.take();
                if let Some(handled) = window_state.as_ref().lock().do_command_handled.take() {
                    return Bool::new(handled);
                } else if handled == YES {
                    return YES;
                }

                let handled = run_callback(PlatformInput::KeyDown(key_down_event));
                return handled;
            }

            let handled = run_callback(PlatformInput::KeyDown(key_down_event.clone()));
            if handled == YES {
                return YES;
            }

            if key_down_event.is_held
                && let Some(key_char) = key_down_event.keystroke.key_char.as_ref()
            {
                let handled = with_input_handler(this, |input_handler| {
                    if !input_handler.apple_press_and_hold_enabled() {
                        input_handler.replace_text_in_range(None, key_char);
                        return YES;
                    }
                    NO
                });
                if handled == Some(YES) {
                    return YES;
                }
            }

            // Don't send key equivalents to the input handler,
            // or macOS shortcuts like cmd-` will stop working.
            if key_equivalent {
                return NO;
            }

            unsafe {
                let input_context: id = msg_send![this, inputContext];
                msg_send![input_context, handleEvent: native_event]
            }
        }

        PlatformInput::KeyUp(_) => {
            drop(lock);
            run_callback(event)
        }

        _ => NO,
    }
}

extern "C" fn handle_view_event(this: id, _: Sel, native_event: id) {
    let window_state = unsafe { get_window_state(this) };
    let weak_window_state = Arc::downgrade(&window_state);
    let mut lock = window_state.as_ref().lock();
    let window_height = lock.content_size().height;
    let event = unsafe { PlatformInput::from_native(native_event, Some(window_height)) };

    if let Some(mut event) = event {
        match &mut event {
            PlatformInput::MouseDown(
                event @ MouseDownEvent {
                    button: MouseButton::Left,
                    modifiers: Modifiers { control: true, .. },
                    ..
                },
            ) => {
                // On mac, a ctrl-left click should be handled as a right click.
                *event = MouseDownEvent {
                    button: MouseButton::Right,
                    modifiers: Modifiers {
                        control: false,
                        ..event.modifiers
                    },
                    click_count: 1,
                    ..*event
                };
            }

            // Handles focusing click.
            PlatformInput::MouseDown(
                event @ MouseDownEvent {
                    button: MouseButton::Left,
                    ..
                },
            ) if (lock.first_mouse) => {
                *event = MouseDownEvent {
                    first_mouse: true,
                    ..*event
                };
                lock.first_mouse = false;
            }

            PlatformInput::ScrollWheel(_)
                if unsafe {
                    let is_key: Bool = msg_send![lock.native_window, isKeyWindow];
                    is_key != YES
                } =>
            {
                let native_window = lock.native_window;
                drop(lock);
                unsafe {
                    let app: id = msg_send![lookup_class(c"NSApplication"), sharedApplication];
                    let _: () = msg_send![app, activateIgnoringOtherApps: YES];
                    let _: () = msg_send![native_window, makeKeyWindow];
                }
                lock = window_state.as_ref().lock();
            }

            // Because we map a ctrl-left_down to a right_down -> right_up let's ignore
            // the ctrl-left_up to avoid having a mismatch in button down/up events if the
            // user is still holding ctrl when releasing the left mouse button
            PlatformInput::MouseUp(
                event @ MouseUpEvent {
                    button: MouseButton::Left,
                    modifiers: Modifiers { control: true, .. },
                    ..
                },
            ) => {
                *event = MouseUpEvent {
                    button: MouseButton::Right,
                    modifiers: Modifiers {
                        control: false,
                        ..event.modifiers
                    },
                    click_count: 1,
                    ..*event
                };
            }

            _ => {}
        };

        if let Some(pointer) = crate::platform::mac::events::tablet_pointer_event(
            unsafe { &*(native_event as *const NSEvent) },
            &event,
            window_height,
        ) {
            event = PlatformInput::Pointer(pointer);
        }

        // CoreGraphics keeps the absolute cursor fixed while it is
        // disassociated. Preserve that stable hit-test position and expose the
        // unbounded AppKit deltas through the portable pointer contract.
        if lock.pointer_lock.status() == PointerLockStatus::Locked
            && let PlatformInput::MouseMove(mouse_move) = &event
        {
            let native_event = unsafe { &*(native_event as *const NSEvent) };
            let mut pointer = PointerInputEvent::from(mouse_move);
            pointer.movement = point(
                px(native_event.deltaX() as f32),
                px(-(native_event.deltaY() as f32)),
            );
            event = PlatformInput::Pointer(pointer);
        }

        match &event {
            PlatformInput::MouseDown(_) => {
                drop(lock);
                unsafe {
                    let input_context: id = msg_send![this, inputContext];
                    let _: BOOL = msg_send![input_context, handleEvent: native_event];
                }
                lock = window_state.as_ref().lock();
            }
            PlatformInput::MouseMove(
                event @ MouseMoveEvent {
                    pressed_button: Some(_),
                    ..
                },
            ) => {
                // Synthetic drag is used for selecting long buffer contents while buffer is being scrolled.
                // External file drag and drop is able to emit its own synthetic mouse events which will conflict
                // with these ones.
                if !lock.external_files_dragged {
                    lock.synthetic_drag_counter += 1;
                    let executor = lock.executor.clone();
                    executor
                        .spawn(synthetic_drag(
                            weak_window_state,
                            lock.synthetic_drag_counter,
                            event.clone(),
                        ))
                        .detach();
                }
            }

            PlatformInput::MouseUp(MouseUpEvent { .. }) => {
                lock.synthetic_drag_counter += 1;
            }

            PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                modifiers,
                capslock,
            }) => {
                // Only raise modifiers changed event when they have actually changed
                if let Some(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                    modifiers: prev_modifiers,
                    capslock: prev_capslock,
                })) = &lock.previous_modifiers_changed_event
                    && prev_modifiers == modifiers
                    && prev_capslock == capslock
                {
                    return;
                }

                lock.previous_modifiers_changed_event = Some(event.clone());
            }

            _ => {}
        }

        if let Some(mut callback) = lock.event_callback.take() {
            drop(lock);
            catch_platform_callback("input event", (), || {
                callback(event);
            });
            window_state.lock().event_callback = Some(callback);
        }
    }
}

extern "C" fn window_did_change_occlusion_state(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let lock = &mut *window_state.lock();
    unsafe {
        if lock
            .native_window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible)
        {
            lock.move_traffic_light();
            lock.start_display_link();
        } else {
            lock.stop_display_link();
        }
    }
}

extern "C" fn window_did_resize(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.move_traffic_light();

    // NSWindow resize notifications can arrive while Kael is still dispatching the input event
    // that initiated the resize (notably a titlebar double click). Updating the app and drawing a
    // frame re-entrantly from that notification leaves the view tree at the previous viewport until
    // another event enters the run loop. Coalesce resize completion onto the next main-thread turn.
    if lock.resize_sync_scheduled {
        return;
    }

    lock.resize_sync_scheduled = true;
    let executor = lock.executor.clone();
    drop(lock);

    executor
        .spawn(async move {
            unsafe { complete_native_resize(window_state) };
        })
        .detach();
}

extern "C" fn window_will_enter_fullscreen(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.fullscreen_restore_bounds = lock.bounds();

    let min_version = NSOperatingSystemVersion {
        majorVersion: 15,
        minorVersion: 3,
        patchVersion: 0,
    };

    if is_macos_version_at_least(min_version) {
        unsafe {
            lock.native_window.setTitlebarAppearsTransparent_(NO);
        }
    }
}

extern "C" fn window_will_exit_fullscreen(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();

    let min_version = NSOperatingSystemVersion {
        majorVersion: 15,
        minorVersion: 3,
        patchVersion: 0,
    };

    if is_macos_version_at_least(min_version) && lock.transparent_titlebar {
        unsafe {
            lock.native_window.setTitlebarAppearsTransparent_(YES);
        }
    }
}

pub(crate) fn is_macos_version_at_least(version: NSOperatingSystemVersion) -> bool {
    NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(version)
}

extern "C" fn window_did_move(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.moved_callback.take() {
        drop(lock);
        catch_platform_callback("window moved", (), &mut callback);
        window_state.lock().moved_callback = Some(callback);
    }
}

extern "C" fn window_did_change_screen(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    window_state.as_ref().lock().start_display_link();

    // AppKit normally follows this notification with
    // `viewDidChangeBackingProperties`, but that delivery can be delayed (or
    // omitted for some display/Space transitions). Synchronize eagerly so a
    // window never presents a drawable rasterized for the previous screen's
    // scale factor.
    sync_backing_scale(&window_state, "window screen change");
}

extern "C" fn window_did_change_key_status(this: id, selector: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.lock();
    let is_active = unsafe { lock.native_window.isKeyWindow() == YES };
    if !is_active && lock.pointer_lock.status() != PointerLockStatus::Unlocked {
        if let Err(error) = lock.release_pointer_lock() {
            log::error!("failed to release macOS pointer lock after focus loss: {error}");
        }
    }
    lock.accessibility_provider
        .update_view_focus_state(is_active);

    // When opening a pop-up while the application isn't active, Cocoa sends a spurious
    // `windowDidBecomeKey` message to the previous key window even though that window
    // isn't actually key. This causes a bug if the application is later activated while
    // the pop-up is still open, making it impossible to activate the previous key window
    // even if the pop-up gets closed. The only way to activate it again is to de-activate
    // the app and re-activate it, which is a pretty bad UX.
    // The following code detects the spurious event and invokes `resignKeyWindow`:
    // in theory, we're not supposed to invoke this method manually but it balances out
    // the spurious `becomeKeyWindow` event and helps us work around that bug.
    if selector == sel!(windowDidBecomeKey:) && !is_active {
        unsafe {
            let _: () = msg_send![lock.native_window, resignKeyWindow];
            return;
        }
    }

    let executor = lock.executor.clone();
    drop(lock);

    // When a window becomes active, trigger an immediate synchronous frame request to prevent
    // tab flicker when switching between windows in native tabs mode.
    //
    // This is only done on subsequent activations (not the first) to ensure the initial focus
    // path is properly established. Without this guard, the focus state would remain unset until
    // the first mouse click, causing keybindings to be non-functional.
    if selector == sel!(windowDidBecomeKey:) && is_active {
        let window_state = unsafe { get_window_state(this) };
        let mut lock = window_state.lock();

        if lock.activated_least_once {
            if let Some(mut callback) = lock.request_frame_callback.take() {
                #[cfg(not(feature = "macos-blade"))]
                lock.renderer.set_presents_with_transaction(true);
                lock.stop_display_link();
                drop(lock);
                catch_platform_callback("frame request", (), || callback(Default::default()));

                let mut lock = window_state.lock();
                lock.request_frame_callback = Some(callback);
                #[cfg(not(feature = "macos-blade"))]
                lock.renderer.set_presents_with_transaction(false);
                lock.start_display_link();
            }
        } else {
            lock.activated_least_once = true;
        }
    }

    executor
        .spawn(async move {
            let mut lock = window_state.as_ref().lock();
            if is_active {
                lock.move_traffic_light();
            }

            if let Some(mut callback) = lock.activate_callback.take() {
                drop(lock);
                catch_platform_callback("window activation", (), || callback(is_active));
                window_state.lock().activate_callback = Some(callback);
            };
        })
        .detach();
}

extern "C" fn window_should_close(this: id, _: Sel, _: id) -> BOOL {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.should_close_callback.take() {
        drop(lock);
        let should_close = catch_platform_callback("window should-close", false, &mut callback);
        window_state.lock().should_close_callback = Some(callback);
        Bool::new(should_close)
    } else {
        YES
    }
}

extern "C" fn close_window(this: id, _: Sel) {
    unsafe {
        let close_callback = {
            let window_state = get_window_state(this);
            let mut lock = window_state.as_ref().lock();
            lock.close_callback.take()
        };

        if let Some(callback) = close_callback {
            catch_platform_callback("window close", (), callback);
        }

        let _: () = msg_send![super(this, lookup_class(c"NSWindow")), close];
    }
}

extern "C" fn make_backing_layer(this: id, _: Sel) -> id {
    let window_state = unsafe { get_window_state(this) };
    let window_state = window_state.as_ref().lock();
    window_state.renderer.layer_ptr() as id
}

extern "C" fn view_did_change_backing_properties(this: id, _: Sel) {
    let window_state = unsafe { get_window_state(this) };
    sync_backing_scale(&window_state, "window backing-scale change");
}

fn sync_backing_scale(window_state: &Arc<Mutex<MacWindowState>>, callback_name: &'static str) {
    let mut lock = window_state.as_ref().lock();

    let scale_factor = lock.scale_factor();
    let size = lock.content_size();
    let drawable_size = size.to_device_pixels(scale_factor);
    unsafe {
        let layer = lock.renderer.layer_ptr() as id;
        let _: () = msg_send![
            layer,
            setContentsScale: scale_factor as f64
        ];
    }

    lock.renderer.update_drawable_size(drawable_size);

    if let Some(mut callback) = lock.resize_callback.take() {
        let content_size = lock.content_size();
        let scale_factor = lock.scale_factor();
        drop(lock);
        catch_platform_callback(callback_name, (), || callback(content_size, scale_factor));
        window_state.as_ref().lock().resize_callback = Some(callback);
    };
}

extern "C" fn set_frame_size(this: id, _: Sel, size: NSSize) {
    let window_state = unsafe { get_window_state(this) };
    unsafe { update_native_view_size(this, &window_state, size, false) };
}

unsafe fn update_native_view_size(
    native_view: id,
    window_state: &Arc<Mutex<MacWindowState>>,
    size: NSSize,
    notify_if_unchanged: bool,
) {
    let mut lock = window_state.as_ref().lock();

    let new_size = Size::<Pixels>::from(size);
    let old_size = unsafe {
        let old_frame: NSRect = msg_send![native_view, frame];
        Size::<Pixels>::from(old_frame.size)
    };

    let size_changed = old_size != new_size;
    if !size_changed && !notify_if_unchanged {
        return;
    }

    if size_changed {
        unsafe {
            let _: () = msg_send![super(native_view, lookup_class(c"NSView")), setFrameSize: size];
        }
    }

    let scale_factor = lock.scale_factor();
    let drawable_size = new_size.to_device_pixels(scale_factor);
    lock.renderer.update_drawable_size(drawable_size);

    if let Some(mut callback) = lock.resize_callback.take() {
        let content_size = lock.content_size();
        let scale_factor = lock.scale_factor();
        drop(lock);
        catch_platform_callback("window resize", (), || callback(content_size, scale_factor));
        window_state.lock().resize_callback = Some(callback);
    };
}

unsafe fn complete_native_resize(window_state: Arc<Mutex<MacWindowState>>) {
    unsafe {
        let (window, native_view) = {
            let mut lock = window_state.lock();
            lock.resize_sync_scheduled = false;
            (lock.native_window, lock.native_view.as_ptr() as id)
        };

        let content_view: id = msg_send![window, contentView];
        if content_view == nil {
            return;
        }

        let content_bounds: NSRect = msg_send![content_view, bounds];

        // Complete the native autoresize ourselves. Calling the shared helper directly avoids
        // depending on AppKit to deliver `setFrameSize:` before the next input event and also
        // guarantees Kael receives an authoritative bounds callback after re-entrant dispatch has
        // unwound, even when AppKit already resized the view.
        update_native_view_size(native_view, &window_state, content_bounds.size, true);

        request_frame_immediately(
            &window_state,
            RequestFrameOptions {
                require_presentation: true,
                force_render: true,
            },
        );

        let _: () = msg_send![native_view, setNeedsDisplay: YES];
        let _: () = msg_send![window, displayIfNeeded];
    }
}

extern "C" fn display_layer(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.lock();
    if let Some(mut callback) = lock.request_frame_callback.take() {
        #[cfg(not(feature = "macos-blade"))]
        lock.renderer.set_presents_with_transaction(true);
        lock.stop_display_link();
        drop(lock);
        catch_platform_callback("frame request", (), || callback(Default::default()));

        let mut lock = window_state.lock();
        lock.request_frame_callback = Some(callback);
        #[cfg(not(feature = "macos-blade"))]
        lock.renderer.set_presents_with_transaction(false);
        lock.start_display_link();
    }
}

unsafe extern "C" fn step(view: *mut c_void) {
    let view = view as id;
    let window_state = unsafe { get_window_state(view) };
    let mut lock = window_state.lock();

    if let Some(mut callback) = lock.request_frame_callback.take() {
        drop(lock);
        catch_platform_callback("frame request", (), || callback(Default::default()));
        window_state.lock().request_frame_callback = Some(callback);
    }
}

extern "C" fn valid_attributes_for_marked_text(_: id, _: Sel) -> id {
    unsafe { msg_send![lookup_class(c"NSArray"), array] }
}

extern "C" fn has_marked_text(this: id, _: Sel) -> BOOL {
    let has_marked_text_result =
        with_input_handler(this, |input_handler| input_handler.marked_text_range()).flatten();

    Bool::new(has_marked_text_result.is_some())
}

extern "C" fn marked_range(this: id, _: Sel) -> NSRange {
    let marked_range_result =
        with_input_handler(this, |input_handler| input_handler.marked_text_range()).flatten();

    marked_range_result.map_or(NSRange::invalid(), |range| range.into())
}

extern "C" fn selected_range(this: id, _: Sel) -> NSRange {
    let selected_range_result = with_input_handler(this, |input_handler| {
        input_handler.selected_text_range(false)
    })
    .flatten();

    selected_range_result.map_or(NSRange::invalid(), |selection| selection.range.into())
}

extern "C" fn first_rect_for_character_range(this: id, _: Sel, range: NSRange, _: id) -> NSRect {
    let frame = get_frame(this);
    with_input_handler(this, |input_handler| {
        input_handler.bounds_for_range(range.to_range()?)
    })
    .flatten()
    .map_or(
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.)),
        |bounds| {
            NSRect::new(
                NSPoint::new(
                    frame.origin.x + bounds.origin.x.0 as f64,
                    frame.origin.y + frame.size.height
                        - bounds.origin.y.0 as f64
                        - bounds.size.height.0 as f64,
                ),
                NSSize::new(bounds.size.width.0 as f64, bounds.size.height.0 as f64),
            )
        },
    )
}

fn get_frame(this: id) -> NSRect {
    unsafe {
        let state = get_window_state(this);
        let lock = state.lock();
        let mut frame = lock.native_window.frame();
        let content_layout_rect: NSRect = msg_send![lock.native_window, contentLayoutRect];
        let style_mask: NSWindowStyleMask = msg_send![lock.native_window, styleMask];
        if !style_mask.contains(NSWindowStyleMask::FullSizeContentView) {
            frame.origin.y -= frame.size.height - content_layout_rect.size.height;
        }
        frame
    }
}

extern "C" fn insert_text(this: id, _: Sel, text: id, replacement_range: NSRange) {
    unsafe {
        let is_attributed_string: BOOL =
            msg_send![text, isKindOfClass: lookup_class(c"NSAttributedString")];
        let text: id = if is_attributed_string == YES {
            msg_send![text, string]
        } else {
            text
        };

        let text = text.to_str();
        let replacement_range = replacement_range.to_range();
        with_input_handler(this, |input_handler| {
            input_handler.replace_text_in_range(replacement_range, text)
        });
    }
}

extern "C" fn set_marked_text(
    this: id,
    _: Sel,
    text: id,
    selected_range: NSRange,
    replacement_range: NSRange,
) {
    unsafe {
        let is_attributed_string: BOOL =
            msg_send![text, isKindOfClass: lookup_class(c"NSAttributedString")];
        let text: id = if is_attributed_string == YES {
            msg_send![text, string]
        } else {
            text
        };
        let selected_range = selected_range.to_range();
        let replacement_range = replacement_range.to_range();
        let text = text.to_str();
        with_input_handler(this, |input_handler| {
            input_handler.replace_and_mark_text_in_range(replacement_range, text, selected_range)
        });
    }
}
extern "C" fn unmark_text(this: id, _: Sel) {
    with_input_handler(this, |input_handler| input_handler.unmark_text());
}

extern "C" fn attributed_substring_for_proposed_range(
    this: id,
    _: Sel,
    range: NSRange,
    actual_range: *mut c_void,
) -> id {
    with_input_handler(this, |input_handler| {
        let range = range.to_range()?;
        if range.is_empty() {
            return None;
        }
        let mut adjusted: Option<Range<usize>> = None;

        let selected_text = input_handler.text_for_range(range.clone(), &mut adjusted)?;
        if let Some(adjusted) = adjusted
            && adjusted != range
        {
            unsafe { (actual_range as *mut NSRange).write(NSRange::from(adjusted)) };
        }
        unsafe {
            let string: id = msg_send![lookup_class(c"NSAttributedString"), alloc];
            let string: id = msg_send![string, initWithString: ns_string(&selected_text)];
            Some(string)
        }
    })
    .flatten()
    .unwrap_or(nil)
}

// We ignore which selector it asks us to do because the user may have
// bound the shortcut to something else.
extern "C" fn do_command_by_selector(this: id, _: Sel, _: Sel) {
    let state = unsafe { get_window_state(this) };
    let mut lock = state.as_ref().lock();
    let keystroke = lock.keystroke_for_do_command.take();
    let mut event_callback = lock.event_callback.take();
    drop(lock);

    if let Some((keystroke, mut callback)) = keystroke.zip(event_callback.as_mut()) {
        let handled = catch_platform_callback("command key", None, || {
            Some((callback)(PlatformInput::KeyDown(KeyDownEvent {
                keystroke,
                is_held: false,
            })))
        });
        state.as_ref().lock().do_command_handled = handled.map(|handled| !handled.propagate);
    }

    state.as_ref().lock().event_callback = event_callback;
}

extern "C" fn view_did_change_effective_appearance(this: id, _: Sel) {
    unsafe {
        let state = get_window_state(this);
        let mut lock = state.as_ref().lock();
        if let Some(mut callback) = lock.appearance_changed_callback.take() {
            drop(lock);
            catch_platform_callback("appearance changed", (), &mut callback);
            state.lock().appearance_changed_callback = Some(callback);
        }
    }
}

extern "C" fn accepts_first_mouse(this: id, _: Sel, _: id) -> BOOL {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.first_mouse = true;
    YES
}

extern "C" fn character_index_for_point(this: id, _: Sel, position: NSPoint) -> u64 {
    let position = screen_point_to_gpui_point(this, position);
    with_input_handler(this, |input_handler| {
        input_handler.character_index_for_point(position)
    })
    .flatten()
    .map(|index| index as u64)
    .unwrap_or(NSUInteger::MAX as u64)
}

fn screen_point_to_gpui_point(this: id, position: NSPoint) -> Point<Pixels> {
    let frame = get_frame(this);
    let window_x = position.x - frame.origin.x;
    let window_y = frame.size.height - (position.y - frame.origin.y);

    point(px(window_x as f32), px(window_y as f32))
}

extern "C" fn dragging_entered(this: id, _: Sel, dragging_info: id) -> NSDragOperation {
    let window_state = unsafe { get_window_state(this) };
    let position = drag_event_position(&window_state, dragging_info);
    if let Some(data) = external_drop_data_from_event(dragging_info)
        && send_new_event(&window_state, file_drop_entered_event(position, data))
    {
        window_state.lock().external_files_dragged = true;
        return NSDragOperationCopy;
    }
    NSDragOperationNone
}

extern "C" fn dragging_updated(this: id, _: Sel, dragging_info: id) -> NSDragOperation {
    let window_state = unsafe { get_window_state(this) };
    let position = drag_event_position(&window_state, dragging_info);
    if send_new_event(
        &window_state,
        PlatformInput::FileDrop(FileDropEvent::Pending { position }),
    ) {
        NSDragOperationCopy
    } else {
        NSDragOperationNone
    }
}

extern "C" fn dragging_exited(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    send_new_event(
        &window_state,
        PlatformInput::FileDrop(FileDropEvent::Exited),
    );
    window_state.lock().external_files_dragged = false;
}

extern "C" fn perform_drag_operation(this: id, _: Sel, dragging_info: id) -> BOOL {
    let window_state = unsafe { get_window_state(this) };
    let position = drag_event_position(&window_state, dragging_info);
    send_new_event(
        &window_state,
        PlatformInput::FileDrop(FileDropEvent::Submit { position }),
    )
    .to_objc()
}

fn external_drop_data_from_event(dragging_info: id) -> Option<ExternalDropData> {
    let mut paths = SmallVec::<[PathBuf; 2]>::new();
    let mut urls = Vec::new();
    let mut text_values = Vec::new();
    let pasteboard: id = unsafe { msg_send![dragging_info, draggingPasteboard] };
    let pasteboard_items: id = unsafe { msg_send![pasteboard, pasteboardItems] };
    if pasteboard_items.is_null() {
        return None;
    }
    let count: NSUInteger = unsafe { msg_send![pasteboard_items, count] };
    for index in 0..count {
        let item: id = unsafe { msg_send![pasteboard_items, objectAtIndex: index] };
        let file_url: id = unsafe { msg_send![item, stringForType: NSPasteboardTypeFileURL] };
        let mut item_has_file_url = false;
        if !file_url.is_null() {
            let url: id = unsafe { msg_send![lookup_class(c"NSURL"), URLWithString: file_url] };
            if !url.is_null() {
                let is_file_url: BOOL = unsafe { msg_send![url, isFileURL] };
                if is_file_url == YES {
                    item_has_file_url = true;
                    let path: id = unsafe { msg_send![url, path] };
                    if !path.is_null() {
                        paths.push(PathBuf::from(unsafe { path.to_str() }.to_string()));
                    }
                }
            }
        }

        let url_value = pasteboard_item_string(item, unsafe { NSPasteboardTypeURL });
        if let Some(url_value) = url_value {
            urls.push(url_value);
        }

        if !item_has_file_url {
            let string_value = pasteboard_item_string(item, unsafe { NSPasteboardTypeString });
            if let Some(string_value) = string_value
                && !string_value.is_empty()
            {
                text_values.push(string_value);
            }
        }
    }
    urls.sort();
    urls.dedup();
    let mut data = ExternalDropData::from_paths(paths).with_urls(urls);
    if !text_values.is_empty() {
        text_values.dedup();
        data = data.with_text(text_values.join("\n"));
    }
    if data.has_paths() || data.has_urls() || data.has_text() {
        Some(data)
    } else {
        None
    }
}

fn pasteboard_item_string(item: id, pasteboard_type: &'static NSPasteboardType) -> Option<String> {
    let value: id = unsafe { msg_send![item, stringForType: pasteboard_type] };
    if value.is_null() {
        return None;
    }
    Some(unsafe { value.to_str() }.to_string())
}

fn file_drop_entered_event(position: Point<Pixels>, data: ExternalDropData) -> PlatformInput {
    if data.has_urls() || data.has_text() {
        PlatformInput::FileDrop(FileDropEvent::DataEntered { position, data })
    } else {
        PlatformInput::FileDrop(FileDropEvent::Entered {
            position,
            paths: data.paths().clone(),
        })
    }
}

extern "C" fn conclude_drag_operation(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    send_new_event(
        &window_state,
        PlatformInput::FileDrop(FileDropEvent::Exited),
    );
}

async fn synthetic_drag(
    window_state: Weak<Mutex<MacWindowState>>,
    drag_id: usize,
    event: MouseMoveEvent,
) {
    loop {
        Timer::after(Duration::from_millis(16)).await;
        if let Some(window_state) = window_state.upgrade() {
            let mut lock = window_state.lock();
            if lock.synthetic_drag_counter == drag_id {
                if let Some(mut callback) = lock.event_callback.take() {
                    drop(lock);
                    catch_platform_callback("synthetic drag", (), || {
                        callback(PlatformInput::MouseMove(event.clone()));
                    });
                    window_state.lock().event_callback = Some(callback);
                }
            } else {
                break;
            }
        }
    }
}

fn send_new_event(window_state_lock: &Mutex<MacWindowState>, e: PlatformInput) -> bool {
    let window_state = window_state_lock.lock().event_callback.take();
    if let Some(mut callback) = window_state {
        catch_platform_callback("input event", (), || {
            callback(e);
        });
        window_state_lock.lock().event_callback = Some(callback);
        true
    } else {
        false
    }
}

fn drag_event_position(window_state: &Mutex<MacWindowState>, dragging_info: id) -> Point<Pixels> {
    let drag_location: NSPoint = unsafe { msg_send![dragging_info, draggingLocation] };
    convert_mouse_position(drag_location, window_state.lock().content_size().height)
}

fn perform_titlebar_double_click_action(window: id) {
    unsafe {
        let defaults: id = msg_send![lookup_class(c"NSUserDefaults"), standardUserDefaults];
        let domain = ns_string("NSGlobalDomain");
        let key = ns_string("AppleActionOnDoubleClick");

        let dict: id = msg_send![defaults, persistentDomainForName: domain];
        let action: id = if !dict.is_null() {
            msg_send![dict, objectForKey: key]
        } else {
            nil
        };

        let action_str = if !action.is_null() {
            action.to_str()
        } else {
            ""
        };

        match action_str {
            "None" => {}
            "Minimize" => {
                let _: () = msg_send![window, performMiniaturize: nil];
            }
            _ => {
                zoom_window_immediately(window);
            }
        }
    }
}

fn zoom_window_immediately(window: id) {
    unsafe {
        let is_zoomed: BOOL = msg_send![window, isZoomed];
        if is_zoomed == YES {
            window.zoom_(nil);
            return;
        }

        let screen = window.screen();
        if screen == nil {
            let _: () = msg_send![window, performZoom: nil];
            return;
        }

        let target_frame: NSRect = msg_send![screen, visibleFrame];
        let _: () = msg_send![window, setFrame: target_frame, display: YES, animate: NO];
    }
}

fn request_frame_immediately(
    window_state: &Arc<Mutex<MacWindowState>>,
    options: RequestFrameOptions,
) {
    let mut lock = window_state.lock();
    let Some(mut callback) = lock.request_frame_callback.take() else {
        return;
    };

    #[cfg(not(feature = "macos-blade"))]
    lock.renderer.set_presents_with_transaction(true);
    lock.stop_display_link();
    drop(lock);

    catch_platform_callback("frame request", (), || callback(options));

    let mut lock = window_state.lock();
    lock.request_frame_callback = Some(callback);
    #[cfg(not(feature = "macos-blade"))]
    lock.renderer.set_presents_with_transaction(false);
    lock.start_display_link();
}

fn with_input_handler<F, R>(window: id, f: F) -> Option<R>
where
    F: FnOnce(&mut PlatformInputHandler) -> R,
{
    let window_state = unsafe { get_window_state(window) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut input_handler) = lock.input_handler.take() {
        drop(lock);
        let result = f(&mut input_handler);
        window_state.lock().input_handler = Some(input_handler);
        Some(result)
    } else {
        None
    }
}

unsafe fn display_id_for_screen(screen: id) -> CGDirectDisplayID {
    unsafe {
        let device_description: id = msg_send![screen, deviceDescription];
        let screen_number_key = ns_string("NSScreenNumber");
        let screen_number: id = msg_send![device_description, objectForKey: screen_number_key];
        let screen_number: NSUInteger = msg_send![screen_number, unsignedIntegerValue];
        screen_number as CGDirectDisplayID
    }
}

extern "C" fn blurred_view_init_with_frame(this: id, _: Sel, frame: NSRect) -> id {
    unsafe {
        let view =
            msg_send![super(this, lookup_class(c"NSVisualEffectView")), initWithFrame: frame];
        // Use a colorless semantic material. The default value `AppearanceBased`, though not
        // manually set, is deprecated.
        let _: () = msg_send![view, setMaterial: NSVisualEffectMaterial::Selection];
        let _: () = msg_send![view, setState: NSVisualEffectState::Active];
        view
    }
}

extern "C" fn blurred_view_update_layer(this: id, _: Sel) {
    unsafe {
        let _: () = msg_send![
            super(this, lookup_class(c"NSVisualEffectView")),
            updateLayer
        ];
        let layer: id = msg_send![this, layer];
        if !layer.is_null() {
            remove_layer_background(layer);
        }
    }
}

unsafe fn remove_layer_background(layer: id) {
    unsafe {
        let _: () = msg_send![layer, setBackgroundColor:nil];

        let class_name: id = msg_send![layer, className];
        let chameleon_layer = ns_string("CAChameleonLayer");
        let is_chameleon_layer: BOOL = msg_send![class_name, isEqualToString: chameleon_layer];
        if is_chameleon_layer == YES {
            // Remove the desktop tinting effect.
            let _: () = msg_send![layer, setHidden: YES];
            return;
        }

        let filters: id = msg_send![layer, filters];
        if !filters.is_null() {
            // Remove the increased saturation.
            // The effect of a `CAFilter` or `CIFilter` is determined by its name, and the
            // `description` reflects its name and some parameters. Currently `NSVisualEffectView`
            // uses a `CAFilter` named "colorSaturate". If one day they switch to `CIFilter`, the
            // `description` will still contain "Saturat" ("... inputSaturation = ...").
            let test_string = ns_string("Saturat");
            let count: NSUInteger = msg_send![filters, count];
            for i in 0..count {
                let filter: id = msg_send![filters, objectAtIndex: i];
                let description: id = msg_send![filter, description];
                let hit: BOOL = msg_send![description, containsString: test_string];
                if hit == NO {
                    continue;
                }

                let all_indices = NSRange {
                    location: 0,
                    length: count,
                };
                let indices: id = msg_send![lookup_class(c"NSMutableIndexSet"), indexSet];
                let _: () = msg_send![indices, addIndexesInRange: all_indices];
                let _: () = msg_send![indices, removeIndex:i];
                let filtered: id = msg_send![filters, objectsAtIndexes: indices];
                let _: () = msg_send![layer, setFilters: filtered];
                break;
            }
        }

        let sublayers: id = msg_send![layer, sublayers];
        if !sublayers.is_null() {
            let count: NSUInteger = msg_send![sublayers, count];
            for i in 0..count {
                let sublayer: id = msg_send![sublayers, objectAtIndex: i];
                remove_layer_background(sublayer);
            }
        }
    }
}

extern "C" fn add_titlebar_accessory_view_controller(this: id, _: Sel, view_controller: id) {
    unsafe {
        let _: () = msg_send![super(this, lookup_class(c"NSWindow")), addTitlebarAccessoryViewController: view_controller];

        // Hide the native tab bar and set its height to 0, since we render our own.
        let accessory_view: id = msg_send![view_controller, view];
        let _: () = msg_send![accessory_view, setHidden: YES];
        let mut frame: NSRect = msg_send![accessory_view, frame];
        frame.size.height = 0.0;
        let _: () = msg_send![accessory_view, setFrame: frame];
    }
}

extern "C" fn move_tab_to_new_window(this: id, _: Sel, _: id) {
    unsafe {
        let _: () = msg_send![super(this, lookup_class(c"NSWindow")), moveTabToNewWindow:nil];

        let window_state = get_window_state(this);
        let mut lock = window_state.as_ref().lock();
        if let Some(mut callback) = lock.move_tab_to_new_window_callback.take() {
            drop(lock);
            catch_platform_callback("move tab to new window", (), &mut callback);
            window_state.lock().move_tab_to_new_window_callback = Some(callback);
        }
    }
}

extern "C" fn merge_all_windows(this: id, _: Sel, _: id) {
    unsafe {
        let _: () = msg_send![super(this, lookup_class(c"NSWindow")), mergeAllWindows:nil];

        let window_state = get_window_state(this);
        let mut lock = window_state.as_ref().lock();
        if let Some(mut callback) = lock.merge_all_windows_callback.take() {
            drop(lock);
            catch_platform_callback("merge windows", (), &mut callback);
            window_state.lock().merge_all_windows_callback = Some(callback);
        }
    }
}

extern "C" fn select_next_tab(this: id, _sel: Sel, _id: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.select_next_tab_callback.take() {
        drop(lock);
        catch_platform_callback("select next tab", (), &mut callback);
        window_state.lock().select_next_tab_callback = Some(callback);
    }
}

extern "C" fn select_previous_tab(this: id, _sel: Sel, _id: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.select_previous_tab_callback.take() {
        drop(lock);
        catch_platform_callback("select previous tab", (), &mut callback);
        window_state.lock().select_previous_tab_callback = Some(callback);
    }
}

extern "C" fn toggle_tab_bar(this: id, _sel: Sel, _id: id) {
    unsafe {
        let _: () = msg_send![super(this, lookup_class(c"NSWindow")), toggleTabBar:nil];

        let window_state = get_window_state(this);
        let mut lock = window_state.as_ref().lock();
        lock.move_traffic_light();

        if let Some(mut callback) = lock.toggle_tab_bar_callback.take() {
            drop(lock);
            catch_platform_callback("toggle tab bar", (), &mut callback);
            window_state.lock().toggle_tab_bar_callback = Some(callback);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_callback_panics_are_contained_with_the_requested_fallback() {
        let value = catch_platform_callback("test", 41, || panic!("callback failure"));
        assert_eq!(value, 41);

        let value = catch_platform_callback("test", 0, || 42);
        assert_eq!(value, 42);
    }

    #[test]
    fn active_frame_polling_retries_a_missing_display_link() {
        assert!(should_start_display_link(true, false, false));
        assert!(should_start_display_link(true, true, false));
        assert!(!should_start_display_link(true, true, true));
        assert!(!should_start_display_link(false, true, true));
        assert!(!should_start_display_link(false, false, false));
    }

    #[test]
    fn native_alert_responses_map_back_to_original_answer_indices() {
        let button_order = [0, 2, 1];
        assert_eq!(alert_response_index(1_000, &button_order), Some(0));
        assert_eq!(alert_response_index(1_001, &button_order), Some(2));
        assert_eq!(alert_response_index(1_002, &button_order), Some(1));
        assert_eq!(alert_response_index(999, &button_order), None);
        assert_eq!(alert_response_index(1_003, &button_order), None);
    }

    #[test]
    fn webview_clipboard_script_exposes_text_clipboard_bridge() {
        let script = webview_clipboard_script("test-nonce");

        assert!(script.contains("navigator, 'clipboard'"));
        assert!(script.contains("readText: () => send('readText')"));
        assert!(script.contains("writeText: value => send('writeText'"));
        assert!(script.contains("bridge.read = () => bridge.readText()"));
        assert!(script.contains("bridge.write = async items"));
        assert!(script.contains("Only text/plain clipboard items are supported"));
        assert!(script.contains("document.execCommand = function"));
        assert!(script.contains("normalized === 'copy' || normalized === 'cut'"));
        assert!(script.contains("__kaelClipboard"));
        assert!(script.contains("__kaelIpcNonce"));
        assert!(script.contains("test-nonce"));
        assert!(script.contains("__kaelClipboardBridge"));
        assert!(script.contains("handler.postMessage(JSON.stringify("));
    }

    #[test]
    fn webview_message_bridge_serializes_the_authenticated_envelope() {
        let script = webview_bridge_script(None, "test-nonce");

        assert!(script.contains("postMessage(JSON.stringify("));
        assert!(script.contains("__kaelIpcNonce: nonce"));
        assert!(script.contains("test-nonce"));
    }

    #[test]
    fn unsafe_popup_allow_is_downgraded_to_current_view_navigation() {
        assert_eq!(
            safe_mac_new_window_policy(WebViewNewWindowPolicy::Allow),
            WebViewNewWindowPolicy::NavigateCurrent
        );
        assert_eq!(
            safe_mac_new_window_policy(WebViewNewWindowPolicy::Deny),
            WebViewNewWindowPolicy::Deny
        );
    }

    #[test]
    fn native_cookie_conversion_preserves_http_only_and_secure() {
        objc2::rc::autoreleasepool(|_| {
            let expected = WebViewCookie::new("session", "secret")
                .domain("example.com")
                .path("/")
                .secure(true)
                .http_only(true);
            let native = mac_webview_cookie_to_ns(expected.clone(), None)
                .expect("valid native cookie properties");
            let actual = unsafe { mac_webview_cookie_from_ns(native) };

            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.value, expected.value);
            assert!(actual.secure);
            assert!(actual.http_only);
        });
    }

    #[test]
    fn mac_webview_bridge_rejects_missing_or_wrong_nonce() {
        let payload = serde_json::json!({
            "__kaelIpcNonce": "expected",
            "body": r#"{"kind":"ready"}"#,
        });
        assert_eq!(
            decode_mac_webview_bridge_message(&payload, "expected"),
            Some(serde_json::json!({ "kind": "ready" }))
        );
        assert_eq!(decode_mac_webview_bridge_message(&payload, "wrong"), None);
        assert_eq!(
            decode_mac_webview_bridge_message(&serde_json::json!({ "body": "{}" }), "expected"),
            None
        );
    }
}
