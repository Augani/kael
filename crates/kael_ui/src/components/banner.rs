//! Banner - a full-width, sentiment-tinted announcement bar.

use crate::astryx::Hue;
use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum BannerVariant {
    #[default]
    Info,
    Success,
    Warning,
    Error,
    /// Brand-accent announcement.
    Announcement,
}

impl BannerVariant {
    fn default_icon(self) -> &'static str {
        match self {
            BannerVariant::Info => "info",
            BannerVariant::Success => "circle-check",
            BannerVariant::Warning => "triangle-alert",
            BannerVariant::Error => "circle-alert",
            BannerVariant::Announcement => "megaphone",
        }
    }
}

#[derive(IntoElement)]
pub struct Banner {
    variant: BannerVariant,
    message: SharedString,
    icon: Option<IconSource>,
    show_icon: bool,
    dismissible: bool,
    on_dismiss: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    action: Option<(SharedString, Rc<dyn Fn(&mut Window, &mut App)>)>,
    style: StyleRefinement,
}

impl Banner {
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            variant: BannerVariant::default(),
            message: message.into(),
            icon: None,
            show_icon: true,
            dismissible: false,
            on_dismiss: None,
            action: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: BannerVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn success(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Success)
    }

    pub fn warning(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Warning)
    }

    pub fn error(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Error)
    }

    pub fn announcement(message: impl Into<SharedString>) -> Self {
        Self::new(message).variant(BannerVariant::Announcement)
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn show_icon(mut self, show: bool) -> Self {
        self.show_icon = show;
        self
    }

    pub fn on_dismiss(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(handler));
        self.dismissible = true;
        self
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }
}

impl Styled for Banner {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Banner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let dark = theme.tokens.background.l < 0.5;
        let hue = match self.variant {
            BannerVariant::Info | BannerVariant::Announcement => Hue::Blue,
            BannerVariant::Success => Hue::Green,
            BannerVariant::Warning => Hue::Yellow,
            BannerVariant::Error => Hue::Red,
        };
        let colors = hue.colors(dark);
        let bg = colors.background;
        let accent = colors.text;
        let user_style = self.style;
        let icon_source = self
            .icon
            .unwrap_or_else(|| IconSource::Named(self.variant.default_icon().into()));

        div()
            .flex()
            .w_full()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(10.0))
            .rounded(theme.tokens.radius_md)
            .bg(bg)
            .text_color(accent)
            .when(self.show_icon, |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .child(Icon::new(icon_source).size(px(18.0)).color(accent)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(theme.tokens.font_family.clone())
                    .child(self.message.clone()),
            )
            .when_some(self.action.clone(), |this, (label, handler)| {
                this.child(
                    div()
                        .id("banner-action")
                        .flex_shrink_0()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent)
                        .cursor(CursorStyle::PointingHand)
                        .hover(|style| style.opacity(0.8))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            (handler)(window, cx);
                        })
                        .child(label),
                )
            })
            .when(self.dismissible, |this| {
                let dismiss_handler = self.on_dismiss.clone();
                this.child(
                    div()
                        .id("banner-dismiss")
                        .flex_shrink_0()
                        .cursor(CursorStyle::PointingHand)
                        .rounded(theme.tokens.radius_sm)
                        .p(px(4.0))
                        .hover(|style| style.bg(theme.tokens.muted))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            if let Some(ref handler) = dismiss_handler {
                                (handler)(window, cx);
                            }
                        })
                        .child(
                            Icon::new("x")
                                .size(px(16.0))
                                .color(theme.tokens.muted_foreground),
                        ),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
