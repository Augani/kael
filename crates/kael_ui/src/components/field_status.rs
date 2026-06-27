//! Field status message component matching Astryx form feedback surfaces.

use crate::{
    astryx,
    components::{icon::Icon, icon_source::IconSource},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FieldStatusVariant {
    #[default]
    Attached,
    Detached,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldStatusTone {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(IntoElement)]
pub struct FieldStatus {
    tone: FieldStatusTone,
    message: SharedString,
    variant: FieldStatusVariant,
    show_icon: bool,
    style: StyleRefinement,
}

impl FieldStatus {
    pub fn new(tone: FieldStatusTone, message: impl Into<SharedString>) -> Self {
        Self {
            tone,
            message: message.into(),
            variant: FieldStatusVariant::Attached,
            show_icon: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn info(message: impl Into<SharedString>) -> Self {
        Self::new(FieldStatusTone::Info, message)
    }

    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new(FieldStatusTone::Success, message)
    }

    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(FieldStatusTone::Warning, message)
    }

    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(FieldStatusTone::Error, message)
    }

    pub fn variant(mut self, variant: FieldStatusVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn attached(mut self) -> Self {
        self.variant = FieldStatusVariant::Attached;
        self
    }

    pub fn detached(mut self) -> Self {
        self.variant = FieldStatusVariant::Detached;
        self
    }

    pub fn show_icon(mut self, show_icon: bool) -> Self {
        self.show_icon = show_icon;
        self
    }
}

impl Styled for FieldStatus {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FieldStatus {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let (status_color, icon_name) = match self.tone {
            FieldStatusTone::Info => (theme.tokens.primary, "info"),
            FieldStatusTone::Success => (theme.tokens.success, "circle-check"),
            FieldStatusTone::Warning => (theme.tokens.warning, "triangle-alert"),
            FieldStatusTone::Error => (theme.tokens.destructive, "circle-alert"),
        };
        let user_style = self.style;

        div()
            .flex()
            .items_start()
            .gap(px(6.0))
            .text_size(px(12.0))
            .line_height(relative(1.35))
            .font_family(theme.tokens.font_family.clone())
            .text_color(status_color)
            .bg(astryx::status_muted(status_color))
            .when(self.variant == FieldStatusVariant::Attached, |this| {
                this.mt(px(-6.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .px(px(8.0))
                    .rounded_b(theme.tokens.radius_md)
            })
            .when(self.variant == FieldStatusVariant::Detached, |this| {
                this.mt(px(4.0)).p(px(8.0)).rounded(theme.tokens.radius_md)
            })
            .when(self.show_icon, |this| {
                this.child(
                    div()
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Icon::new(IconSource::Named(icon_name.into()))
                                .size(px(14.0))
                                .color(status_color),
                        ),
                )
            })
            .child(div().flex_1().child(self.message))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
