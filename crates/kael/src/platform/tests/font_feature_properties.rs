// Feature: platform-parity-electron-features, Property 13: OpenType feature tag tolerance

use proptest::prelude::*;

use crate::platform::PlatformTextSystem;
use crate::{FontFeature, FontId, FontRun, NoopTextSystem};

/// Generates an arbitrary 4-byte sequence to use as an OpenType feature tag.
///
/// Valid OpenType tags are 4-byte ASCII identifiers (e.g., `b"liga"`), but
/// this property must hold for *any* 4-byte sequence — including non-ASCII,
/// null bytes, and other arbitrary data — to verify tolerance.
fn arbitrary_tag_strategy() -> impl Strategy<Value = [u8; 4]> {
    prop::array::uniform4(any::<u8>())
}

/// Generates an arbitrary feature value (u32).
fn arbitrary_value_strategy() -> impl Strategy<Value = u32> {
    any::<u32>()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 29.4**
    ///
    /// For any arbitrary 4-byte sequence as a feature tag,
    /// `layout_line_with_features` does not panic or error.
    /// Unsupported tags are silently ignored.
    #[test]
    fn opentype_feature_tag_tolerance(
        tag in arbitrary_tag_strategy(),
        value in arbitrary_value_strategy(),
    ) {
        let text_system = NoopTextSystem;
        let text = "Hello, world!";
        let font_size = crate::px(16.0);

        let runs = vec![FontRun {
            len: text.len(),
            font_id: FontId(0),
        }];

        let features = vec![FontFeature::new(tag, value)];

        // This must not panic or error for any arbitrary tag.
        let layout = text_system.layout_line_with_features(text, font_size, &runs, &features);

        // Basic sanity: the layout should still produce valid output.
        prop_assert!(
            layout.width.0 >= 0.0,
            "layout width must be non-negative, got {}",
            layout.width.0
        );
        prop_assert_eq!(
            layout.len,
            text.len(),
            "layout len must equal input text byte length"
        );
    }
}
