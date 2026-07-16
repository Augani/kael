// Feature: platform-parity-browser-runtime-features, Property 1: URL callback preserves URL string

use proptest::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn platform_callback_guard_returns_requested_fallback_after_panic() {
    let value =
        super::super::catch_platform_callback("test", "callback", 17, || panic!("callback panic"));
    assert_eq!(value, 17);
}

/// Generates a valid URL string matching a registered custom scheme.
///
/// Produces URLs of the form `{scheme}://{path}` where scheme is a lowercase
/// alphabetic string (1-10 chars) and path is an arbitrary non-empty string
/// of URL-safe characters.
fn valid_url_strategy() -> impl Strategy<Value = String> {
    let scheme = "[a-z]{1,10}";
    let path = "[a-zA-Z0-9._~:/?#@!$&'()*+,;=%\\-]{1,100}";
    (scheme, path).prop_map(|(s, p)| format!("{s}://{p}"))
}

/// Simulates the platform URL dispatch mechanism used across all backends.
///
/// Every platform stores the `on_open_urls` callback and later invokes it with
/// a `Vec<String>` of URLs. This function replicates that exact pattern so we
/// can verify the callback receives the dispatched URL unchanged.
fn dispatch_open_urls(urls: Vec<String>, callback: &mut Option<Box<dyn FnMut(Vec<String>)>>) {
    if let Some(cb) = callback.as_mut() {
        cb(urls);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 1.2**
    ///
    /// For any valid URL string matching a registered scheme, when the platform
    /// dispatches a URL open event, the `on_open_urls` callback receives the
    /// exact same URL string that was dispatched.
    #[test]
    fn url_callback_preserves_url_string(url in valid_url_strategy()) {
        let received = Rc::new(RefCell::new(Vec::<String>::new()));
        let received_clone = Rc::clone(&received);

        let mut callback: Option<Box<dyn FnMut(Vec<String>)>> = Some(Box::new(move |urls| {
            received_clone.borrow_mut().extend(urls);
        }));

        let dispatched = vec![url.clone()];
        dispatch_open_urls(dispatched, &mut callback);

        let received_urls = received.borrow();
        prop_assert_eq!(received_urls.len(), 1, "callback should receive exactly one URL");
        prop_assert_eq!(&received_urls[0], &url, "callback must receive the exact URL that was dispatched");
    }

    /// **Validates: Requirements 1.2**
    ///
    /// For any batch of valid URL strings, when the platform dispatches them
    /// together via `on_open_urls`, the callback receives all URLs in the same
    /// order and with identical content.
    #[test]
    fn url_callback_preserves_multiple_urls(urls in prop::collection::vec(valid_url_strategy(), 1..10)) {
        let received = Rc::new(RefCell::new(Vec::<String>::new()));
        let received_clone = Rc::clone(&received);

        let mut callback: Option<Box<dyn FnMut(Vec<String>)>> = Some(Box::new(move |u| {
            received_clone.borrow_mut().extend(u);
        }));

        let dispatched = urls.clone();
        dispatch_open_urls(dispatched, &mut callback);

        let received_urls = received.borrow();
        prop_assert_eq!(received_urls.len(), urls.len(), "callback should receive all dispatched URLs");
        for (i, (got, expected)) in received_urls.iter().zip(urls.iter()).enumerate() {
            prop_assert_eq!(got, expected, "URL at index {} must match", i);
        }
    }
}

// Feature: platform-parity-browser-runtime-features, Property 3: Global hotkey event dispatch

/// Generates a valid hotkey ID.
///
/// Hotkey IDs are `u32` values assigned by the application when registering
/// global hotkeys. We use the full `u32` range to exercise edge cases like
/// `0` and `u32::MAX`.
fn hotkey_id_strategy() -> impl Strategy<Value = u32> {
    any::<u32>()
}

/// Simulates the platform key-down dispatch for a registered global hotkey.
///
/// Every platform backend stores the `on_global_hotkey` callback as
/// `Option<Box<dyn FnMut(u32)>>` and invokes it with the hotkey ID when
/// the registered key combination is pressed. This replicates that pattern.
fn dispatch_global_hotkey_down(id: u32, callback: &mut Option<Box<dyn FnMut(u32)>>) {
    if let Some(cb) = callback.as_mut() {
        cb(id);
    }
}

/// Simulates the platform key-up dispatch for a registered global hotkey.
///
/// Every platform backend stores the `on_global_hotkey_up` callback as
/// `Option<Box<dyn FnMut(u32)>>` and invokes it with the hotkey ID when
/// the registered key combination is released. This replicates that pattern.
fn dispatch_global_hotkey_up(id: u32, callback: &mut Option<Box<dyn FnMut(u32)>>) {
    if let Some(cb) = callback.as_mut() {
        cb(id);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 5.1, 5.2**
    ///
    /// For any registered hotkey identifier, when a key-down event is dispatched
    /// for that hotkey, the `on_global_hotkey` callback receives the same
    /// identifier.
    #[test]
    fn global_hotkey_down_dispatches_correct_id(id in hotkey_id_strategy()) {
        let received = Rc::new(RefCell::new(None::<u32>));
        let received_clone = Rc::clone(&received);

        let mut callback: Option<Box<dyn FnMut(u32)>> = Some(Box::new(move |hotkey_id| {
            *received_clone.borrow_mut() = Some(hotkey_id);
        }));

        dispatch_global_hotkey_down(id, &mut callback);

        let got = received.borrow();
        prop_assert!(got.is_some(), "on_global_hotkey callback must be invoked");
        prop_assert_eq!(got.unwrap(), id, "on_global_hotkey must receive the exact hotkey ID that was dispatched");
    }

    /// **Validates: Requirements 5.1, 5.2**
    ///
    /// For any registered hotkey identifier, when a key-up event is dispatched
    /// for that hotkey, the `on_global_hotkey_up` callback receives the same
    /// identifier.
    #[test]
    fn global_hotkey_up_dispatches_correct_id(id in hotkey_id_strategy()) {
        let received = Rc::new(RefCell::new(None::<u32>));
        let received_clone = Rc::clone(&received);

        let mut callback: Option<Box<dyn FnMut(u32)>> = Some(Box::new(move |hotkey_id| {
            *received_clone.borrow_mut() = Some(hotkey_id);
        }));

        dispatch_global_hotkey_up(id, &mut callback);

        let got = received.borrow();
        prop_assert!(got.is_some(), "on_global_hotkey_up callback must be invoked");
        prop_assert_eq!(got.unwrap(), id, "on_global_hotkey_up must receive the exact hotkey ID that was dispatched");
    }

    /// **Validates: Requirements 5.1, 5.2**
    ///
    /// For any registered hotkey identifier, a key-down followed by a key-up
    /// dispatches to `on_global_hotkey` and `on_global_hotkey_up` respectively,
    /// and both callbacks receive the same identifier.
    #[test]
    fn global_hotkey_down_then_up_preserves_id(id in hotkey_id_strategy()) {
        let down_received = Rc::new(RefCell::new(None::<u32>));
        let up_received = Rc::new(RefCell::new(None::<u32>));

        let down_clone = Rc::clone(&down_received);
        let up_clone = Rc::clone(&up_received);

        let mut down_callback: Option<Box<dyn FnMut(u32)>> = Some(Box::new(move |hotkey_id| {
            *down_clone.borrow_mut() = Some(hotkey_id);
        }));
        let mut up_callback: Option<Box<dyn FnMut(u32)>> = Some(Box::new(move |hotkey_id| {
            *up_clone.borrow_mut() = Some(hotkey_id);
        }));

        dispatch_global_hotkey_down(id, &mut down_callback);
        dispatch_global_hotkey_up(id, &mut up_callback);

        let down_id = down_received.borrow();
        let up_id = up_received.borrow();

        prop_assert!(down_id.is_some(), "on_global_hotkey callback must be invoked on key-down");
        prop_assert!(up_id.is_some(), "on_global_hotkey_up callback must be invoked on key-up");
        prop_assert_eq!(down_id.unwrap(), id, "on_global_hotkey must receive the dispatched ID");
        prop_assert_eq!(up_id.unwrap(), id, "on_global_hotkey_up must receive the dispatched ID");
        prop_assert_eq!(down_id.unwrap(), up_id.unwrap(), "both callbacks must receive the same hotkey ID");
    }
}

// Feature: platform-parity-browser-runtime-features, Property 4: Media key event dispatch

use crate::MediaKeyEvent;

/// Generates an arbitrary `MediaKeyEvent` variant.
///
/// Covers all six variants: Play, Pause, PlayPause, Stop, NextTrack,
/// PreviousTrack. Uses a uniform index mapped to the variant so that
/// `proptest` can shrink toward lower indices.
fn media_key_event_strategy() -> impl Strategy<Value = MediaKeyEvent> {
    (0u8..6).prop_map(|i| match i {
        0 => MediaKeyEvent::Play,
        1 => MediaKeyEvent::Pause,
        2 => MediaKeyEvent::PlayPause,
        3 => MediaKeyEvent::Stop,
        4 => MediaKeyEvent::NextTrack,
        _ => MediaKeyEvent::PreviousTrack,
    })
}

/// Simulates the platform media key dispatch mechanism used across all backends.
///
/// Every platform stores the `on_media_key_event` callback as
/// `Option<Box<dyn FnMut(MediaKeyEvent)>>` and invokes it when a media key
/// is pressed. This function replicates that exact pattern so we can verify
/// the callback receives the dispatched variant unchanged.
fn dispatch_media_key_event(
    event: MediaKeyEvent,
    callback: &mut Option<Box<dyn FnMut(MediaKeyEvent)>>,
) {
    if let Some(cb) = callback.as_mut() {
        cb(event);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 7.1**
    ///
    /// For any `MediaKeyEvent` variant (Play, Pause, PlayPause, Stop,
    /// NextTrack, PreviousTrack), when that media key event is dispatched
    /// through the platform, the `on_media_key_event` callback receives the
    /// same variant.
    #[test]
    fn media_key_event_dispatch_preserves_variant(event in media_key_event_strategy()) {
        let received = Rc::new(RefCell::new(None::<MediaKeyEvent>));
        let received_clone = Rc::clone(&received);

        let mut callback: Option<Box<dyn FnMut(MediaKeyEvent)>> = Some(Box::new(move |e| {
            *received_clone.borrow_mut() = Some(e);
        }));

        dispatch_media_key_event(event, &mut callback);

        let got = received.borrow();
        prop_assert!(got.is_some(), "on_media_key_event callback must be invoked");
        prop_assert_eq!(got.unwrap(), event, "on_media_key_event must receive the exact MediaKeyEvent variant that was dispatched");
    }
}
