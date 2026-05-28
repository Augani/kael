# Remaining cocoa/objc → objc2 Migration

This document specifies the work remaining to complete the macOS FFI migration
from the deprecated `cocoa` / `objc` 0.2 crates to `objc2` 0.6+.

**State at PR #2 head:** 18 of 21 mac source files have been moved off
cocoa/objc 0.2 entirely (status_item.rs deleted as dead code). 3 files still
import the deprecated crates and need to migrate.

| File | Lines | NSObject subclasses | `msg_send!` sites |
|---|---|---|---|
| `crates/kael/src/platform/mac/platform.rs` | ~2,625 | 3 (NSApplication + GPUIApplicationDelegate + GPUINotificationDelegate) | 134 |
| `crates/kael/src/platform/mac/window.rs` | ~3,934 | 5 (GPUIView + BlurredView + GPUIWebViewDelegate + GPUIPrintView + dynamic GPUIWindow/GPUIPanel) | 262 |
| `crates/kael/src/platform/mac/events.rs` | ~557 | 0 — but uses cocoa NSEvent extensively; tightly coupled to window.rs's input pipeline | 1 |

The transitional `#![allow(deprecated)]` at `crates/kael/src/platform/mac.rs:3`
stays in place until **all three** files are migrated; that's the gate.

---

## 1. Established pattern (already proven in PR #2)

Five `define_class!` migrations have already landed in PR #2 (media_capture.rs,
screen_capture.rs × 2 subclasses). Use these as the working template.

### 1.1 Imports

Replace the cocoa/objc 0.2 imports with:

```rust
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString};
use std::ffi::CStr;
```

For block callbacks (AVFoundation / SCStream / completion handlers):

```rust
use block2::RcBlock;
```

`block2` is already a direct dep of `crates/kael` (added in PR #2 for permissions/biometric).

### 1.2 Runtime class/selector lookup (no typed crate needed)

```rust
unsafe fn lookup_class(name: &CStr) -> &'static AnyClass {
    AnyClass::get(name).unwrap_or_else(|| panic!("missing class {name:?}"))
}

let cls = unsafe { lookup_class(c"NSApplication") };
let sel = Sel::register(c"applicationWillFinishLaunching:");
```

This avoids the `MainThreadOnly` requirement of `objc2_app_kit::NSApplication`
that would force ripping through every call site that touches NSApp.

### 1.3 `ClassDecl::new(...) + add_method(...)` → `define_class!`

The canonical migration. For each existing subclass:

```rust
// Before (objc 0.2):
let mut decl = ClassDecl::new("GPUIMyDelegate", class!(NSObject)).unwrap();
decl.add_ivar::<*mut c_void>(STATE_IVAR);
decl.add_method(
    sel!(someMethod:withArg:),
    some_handler as extern "C" fn(&Object, Sel, id, id),
);
SOME_CLASS = decl.register();

// After (objc2 0.6):
#[derive(Default)]
struct GPUIMyDelegateIvars {
    state: std::cell::Cell<*mut c_void>,
    // (use Cell for interior mutability inside &self methods)
}

define_class!(
    #[unsafe(super = NSObject)]
    #[ivars = GPUIMyDelegateIvars]
    #[name = "GPUIMyDelegate"]
    struct GPUIMyDelegate;

    unsafe impl NSObjectProtocol for GPUIMyDelegate {}

    impl GPUIMyDelegate {
        #[unsafe(method(someMethod:withArg:))]
        fn some_handler(&self, arg1: *mut AnyObject, arg2: *mut AnyObject) {
            let state_ptr = self.ivars().state.get();
            // ... body translated from the old extern "C" fn ...
        }
    }
);

impl GPUIMyDelegate {
    fn new(state: *mut c_void) -> Retained<Self> {
        let this = Self::alloc().set_ivars(GPUIMyDelegateIvars {
            state: std::cell::Cell::new(state),
        });
        unsafe { msg_send![super(this), init] }
    }
}
```

Key rules:
- **Receiver was `&Object`** → method now takes `&self` (or `&mut self` if you need it). Body uses `self.ivars().state.get()` instead of `this.get_ivar::<*mut c_void>(STATE_IVAR)`.
- **Each method's selector** stays the same — put it in `#[unsafe(method(selectorWithColons:))]`.
- **Params**: `id` → `*mut AnyObject`. `BOOL` → `Bool` (and use `.as_bool()` to read; `Bool::YES`/`Bool::NO` to write). Return-types: replace `id` with `*mut AnyObject`, `BOOL` with `Bool`.
- **`Self::alloc()`** requires `use objc2::AnyThread;`.
- **`unsafe impl NSObjectProtocol for Foo {}`** is mandatory (empty impl).

### 1.4 `msg_send!` site translation (the bulk of the work)

The objc 0.2 macro:
```rust
let x: id = unsafe { msg_send![receiver, methodName: arg] };
```

becomes:
```rust
let x: *mut AnyObject = unsafe { msg_send![receiver, methodName: arg] };
```

with the imports swapped to `use objc2::msg_send;`. The macro **call syntax**
is the same. What changes is:

- **Receivers** that were `id` (= `*mut Object`) → `*mut AnyObject`.
- **`class!(Foo)`** → `unsafe { lookup_class(c"Foo") }` (see 1.2). Class refs are
  `&'static AnyClass` and msg_send accepts that as receiver.
- **`sel!(name:)`** when passed as an argument → `Sel::register(c"name:")`.
  When used only in `decl.add_method(sel!(...), ...)`, it goes into the
  `#[unsafe(method(...))]` attribute instead.
- **Arguments** that are typed cocoa structs (NSSize, NSRect, NSRange) need
  to be the **objc2_foundation** versions (`objc2_foundation::NSSize`,
  `NSRect`, `NSRange::new(...)`). The cocoa::* versions implement the OLD
  `objc::Encode` trait, not `objc2::Encode`. Build a fresh value at the
  msg_send boundary:
  ```rust
  let range = objc2_foundation::NSRange::new(loc as usize, len as usize);
  let _: *mut AnyObject = msg_send![obj, doStuffWithRange: range];
  ```
- **Dispatch queues** (`*mut dispatch_queue_s`) don't impl `objc2::Encode` —
  cast to `*mut c_void` at the msg_send boundary:
  ```rust
  let queue = dispatch_get_global_queue(...);
  let queue_obj = queue as *mut c_void;
  let _: () = msg_send![out, setSampleBufferDelegate: &*delegate, queue: queue_obj];
  ```
- **CoreFoundation pointers** (e.g. `CGColorSpaceRef`): cast via
  `as_ptr() as *mut std::ffi::c_void` (use `ForeignType::as_ptr`, NOT
  `TCFType::as_concrete_TypeRef`).

### 1.5 `block::ConcreteBlock` → `block2::RcBlock`

```rust
// Before:
let handler = block::ConcreteBlock::new(move |arg: BOOL| {
    // body uses cocoa-style BOOL
});
let handler = handler.copy();

// After:
let handler = block2::RcBlock::new(move |arg: Bool| {
    // body uses .as_bool() to read
});
// No .copy() needed — RcBlock is already heap-allocated.
```

Passing the block to msg_send: `&*handler` (deref to `Block`).

If the block closure captures state that needs once-call semantics with
thread-safe extraction, the pattern is `Arc<AtomicPtr<c_void>>` (lifted
verbatim from the old code — see `permissions.rs:request_media_permission`
in PR #2 for the canonical example).

### 1.6 Common boilerplate

```rust
unsafe fn ns_string_to_str(s: *mut AnyObject) -> String {
    if s.is_null() {
        return String::new();
    }
    let ns: &NSString = unsafe { &*(s as *const NSString) };
    ns.to_string()
}

unsafe fn release_obj(object: *mut AnyObject) {
    if !object.is_null() {
        unsafe {
            let _: () = msg_send![object, release];
        }
    }
}
```

For NSString construction, use `NSString::from_str("…")` and pass `&*ns` to
`msg_send!`.

### 1.7 Reference memory file

See `~/.claude/projects/-Users-augustusotu-Projects-kael/memory/kael-objc2-define-class-pattern.md`
for the same template captured during PR #2.

---

## 2. `platform.rs` — 3 subclasses

### 2.1 Imports to drop

```rust
use block::ConcreteBlock;                          // drop, use block2::RcBlock
use cocoa::{
    appkit::{                                      // drop entire cocoa::appkit
        NSApplication, NSApplicationActivationPolicy::*, NSEventModifierFlags,
        NSMenu, NSMenuItem, NSModalResponse, NSOpenPanel, NSPasteboard,
        NSPasteboardTypePNG, NSPasteboardTypeRTF, NSPasteboardTypeRTFD,
        NSPasteboardTypeString, NSPasteboardTypeTIFF, NSSavePanel, NSWindow,
    },
    base::{BOOL, NO, YES, id, nil, selector},      // drop
    foundation::{                                   // drop
        NSArray, NSAutoreleasePool, NSBundle, NSData, NSInteger, NSProcessInfo,
        NSRange, NSSize, NSString, NSUInteger, NSURL,
    },
};
use objc::{                                        // drop
    class, declare::ClassDecl, msg_send,
    runtime::{Class, Object, Sel},
    sel, sel_impl,
};
```

Pasteboard-type constants (`NSPasteboardTypePNG`, etc.) need replacements:
either look them up at runtime via `objc2::runtime::AnyClass::get` for the
class that exposes them, or declare them via `#[link(name = "AppKit", ...)] extern "C" { static NSPasteboardTypeXxx: *mut AnyObject; }`.

### 2.2 Three ClassDecls → three `define_class!` blocks

Located in `build_classes()` (currently at lines ~75–203).

#### A. `GPUIApplication` (NSApplication subclass)

```rust
struct GPUIApplicationIvars {
    platform: std::cell::Cell<*mut c_void>,
}

define_class!(
    #[unsafe(super = NSObject)]  // NSApplication isn't typed via objc2-app-kit
                                  // without MainThreadOnly; using NSObject + runtime
                                  // msg_send for the parent methods keeps it free.
    // Actually for subclassing NSApplication, you MUST use the real superclass.
    // Either use objc2_app_kit::NSApplication (requires MainThreadOnly) or
    // declare a one-line extern_class for NSApplication. See [§ 2.2 special note].
    #[ivars = GPUIApplicationIvars]
    #[name = "GPUIApplication"]
    struct GPUIApplication;

    unsafe impl NSObjectProtocol for GPUIApplication {}

    impl GPUIApplication { /* no extra methods — currently has none */ }
);
```

**Special note for `GPUIApplication`**: the existing ClassDecl subclasses
`class!(NSApplication)` and adds **no methods** — it only has the `platform`
ivar. The simplest objc2 path is `#[unsafe(super = NSApplication)]` via
`objc2_app_kit::NSApplication`, with `#[thread_kind = MainThreadOnly]`. That
adds an `MainThreadMarker` requirement at construction sites — verify the
single call site (search for `APP_CLASS` usage) is on the main thread before
deciding.

#### B. `GPUIApplicationDelegate` (NSResponder subclass)

22 methods — list them in `define_class!`'s `impl` block. Each becomes
`#[unsafe(method(<selector>:))] fn <rust_name>(&self, ...args...) -> Ret`.

| Selector | Rust name (current `extern "C" fn`) | Signature (args after `&self`) | Returns |
|---|---|---|---|
| `applicationWillFinishLaunching:` | `will_finish_launching` | `_notification: *mut AnyObject` | `()` |
| `applicationDidFinishLaunching:` | `did_finish_launching` | `_notification: *mut AnyObject` | `()` |
| `applicationShouldHandleReopen:hasVisibleWindows:` | `should_handle_reopen` | `_app: *mut AnyObject, has_open_windows: Bool` | `()` |
| `applicationWillTerminate:` | `will_terminate` | `_notification: *mut AnyObject` | `()` |
| `handleGPUIMenuItem:` | `handle_menu_item` | `item: *mut AnyObject` | `()` |
| `handleTrayMenuItem:` | `handle_tray_menu_item` | `item: *mut AnyObject` | `()` |
| `handleTrayPanelClick:` | `handle_tray_panel_click` | `_sender: *mut AnyObject` | `()` |
| `cut:` | `handle_menu_item` (shared) | `sender: *mut AnyObject` | `()` |
| `copy:` | `handle_menu_item` (shared) | `sender: *mut AnyObject` | `()` |
| `paste:` | `handle_menu_item` (shared) | `sender: *mut AnyObject` | `()` |
| `selectAll:` | `handle_menu_item` (shared) | `sender: *mut AnyObject` | `()` |
| `undo:` | `handle_menu_item` (shared) | `sender: *mut AnyObject` | `()` |
| `redo:` | `handle_menu_item` (shared) | `sender: *mut AnyObject` | `()` |
| `validateMenuItem:` | `validate_menu_item` | `item: *mut AnyObject` | `Bool` |
| `menuWillOpen:` | `menu_will_open` | `_menu: *mut AnyObject` | `()` |
| `applicationDockMenu:` | `handle_dock_menu` | `_app: *mut AnyObject` | `*mut AnyObject` |
| `application:openURLs:` | `open_urls` | `_app: *mut AnyObject, urls: *mut AnyObject` | `()` |
| `onKeyboardLayoutChange:` | `on_keyboard_layout_change` | `_notification: *mut AnyObject` | `()` |
| `applicationShouldTerminateAfterLastWindowClosed:` | `should_terminate_after_last_window_closed` | `_app: *mut AnyObject` | `Bool` |
| `handleSystemPowerEvent:` | `handle_system_power_event` | `notification: *mut AnyObject` | `()` |
| `handleContextMenuItem:` | `handle_context_menu_item` | `item: *mut AnyObject` | `()` |

Each method body is the body of the existing `extern "C" fn` at lines
~2051–2381, with:
- `this.get_ivar::<*mut c_void>(MAC_PLATFORM_IVAR)` → `self.ivars().platform.get()`.
- All `msg_send!` calls inside the body translated per § 1.4.
- `BOOL`-returning methods: convert `bool`→`Bool` via `Bool::new(v)`.
- The 6 menu-item handlers (`cut:`/`copy:`/`paste:`/`selectAll:`/`undo:`/`redo:`)
  all map to the same Rust function — see how objc2's `define_class!` allows
  multiple `#[unsafe(method(...))]` attributes on the same Rust fn (or, more
  cleanly, declare each as its own one-liner that delegates).

For the shared `handle_menu_item` selectors, the cleanest objc2 idiom is:
```rust
#[unsafe(method(handleGPUIMenuItem:))]
fn handle_menu_item(&self, item: *mut AnyObject) { /* body */ }
#[unsafe(method(cut:))]
fn cut(&self, item: *mut AnyObject) { unsafe { self.handle_menu_item(item) } }
#[unsafe(method(copy:))]
fn copy(&self, item: *mut AnyObject) { unsafe { self.handle_menu_item(item) } }
// … same for paste:/selectAll:/undo:/redo:
```

#### C. `GPUINotificationDelegate` (NSObject subclass)

2 methods. Same pattern.

| Selector | Rust name | Signature (args after `&self`) |
|---|---|---|
| `userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:` | `handle_notification_response` | `_center: *mut AnyObject, response: *mut AnyObject, completion_handler: *mut AnyObject` |
| `userNotificationCenter:willPresentNotification:withCompletionHandler:` | `handle_will_present_notification` | `_center: *mut AnyObject, notification: *mut AnyObject, completion_handler: *mut AnyObject` |

The completion-handler argument is itself a block. Inside the method body
it's invoked via `msg_send![completion_handler, invoke]` (with the right args
per the protocol) — keep the current invocation untouched, just pass
`completion_handler` as `*mut AnyObject`.

### 2.3 msg_send sites (134 across the file)

Mostly mechanical. Common ones in this file:

- `msg_send![class!(NSApplication), sharedApplication]` → `msg_send![lookup_class(c"NSApplication"), sharedApplication]`.
- `msg_send![pasteboard, generalPasteboard]`, `setString:forType:`, `dataForType:`, etc. — all just receiver/method translations.
- `NSAutoreleasePool::new(nil).drain()` patterns — objc2 uses `autoreleasepool(|pool| { ... })` from `objc2::rc`. This is the **biggest behavioral substitution** in platform.rs; review each autorelease-pool use individually.
- `NSOpenPanel`/`NSSavePanel` runModal + URL extraction — straightforward once you have `lookup_class(c"NSOpenPanel")`.
- The Carbon `RegisterEventHotKey` extern stays unchanged (it's C, not Obj-C).

### 2.4 Type-level state migration

`MacPlatformState`'s `id` fields (lines ~213–245):

```rust
pasteboard: id,                                    // → *mut AnyObject
text_hash_pasteboard_type: id,                     // → *mut AnyObject (NSString really)
metadata_pasteboard_type: id,                      // → *mut AnyObject (NSString really)
dock_menu: Option<id>,                             // → Option<*mut AnyObject>
global_hotkey_monitors: Vec<id>,                   // → Vec<*mut AnyObject>
media_key_monitor: Option<id>,                     // → Option<*mut AnyObject>
notification_delegate: Option<id>,                 // → Option<Retained<GPUINotificationDelegate>>
```

Everywhere these fields are stored from a `msg_send![...]` result, the
return-type annotation changes from `: id` to `: *mut AnyObject`.

### 2.5 ConcreteBlock sites

Find each `ConcreteBlock::new(move |...| { ... })` and replace with
`RcBlock::new(move |...| { ... })`. Drop the `.copy()` call (RcBlock is
already heap-allocated). Pass to msg_send as `&*block`.

---

## 3. `window.rs` — 5 subclasses (the giant)

### 3.1 ClassDecls and their selectors

| # | ClassDecl line | Class name | Super | Ivar | # methods |
|---|---|---|---|---|---|
| A | 160 | `GPUIView` | `NSView` | `WINDOW_STATE_IVAR` | ~24 |
| B | 309 | `BlurredView` | `NSVisualEffectView` | (none) | 2 |
| C | 323 | `GPUIWebViewDelegate` | `NSObject` | `WEBVIEW_STATE_IVAR` | 3 + 2 protocols |
| D | 349 | `GPUIPrintView` | `NSView` | `PRINT_VIEW_STATE_IVAR` | 5 |
| E | 901 (dynamic) | `GPUIWindow` or `GPUIPanel` (name varies) | `NSWindow` or `NSPanel` | `WINDOW_STATE_IVAR` | ~20 |

### 3.2 GPUIView selectors (the input-handling view)

Read the source at lines 160–305 for the exact list; each entry follows the
same `#[unsafe(method(sel))]` template. Key categories:

- **Mouse**: `mouseDown:`, `mouseUp:`, `rightMouseDown:`, `rightMouseUp:`,
  `otherMouseDown:`, `otherMouseUp:`, `mouseMoved:`, `mouseDragged:`,
  `mouseExited:`, `mouseEntered:`, `scrollWheel:`, `magnifyWithEvent:`,
  `flagsChanged:`. **All forward to `handle_view_event` in the current code.**
- **Layer + drawing**: `makeBackingLayer` → `*mut AnyObject`, `displayLayer:`,
  `viewDidChangeBackingProperties`, `setFrameSize:` (param is `NSSize`),
  `viewDidChangeEffectiveAppearance`.
- **Lifecycle**: `dealloc` (calls `drop_state(this)` + super).
- **CALayerDelegate protocol**: just `displayLayer:`.
- **NSTextInputClient protocol** (IME): `validAttributesForMarkedText` (returns `id`),
  `hasMarkedText`, `markedRange`, `selectedRange`, `firstRectForCharacterRange:actualRange:`,
  `insertText:replacementRange:`, `setMarkedText:selectedRange:replacementRange:`,
  `unmarkText`, `attributedSubstringForProposedRange:actualRange:`,
  `doCommandBySelector:` (the second `Sel` arg → `Sel`),
  `acceptsFirstMouse:`.

Bodies live in `extern "C" fn handle_view_event` (line 3003),
`handle_key_event` (line 2886), `make_backing_layer` (line 3311), etc.
Each body uses `get_state(this)` → migrate to `self.ivars().state.get()`.

### 3.3 BlurredView (2 methods)

```rust
#[unsafe(method(viewDidChangeBackingProperties))]
fn view_did_change_backing_properties(&self) { /* body */ }

#[unsafe(method(allowsVibrancy))]
fn allows_vibrancy(&self) -> Bool { /* body */ }
```

### 3.4 GPUIWebViewDelegate

```rust
// Adds protocols WKScriptMessageHandler + WKNavigationDelegate.
#[unsafe(method(userContentController:didReceiveScriptMessage:))]
fn did_receive_script_message(&self, _ctrl: *mut AnyObject, message: *mut AnyObject) { /* body */ }

#[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
fn decide_policy(&self, _wv: *mut AnyObject, action: *mut AnyObject, handler: *mut AnyObject) { /* body */ }

#[unsafe(method(dealloc))]
fn dealloc(&self) { /* drop_state_webview(self) + super */ }
```

For the two `protocol` adds, declare `unsafe impl <Proto> for GPUIWebViewDelegate {}` blocks. Without typed protocols, declare as bare `impl GPUIWebViewDelegate { ... }` and the runtime will register them.

### 3.5 GPUIPrintView (5 methods)

```rust
#[unsafe(method(dealloc))]
fn dealloc(&self) { /* drop_state_print + super */ }

#[unsafe(method(isFlipped))]
fn is_flipped(&self) -> Bool { Bool::YES }

#[unsafe(method(knowsPageRange:))]
fn knows_page_range(&self, range: NSRangePointer) -> Bool { /* body */ }

#[unsafe(method(rectForPage:))]
fn rect_for_page(&self, _page: NSInteger) -> NSRect { /* body — return objc2_foundation::NSRect */ }

#[unsafe(method(drawRect:))]
fn draw_rect(&self, _r: NSRect) { /* body */ }
```

### 3.6 GPUIWindow / GPUIPanel (dynamic ClassDecl at line 901)

This is the big one — the NSWindow/NSPanel subclass with ~20 methods. The
current code conditionally chooses superclass and class name based on
panel-mode at runtime. With objc2's `define_class!` (compile-time), use **two
separate `define_class!` blocks** (one with `super = NSWindow`, one with
`super = NSPanel`) and pick which to instantiate at runtime. Both share the
same ivar and method bodies — use a private trait or shared free fns for the
implementations.

Methods (from lines 901–998):

| Selector | Rust handler |
|---|---|
| `dealloc` | `dealloc_window` |
| `windowDidBecomeKey:` / `windowDidResignKey:` (handled by `window_did_change_key_status`) | shared handler |
| `windowShouldClose:` | `window_should_close` |
| `close` | `close_window` |
| `windowDidResize:` | `window_did_resize` |
| `windowWillEnterFullScreen:` | `window_will_enter_fullscreen` |
| `windowWillExitFullScreen:` | `window_will_exit_fullscreen` |
| `windowDidMove:` | `window_did_move` |
| `windowDidChangeScreen:` | `window_did_change_screen` |
| `windowDidChangeOcclusionState:` | `window_did_change_occlusion_state` |
| `performKeyEquivalent:` | `handle_key_equivalent` (returns `Bool`) |
| `keyDown:` | `handle_key_down` |
| `keyUp:` | `handle_key_up` |
| (more — read the full list at lines 901–998) | |

### 3.7 NSEvent translation across window.rs

Many methods take `id` event params and call methods like `event.eventType()`,
`event.modifierFlags()`, `event.charactersIgnoringModifiers()`, etc. Two
options:

1. **Quick path** (matches what tray.rs/global_hotkey.rs do): keep `*mut AnyObject` and call methods via runtime `msg_send!`:
   ```rust
   let event_type: u64 = msg_send![event, type];
   let flags: u64 = msg_send![event, modifierFlags];
   ```
2. **Typed path**: cast to `&objc2_app_kit::NSEvent` and use the typed methods (`event.r#type()`, `event.modifierFlags()`, etc.). This is what `global_hotkey.rs` does — see PR #2's commit `a52471a` for the pattern.

Use the typed path where convenient — the methods don't require
`MainThreadMarker`, so the cast `&*(event as *const NSEvent)` works.

### 3.8 Volume

~262 msg_send sites + ~30+ extern "C" fn handlers. Plan for several
incremental commits within the window.rs migration, each one a self-contained
chunk of related methods (e.g., one commit for GPUIView mouse handlers, one
for IME methods, one for GPUIWindow delegate methods, one for drawing, one
for the dynamic instantiation logic). Verify with
`cargo clippy -p kael --lib -- -D warnings` after each commit.

---

## 4. `events.rs` — couples to window.rs

This file uses `cocoa::appkit::{NSEvent, NSEventModifierFlags, NSEventType, NSEventPhase}` and `cocoa::base::{YES, id}`. It has only **1** remaining `msg_send!` site (a leftover `release` cast added during PR #2's TIS-extern bridge — `msg_send![keyboard as *mut Object, release]` at two sites).

The migration is **largely mechanical**:

1. Replace cocoa imports with `objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSEventPhase}`.
2. **Variant rename table** (cocoa::appkit::NSEventType uses the old `NS<X>` prefix; objc2_app_kit drops the `NS` prefix):
   - `NSKeyDown` → `NSEventType::KeyDown`
   - `NSKeyUp` → `NSEventType::KeyUp`
   - `NSFlagsChanged` → `NSEventType::FlagsChanged`
   - `NSLeftMouseDown` → `NSEventType::LeftMouseDown`
   - `NSRightMouseDown` → `NSEventType::RightMouseDown`
   - `NSOtherMouseDown` → `NSEventType::OtherMouseDown`
   - `NSLeftMouseUp` → `NSEventType::LeftMouseUp`
   - `NSRightMouseUp` → `NSEventType::RightMouseUp`
   - `NSOtherMouseUp` → `NSEventType::OtherMouseUp`
   - `NSScrollWheel` → `NSEventType::ScrollWheel`
   - `NSMouseMoved` → `NSEventType::MouseMoved`
   - `NSLeftMouseDragged` → `NSEventType::LeftMouseDragged`
   - `NSMagnify` → `NSEventType::Magnify`
   - (full list — check `objc2_app_kit::NSEventType` in
     `~/.cargo/registry/src/index.crates.io-*/objc2-app-kit-0.3.2/src/generated/NSEvent.rs`)
3. **Modifier flag rename table**:
   - `NSCommandKeyMask` → `NSEventModifierFlags::Command`
   - `NSControlKeyMask` → `NSEventModifierFlags::Control`
   - `NSAlternateKeyMask` → `NSEventModifierFlags::Option`
   - `NSShiftKeyMask` → `NSEventModifierFlags::Shift`
   - `NSAlphaShiftKeyMask` → `NSEventModifierFlags::CapsLock`
   - `NSFunctionKeyMask` → `NSEventModifierFlags::Function`
4. **Method names**: `event.eventType()` → `event.r#type()`. `event.isARepeat()` returns `Bool` now. `event.buttonNumber()`, `event.locationInWindow()`, `event.clickCount()`, `event.modifierFlags()` all exist on objc2's typed NSEvent.
5. The two `msg_send![keyboard as *mut Object, release]` casts can become `msg_send![keyboard as *mut AnyObject, release]` once the file no longer imports `objc::runtime::Object`.
6. The `YES` → `Bool::YES.as_bool()` or just direct comparison. `native_event.isARepeat() == YES` → `native_event.isARepeat().as_bool()`.
7. Function signatures: `native_event: id` → either keep `id` if window.rs hasn't migrated yet, or change to `event: &NSEvent` once window.rs is done.

The `cocoa::appkit::*` glob at line 29 (`use cocoa::appkit::*;` inside
`key_to_native`) and line 323 each pull a long list of `NS<X>FunctionKey`
constants for keyboard handling — replace those with the corresponding
`objc2_app_kit::NSF1FunctionKey` etc.

---

## 5. After all three files migrate

Once `platform.rs`, `window.rs`, and `events.rs` all build with no
`use objc::` / `use cocoa::` references:

```bash
# 1. Remove the transitional umbrella allow
# In crates/kael/src/platform/mac.rs, delete line 3:
#     #![allow(deprecated)]

# 2. Optional: also drop the cocoa/objc/block crate dependencies from
#    crates/kael/Cargo.toml if nothing else uses them. ctor stays (still used).

# 3. Local verification gate:
cargo clippy -p kael --lib -- -D warnings
cargo test -p kael --lib smoke_tests::window_handles_do_not_panic
cargo test -p kael --lib    # full lib test suite

# 4. Push to refactor/objc2-migration-complete and let CI run.
```

The macOS app should be **manually exercised** between major commits during
this work — clippy will not catch behavioral regressions in NSWindow / NSView
/ NSApplication delegate semantics. Key things to test on macOS:
- App launch + menu commands.
- Window resizing, fullscreen toggle, minimize/zoom.
- Click and key input in a window, IME compose (Chinese/Japanese),
  Cmd+C/V/Z, scroll wheel, trackpad pinch.
- Tray menu and tray panel click.
- Open/save dialogs.
- URL opening (`open kael://something` from Terminal).
- System dark/light mode switch.

---

## 6. Reference: completed PR #2 commits as worked examples

| Commit | Pattern demonstrated |
|---|---|
| `313b963` (media_capture.rs) | First `define_class!` migration; ivar with `Cell<*mut c_void>`; AVFoundation runtime msg_send |
| `13a139a` (screen_capture.rs) | **Two** `define_class!` in one file; `block2::RcBlock`; SCStream protocol methods |
| `d905b85` (tray.rs) | Pure runtime msg_send migration (no `define_class!`); `Sel::register(c"…")`; manual retain/release |
| `24a3af5` (permissions.rs) | `block2::RcBlock` with `Arc<AtomicPtr>` once-call guard |
| `a52471a` (global_hotkey.rs) | Casting `id` → `&NSEvent` for typed NSEvent method calls |
| `6c0df81` (metal_renderer.rs) | Handling cocoa types incompatible with objc2::Encode (NSSize/CGColorSpaceRef) via `*mut c_void` casts |

Each commit on `refactor/objc2-migration-complete` is small and reviewable;
follow the same style for the remaining files.
