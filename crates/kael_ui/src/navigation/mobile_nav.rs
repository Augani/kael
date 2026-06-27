//! MobileNav component - compact ASTRYX navigation shell.

use crate::{components::icon::Icon, navigation::nav_item::NavItem, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct MobileNavToggle {
    open: bool,
    label: SharedString,
    on_toggle: Option<Box<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl MobileNavToggle {
    pub fn new() -> Self {
        Self {
            open: false,
            label: "Open navigation".into(),
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl Default for MobileNavToggle {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for MobileNavToggle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MobileNavToggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.open;
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        div()
            .relative()
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.tokens.radius_md)
            .cursor_pointer()
            .hover(move |style| style.bg(overlay_hover))
            .when_some(self.on_toggle, |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                    handler(!open, window, cx);
                })
            })
            .child(
                div()
                    .absolute()
                    .left(px(-10000.0))
                    .top(px(0.0))
                    .size(px(1.0))
                    .overflow_hidden()
                    .child(self.label),
            )
            .child(
                Icon::new(if open { "x" } else { "menu" })
                    .size(px(16.0))
                    .color(theme.tokens.foreground),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct MobileNav {
    title: SharedString,
    open: bool,
    items: Vec<NavItem>,
    on_toggle: Option<Box<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl MobileNav {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            open: false,
            items: Vec::new(),
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn item(mut self, item: NavItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl Styled for MobileNav {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MobileNav {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.open;
        let user_style = self.style;

        div()
            .flex()
            .flex_col()
            .bg(theme.tokens.popover)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(48.0))
                    .px(px(8.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .text_size(px(15.0))
                            .line_height(px(20.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .child(self.title),
                    )
                    .child(
                        MobileNavToggle::new()
                            .open(open)
                            .when_some(self.on_toggle, |toggle, handler| toggle.on_toggle(handler)),
                    ),
            )
            .when(open, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .p(px(8.0))
                        .children(self.items),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
