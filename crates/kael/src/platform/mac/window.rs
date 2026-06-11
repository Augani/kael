use super::{BoolExt, MacDisplay, NSRange, NSRangeExt, NSStringExt, ns_string, renderer};
use crate::{
    AnyWindowHandle, AsyncWindowContext, Bounds, Capslock, DisplayLink, Edges, ExternalPaths,
    FileDropEvent, ForegroundExecutor, KeyDownEvent, Keystroke, Modifiers, ModifiersChangedEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformAtlas,
    PlatformDisplay, PlatformInput, PlatformWindow, Point, PromptButton, PromptLevel, RenderImage,
    RequestFrameOptions, Rgba, SharedString, Size, SystemWindowTab, Timer, WindowAppearance,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowKind, WindowParams,
    dispatch_get_main_queue,
    dispatch_sys::dispatch_async_f,
    platform::PlatformInputHandler,
    point,
    print::{PlatformPrintJob, PlatformPrintPage, PrintCommand, PrintImageFit},
    px, size,
    webview::{
        NavigationPolicy, PlatformWebView, PlatformWebViewCommand, WebViewMessageHandler,
        WebViewNavigationHandler,
    },
};
use block2::{Block, RcBlock};

use core_graphics::display::CGDirectDisplayID;
use ctor::ctor;
use futures::channel::oneshot;
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba as ImageRgba};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::*;
use objc2_foundation::{
    NSInteger, NSOperatingSystemVersion, NSPoint, NSProcessInfo, NSRect, NSSize, NSUInteger,
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
    sync::{Arc, Weak},
    time::Duration,
};
use util::ResultExt;

const WINDOW_STATE_IVAR: &str = "windowState";
const WEBVIEW_STATE_IVAR: &str = "webViewState";
const PRINT_VIEW_STATE_IVAR: &str = "printViewState";
const WEBVIEW_MESSAGE_HANDLER_NAME: &str = "gpui";

#[allow(non_camel_case_types)]
type id = *mut AnyObject;
type Object = AnyObject;
type Class = AnyClass;
type BOOL = Bool;
type Method0<R> = extern "C" fn(id, Sel) -> R;
type Method1<A, R> = extern "C" fn(id, Sel, A) -> R;
type Method2<A, B, R> = extern "C" fn(id, Sel, A, B) -> R;
type Method3<A, B, C, R> = extern "C" fn(id, Sel, A, B, C) -> R;

const YES: Bool = Bool::YES;
const NO: Bool = Bool::NO;
#[allow(non_upper_case_globals)]
const nil: id = ptr::null_mut();

static mut WINDOW_CLASS: *const Class = ptr::null();
static mut PANEL_CLASS: *const Class = ptr::null();
static mut VIEW_CLASS: *const Class = ptr::null();
static mut BLURRED_VIEW_CLASS: *const Class = ptr::null();
static mut WEBVIEW_DELEGATE_CLASS: *const Class = ptr::null();
static mut PRINT_VIEW_CLASS: *const Class = ptr::null();

unsafe fn lookup_class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing class {name:?}"))
}

unsafe fn ivar_ptr<T: objc2::encode::Encode>(object: id, name: &str) -> *mut T {
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

                decl.add_method(sel!(dealloc), dealloc_webview_delegate);
                if let Some(protocol) = AnyProtocol::get(c"WKNavigationDelegate") {
                    decl.add_protocol(protocol);
                }
                if let Some(protocol) = AnyProtocol::get(c"WKScriptMessageHandler") {
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
        | PlatformWebViewCommand::EvaluateJavaScript { id, .. }
        | PlatformWebViewCommand::PostMessage { id, .. }
        | PlatformWebViewCommand::Reload { id }
        | PlatformWebViewCommand::GoBack { id }
        | PlatformWebViewCommand::GoForward { id } => id.clone(),
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

fn webview_bridge_script(storage_key: Option<&SharedString>) -> String {
    let storage_key = storage_key
        .map(|storage_key| {
            format!(
                "window.GPUI_WEBVIEW_STORAGE_ID = {};",
                json_string_literal(storage_key.as_ref())
            )
        })
        .unwrap_or_default();

    format!(
        "(() => {{ {storage_key} if (!window.external) {{ window.external = {{}}; }} window.external.invoke = function(message) {{ const payload = typeof message === 'string' ? message : JSON.stringify(message); window.webkit.messageHandlers.{WEBVIEW_MESSAGE_HANDLER_NAME}.postMessage(payload); }}; if (!window.gpui) {{ window.gpui = {{}}; }} window.gpui.postMessage = function(message) {{ window.external.invoke(message); }}; }})();"
    )
}

fn webview_css_script(css: &str) -> String {
    format!(
        "(() => {{ const mount = () => {{ if (!document.head) {{ return; }} const style = document.createElement('style'); style.setAttribute('data-gpui-webview-style', 'true'); style.textContent = {}; document.head.appendChild(style); }}; if (document.head) {{ mount(); }} else {{ document.addEventListener('DOMContentLoaded', mount, {{ once: true }}); }} }})();",
        json_string_literal(css)
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
    let frame = NSRect::new(
        NSPoint::new(0., 0.),
        NSSize::new(job.page_size.width.0 as f64, job.page_size.height.0 as f64),
    );
    let view: id = unsafe { msg_send![view, initWithFrame: frame] };
    let state = Box::new(MacPrintViewState {
        page_size: job.page_size,
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
            let _: () = msg_send![
                ns_image,
                drawInRect: target_rect,
                fromRect: source_rect,
                operation: NSCompositingOperationSourceOver,
                fraction: 1.0f64
            ];
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

unsafe fn call_navigation_decision_handler(decision_handler: id, policy: NSInteger) {
    unsafe {
        let decision_handler = &*(decision_handler as *const Block<dyn Fn(NSInteger)>);
        decision_handler.call((policy,));
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
    accessibility_provider: super::accessibility::MacAccessibilityProvider,
    webviews: HashMap<SharedString, MacWebViewHost>,
    pending_webview_commands: HashMap<SharedString, Vec<PlatformWebViewCommand>>,
}

struct MacWebViewDelegateState {
    async_window: AsyncWindowContext,
    message_handler: Option<WebViewMessageHandler>,
    navigation_handler: Option<WebViewNavigationHandler>,
}

struct MacPrintViewState {
    page_size: Size<Pixels>,
    margins: Edges<Pixels>,
    pages: Vec<PlatformPrintPage>,
}

struct MacWebViewHost {
    webview: id,
    controller: id,
    delegate: id,
    state: Box<MacWebViewDelegateState>,
    declared_url: SharedString,
    user_agent: Option<SharedString>,
    storage_key: Option<SharedString>,
    injected_css: Vec<SharedString>,
    injected_javascript: Vec<SharedString>,
    visible: bool,
}

impl MacWindowState {
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

        for webview in webviews {
            let webview_id = webview.id.clone();
            active_ids.insert(webview_id.clone());

            let needs_rebuild = self
                .webviews
                .get(&webview_id)
                .is_some_and(|host| host.storage_key != webview.storage_key);
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
                if let Some(commands) = self.pending_webview_commands.remove(&webview_id) {
                    for command in commands {
                        host.apply_command(command);
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
            self.pending_webview_commands.remove(&webview_id);
        }

        let content_view = unsafe { self.native_window.contentView() };
        let mut previous_view = self.native_view.as_ptr() as id;
        for webview in webviews {
            if let Some(host) = self.webviews.get(&webview.id) {
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

    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) {
        let webview_id = webview_command_id(&command);
        if let Some(host) = self.webviews.get_mut(&webview_id) {
            host.apply_command(command);
        } else {
            self.pending_webview_commands
                .entry(webview_id)
                .or_default()
                .push(command);
        }
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
        let content_view = unsafe { native_window.contentView() };
        let frame = unsafe {
            ns_rect_from_bounds(webview.bounds, px(content_view.bounds().size.height as f32))
        };

        let config: id = unsafe { msg_send![lookup_class(c"WKWebViewConfiguration"), alloc] };
        let config: id = unsafe { msg_send![config, init] };
        let controller: id = unsafe { msg_send![lookup_class(c"WKUserContentController"), alloc] };
        let controller: id = unsafe { msg_send![controller, init] };
        let data_store: id = unsafe {
            if webview.storage_key.is_some() {
                msg_send![lookup_class(c"WKWebsiteDataStore"), defaultDataStore]
            } else {
                msg_send![lookup_class(c"WKWebsiteDataStore"), nonPersistentDataStore]
            }
        };
        let _: () = unsafe { msg_send![config, setWebsiteDataStore: data_store] };
        let _: () = unsafe { msg_send![config, setUserContentController: controller] };

        let mut state = Box::new(MacWebViewDelegateState {
            async_window: webview.async_window.clone(),
            message_handler: webview.message_handler.clone(),
            navigation_handler: webview.navigation_handler.clone(),
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

        let webview_view: id = unsafe { msg_send![lookup_class(c"WKWebView"), alloc] };
        let webview_view: id =
            unsafe { msg_send![webview_view, initWithFrame: frame, configuration: config] };
        let _: () = unsafe { msg_send![webview_view, setNavigationDelegate: delegate] };
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
            webview: unsafe { webview_view.autorelease() },
            controller: unsafe { controller.autorelease() },
            delegate: unsafe { delegate.autorelease() },
            state,
            declared_url: SharedString::default(),
            user_agent: None,
            storage_key: webview.storage_key.clone(),
            injected_css: Vec::new(),
            injected_javascript: Vec::new(),
            visible: webview.visible,
        };
        host.sync(webview, native_window, native_view);
        host
    }

    fn sync(&mut self, webview: &PlatformWebView, native_window: id, _native_view: id) {
        self.state.async_window = webview.async_window.clone();
        self.state.message_handler = webview.message_handler.clone();
        self.state.navigation_handler = webview.navigation_handler.clone();

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

        if self.declared_url != webview.url {
            self.load_url(webview.url.as_ref());
            self.declared_url = webview.url.clone();
        } else if (scripts_changed || user_agent_changed) && !self.declared_url.as_ref().is_empty()
        {
            self.reload();
        }
    }

    fn sync_user_scripts(&mut self, webview: &PlatformWebView) -> bool {
        if self.storage_key == webview.storage_key
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
                &webview_bridge_script(webview.storage_key.as_ref()),
                WKUserScriptInjectionTimeAtDocumentStart,
            );
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
        self.injected_css = webview.injected_css.clone();
        self.injected_javascript = webview.injected_javascript.clone();
        true
    }

    fn apply_command(&mut self, command: PlatformWebViewCommand) {
        match command {
            PlatformWebViewCommand::Navigate { url, .. } => self.load_url(url.as_ref()),
            PlatformWebViewCommand::EvaluateJavaScript { script, .. } => {
                self.evaluate_javascript(script.as_ref())
            }
            PlatformWebViewCommand::PostMessage { message, .. } => {
                let payload = serde_json::to_string(&message).unwrap_or_else(|_| "null".into());
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
        }
    }

    fn load_url(&mut self, url: &str) {
        if url.is_empty() {
            return;
        }

        unsafe {
            let url: id = msg_send![lookup_class(c"NSURL"), URLWithString: ns_string(url)];
            if url.is_null() {
                return;
            }
            let request: id = msg_send![lookup_class(c"NSURLRequest"), requestWithURL: url];
            let _: () = msg_send![self.webview, loadRequest: request];
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

    fn reload(&mut self) {
        unsafe {
            let _: () = msg_send![self.webview, reload];
        }
    }
}

impl Drop for MacWebViewHost {
    fn drop(&mut self) {
        unsafe {
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
                let _: () = msg_send![self.webview, setNavigationDelegate: nil];
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
    ) -> Self {
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
                WindowKind::Normal | WindowKind::Floating => msg_send![WINDOW_CLASS, alloc],
                WindowKind::PopUp | WindowKind::Overlay => {
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
            assert!(!native_window.is_null());
            let dragged_types: id = msg_send![
                lookup_class(c"NSArray"),
                arrayWithObject: NSPasteboardTypeFileURL
            ];
            let () = msg_send![
                native_window,
                registerForDraggedTypes: dragged_types
            ];
            let () = msg_send![
                native_window,
                setReleasedWhenClosed: NO
            ];

            let content_view = native_window.contentView();
            let native_view: id = msg_send![VIEW_CLASS, alloc];
            let native_view = native_view.initWithFrame_(content_view.bounds());
            assert!(!native_view.is_null());

            let mut window = Self(Arc::new(Mutex::new(MacWindowState {
                handle,
                executor,
                native_window,
                native_view: NonNull::new_unchecked(native_view),
                blurred_view: None,
                display_link: None,
                frame_polling_active: false,
                renderer: renderer::new_renderer(
                    renderer_context,
                    native_window as *mut _,
                    native_view as *mut _,
                    bounds.size.map(|pixels| pixels.0),
                    false,
                ),
                request_frame_callback: None,
                event_callback: None,
                activate_callback: None,
                resize_callback: None,
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
                accessibility_provider: super::accessibility::MacAccessibilityProvider::new(
                    "Kael",
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
                WindowKind::Normal | WindowKind::Floating => {
                    native_window.setLevel_(NSNormalWindowLevel);
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
                WindowKind::Overlay => {
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

            // Set the initial position of the window to the specified origin.
            // Although we already specified the position using `initWithContentRect_styleMask_backing_defer_screen_`,
            // the window position might be incorrect if the main screen (the screen that contains the window that has focus)
            //  is different from the primary screen.
            let _: () = msg_send![native_window, setFrameTopLeftPoint: window_rect.origin];
            window.0.lock().move_traffic_light();

            window
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
                let value_ptr: *const i8 = msg_send![value, UTF8String];
                CStr::from_ptr(value_ptr).to_string_lossy()
            } else {
                "".into()
            };

            match value_str.as_ref() {
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

            for (ix, answer) in answers
                .iter()
                .enumerate()
                .filter(|&(ix, _)| Some(ix) != latest_non_cancel_label.map(|(ix, _)| ix))
            {
                let button: id = msg_send![alert, addButtonWithTitle: ns_string(answer.label())];
                let _: () = msg_send![button, setTag: ix as NSInteger];

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
            }

            let (done_tx, done_rx) = oneshot::channel();
            let done_tx = Cell::new(Some(done_tx));
            let block = RcBlock::new(move |answer: NSInteger| {
                if let Some(done_tx) = done_tx.take() {
                    let _ = done_tx.send(answer.try_into().unwrap());
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

    fn set_frame_polling(&self, active: bool) {
        let mut this = self.0.as_ref().lock();
        let was_active = this.frame_polling_active;
        this.frame_polling_active = active;
        if active && !was_active {
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
        self.0.lock().sync_webviews(webviews);
    }

    fn dispatch_webview_command(&mut self, command: PlatformWebViewCommand) -> anyhow::Result<()> {
        self.0.lock().dispatch_webview_command(command);
        Ok(())
    }

    fn print(&mut self, job: PlatformPrintJob) -> anyhow::Result<()> {
        let native_window = self.0.lock().native_window;
        unsafe { run_print_job(native_window, job, false) }
    }

    fn show_print_dialog(&mut self, job: PlatformPrintJob) -> anyhow::Result<()> {
        let native_window = self.0.lock().native_window;
        unsafe { run_print_job(native_window, job, true) }
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

    fn titlebar_double_click(&self) {
        let window = self.0.lock().native_window;
        perform_titlebar_double_click_action(window)
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

    fn update_accessibility_tree(&mut self, tree: &crate::AccessibilityTree) {
        let mut this = self.0.lock();
        this.accessibility_provider.update_tree(tree);
        let actions = this.accessibility_provider.drain_actions();
        drop(this);
        for (target, action) in actions {
            log::debug!(
                "AccessKit action request: {:?} on node {}",
                action,
                target.0
            );
        }
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

extern "C" fn webview_did_receive_script_message(this: id, _: Sel, _: id, message: id) {
    let Some(state) = (unsafe { get_webview_delegate_state(this) }) else {
        return;
    };
    let Some(handler) = state.message_handler.clone() else {
        return;
    };

    let body: id = unsafe { msg_send![message, body] };
    let payload = unsafe { webview_message_value(body) };
    let mut async_window = state.async_window.clone();
    let _ = async_window.update(|window, cx| {
        handler(payload, window, cx);
    });
}

extern "C" fn webview_decide_policy_for_navigation_action(
    this: id,
    _: Sel,
    _: id,
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
    let url: id = unsafe { msg_send![request, URL] };
    let url_string = if url.is_null() {
        String::new()
    } else {
        let absolute_string: id = unsafe { msg_send![url, absoluteString] };
        if absolute_string.is_null() {
            String::new()
        } else {
            unsafe { absolute_string.to_str().to_string() }
        }
    };
    let target_frame: id = unsafe { msg_send![navigation_action, targetFrame] };

    let default_policy = if target_frame.is_null() {
        NavigationPolicy::Deny
    } else {
        NavigationPolicy::Allow
    };

    let policy = if let Some(handler) = state.navigation_handler.clone() {
        let mut async_window = state.async_window.clone();
        async_window
            .update(|window, cx| handler(url_string.clone().into(), window, cx))
            .unwrap_or(default_policy)
    } else {
        default_policy
    };

    unsafe {
        call_navigation_decision_handler(
            decision_handler,
            if policy == NavigationPolicy::Allow && !target_frame.is_null() {
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
            Bool::new(!callback(event).propagate)
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
            callback(event);
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
    window_state.as_ref().lock().move_traffic_light();
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
        callback();
        window_state.lock().moved_callback = Some(callback);
    }
}

extern "C" fn window_did_change_screen(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    lock.start_display_link();
}

extern "C" fn window_did_change_key_status(this: id, selector: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.lock();
    let is_active = unsafe { lock.native_window.isKeyWindow() == YES };
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
                callback(Default::default());

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
                callback(is_active);
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
        let should_close = callback();
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
            callback();
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
        callback(content_size, scale_factor);
        window_state.as_ref().lock().resize_callback = Some(callback);
    };
}

extern "C" fn set_frame_size(this: id, _: Sel, size: NSSize) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();

    let new_size = Size::<Pixels>::from(size);
    let old_size = unsafe {
        let old_frame: NSRect = msg_send![this, frame];
        Size::<Pixels>::from(old_frame.size)
    };

    if old_size == new_size {
        return;
    }

    unsafe {
        let _: () = msg_send![super(this, lookup_class(c"NSView")), setFrameSize: size];
    }

    let scale_factor = lock.scale_factor();
    let drawable_size = new_size.to_device_pixels(scale_factor);
    lock.renderer.update_drawable_size(drawable_size);

    if let Some(mut callback) = lock.resize_callback.take() {
        let content_size = lock.content_size();
        let scale_factor = lock.scale_factor();
        drop(lock);
        callback(content_size, scale_factor);
        window_state.lock().resize_callback = Some(callback);
    };
}

extern "C" fn display_layer(this: id, _: Sel, _: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.lock();
    if let Some(mut callback) = lock.request_frame_callback.take() {
        #[cfg(not(feature = "macos-blade"))]
        lock.renderer.set_presents_with_transaction(true);
        lock.stop_display_link();
        drop(lock);
        callback(Default::default());

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
        callback(Default::default());
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
        let handled = (callback)(PlatformInput::KeyDown(KeyDownEvent {
            keystroke,
            is_held: false,
        }));
        state.as_ref().lock().do_command_handled = Some(!handled.propagate);
    }

    state.as_ref().lock().event_callback = event_callback;
}

extern "C" fn view_did_change_effective_appearance(this: id, _: Sel) {
    unsafe {
        let state = get_window_state(this);
        let mut lock = state.as_ref().lock();
        if let Some(mut callback) = lock.appearance_changed_callback.take() {
            drop(lock);
            callback();
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
    let paths = external_paths_from_event(dragging_info);
    if let Some(event) =
        paths.map(|paths| PlatformInput::FileDrop(FileDropEvent::Entered { position, paths }))
        && send_new_event(&window_state, event)
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

fn external_paths_from_event(dragging_info: id) -> Option<ExternalPaths> {
    let mut paths = SmallVec::new();
    let pasteboard: id = unsafe { msg_send![dragging_info, draggingPasteboard] };
    let pasteboard_items: id = unsafe { msg_send![pasteboard, pasteboardItems] };
    if pasteboard_items.is_null() {
        return None;
    }
    let count: NSUInteger = unsafe { msg_send![pasteboard_items, count] };
    for index in 0..count {
        let item: id = unsafe { msg_send![pasteboard_items, objectAtIndex: index] };
        let file_url: id = unsafe { msg_send![item, stringForType: NSPasteboardTypeFileURL] };
        if file_url.is_null() {
            continue;
        }

        let url: id = unsafe { msg_send![lookup_class(c"NSURL"), URLWithString: file_url] };
        if url.is_null() {
            continue;
        }

        let is_file_url: BOOL = unsafe { msg_send![url, isFileURL] };
        if is_file_url != YES {
            continue;
        }

        let path: id = unsafe { msg_send![url, path] };
        if !path.is_null() {
            paths.push(PathBuf::from(unsafe { path.to_str() }.to_string()));
        }
    }
    Some(ExternalPaths(paths))
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
                    callback(PlatformInput::MouseMove(event.clone()));
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
        callback(e);
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
            callback();
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
            callback();
            window_state.lock().merge_all_windows_callback = Some(callback);
        }
    }
}

extern "C" fn select_next_tab(this: id, _sel: Sel, _id: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.select_next_tab_callback.take() {
        drop(lock);
        callback();
        window_state.lock().select_next_tab_callback = Some(callback);
    }
}

extern "C" fn select_previous_tab(this: id, _sel: Sel, _id: id) {
    let window_state = unsafe { get_window_state(this) };
    let mut lock = window_state.as_ref().lock();
    if let Some(mut callback) = lock.select_previous_tab_callback.take() {
        drop(lock);
        callback();
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
            callback();
            window_state.lock().toggle_tab_bar_callback = Some(callback);
        }
    }
}
