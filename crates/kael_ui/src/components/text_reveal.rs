use crate::animations::{delayed_animation_progress, easings, stagger_delay};
use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealMode {
    ByWord,
    ByLine,
    ByCharacter,
}

#[derive(IntoElement)]
pub struct TextReveal {
    id: ElementId,
    text: SharedString,
    mode: RevealMode,
    stagger: Duration,
    duration: Duration,
    easing: fn(f32) -> f32,
    text_size: Option<Pixels>,
    font_weight: Option<FontWeight>,
    text_color: Option<Hsla>,
    style: StyleRefinement,
}

impl TextReveal {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            mode: RevealMode::ByWord,
            stagger: Duration::from_millis(50),
            duration: Duration::from_millis(400),
            easing: easings::ease_out_cubic,
            text_size: None,
            font_weight: None,
            text_color: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn mode(mut self, mode: RevealMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn stagger(mut self, stagger: Duration) -> Self {
        self.stagger = stagger;
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn easing(mut self, easing: fn(f32) -> f32) -> Self {
        self.easing = easing;
        self
    }

    pub fn text_size(mut self, size: Pixels) -> Self {
        if f32::from(size).is_finite() && size > px(0.0) {
            self.text_size = Some(size);
        }
        self
    }

    pub fn font_weight(mut self, weight: FontWeight) -> Self {
        self.font_weight = Some(weight);
        self
    }

    pub fn text_color(mut self, color: Hsla) -> Self {
        self.text_color = Some(color);
        self
    }
}

impl Styled for TextReveal {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TextReveal {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let accessible_text = self.text.clone();
        let segments: Vec<String> = match self.mode {
            RevealMode::ByWord => self
                .text
                .split_whitespace()
                .map(|s| s.to_string())
                .collect(),
            RevealMode::ByLine => self.text.lines().map(|s| s.to_string()).collect(),
            RevealMode::ByCharacter => self.text.chars().map(|c| c.to_string()).collect(),
        };

        let is_line_mode = self.mode == RevealMode::ByLine;
        let mode = self.mode;
        let stagger = self.stagger;
        let duration = self.duration;
        let easing = self.easing;
        let animations_enabled = window.animations_enabled();
        let user_style = self.style;

        let mut container = if is_line_mode {
            div().flex().flex_col()
        } else {
            div().flex().flex_row().flex_wrap()
        };

        container = container.accessibility(
            AccessibilityAttributes::new(AccessibilityRole::StaticText)
                .label(accessible_text.to_string()),
        );

        container = container.map(|mut el| {
            el.style().refine(&user_style);
            el
        });

        if let Some(size) = self.text_size {
            container = container.text_size(size);
        }
        if let Some(weight) = self.font_weight {
            container = container.font_weight(weight);
        }
        if let Some(color) = self.text_color {
            container = container.text_color(color);
        }

        for (i, segment) in segments.into_iter().enumerate() {
            let seg_id: ElementId = ElementId::Name(format!("{}-seg-{}", self.id, i).into());
            let delay = stagger_delay(stagger, i);
            let total_duration = duration.saturating_add(delay);

            let display_text: SharedString = if mode == RevealMode::ByWord && i > 0 {
                format!(" {}", segment).into()
            } else {
                segment.into()
            };

            let seg_el = div()
                .id(seg_id.clone())
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Group)
                        .states(AccessibilityState::HIDDEN),
                )
                .child(display_text);
            let seg_el = if animations_enabled {
                seg_el
                    .with_animation(
                        seg_id,
                        Animation::new(total_duration).with_easing(easing),
                        move |el, delta| {
                            let seg_t = delayed_animation_progress(delta, delay, duration);
                            let offset = (1.0 - seg_t) * 8.0;
                            el.opacity(seg_t).mt(px(offset))
                        },
                    )
                    .into_any_element()
            } else {
                seg_el.into_any_element()
            };

            container = container.child(seg_el);
        }

        container
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_text_size_is_ignored() {
        let reveal = TextReveal::new("reveal", "Accessible motion").text_size(px(f32::NAN));
        assert_eq!(reveal.text_size, None);
    }
}
