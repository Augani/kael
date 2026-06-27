//! Blockquote component - quoted content with optional attribution.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct Blockquote {
    content: AnyElement,
    cite: Option<AnyElement>,
    style: StyleRefinement,
}

impl Blockquote {
    pub fn new(content: impl IntoElement) -> Self {
        Self {
            content: content.into_any_element(),
            cite: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn cite(mut self, cite: impl IntoElement) -> Self {
        self.cite = Some(cite.into_any_element());
        self
    }
}

impl Styled for Blockquote {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Blockquote {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .border_l(px(2.0))
            .border_color(theme.tokens.foreground.opacity(0.32))
            .pl(px(16.0))
            .font_family(theme.tokens.font_family.clone())
            .text_color(theme.tokens.muted_foreground)
            .child(self.content)
            .when_some(self.cite, |this, cite| {
                this.child(
                    div()
                        .mt(px(8.0))
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .child(cite),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
