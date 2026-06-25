//! Badge component - status labels, tags, and categorical tokens.

use crate::astryx::Hue;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BadgeVariant {
    Default,
    Secondary,
    Destructive,
    Success,
    Warning,
    Outline,
    /// A soft categorical badge from the astryx hue palette: a subtle tinted
    /// fill, a saturated ring, and high-contrast text.
    Hue(Hue),
}

pub struct Badge {
    label: SharedString,
    variant: BadgeVariant,
    style: StyleRefinement,
}

impl Badge {
    pub fn new<T: Into<SharedString>>(label: T) -> Self {
        Self {
            label: label.into(),
            variant: BadgeVariant::Default,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Color this badge with a categorical hue from the astryx palette.
    pub fn hue(mut self, hue: Hue) -> Self {
        self.variant = BadgeVariant::Hue(hue);
        self
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for Badge {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let user_style = self.style;
        let dark = theme.tokens.background.l < 0.5;

        let (bg_color, fg_color, border_color, has_border) = match self.variant {
            BadgeVariant::Default => (
                theme.tokens.primary,
                theme.tokens.primary_foreground,
                kael::transparent_black(),
                false,
            ),
            BadgeVariant::Secondary => (
                theme.tokens.secondary,
                theme.tokens.secondary_foreground,
                kael::transparent_black(),
                false,
            ),
            BadgeVariant::Destructive => (
                theme.tokens.destructive,
                theme.tokens.destructive_foreground,
                kael::transparent_black(),
                false,
            ),
            BadgeVariant::Success => (
                theme.tokens.success,
                theme.tokens.success_foreground,
                kael::transparent_black(),
                false,
            ),
            BadgeVariant::Warning => (
                theme.tokens.warning,
                theme.tokens.warning_foreground,
                kael::transparent_black(),
                false,
            ),
            BadgeVariant::Outline => (
                kael::transparent_black(),
                theme.tokens.foreground,
                theme.tokens.border,
                true,
            ),
            BadgeVariant::Hue(hue) => {
                let c = hue.colors(dark);
                (c.background, c.text, c.border, true)
            }
        };

        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(8.0))
            .py(px(2.0))
            .rounded_full()
            .text_size(px(12.0))
            .line_height(px(16.0))
            .font_family(theme.tokens.font_family.clone())
            .font_weight(FontWeight::MEDIUM)
            .bg(bg_color)
            .text_color(fg_color)
            .when(has_border, |el| el.border_1().border_color(border_color))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(self.label)
    }
}
