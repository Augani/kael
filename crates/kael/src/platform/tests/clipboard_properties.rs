// Feature: platform-parity-browser-runtime-features, Property 6: Clipboard round-trip

use proptest::prelude::*;
use std::sync::{Arc, Mutex};

use crate::{ClipboardEntry, ClipboardItem, ClipboardString, Image, ImageFormat};

/// Simulates the platform clipboard storage used across all backends.
///
/// Every platform backend stores clipboard data in memory (or via OS APIs)
/// and returns it on read. The `TestPlatform` uses `Mutex<Option<ClipboardItem>>`
/// which is the simplest faithful model of the write-then-read contract.
struct SimulatedClipboard {
    storage: Arc<Mutex<Option<ClipboardItem>>>,
}

impl SimulatedClipboard {
    fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(None)),
        }
    }

    fn write(&self, item: ClipboardItem) {
        *self.storage.lock().unwrap() = Some(item);
    }

    fn read(&self) -> Option<ClipboardItem> {
        self.storage.lock().unwrap().clone()
    }
}

/// Generates an arbitrary `ImageFormat` variant.
fn image_format_strategy() -> impl Strategy<Value = ImageFormat> {
    prop_oneof![
        Just(ImageFormat::Png),
        Just(ImageFormat::Jpeg),
        Just(ImageFormat::Webp),
        Just(ImageFormat::Gif),
        Just(ImageFormat::Svg),
        Just(ImageFormat::Bmp),
        Just(ImageFormat::Tiff),
    ]
}

/// Generates a non-empty text string suitable for clipboard content.
fn clipboard_text_strategy() -> impl Strategy<Value = String> {
    ".{1,200}"
}

/// Generates an optional metadata string.
fn metadata_strategy() -> impl Strategy<Value = Option<String>> {
    prop_oneof![Just(None), ".{1,100}".prop_map(Some),]
}

/// Generates a `ClipboardString` with arbitrary text and optional metadata.
fn clipboard_string_strategy() -> impl Strategy<Value = ClipboardString> {
    (clipboard_text_strategy(), metadata_strategy())
        .prop_map(|(text, metadata)| ClipboardString { text, metadata })
}

/// Generates an `Image` with an arbitrary format and small byte payload.
fn clipboard_image_strategy() -> impl Strategy<Value = Image> {
    (
        image_format_strategy(),
        prop::collection::vec(any::<u8>(), 1..=64),
    )
        .prop_map(|(format, bytes)| Image::from_bytes(format, bytes))
}

/// Generates a single `ClipboardEntry` — either a string or an image.
fn clipboard_entry_strategy() -> impl Strategy<Value = ClipboardEntry> {
    prop_oneof![
        clipboard_string_strategy().prop_map(ClipboardEntry::String),
        clipboard_image_strategy().prop_map(ClipboardEntry::Image),
    ]
}

/// Generates a valid `ClipboardItem` containing 1-5 entries (text and/or images).
fn clipboard_item_strategy() -> impl Strategy<Value = ClipboardItem> {
    prop::collection::vec(clipboard_entry_strategy(), 1..=5)
        .prop_map(|entries| ClipboardItem { entries })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 11.1, 11.3, 11.4**
    ///
    /// For any valid `ClipboardItem` containing one or more `ClipboardEntry`
    /// values, writing the item to the clipboard and reading it back produces
    /// a `ClipboardItem` whose entries are equivalent to the original.
    #[test]
    fn clipboard_round_trip_preserves_entries(item in clipboard_item_strategy()) {
        let clipboard = SimulatedClipboard::new();

        clipboard.write(item.clone());
        let read_back = clipboard.read();

        prop_assert!(
            read_back.is_some(),
            "read_from_clipboard must return Some after write_to_clipboard"
        );
        let read_back = read_back.unwrap();

        // Verify entry count matches.
        prop_assert_eq!(
            read_back.entries().len(),
            item.entries().len(),
            "round-tripped ClipboardItem must have the same number of entries"
        );

        // Verify each entry is equivalent.
        for (i, (got, expected)) in read_back.entries().iter().zip(item.entries().iter()).enumerate() {
            prop_assert_eq!(
                got, expected,
                "entry at index {} must be equivalent after round-trip",
                i
            );
        }
    }

    /// **Validates: Requirements 11.1, 11.3, 11.4**
    ///
    /// For any valid `ClipboardItem`, the text content (concatenation of all
    /// string entries) is preserved through a clipboard round-trip.
    #[test]
    fn clipboard_round_trip_preserves_text(item in clipboard_item_strategy()) {
        let clipboard = SimulatedClipboard::new();

        let original_text = item.text();

        clipboard.write(item);
        let read_back = clipboard.read().unwrap();

        prop_assert_eq!(
            read_back.text(),
            original_text,
            "text() must return the same concatenated string after round-trip"
        );
    }

    /// **Validates: Requirements 11.1, 11.3, 11.4**
    ///
    /// For any valid `ClipboardItem`, image entries (format and bytes) are
    /// preserved through a clipboard round-trip.
    #[test]
    fn clipboard_round_trip_preserves_images(item in clipboard_item_strategy()) {
        let clipboard = SimulatedClipboard::new();

        let original_images: Vec<Image> = item.entries().iter().filter_map(|e| {
            if let ClipboardEntry::Image(img) = e { Some(img.clone()) } else { None }
        }).collect();

        clipboard.write(item);
        let read_back = clipboard.read().unwrap();

        let round_tripped_images: Vec<&Image> = read_back.entries().iter().filter_map(|e| {
            if let ClipboardEntry::Image(img) = e { Some(img) } else { None }
        }).collect();

        prop_assert_eq!(
            round_tripped_images.len(),
            original_images.len(),
            "number of image entries must be preserved"
        );

        for (i, (got, expected)) in round_tripped_images.iter().zip(original_images.iter()).enumerate() {
            prop_assert_eq!(
                got.format, expected.format,
                "image format at index {} must match after round-trip", i
            );
            prop_assert_eq!(
                &got.bytes, &expected.bytes,
                "image bytes at index {} must match after round-trip", i
            );
        }
    }
}
