// Feature: platform-parity-browser-runtime-features, Property 5: Text layout produces valid metrics

use proptest::prelude::*;

use crate::platform::PlatformTextSystem;
use crate::{FontId, FontRun, NoopTextSystem};

/// Generates a non-empty text string of printable ASCII characters (1-200 chars).
///
/// We use printable ASCII to avoid surrogate-pair edge cases that would
/// complicate the `len` assertion (which is in UTF-8 bytes). The property
/// should hold for any non-empty text, and ASCII is a representative subset.
fn non_empty_text_strategy() -> impl Strategy<Value = String> {
    "[\\x20-\\x7E]{1,200}"
}

/// Generates a positive font size in the range 1.0..=200.0 pixels.
///
/// Font sizes must be positive for meaningful layout. The range covers
/// typical UI sizes (8-72px) and stress-tests with larger values.
fn font_size_strategy() -> impl Strategy<Value = f32> {
    1.0f32..=200.0
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// **Validates: Requirements 10.1**
    ///
    /// For any non-empty text string and valid font run configuration (with
    /// positive font size and valid font family), `layout_line` produces a
    /// `LineLayout` with non-negative width, finite ascent and descent values,
    /// and a `len` equal to the input text length.
    #[test]
    fn text_layout_produces_valid_metrics(
        text in non_empty_text_strategy(),
        font_size_val in font_size_strategy(),
    ) {
        let text_system = NoopTextSystem;
        let font_size = crate::px(font_size_val);

        // Build a single font run covering the entire text.
        let runs = vec![FontRun {
            len: text.len(),
            font_id: FontId(0),
        }];

        let layout = text_system.layout_line(&text, font_size, &runs);

        // Width must be non-negative.
        prop_assert!(
            layout.width.0 >= 0.0,
            "layout width must be non-negative, got {}",
            layout.width.0
        );

        // Ascent must be finite.
        prop_assert!(
            layout.ascent.0.is_finite(),
            "layout ascent must be finite, got {}",
            layout.ascent.0
        );

        // Descent must be finite.
        prop_assert!(
            layout.descent.0.is_finite(),
            "layout descent must be finite, got {}",
            layout.descent.0
        );

        // len must equal the input text length in bytes.
        prop_assert_eq!(
            layout.len,
            text.len(),
            "layout len must equal input text byte length"
        );
    }
}
