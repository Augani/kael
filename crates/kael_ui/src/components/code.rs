//! Code component - inline ASTRYX code token.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CodeVariant {
    #[default]
    Inline,
    Subtle,
}

#[derive(IntoElement)]
pub struct Code {
    content: SharedString,
    variant: CodeVariant,
    style: StyleRefinement,
}

impl Code {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            variant: CodeVariant::Inline,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: CodeVariant) -> Self {
        self.variant = variant;
        self
    }
}

impl Styled for Code {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Code {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .px(px(4.0))
            .py(px(0.0))
            .rounded(theme.tokens.radius_sm)
            .bg(match self.variant {
                CodeVariant::Inline => theme.tokens.secondary,
                CodeVariant::Subtle => transparent_black(),
            })
            .font_family(theme.tokens.font_mono.clone())
            .text_size(px(14.0))
            .text_color(theme.tokens.foreground)
            .child(self.content)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
