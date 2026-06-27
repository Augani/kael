//! NavItem component - ASTRYX navigation row primitive.

use crate::{navigation::nav_icon::NavIcon, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct NavItem {
    label: SharedString,
    icon: Option<SharedString>,
    badge: Option<SharedString>,
    selected: bool,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl NavItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            badge: None,
            selected: false,
            disabled: false,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for NavItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NavItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let disabled = self.disabled;
        let selected = self.selected;
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(36.0))
            .px(px(8.0))
            .rounded(theme.tokens.radius_md)
            .bg(if selected {
                theme.tokens.accent
            } else {
                transparent_black()
            })
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| {
                this.cursor_pointer().hover(move |style| {
                    style.bg(if selected {
                        theme.tokens.accent
                    } else {
                        overlay_hover
                    })
                })
            })
            .when_some(self.on_click.filter(|_| !disabled), |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                    handler(window, cx);
                })
            })
            .when_some(self.icon, |this, icon| {
                this.child(NavIcon::new(icon).selected(self.selected))
            })
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.0))
                    .line_height(px(20.0))
                    .font_weight(if self.selected {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(theme.tokens.foreground)
                    .child(self.label),
            )
            .when_some(self.badge, |this, badge| {
                this.child(
                    div()
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded_full()
                        .bg(theme.tokens.muted)
                        .text_size(px(11.0))
                        .line_height(px(14.0))
                        .text_color(theme.tokens.muted_foreground)
                        .child(badge),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
