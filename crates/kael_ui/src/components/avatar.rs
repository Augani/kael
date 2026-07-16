//! Avatar component - User profile image with fallback to initials or icon.

use crate::{
    components::{icon::Icon, icon_source::IconSource},
    theme::use_theme,
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AvatarSize {
    Xs,
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
}

impl AvatarSize {
    pub fn size_px(&self) -> f32 {
        match self {
            Self::Xs => 20.0,
            Self::Sm => 24.0,
            Self::Md => 36.0,
            Self::Lg => 48.0,
            Self::Xl => 128.0,
        }
    }

    fn text_size_px(&self) -> f32 {
        self.size_px() * 0.4
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum AvatarStatusDotVariant {
    #[default]
    Success,
    Neutral,
    Error,
}

#[derive(IntoElement)]
pub struct AvatarStatusDot {
    variant: AvatarStatusDotVariant,
    avatar_size: AvatarSize,
    label: Option<SharedString>,
    icon: Option<IconSource>,
    decorative: bool,
    style: StyleRefinement,
}

impl AvatarStatusDot {
    pub fn new() -> Self {
        Self {
            variant: AvatarStatusDotVariant::Success,
            avatar_size: AvatarSize::Md,
            label: None,
            icon: None,
            decorative: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: AvatarStatusDotVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn avatar_size(mut self, size: AvatarSize) -> Self {
        self.avatar_size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn decorative(mut self, decorative: bool) -> Self {
        self.decorative = decorative;
        self
    }

    fn accessibility_label(&self) -> SharedString {
        self.label.clone().unwrap_or_else(|| match self.variant {
            AvatarStatusDotVariant::Success => "Online".into(),
            AvatarStatusDotVariant::Neutral => "Away".into(),
            AvatarStatusDotVariant::Error => "Busy".into(),
        })
    }

    fn metrics(avatar_size: AvatarSize) -> (Pixels, Pixels, Pixels) {
        let size = avatar_size.size_px();
        if size <= 36.0 {
            (px(10.0), px(1.0), px(0.0))
        } else if size <= 72.0 {
            (px(20.0), px(2.0), px(12.0))
        } else {
            (px(32.0), px(4.0), px(18.0))
        }
    }
}

impl Default for AvatarStatusDot {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for AvatarStatusDot {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AvatarStatusDot {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let accessibility_label = self.accessibility_label();
        let user_style = self.style;
        let (dot_size, border_width, icon_size) = Self::metrics(self.avatar_size);
        let color = match self.variant {
            AvatarStatusDotVariant::Success => theme.tokens.success,
            AvatarStatusDotVariant::Neutral => theme.tokens.muted_foreground,
            AvatarStatusDotVariant::Error => theme.tokens.destructive,
        };

        let accessibility = if self.decorative {
            AccessibilityAttributes::new(AccessibilityRole::Group)
                .states(AccessibilityState::HIDDEN)
        } else {
            AccessibilityAttributes::new(AccessibilityRole::Image)
                .label(accessibility_label.to_string())
        };

        div()
            .accessibility(accessibility)
            .relative()
            .size(dot_size)
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(color)
            .border(border_width)
            .border_color(theme.tokens.background)
            .when(icon_size > px(0.0), |this| {
                this.when_some(self.icon, |this, icon| {
                    this.child(
                        Icon::new(icon)
                            .size(icon_size)
                            .color(theme.tokens.background),
                    )
                })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct Avatar {
    src: Option<SharedString>,
    fallback_src: Option<SharedString>,
    alt: Option<SharedString>,
    name: Option<SharedString>,
    fallback_text: Option<SharedString>,
    size: AvatarSize,
    status: Option<AnyElement>,
    status_dot: Option<AvatarStatusDot>,
    colorful: bool,
    style: StyleRefinement,
}

impl Avatar {
    pub fn new() -> Self {
        Self {
            src: None,
            fallback_src: None,
            alt: None,
            name: None,
            fallback_text: None,
            size: AvatarSize::default(),
            status: None,
            status_dot: None,
            colorful: false,
            style: StyleRefinement::default(),
        }
    }

    /// Use a name-derived colored background instead of the default neutral
    /// gray fallback. Off by default to match the Astryx look.
    pub fn colorful(mut self, colorful: bool) -> Self {
        self.colorful = colorful;
        self
    }

    pub fn src(mut self, src: impl Into<SharedString>) -> Self {
        self.src = Some(src.into());
        self
    }

    pub fn fallback_src(mut self, src: impl Into<SharedString>) -> Self {
        self.fallback_src = Some(src.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn fallbackSrc(self, src: impl Into<SharedString>) -> Self {
        self.fallback_src(src)
    }

    pub fn alt(mut self, alt: impl Into<SharedString>) -> Self {
        self.alt = Some(alt.into());
        self
    }

    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn fallback_text(mut self, text: impl Into<SharedString>) -> Self {
        self.fallback_text = Some(text.into());
        self
    }

    pub fn size(mut self, size: AvatarSize) -> Self {
        self.size = size;
        self
    }

    pub fn status(mut self, status: impl IntoElement) -> Self {
        self.status = Some(status.into_any_element());
        self
    }

    pub fn status_dot(mut self, status: AvatarStatusDot) -> Self {
        self.status_dot = Some(status);
        self
    }

    fn extract_initials(name: &str) -> String {
        let words: Vec<&str> = name.split_whitespace().collect();

        if words.len() >= 2 {
            format!(
                "{}{}",
                words[0].chars().next().unwrap_or('?').to_uppercase(),
                words
                    .last()
                    .unwrap_or(&words[1])
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
            )
        } else if let Some(first_word) = words.first() {
            first_word
                .chars()
                .take(1)
                .collect::<String>()
                .to_uppercase()
        } else {
            "??".to_string()
        }
    }

    fn name_to_color(name: &str, _theme: &crate::theme::Theme) -> Hsla {
        const COLOR_PALETTE: &[(f32, f32, f32)] = &[
            (0.0, 0.7, 0.6),
            (120.0, 0.6, 0.5),
            (240.0, 0.7, 0.6),
            (30.0, 0.7, 0.6),
            (180.0, 0.6, 0.5),
            (300.0, 0.7, 0.6),
            (60.0, 0.6, 0.5),
            (330.0, 0.7, 0.6),
        ];

        let hash: u64 = name.bytes().map(|b| b as u64).sum();
        let index = (hash as usize) % COLOR_PALETTE.len();
        let (hue, saturation, lightness) = COLOR_PALETTE[index];

        hsla(hue / 360.0, saturation, lightness, 1.0)
    }
}

impl Default for Avatar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Avatar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Avatar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let size_px = self.size.size_px();
        let text_size_px = self.size.text_size_px();
        let user_style = self.style;

        let accessible_name = self
            .alt
            .clone()
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| "Avatar".into());

        let (content, bg_color, text_color) = if let Some(src) = self.src {
            let image_size = px(size_px);
            (
                img(src)
                    .size(image_size)
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
                    .when_some(self.fallback_src, move |image, fallback_src| {
                        image.with_fallback(move || {
                            img(fallback_src.clone())
                                .size(image_size)
                                .rounded_full()
                                .object_fit(ObjectFit::Cover)
                                .into_any_element()
                        })
                    })
                    .into_any_element(),
                theme.tokens.muted,
                theme.tokens.foreground,
            )
        } else if let Some(src) = self.fallback_src {
            (
                img(src)
                    .size(px(size_px))
                    .rounded_full()
                    .object_fit(ObjectFit::Cover)
                    .into_any_element(),
                theme.tokens.muted,
                theme.tokens.foreground,
            )
        } else if let Some(name) = self.name {
            let initials = Self::extract_initials(&name);
            let (bg_color, text_color) = if self.colorful {
                (
                    Self::name_to_color(&name, &theme).opacity(0.8),
                    theme.tokens.background,
                )
            } else {
                (theme.tokens.secondary, theme.tokens.muted_foreground)
            };

            (
                div()
                    .text_size(px(text_size_px))
                    .font_weight(FontWeight::MEDIUM)
                    .child(StyledText::new(initials).accessibility_hidden(true))
                    .into_any_element(),
                bg_color,
                text_color,
            )
        } else if let Some(fallback) = self.fallback_text {
            (
                div()
                    .text_size(px(text_size_px))
                    .font_weight(FontWeight::MEDIUM)
                    .child(StyledText::new(fallback).accessibility_hidden(true))
                    .into_any_element(),
                theme.tokens.muted,
                theme.tokens.muted_foreground,
            )
        } else {
            (
                div()
                    .text_size(px(text_size_px * 0.7))
                    .child(Icon::new("user").size(px(size_px * 0.6)))
                    .into_any_element(),
                theme.tokens.muted,
                theme.tokens.muted_foreground,
            )
        };

        let status = self.status;
        let status_description = self
            .status_dot
            .as_ref()
            .map(AvatarStatusDot::accessibility_label);
        let status_dot = self.status_dot;

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Image)
            .label(accessible_name.to_string());
        if let Some(description) = status_description {
            accessibility = accessibility.description(description.to_string());
        }

        div()
            .accessibility(accessibility)
            .relative()
            .size(px(size_px))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(bg_color)
            .text_color(text_color)
            .font_family(theme.tokens.font_family.clone())
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(
                div()
                    .size(px(size_px))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .overflow_hidden()
                    .child(content),
            )
            .when_some(status, |this, status| {
                this.child(
                    div()
                        .absolute()
                        .right(px(-2.0))
                        .bottom(px(-2.0))
                        .child(status),
                )
            })
            .when_some(status_dot, |this, status| {
                let edge_offset = size_px * ((1.0 - 1.0 / std::f32::consts::SQRT_2) / 2.0);
                let dot_size = if size_px <= 36.0 {
                    10.0
                } else if size_px <= 72.0 {
                    20.0
                } else {
                    32.0
                };
                let offset = edge_offset - dot_size / 2.0;
                this.child(
                    div()
                        .absolute()
                        .right(px(offset))
                        .bottom(px(offset))
                        .child(status.avatar_size(self.size).decorative(true)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn initials_are_stable_for_names_and_empty_input() {
        assert_eq!(Avatar::extract_initials("Ada Lovelace"), "AL");
        assert_eq!(Avatar::extract_initials("  Grace   Hopper  "), "GH");
        assert_eq!(Avatar::extract_initials("Prince"), "P");
        assert_eq!(Avatar::extract_initials(" "), "??");
    }

    #[::core::prelude::v1::test]
    fn status_dots_have_meaningful_default_labels() {
        assert_eq!(
            AvatarStatusDot::new().accessibility_label().as_ref(),
            "Online"
        );
        assert_eq!(
            AvatarStatusDot::new()
                .variant(AvatarStatusDotVariant::Error)
                .accessibility_label()
                .as_ref(),
            "Busy"
        );
    }
}
