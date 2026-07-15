//! Badge component - status labels, tags, and categorical tokens.

use crate::astryx::Hue;
use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BadgeVariant {
    Default,
    Neutral,
    Secondary,
    Info,
    Destructive,
    Error,
    Success,
    Warning,
    Outline,
    Blue,
    Cyan,
    Green,
    Orange,
    Pink,
    Purple,
    Red,
    Teal,
    Yellow,
    /// A soft categorical badge from the astryx hue palette: a subtle tinted
    /// fill, a saturated ring, and high-contrast text.
    Hue(Hue),
}

fn categorical_hue(variant: BadgeVariant) -> Option<Hue> {
    match variant {
        BadgeVariant::Info | BadgeVariant::Blue => Some(Hue::Blue),
        BadgeVariant::Cyan => Some(Hue::Cyan),
        BadgeVariant::Green => Some(Hue::Green),
        BadgeVariant::Orange => Some(Hue::Orange),
        BadgeVariant::Pink => Some(Hue::Pink),
        BadgeVariant::Purple => Some(Hue::Purple),
        BadgeVariant::Red => Some(Hue::Red),
        BadgeVariant::Teal => Some(Hue::Teal),
        BadgeVariant::Yellow => Some(Hue::Yellow),
        BadgeVariant::Hue(hue) => Some(hue),
        _ => None,
    }
}

pub struct Badge {
    label: SharedString,
    variant: BadgeVariant,
    icon: Option<IconSource>,
    accessibility_hidden: bool,
    style: StyleRefinement,
}

impl Badge {
    pub fn new<T: Into<SharedString>>(label: T) -> Self {
        Self {
            label: label.into(),
            variant: BadgeVariant::Default,
            icon: None,
            accessibility_hidden: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    /// Color this badge with a categorical hue from the astryx palette.
    pub fn hue(mut self, hue: Hue) -> Self {
        self.variant = BadgeVariant::Hue(hue);
        self
    }

    /// Display an icon before the label, matching astryx Badge's optional
    /// leading icon slot.
    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Hide the badge label when a semantic parent already exposes the same
    /// status. The badge remains visible.
    pub fn accessibility_hidden(mut self, hidden: bool) -> Self {
        self.accessibility_hidden = hidden;
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

        let categorical_colors = categorical_hue(self.variant).map(|hue| hue.colors(dark));
        let (bg_color, fg_color, border_color, has_border) =
            if let Some(colors) = categorical_colors {
                (colors.background, colors.text, colors.border, true)
            } else {
                match self.variant {
                    BadgeVariant::Default | BadgeVariant::Neutral => (
                        theme.tokens.secondary,
                        theme.tokens.secondary_foreground,
                        kael::transparent_black(),
                        false,
                    ),
                    BadgeVariant::Secondary => (
                        theme.tokens.primary,
                        theme.tokens.primary_foreground,
                        kael::transparent_black(),
                        false,
                    ),
                    BadgeVariant::Destructive | BadgeVariant::Error => (
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
                    BadgeVariant::Info
                    | BadgeVariant::Blue
                    | BadgeVariant::Cyan
                    | BadgeVariant::Green
                    | BadgeVariant::Orange
                    | BadgeVariant::Pink
                    | BadgeVariant::Purple
                    | BadgeVariant::Red
                    | BadgeVariant::Teal
                    | BadgeVariant::Yellow
                    | BadgeVariant::Hue(_) => unreachable!("categorical variants resolve above"),
                }
            };

        div()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.0))
            .h(px(20.0))
            .px(px(8.0))
            .py(px(0.0))
            .rounded_full()
            .text_size(px(12.0))
            .line_height(px(16.0))
            .font_family(theme.tokens.font_family.clone())
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .bg(bg_color)
            .text_color(fg_color)
            .when(has_border, |el| el.border_1().border_color(border_color))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .when_some(self.icon, |this, source| {
                this.child(Icon::new(source).size(px(12.0)).color(fg_color))
            })
            .child(StyledText::new(self.label).accessibility_hidden(self.accessibility_hidden))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn categorical_variants_resolve_to_the_expected_hues() {
        assert_eq!(categorical_hue(BadgeVariant::Info), Some(Hue::Blue));
        assert_eq!(categorical_hue(BadgeVariant::Purple), Some(Hue::Purple));
        assert_eq!(
            categorical_hue(BadgeVariant::Hue(Hue::Gray)),
            Some(Hue::Gray)
        );
        assert_eq!(categorical_hue(BadgeVariant::Secondary), None);
    }
}
