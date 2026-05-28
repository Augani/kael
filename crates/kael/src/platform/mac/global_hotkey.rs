use crate::Keystroke;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use std::collections::HashMap;

pub(crate) fn keystroke_matches_event(keystroke: &Keystroke, event: *mut AnyObject) -> bool {
    let event: &NSEvent = unsafe { &*(event as *const NSEvent) };
    if event.r#type() != NSEventType::KeyDown {
        return false;
    }
    matches_modifiers_and_key(keystroke, event)
}

pub(crate) fn is_hotkey_released(keystroke: &Keystroke, event: *mut AnyObject) -> bool {
    let event: &NSEvent = unsafe { &*(event as *const NSEvent) };
    match event.r#type() {
        NSEventType::KeyUp => key_matches_ignoring_modifiers(keystroke, event),
        NSEventType::FlagsChanged => required_modifier_released(keystroke, event),
        _ => false,
    }
}

fn key_matches_ignoring_modifiers(keystroke: &Keystroke, event: &NSEvent) -> bool {
    let Some(chars_ignoring) = event.charactersIgnoringModifiers() else {
        return false;
    };
    let chars_str = chars_ignoring.UTF8String();
    if chars_str.is_null() {
        return false;
    }
    let chars = unsafe { std::ffi::CStr::from_ptr(chars_str) }
        .to_str()
        .unwrap_or("");
    let event_key = chars.to_lowercase();
    let target_key = keystroke.key.to_lowercase();
    event_key == target_key || normalize_key_name(&event_key) == target_key
}

fn required_modifier_released(keystroke: &Keystroke, event: &NSEvent) -> bool {
    let modifiers = event.modifierFlags();
    let has_cmd = modifiers.contains(NSEventModifierFlags::Command);
    let has_ctrl = modifiers.contains(NSEventModifierFlags::Control);
    let has_alt = modifiers.contains(NSEventModifierFlags::Option);
    let has_shift = modifiers.contains(NSEventModifierFlags::Shift);

    (keystroke.modifiers.platform && !has_cmd)
        || (keystroke.modifiers.control && !has_ctrl)
        || (keystroke.modifiers.alt && !has_alt)
        || (keystroke.modifiers.shift && !has_shift)
}

fn matches_modifiers_and_key(keystroke: &Keystroke, event: &NSEvent) -> bool {
    let modifiers = event.modifierFlags();
    let event_cmd = modifiers.contains(NSEventModifierFlags::Command);
    let event_ctrl = modifiers.contains(NSEventModifierFlags::Control);
    let event_alt = modifiers.contains(NSEventModifierFlags::Option);
    let event_shift = modifiers.contains(NSEventModifierFlags::Shift);

    if keystroke.modifiers.platform != event_cmd
        || keystroke.modifiers.control != event_ctrl
        || keystroke.modifiers.alt != event_alt
        || keystroke.modifiers.shift != event_shift
    {
        return false;
    }

    let Some(chars_ignoring) = event.charactersIgnoringModifiers() else {
        return false;
    };
    let chars_str = chars_ignoring.UTF8String();
    if chars_str.is_null() {
        return false;
    }
    let chars = unsafe { std::ffi::CStr::from_ptr(chars_str) }
        .to_str()
        .unwrap_or("");

    let event_key = chars.to_lowercase();
    let target_key = keystroke.key.to_lowercase();

    event_key == target_key || normalize_key_name(&event_key) == target_key
}

fn normalize_key_name(char_key: &str) -> &str {
    match char_key {
        " " => "space",
        "\t" => "tab",
        "\r" | "\n" => "enter",
        "\u{1b}" => "escape",
        "\u{7f}" => "backspace",
        "\u{f700}" => "up",
        "\u{f701}" => "down",
        "\u{f702}" => "left",
        "\u{f703}" => "right",
        "\u{f728}" => "delete",
        "\u{f729}" => "home",
        "\u{f72b}" => "end",
        "\u{f72c}" => "pageup",
        "\u{f72d}" => "pagedown",
        other => other,
    }
}

pub(crate) fn find_matching_hotkey(
    registrations: &HashMap<u32, Keystroke>,
    event: *mut AnyObject,
) -> Option<u32> {
    for (id, keystroke) in registrations {
        if keystroke_matches_event(keystroke, event) {
            return Some(*id);
        }
    }
    None
}

pub(crate) fn find_matching_hotkey_released(
    active_hotkey: Option<u32>,
    registrations: &HashMap<u32, Keystroke>,
    event: *mut AnyObject,
) -> Option<u32> {
    let active_id = active_hotkey?;
    let keystroke = registrations.get(&active_id)?;
    if is_hotkey_released(keystroke, event) {
        Some(active_id)
    } else {
        None
    }
}
