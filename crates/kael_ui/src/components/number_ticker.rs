use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::animations::easings;
use crate::fonts::mono_font_family;
use crate::theme::Theme;

#[derive(IntoElement)]
pub struct NumberTicker {
    id: ElementId,
    base: Div,
    value: i64,
    separator: Option<char>,
    prefix: Option<SharedString>,
    suffix: Option<SharedString>,
    duration: Duration,
    text_size: Pixels,
}

impl NumberTicker {
    pub fn new(id: impl Into<ElementId>, value: i64) -> Self {
        Self {
            id: id.into(),
            base: div(),
            value,
            separator: None,
            prefix: None,
            suffix: None,
            duration: Duration::from_millis(600),
            text_size: px(16.0),
        }
    }

    pub fn separator(mut self, sep: char) -> Self {
        self.separator = Some(sep);
        self
    }

    pub fn prefix(mut self, prefix: impl Into<SharedString>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = if duration.is_zero() {
            Duration::from_millis(600)
        } else {
            duration
        };
        self
    }

    /// Set the digit size. The rolling viewport tracks this value to avoid clipping.
    pub fn text_size(mut self, size: Pixels) -> Self {
        let size = f32::from(size);
        if size.is_finite() && size > 0.0 {
            self.text_size = px(size);
        }
        self
    }
}

impl Styled for NumberTicker {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

fn format_with_separator(value: i64, separator: Option<char>) -> Vec<DigitOrSeparator> {
    let is_negative = value < 0;
    let abs_str = value.unsigned_abs().to_string();
    let mut result = Vec::new();

    if is_negative {
        result.push(DigitOrSeparator::Separator('-'));
    }

    let digits: Vec<u8> = abs_str.bytes().map(|b| b - b'0').collect();
    let len = digits.len();

    for (i, &digit) in digits.iter().enumerate() {
        result.push(DigitOrSeparator::Digit(digit));
        if let Some(sep) = separator {
            let remaining = len - 1 - i;
            if remaining > 0 && remaining.is_multiple_of(3) {
                result.push(DigitOrSeparator::Separator(sep));
            }
        }
    }

    result
}

#[derive(Clone, Copy)]
enum DigitOrSeparator {
    Digit(u8),
    Separator(char),
}

fn ticker_accessible_value(
    chars: &[DigitOrSeparator],
    prefix: Option<&str>,
    suffix: Option<&str>,
) -> String {
    let mut value = prefix.unwrap_or_default().to_string();
    for item in chars {
        match item {
            DigitOrSeparator::Digit(digit) => value.push(char::from(b'0' + *digit)),
            DigitOrSeparator::Separator(separator) => value.push(*separator),
        }
    }
    value.push_str(suffix.unwrap_or_default());
    value
}

impl RenderOnce for NumberTicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let chars = format_with_separator(self.value, self.separator);
        let duration = if window.animations_enabled() {
            self.duration
        } else {
            Duration::ZERO
        };
        let digit_height = self.text_size * 1.25;
        let column_height = digit_height * 10.0;
        let accessible_value = ticker_accessible_value(
            &chars,
            self.prefix.as_ref().map(SharedString::as_ref),
            self.suffix.as_ref().map(SharedString::as_ref),
        );

        self.base
            .id(self.id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::StaticText).label(accessible_value),
            )
            .flex()
            .flex_row()
            .items_center()
            .text_size(self.text_size)
            .text_color(theme.tokens.foreground)
            .font_family(mono_font_family())
            .when_some(self.prefix.clone(), |el, prefix| {
                el.child(div().child(StyledText::new(prefix).accessibility_hidden(true)))
            })
            .children(chars.iter().enumerate().map(move |(pos, item)| {
                match *item {
                    DigitOrSeparator::Separator(ch) => div()
                        .flex_shrink_0()
                        .child(
                            StyledText::new(SharedString::from(String::from(ch)))
                                .accessibility_hidden(true),
                        )
                        .into_any_element(),
                    DigitOrSeparator::Digit(digit) => {
                        let target_offset = -(digit_height * digit as f32);

                        div()
                            .flex_shrink_0()
                            .h(digit_height)
                            .overflow_hidden()
                            .child(
                                div()
                                    .id(("digit-col", pos as u32))
                                    .relative()
                                    .flex_shrink_0()
                                    .flex()
                                    .flex_col()
                                    .h(column_height)
                                    .children((0..10u8).map(|d| {
                                        div()
                                            .h(digit_height)
                                            .flex_shrink_0()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                StyledText::new(SharedString::from(d.to_string()))
                                                    .accessibility_hidden(true),
                                            )
                                    }))
                                    .with_animation(
                                        ("digit-roll", pos as u32),
                                        Animation::new(duration)
                                            .with_easing(easings::ease_out_cubic),
                                        move |el, delta| {
                                            let offset = target_offset * delta;
                                            el.top(offset)
                                        },
                                    ),
                            )
                            .into_any_element()
                    }
                }
            }))
            .when_some(self.suffix, |el, suffix| {
                el.child(div().child(StyledText::new(suffix).accessibility_hidden(true)))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn accessible_value_matches_the_visible_formatted_number() {
        let chars = format_with_separator(-1_482_390, Some(','));
        assert_eq!(
            ticker_accessible_value(&chars, Some("$"), Some(" MRR")),
            "$-1,482,390 MRR"
        );
    }

    #[::core::prelude::v1::test]
    fn zero_duration_uses_the_safe_default() {
        assert_eq!(
            NumberTicker::new("ticker", 42)
                .duration(Duration::ZERO)
                .duration,
            Duration::from_millis(600)
        );
    }

    #[::core::prelude::v1::test]
    fn text_size_rejects_invalid_geometry() {
        assert_eq!(
            NumberTicker::new("ticker", 42)
                .text_size(px(f32::NAN))
                .text_size,
            px(16.0)
        );
        assert_eq!(
            NumberTicker::new("ticker", 42)
                .text_size(px(-1.0))
                .text_size,
            px(16.0)
        );
    }
}
