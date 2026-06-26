use crate::theme::use_theme;
use kael::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KBDSize {
    Sm,
    #[default]
    Md,
    Lg,
}

pub struct KBD {
    label: SharedString,
    size: KBDSize,
    style: StyleRefinement,
}

impl KBD {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            size: KBDSize::default(),
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: KBDSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for KBD {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for KBD {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let user_style = self.style;

        let (text_size, px_val, height, min_w) = match self.size {
            KBDSize::Sm => (px(11.0), px(4.0), px(18.0), px(16.0)),
            KBDSize::Md => (px(12.0), px(4.0), px(20.0), px(20.0)),
            KBDSize::Lg => (px(13.0), px(6.0), px(24.0), px(24.0)),
        };

        let mut el = div()
            .flex()
            .items_center()
            .justify_center()
            .px(px_val)
            .h(height)
            .min_w(min_w)
            .bg(theme.tokens.secondary)
            .rounded(px(6.0))
            .text_size(text_size)
            .font_family(theme.tokens.font_family.clone())
            .text_color(theme.tokens.muted_foreground)
            .font_weight(FontWeight::MEDIUM)
            .line_height(relative(1.0))
            .child(self.label);
        el.style().refine(&user_style);
        el
    }
}
