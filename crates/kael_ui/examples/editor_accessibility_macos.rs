//! Opt-in native protocol driver for the real Editor window. No synthetic
//! accessibility nodes or direct Editor mutations are used to perform actions.

use super::{ORIGINAL, REPLACEMENT, TextFixture};
use kael::{App, AsyncApp, Context, Focusable, Timer, Window, WindowHandle};
use objc2::{msg_send, msg_send_id, rc::Id, runtime::ProtocolObject, sel};
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeString, NSPasteboardWriting};
use objc2_foundation::{
    NSArray, NSData, NSInteger, NSNotFound, NSObject, NSPoint, NSRange, NSRect, NSString,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, String>;
type Handle = WindowHandle<TextFixture>;

pub(super) fn start(window: Handle, cx: &mut App) {
    cx.spawn(async move |cx| {
        if let Err(error) = run(window, cx).await {
            eprintln!("NATIVE_TEXT_ACCESSIBILITY_FAILURE platform=macos: {error}");
            std::process::exit(1);
        }
    })
    .detach();
}

fn require(condition: bool, message: &str) -> Result<()> {
    condition.then_some(()).ok_or_else(|| message.into())
}

// The Window owns the AppKit view throughout these calls. Retention allows a
// native object to survive between protocol operations while the main-thread
// task yields; it neither obtains system AX trust nor accesses another process.
fn native_view(window: &Window) -> Result<Id<NSObject>> {
    let raw = HasWindowHandle::window_handle(window)
        .map_err(|error| error.to_string())?
        .as_raw();
    let RawWindowHandle::AppKit(raw) = raw else {
        return Err("owned Window did not provide an AppKit NSView".into());
    };
    unsafe { Id::retain(raw.ns_view.as_ptr().cast::<NSObject>()) }
        .ok_or_else(|| "owned AppKit view is null".into())
}

fn named(window: &Window, title: &str) -> Result<Id<NSObject>> {
    let mut queue = vec![native_view(window)?];
    let mut visited = 0;
    while let Some(node) = queue.pop() {
        visited += 1;
        require(
            visited <= 512,
            "native Editor hierarchy exceeded its layout bound",
        )?;
        let name: Option<Id<NSString>> = unsafe { msg_send_id![&*node, accessibilityTitle] };
        if name.is_some_and(|name| name.to_string() == title) {
            // AccessKit may derive an ancestor's title from a named child.
            // Select the actual text/button object, not that inherited label.
            let selector = if title.ends_with("Unicode document") {
                sel!(accessibilityNumberOfCharacters)
            } else {
                sel!(accessibilityPerformPress)
            };
            let capable: bool =
                unsafe { msg_send![&*node, isAccessibilitySelectorAllowed: selector] };
            if capable {
                return Ok(node);
            }
        }
        let children: Option<Id<NSArray<NSObject>>> =
            unsafe { msg_send_id![&*node, accessibilityChildren] };
        if let Some(children) = children {
            require(
                children.len() <= 512,
                "native Editor child count exceeded its bound",
            )?;
            for index in 0..children.len() {
                let child: Id<NSObject> = unsafe { msg_send_id![&*children, objectAtIndex: index] };
                queue.push(child);
            }
        }
    }
    Err(format!("native object not ready: {title}"))
}

fn value(node: &NSObject) -> Result<String> {
    let value: Option<Id<NSString>> = unsafe { msg_send_id![node, accessibilityValue] };
    value
        .map(|value| value.to_string())
        .ok_or_else(|| "native full text is unavailable".into())
}

fn press(window: &Window, title: &str) -> Result<()> {
    let button = named(window, title)?;
    let accepted: bool = unsafe { msg_send![&*button, accessibilityPerformPress] };
    require(
        accepted,
        &format!("native button action was rejected: {title}"),
    )
}

fn selection(node: &NSObject) -> NSRange {
    unsafe { msg_send![node, accessibilitySelectedTextRange] }
}

fn set_selection(node: &NSObject, range: NSRange) {
    let _: () = unsafe { msg_send![node, setAccessibilitySelectedTextRange: range] };
}

fn replace_selected(node: &NSObject, text: &str) {
    let text = NSString::from_str(text);
    let _: () = unsafe { msg_send![node, setAccessibilitySelectedText: &*text] };
}

fn text_action_names(node: &NSObject) -> Vec<String> {
    let names: Id<NSArray<NSString>> = unsafe { msg_send_id![node, accessibilityActionNames] };
    (0..names.len())
        .map(|index| {
            let name: Id<NSString> = unsafe { msg_send_id![&*names, objectAtIndex: index] };
            name.to_string()
        })
        .collect()
}

fn perform_text_action(node: &NSObject, action: &str) {
    let action = NSString::from_str(action);
    let _: () = unsafe { msg_send![node, accessibilityPerformAction: &*action] };
}

fn text_action(node: &NSObject, action: &str) -> Result<()> {
    require(
        text_action_names(node).iter().any(|name| name == action),
        "native clipboard action is not advertised",
    )?;
    perform_text_action(node, action);
    Ok(())
}

// Data providers may promise representations lazily. Materialize every item
// and type before changing the general pasteboard, retaining no owner objects.
// Oversized or unavailable representations fail before the first test write.
// Prior content and type names never enter diagnostic messages.
struct PasteboardGuard {
    pasteboard: Id<NSPasteboard>,
    items: Vec<Id<NSPasteboardItem>>,
    owned_count: NSInteger,
    changed: bool,
}

impl PasteboardGuard {
    fn preserve() -> Result<Self> {
        let pasteboard = unsafe { NSPasteboard::generalPasteboard() };
        let original_count = unsafe { pasteboard.changeCount() };
        let original_items = unsafe { pasteboard.pasteboardItems() };
        let count = original_items.as_ref().map_or(0, |items| items.len());
        require(count <= 1024, "pasteboard preservation item limit exceeded")?;
        if count == 0 {
            let types = unsafe { pasteboard.types() };
            require(
                types.is_none_or(|types| types.is_empty()),
                "pasteboard representations cannot be fully preserved",
            )?;
        }
        let mut items = Vec::with_capacity(count);
        let mut total_bytes = 0usize;
        let mut total_types = 0usize;
        if let Some(original_items) = original_items {
            for index in 0..count {
                let original: Id<NSPasteboardItem> =
                    unsafe { msg_send_id![&*original_items, objectAtIndex: index] };
                let types = unsafe { original.types() };
                total_types = total_types
                    .checked_add(types.len())
                    .ok_or("pasteboard preservation representation count overflow")?;
                require(
                    total_types <= 4096,
                    "pasteboard preservation representation limit exceeded",
                )?;
                let item = unsafe { NSPasteboardItem::new() };
                for type_index in 0..types.len() {
                    let kind: Id<NSString> =
                        unsafe { msg_send_id![&*types, objectAtIndex: type_index] };
                    let data = unsafe { original.dataForType(&kind) }
                        .ok_or("pasteboard representation cannot be materialized")?;
                    total_bytes = total_bytes
                        .checked_add(data.len())
                        .ok_or("pasteboard preservation byte count overflow")?;
                    require(
                        total_bytes <= 64 * 1024 * 1024,
                        "pasteboard preservation byte limit exceeded",
                    )?;
                    let independent_data = NSData::with_bytes(data.bytes());
                    require(
                        unsafe { item.setData_forType(&independent_data, &kind) },
                        "pasteboard representation cannot be preserved",
                    )?;
                }
                items.push(item);
            }
        }
        require(
            unsafe { pasteboard.changeCount() } == original_count,
            "pasteboard changed during preservation; test made no clipboard write",
        )?;
        Ok(Self {
            pasteboard,
            items,
            owned_count: original_count,
            changed: false,
        })
    }

    fn ensure_owned(&self) -> Result<()> {
        require(
            unsafe { self.pasteboard.changeCount() } == self.owned_count,
            "pasteboard changed outside the test; current clipboard remains untouched",
        )
    }

    fn expect_write(&mut self, expected: &str) -> Result<()> {
        // Production plain-text Copy/Cut performs one clearContents, then
        // setData without another ownership change. Apple documents the
        // ownership counter at https://developer.apple.com/documentation/appkit/nspasteboard/changecount.
        let next = self
            .owned_count
            .checked_add(1)
            .ok_or("pasteboard ownership counter overflow")?;
        require(
            unsafe { self.pasteboard.changeCount() } == next,
            "native clipboard write is not ready or ownership changed",
        )?;
        let text = unsafe { self.pasteboard.stringForType(NSPasteboardTypeString) };
        require(
            text.is_some_and(|text| text.to_string() == expected),
            "native clipboard write does not contain the expected test text",
        )?;
        require(
            unsafe { self.pasteboard.changeCount() } == next,
            "pasteboard changed while verifying the test write",
        )?;
        self.owned_count = next;
        self.changed = true;
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        if !self.changed {
            return Ok(());
        }
        self.ensure_owned()?;
        let cleared = unsafe { self.pasteboard.clearContents() };
        self.owned_count = cleared;
        if !self.items.is_empty() {
            let objects: Vec<Id<ProtocolObject<dyn NSPasteboardWriting>>> = self
                .items
                .iter()
                .cloned()
                .map(ProtocolObject::from_retained)
                .collect();
            let objects = NSArray::from_vec(objects);
            require(
                unsafe { self.pasteboard.writeObjects(&objects) },
                "preserved pasteboard items could not be restored",
            )?;
        }
        // No asynchronous operation separates the ownership check, restoration
        // and verification. Still check the server counter before reading.
        self.ensure_owned()?;
        let restored = unsafe { self.pasteboard.pasteboardItems() };
        require(
            restored.as_ref().map_or(0, |items| items.len()) == self.items.len(),
            "restored pasteboard item count differs",
        )?;
        if let Some(restored) = restored {
            for (index, expected) in self.items.iter().enumerate() {
                let actual: Id<NSPasteboardItem> =
                    unsafe { msg_send_id![&*restored, objectAtIndex: index] };
                let types = unsafe { expected.types() };
                require(
                    unsafe { actual.types() }.len() == types.len(),
                    "restored pasteboard representation count differs",
                )?;
                for type_index in 0..types.len() {
                    let kind: Id<NSString> =
                        unsafe { msg_send_id![&*types, objectAtIndex: type_index] };
                    let expected_data = unsafe { expected.dataForType(&kind) }
                        .ok_or("preserved pasteboard representation is unavailable")?;
                    let actual_data = unsafe { actual.dataForType(&kind) }
                        .ok_or("restored pasteboard representation is unavailable")?;
                    require(
                        actual_data.bytes() == expected_data.bytes(),
                        "restored pasteboard representation differs",
                    )?;
                }
            }
        }
        self.ensure_owned()?;
        self.changed = false;
        Ok(())
    }
}

impl Drop for PasteboardGuard {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            // Contents and type names are deliberately absent from this error.
            eprintln!("NATIVE_TEXT_PASTEBOARD_RESTORE: {error}");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    version: u64,
    content: String,
    selection: (usize, usize),
    undo: usize,
    redo: usize,
}

fn stamp(fixture: &TextFixture, cx: &Context<TextFixture>) -> Stamp {
    let editor = fixture.editor.read(cx);
    Stamp {
        version: editor.content_version(),
        content: editor.content(),
        selection: editor.selection_bytes(),
        undo: editor.undo_depth(),
        redo: editor.redo_depth(),
    }
}

async fn poll<T>(
    window: Handle,
    cx: &mut AsyncApp,
    description: &str,
    mut read: impl FnMut(&TextFixture, &mut Window, &Context<TextFixture>) -> Result<T>,
) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match window.update(cx, |fixture, window, cx| read(fixture, window, cx)) {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) if Instant::now() >= deadline => {
                return Err(format!("{description}: {error}"));
            }
            Err(error) => return Err(format!("{description}: {error}")),
            _ => {
                Timer::after(Duration::from_millis(25)).await;
            }
        }
    }
}

async fn poll_native<T>(description: &str, mut read: impl FnMut() -> Result<T>) -> Result<T> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match read() {
            Ok(value) => return Ok(value),
            Err(error) if Instant::now() >= deadline => {
                return Err(format!("{description}: {error}"));
            }
            _ => {
                Timer::after(Duration::from_millis(25)).await;
            }
        }
    }
}

fn call<T>(
    window: Handle,
    cx: &mut AsyncApp,
    operation: impl FnOnce(&Window) -> Result<T>,
) -> Result<T> {
    window
        .update(cx, |_, window, _| operation(window))
        .map_err(|error| error.to_string())?
}

fn native_range(text: &str, start: usize, end: usize) -> NSRange {
    let location = text[..start].encode_utf16().count();
    NSRange::new(location, text[start..end].encode_utf16().count())
}

async fn contents(window: Handle, cx: &mut AsyncApp, expected: &str) -> Result<Stamp> {
    poll(
        window,
        cx,
        "native document revision",
        |fixture, window, cx| {
            let node = named(window, "Native Unicode document")?;
            require(
                value(&node)? == expected,
                "native full value differs from the document",
            )?;
            let count: NSInteger = unsafe { msg_send![&*node, accessibilityNumberOfCharacters] };
            require(
                count as usize == expected.encode_utf16().count(),
                "native UTF-16 count mismatch",
            )?;
            let current = stamp(fixture, cx);
            require(
                current.content == expected,
                "foreground Editor contents differ",
            )?;
            Ok(current)
        },
    )
    .await
}

async fn ready_selection(
    window: Handle,
    cx: &mut AsyncApp,
    node: &NSObject,
    text: &str,
    start: usize,
    end: usize,
) -> Result<Stamp> {
    poll(window, cx, "native selection capability", |_, _, _| {
        let supported: bool = unsafe {
            msg_send![node, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedTextRange:)]
        };
        let length: NSInteger = unsafe { msg_send![node, accessibilityNumberOfCharacters] };
        require(
            supported && value(node)? == text && length as usize == text.encode_utf16().count(),
            "native document revision is still being prepared",
        )
    })
    .await?;
    call(window, cx, |_| {
        set_selection(node, native_range(text, start, end));
        Ok(())
    })?;
    poll(
        window,
        cx,
        "native action selection",
        |fixture, window, cx| {
            let current = stamp(fixture, cx);
            require(
                current.content == text
                    && current.selection == (start, end)
                    && fixture.editor.read(cx).focus_handle(cx).is_focused(window),
                "native action selection is not ready",
            )?;
            Ok(current)
        },
    )
    .await
}

async fn undo_redo(
    window: Handle,
    cx: &mut AsyncApp,
    before: &Stamp,
    changed: &Stamp,
) -> Result<()> {
    require(
        changed.undo == before.undo + 1 && changed.redo == 0,
        "native edit did not create exactly one undo transaction",
    )?;
    call(window, cx, |window| press(window, "Undo document"))?;
    let undone = contents(window, cx, &before.content).await?;
    require(
        undone.undo == before.undo && undone.redo == 1,
        "one native Undo did not restore the previous document",
    )?;
    call(window, cx, |window| press(window, "Redo document"))?;
    let redone = contents(window, cx, &changed.content).await?;
    require(
        redone.undo == changed.undo && redone.redo == 0,
        "one native Redo did not restore the complete edit",
    )?;
    call(window, cx, |window| press(window, "Undo document"))?;
    require(
        contents(window, cx, &before.content).await?.undo == before.undo,
        "final native Undo did not restore the document",
    )
}

async fn clipboard_checks(
    window: Handle,
    cx: &mut AsyncApp,
    input: &NSObject,
    read_only: &NSObject,
    needle: &str,
    start: usize,
    end: usize,
) -> Result<()> {
    let mut pasteboard = PasteboardGuard::preserve()?;
    let before = ready_selection(window, cx, input, ORIGINAL, start, end).await?;
    pasteboard.ensure_owned()?;
    call(window, cx, |_| text_action(input, "Copy text"))?;
    poll(window, cx, "native clipboard Copy", |_, _, _| {
        pasteboard.expect_write(needle)
    })
    .await?;
    require(
        contents(window, cx, ORIGINAL).await? == before,
        "native Copy changed the Editor selection or history",
    )?;

    pasteboard.ensure_owned()?;
    call(window, cx, |_| text_action(read_only, "Copy text"))?;
    poll(window, cx, "read-only native Copy", |_, _, _| {
        pasteboard.expect_write(needle)
    })
    .await?;
    call(window, cx, |_| {
        let actions = text_action_names(read_only);
        require(
            actions.iter().any(|name| name == "Copy text")
                && !actions
                    .iter()
                    .any(|name| name == "Cut text" || name == "Paste text"),
            "read-only native clipboard capabilities are incorrect",
        )?;
        perform_text_action(read_only, "Cut text");
        perform_text_action(read_only, "Paste text");
        Ok(())
    })?;
    Timer::after(Duration::from_millis(100)).await;
    pasteboard.ensure_owned()?;
    poll(
        window,
        cx,
        "read-only native clipboard guard",
        |fixture, _, cx| {
            let editor = fixture.read_only.read(cx);
            require(
                editor.content() == ORIGINAL
                    && editor.selection_bytes() == (start, end)
                    && editor.undo_depth() == 0
                    && editor.redo_depth() == 0,
                "read-only clipboard action changed the Editor",
            )
        },
    )
    .await?;

    // A native Cut captures the selected range atomically, then copies exactly
    // that Unicode text and removes it in one history entry.
    let before = ready_selection(window, cx, input, ORIGINAL, start, end).await?;
    pasteboard.ensure_owned()?;
    call(window, cx, |_| text_action(input, "Cut text"))?;
    poll(window, cx, "native clipboard Cut", |_, _, _| {
        pasteboard.expect_write(needle)
    })
    .await?;
    let cut = format!("{}{}", &ORIGINAL[..start], &ORIGINAL[end..]);
    let changed = contents(window, cx, &cut).await?;
    require(
        changed.selection == (start, start),
        "native Cut did not leave the caret at the removed range",
    )?;
    undo_redo(window, cx, &before, &changed).await?;

    let before = ready_selection(window, cx, input, ORIGINAL, start, start).await?;
    pasteboard.ensure_owned()?;
    call(window, cx, |_| text_action(input, "Paste text"))?;
    let pasted = format!("{}{needle}{}", &ORIGINAL[..start], &ORIGINAL[start..]);
    let changed = contents(window, cx, &pasted).await?;
    pasteboard.ensure_owned()?;
    require(
        changed.selection == (end, end),
        "native Paste did not leave the caret after the Unicode text",
    )?;
    undo_redo(window, cx, &before, &changed).await?;

    call(window, cx, |window| {
        press(window, "Toggle disabled document")
    })?;
    let disabled = poll(
        window,
        cx,
        "disabled clipboard capabilities",
        |fixture, _, cx| {
            require(fixture.disabled, "Editor is not disabled")?;
            let actions = text_action_names(input);
            require(
                !actions
                    .iter()
                    .any(|name| name == "Copy text" || name == "Cut text" || name == "Paste text"),
                "disabled clipboard actions remain advertised",
            )?;
            Ok(stamp(fixture, cx))
        },
    )
    .await?;
    pasteboard.ensure_owned()?;
    call(window, cx, |_| {
        for action in ["Copy text", "Cut text", "Paste text"] {
            perform_text_action(input, action);
        }
        Ok(())
    })?;
    Timer::after(Duration::from_millis(100)).await;
    pasteboard.ensure_owned()?;
    poll(
        window,
        cx,
        "disabled clipboard action guard",
        |fixture, _, cx| {
            require(
                stamp(fixture, cx) == disabled,
                "disabled clipboard action changed the Editor",
            )
        },
    )
    .await?;
    call(window, cx, |window| {
        press(window, "Toggle disabled document")
    })?;
    poll(
        window,
        cx,
        "clipboard capability recovery",
        |fixture, _, _| {
            require(
                !fixture.disabled
                    && text_action_names(input)
                        .iter()
                        .any(|name| name == "Copy text"),
                "native clipboard capability did not recover",
            )
        },
    )
    .await?;
    pasteboard.restore()
}

fn mark_text(view: &NSObject, text: &str, selected: NSRange) {
    let text = NSString::from_str(text);
    let _: () = unsafe {
        msg_send![view, setMarkedText: &*text, selectedRange: selected, replacementRange: NSRange::new(NSNotFound as usize, 0)]
    };
}

fn commit_text(view: &NSObject, text: &str) {
    let text = NSString::from_str(text);
    let _: () = unsafe {
        msg_send![view, insertText: &*text, replacementRange: NSRange::new(NSNotFound as usize, 0)]
    };
}

async fn composition_checks(
    window: Handle,
    cx: &mut AsyncApp,
    input: &NSObject,
    read_only: &NSObject,
    start: usize,
    end: usize,
) -> Result<()> {
    let previous_version = contents(window, cx, ORIGINAL).await?.version;
    call(window, cx, |window| press(window, "Reset document"))?;
    poll(
        window,
        cx,
        "composition reset revision",
        |fixture, _, cx| {
            let reset = stamp(fixture, cx);
            require(
                reset.version > previous_version
                    && reset.content == ORIGINAL
                    && reset.undo == 0
                    && reset.redo == 0
                    && reset.selection == (0, 0)
                    && selection(input) == NSRange::new(0, 0),
                "composition reset has not replaced the previous revision and history",
            )
        },
    )
    .await?;
    let before = ready_selection(window, cx, input, ORIGINAL, start, end).await?;
    let view = call(window, cx, native_view)?;
    let native_selection: NSRange = unsafe { msg_send![&*view, selectedRange] };
    require(
        native_selection == native_range(ORIGINAL, start, end),
        "the owned NSTextInputClient is not focused on the selected Editor",
    )?;
    let marked: bool = unsafe { msg_send![&*view, hasMarkedText] };
    require(!marked, "composition started with an existing marked range")?;

    // NSTextInputClient synchronously enters the Window's input handler. Make
    // these real platform calls outside WindowHandle::update, as AppKit does;
    // a nested Window lease would prevent the input handler from running.
    mark_text(&view, "に", NSRange::new(1, 0));
    let first = format!("{}に{}", &ORIGINAL[..start], &ORIGINAL[end..]);
    let first_state = contents(window, cx, &first).await?;
    require(
        first_state.undo == before.undo + 1,
        "first marked revision did not start one composition transaction",
    )?;
    let first_mark: NSRange = unsafe { msg_send![&*view, markedRange] };
    let first_selection: NSRange = unsafe { msg_send![&*view, selectedRange] };
    require(
        first_mark == NSRange::new(native_range(ORIGINAL, start, start).location, 1)
            && first_selection == NSRange::new(first_mark.location + 1, 0)
            && first_state.selection == (start + "に".len(), start + "に".len()),
        "first marked UTF-16 range or foreground caret differs",
    )?;

    let emoji = "👩🏽\u{200d}💻";
    let first_line = format!("日本{emoji}e\u{301}\n");
    let marked_text = format!("{first_line}次の行");
    let local_selection = NSRange::new(2, emoji.encode_utf16().count());
    mark_text(&view, &marked_text, local_selection);
    let second = format!("{}{marked_text}{}", &ORIGINAL[..start], &ORIGINAL[end..]);
    let second_state = contents(window, cx, &second).await?;
    require(
        second_state.undo == first_state.undo
            && second_state.selection == (start + "日本".len(), start + "日本".len() + emoji.len()),
        "marked revisions did not coalesce or selected grapheme bytes differ",
    )?;
    let mark: NSRange = unsafe { msg_send![&*view, markedRange] };
    let native_selection: NSRange = unsafe { msg_send![&*view, selectedRange] };
    let has_mark: bool = unsafe { msg_send![&*view, hasMarkedText] };
    require(
        has_mark
            && mark == NSRange::new(first_mark.location, marked_text.encode_utf16().count())
            && native_selection
                == NSRange::new(
                    mark.location + local_selection.location,
                    local_selection.length,
                ),
        "native marked or selected UTF-16 grapheme range differs",
    )?;
    poll_native("marked text candidate rectangle", || {
        let requested = NSRange::new(mark.location, 1);
        let mut actual = NSRange::new(NSNotFound as usize, 0);
        let rect: NSRect = unsafe {
            msg_send![&*view, firstRectForCharacterRange: requested, actualRange: &mut actual as *mut NSRange]
        };
        let owner: NSRect = unsafe { msg_send![input, accessibilityFrame] };
        require(
            actual == requested
                && rect.size.width > 0.0
                && rect.size.height > 0.0
                && rect.origin.x >= owner.origin.x
                && rect.origin.x < owner.origin.x + owner.size.width
                && rect.origin.y >= owner.origin.y
                && rect.origin.y < owner.origin.y + owner.size.height,
            "owned NSTextInputClient candidate rectangle is outside the shaped Editor",
        )?;
        let optional_rect: NSRect = unsafe {
            msg_send![&*view, firstRectForCharacterRange: requested, actualRange: std::ptr::null_mut::<NSRange>()]
        };
        require(optional_rect == rect, "optional candidate range pointer changed geometry")?;
        let mut rejected_actual = NSRange::new(0, 1);
        let rejected: NSRect = unsafe {
            msg_send![&*view, firstRectForCharacterRange: NSRange::new(NSNotFound as usize, 0), actualRange: &mut rejected_actual as *mut NSRange]
        };
        require(
            rejected.size.width == 0.0
                && rejected.size.height == 0.0
                && rejected_actual == NSRange::new(NSNotFound as usize, 0),
            "invalid candidate range did not return empty geometry and NSNotFound",
        )
    })
    .await?;

    poll_native("candidate first-line and grapheme ranges", || {
        let requested = mark;
        let mut actual = NSRange::new(NSNotFound as usize, 0);
        let rect: NSRect = unsafe {
            msg_send![&*view, firstRectForCharacterRange: requested, actualRange: &mut actual as *mut NSRange]
        };
        require(
            actual == NSRange::new(mark.location, first_line.encode_utf16().count())
                && rect.size.width > 0.0
                && rect.size.height > 0.0,
            "multiline candidate rectangle claims characters outside its first line",
        )?;
        let combining = mark.location + "日本".encode_utf16().count()
            + emoji.encode_utf16().count();
        let requested = NSRange::new(combining + 1, 1);
        let rect: NSRect = unsafe {
            msg_send![&*view, firstRectForCharacterRange: requested, actualRange: &mut actual as *mut NSRange]
        };
        require(
            actual == NSRange::new(combining, 2)
                && rect.size.width > 0.0
                && rect.size.height > 0.0,
            "candidate range does not cover the actual combining grapheme",
        )?;
        let caret = NSRange::new(mark.location, 0);
        let rect: NSRect = unsafe {
            msg_send![&*view, firstRectForCharacterRange: caret, actualRange: &mut actual as *mut NSRange]
        };
        require(
            actual == caret && rect.size.width == 0.0 && rect.size.height > 0.0,
            "candidate insertion rectangle must have zero width",
        )
    })
    .await?;

    let committed_text = "確定 日本🙂 cafe\u{301}";
    commit_text(&view, committed_text);
    let committed = format!("{}{committed_text}{}", &ORIGINAL[..start], &ORIGINAL[end..]);
    let changed = contents(window, cx, &committed).await?;
    let marked: bool = unsafe { msg_send![&*view, hasMarkedText] };
    let cleared: NSRange = unsafe { msg_send![&*view, markedRange] };
    require(
        !marked
            && cleared == NSRange::new(NSNotFound as usize, 0)
            && changed.undo == first_state.undo
            && changed.selection == (start + committed_text.len(), start + committed_text.len()),
        "composition commit did not clear its marked range and preserve one transaction",
    )?;
    undo_redo(window, cx, &before, &changed).await?;

    // Focus through the real selection setter, then send input through the
    // same owned view. Read-only input handlers must reject both entry points.
    call(window, cx, |_| {
        set_selection(read_only, native_range(ORIGINAL, start, end));
        Ok(())
    })?;
    poll_native("read-only NSTextInputClient focus", || {
        let selected: NSRange = unsafe { msg_send![&*view, selectedRange] };
        require(
            selected == native_range(ORIGINAL, start, end),
            "read-only input-client focus is not ready",
        )
    })
    .await?;
    poll(
        window,
        cx,
        "read-only NSTextInputClient focus",
        |fixture, window, cx| {
            require(
                fixture.read_only.read(cx).selection_bytes() == (start, end)
                    && fixture
                        .read_only
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window),
                "read-only input-client focus is not ready",
            )
        },
    )
    .await?;
    mark_text(&view, "invalid", NSRange::new(7, 0));
    commit_text(&view, "invalid");
    Timer::after(Duration::from_millis(100)).await;
    let marked: bool = unsafe { msg_send![&*view, hasMarkedText] };
    require(!marked, "read-only input accepted a marked range")?;
    poll(
        window,
        cx,
        "read-only native composition guard",
        |fixture, _, cx| {
            let editor = fixture.read_only.read(cx);
            require(
                editor.content() == ORIGINAL
                    && editor.selection_bytes() == (start, end)
                    && editor.undo_depth() == 0
                    && editor.redo_depth() == 0,
                "read-only NSTextInputClient composition changed the Editor",
            )
        },
    )
    .await?;

    let _ = ready_selection(window, cx, input, ORIGINAL, start, end).await?;
    call(window, cx, |window| {
        press(window, "Toggle disabled document")
    })?;
    let disabled = poll(
        window,
        cx,
        "disabled composition state",
        |fixture, _, cx| {
            require(fixture.disabled, "Editor is not disabled")?;
            Ok(stamp(fixture, cx))
        },
    )
    .await?;
    mark_text(&view, "invalid", NSRange::new(7, 0));
    commit_text(&view, "invalid");
    Timer::after(Duration::from_millis(100)).await;
    poll(
        window,
        cx,
        "disabled native composition guard",
        |fixture, _, cx| {
            require(
                stamp(fixture, cx) == disabled,
                "disabled NSTextInputClient composition changed the Editor",
            )
        },
    )
    .await?;
    call(window, cx, |window| {
        press(window, "Toggle disabled document")
    })?;
    poll(
        window,
        cx,
        "composition capability recovery",
        |fixture, _, _| {
            let enabled: bool = unsafe { msg_send![input, isAccessibilityEnabled] };
            require(
                !fixture.disabled && enabled,
                "Editor did not recover after composition guard",
            )
        },
    )
    .await
}

async fn run(window: Handle, cx: &mut AsyncApp) -> Result<()> {
    let needle = "日本語 👩🏽\u{200d}💻 cafe\u{301}";
    let start = ORIGINAL
        .find(needle)
        .ok_or("canonical Unicode needle is absent")?;
    let end = start + needle.len();
    let range = native_range(ORIGINAL, start, end);
    let input = poll(window, cx, "prepared native Editor", |_, window, _| {
        let node = named(window, "Native Unicode document")?;
        let supported: bool = unsafe {
            msg_send![&*node, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedTextRange:)]
        };
        require(supported && value(&node)? == ORIGINAL, "native text selection is not ready")?;
        Ok(node)
    }).await?;
    contents(window, cx, ORIGINAL).await?;
    call(window, cx, |_| {
        set_selection(&input, range);
        Ok(())
    })?;
    let selected = poll(window, cx, "native Unicode selection", |fixture, _, cx| {
        let actual = selection(&input);
        require(actual == range, "native selected UTF-16 range mismatch")?;
        let selected: Option<Id<NSString>> =
            unsafe { msg_send_id![&*input, accessibilitySelectedText] };
        require(
            selected.is_some_and(|text| text.to_string() == needle),
            "native selected text mismatch",
        )?;
        let current = stamp(fixture, cx);
        require(
            current.selection == (start, end),
            "foreground selected byte range mismatch",
        )?;
        Ok(current)
    })
    .await?;
    let frame: NSRect = unsafe {
        msg_send![&*input, accessibilityFrameForRange: native_range(ORIGINAL, start, start + '日'.len_utf8())]
    };
    require(
        frame.size.width > 0.0 && frame.size.height > 0.0,
        "mounted glyph has no native bounds",
    )?;
    let hit: NSRange = unsafe {
        msg_send![&*input, accessibilityRangeForPosition: NSPoint::new(frame.origin.x + frame.size.width / 2.0, frame.origin.y + frame.size.height / 2.0)]
    };
    require(
        hit.location == range.location,
        "native shaped glyph hit-test mismatch",
    )?;
    let visible: NSRange = unsafe { msg_send![&*input, accessibilityVisibleCharacterRange] };
    require(
        visible.length > 0 && visible.length < ORIGINAL.encode_utf16().count(),
        "visible text range must be bounded to the viewport",
    )?;

    let offscreen = ORIGINAL
        .find("KAEL_TEXT_OFFSCREEN_END")
        .ok_or("offscreen sentinel is absent")?;
    let offscreen_range = native_range(ORIGINAL, offscreen, offscreen + 1);
    call(window, cx, |_| {
        let _: () =
            unsafe { msg_send![&*input, setAccessibilityVisibleCharacterRange: offscreen_range] };
        Ok(())
    })?;
    poll(window, cx, "offscreen native reveal", |fixture, _, cx| {
        let glyph: NSRect =
            unsafe { msg_send![&*input, accessibilityFrameForRange: offscreen_range] };
        let owner: NSRect = unsafe { msg_send![&*input, accessibilityFrame] };
        require(
            glyph.size.width > 0.0 && glyph.size.height > 0.0,
            "revealed glyph has no shaped bounds",
        )?;
        require(
            glyph.origin.y >= owner.origin.y && glyph.origin.y < owner.origin.y + owner.size.height,
            "offscreen glyph is outside the viewport",
        )?;
        require(
            stamp(fixture, cx) == selected,
            "native reveal changed selection, contents or history",
        )?;
        Ok(())
    })
    .await?;
    call(window, cx, |_| {
        set_selection(&input, NSRange::new(ORIGINAL.encode_utf16().count(), 0));
        Ok(())
    })?;
    poll(window, cx, "native EOF caret", |fixture, _, cx| {
        require(
            stamp(fixture, cx).selection == (ORIGINAL.len(), ORIGINAL.len()),
            "native EOF did not map to the final byte",
        )?;
        require(
            selection(&input) == NSRange::new(ORIGINAL.encode_utf16().count(), 0),
            "native EOF UTF-16 mismatch",
        )?;
        Ok(())
    })
    .await?;

    let read_only = call(window, cx, |window| {
        named(window, "Read-only Unicode document")
    })?;
    require(
        value(&read_only)? == ORIGINAL,
        "read-only full document mismatch",
    )?;
    let mutable: bool = unsafe {
        msg_send![&*read_only, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedText:)]
    };
    let mutable_value: bool = unsafe {
        msg_send![&*read_only, isAccessibilitySelectorAllowed: sel!(setAccessibilityValue:)]
    };
    let selectable: bool = unsafe {
        msg_send![&*read_only, isAccessibilitySelectorAllowed: sel!(setAccessibilitySelectedTextRange:)]
    };
    require(
        !mutable && !mutable_value && selectable,
        "read-only native setter capabilities are incorrect",
    )?;
    let legacy_settable: bool = unsafe {
        msg_send![&*read_only, respondsToSelector: sel!(accessibilityIsAttributeSettable:)]
    };
    if legacy_settable {
        let attribute = NSString::from_str("AXValue");
        let writable: bool =
            unsafe { msg_send![&*read_only, accessibilityIsAttributeSettable: &*attribute] };
        require(
            !writable,
            "read-only legacy AXValue is advertised as writable",
        )?;
    }
    call(window, cx, |_| {
        set_selection(&read_only, range);
        Ok(())
    })?;
    poll(
        window,
        cx,
        "read-only native selection",
        |fixture, _, cx| {
            require(
                selection(&read_only) == range
                    && fixture.read_only.read(cx).selection_bytes() == (start, end),
                "read-only native selection did not reach the foreground Editor",
            )
        },
    )
    .await?;
    call(window, cx, |_| {
        replace_selected(&read_only, "invalid");
        let invalid = NSString::from_str("invalid whole document");
        let _: () = unsafe { msg_send![&*read_only, setAccessibilityValue: &*invalid] };
        Ok(())
    })?;
    Timer::after(Duration::from_millis(100)).await;
    poll(window, cx, "read-only mutation guard", |fixture, _, cx| {
        require(
            fixture.read_only.read(cx).content() == ORIGINAL
                && fixture.read_only.read(cx).selection_bytes() == (start, end)
                && fixture.read_only.read(cx).undo_depth() == 0
                && value(&read_only)? == ORIGINAL,
            "native read-only Editor changed",
        )
    })
    .await?;
    call(window, cx, |window| {
        press(window, "Toggle disabled document")
    })?;
    let disabled = poll(window, cx, "disabled native Editor", |fixture, _, cx| {
        let enabled: bool = unsafe { msg_send![&*input, isAccessibilityEnabled] };
        require(
            fixture.disabled && !enabled,
            "native disabled state was not exported",
        )?;
        Ok(stamp(fixture, cx))
    })
    .await?;
    call(window, cx, |_| {
        set_selection(&input, range);
        replace_selected(&input, "invalid");
        Ok(())
    })?;
    Timer::after(Duration::from_millis(100)).await;
    poll(window, cx, "disabled mutation guard", |fixture, _, cx| {
        require(
            stamp(fixture, cx) == disabled,
            "disabled native action mutated Editor",
        )
    })
    .await?;
    call(window, cx, |window| {
        press(window, "Toggle disabled document")
    })?;
    poll(window, cx, "enabled native Editor", |fixture, _, _| {
        let enabled: bool = unsafe { msg_send![&*input, isAccessibilityEnabled] };
        require(
            !fixture.disabled && enabled,
            "native capability did not recover",
        )
    })
    .await?;

    call(window, cx, |_| {
        set_selection(&input, range);
        Ok(())
    })?;
    let before = poll(window, cx, "partial edit origin", |fixture, _, cx| {
        let current = stamp(fixture, cx);
        require(
            current.selection == (start, end),
            "native partial edit selection is not ready",
        )?;
        Ok(current)
    })
    .await?;
    let replacement = "新しい 日本🙂";
    call(window, cx, |_| {
        replace_selected(&input, replacement);
        Ok(())
    })?;
    let expected = format!("{}{replacement}{}", &ORIGINAL[..start], &ORIGINAL[end..]);
    let changed = contents(window, cx, &expected).await?;
    require(
        changed.undo == before.undo + 1,
        "native partial edit did not create exactly one undo transaction",
    )?;
    call(window, cx, |window| press(window, "Undo document"))?;
    require(
        contents(window, cx, ORIGINAL).await?.undo == before.undo,
        "one native Undo did not restore history",
    )?;
    call(window, cx, |window| press(window, "Redo document"))?;
    require(
        contents(window, cx, &expected).await?.undo == changed.undo,
        "one native Redo did not restore the edit",
    )?;
    call(window, cx, |window| press(window, "Undo document"))?;
    contents(window, cx, ORIGINAL).await?;

    clipboard_checks(window, cx, &input, &read_only, needle, start, end).await?;
    composition_checks(window, cx, &input, &read_only, start, end).await?;

    // All three are real native calls made before the foreground queue drains.
    // Replace runs first; subsequent old-origin selection/edit callbacks must
    // reject their captured document IDs even though the Editor object is stable.
    call(window, cx, |window| {
        press(window, "Replace document")?;
        set_selection(&input, range);
        replace_selected(&input, "stale native edit");
        Ok(())
    })?;
    let replaced = contents(window, cx, REPLACEMENT).await?;
    require(
        replaced.selection == (0, 0) && replaced.undo == 0,
        "queued old-document native action survived replacement",
    )?;
    Timer::after(Duration::from_millis(100)).await;
    require(
        contents(window, cx, REPLACEMENT).await? == replaced,
        "late stale native action mutated the replacement",
    )?;
    require(
        value(&read_only)? == ORIGINAL,
        "independent read-only document changed",
    )?;
    println!(
        "NATIVE_TEXT_ACCESSIBILITY_OK platform=macos utf8=105391 utf16=66983 selection=true geometry=true hit_test=true visible_range=true reveal=true eof=true editable=true atomic_undo=true readonly=true disabled=true stale_origin=true clipboard=true pasteboard_restored=true ime_protocol=true physical_ime=false"
    );
    call(window, cx, |window| {
        press(window, "Native text checks complete")
    })?;
    Ok(())
}
