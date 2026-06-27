//! Context menu component for right-click menus.

use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;
use std::time::Duration;

use crate::animations::easings;
use crate::components::icon::Icon;
use crate::theme::Theme;

#[derive(Clone)]
pub struct ContextMenuItem {
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub icon: Option<SharedString>,
    pub end_content: Option<SharedString>,
    pub disabled: bool,
    pub divider: bool,
    pub destructive: bool,
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

pub type ContextMenuProps = ContextMenu;
pub type ContextMenuItemProps = ContextMenuItem;
pub type ContextMenuItemData = ContextMenuItem;
pub type ContextMenuDivider = ContextMenuItem;
pub type ContextMenuOption = ContextMenuItem;
pub type ContextMenuSection = Vec<ContextMenuItem>;

impl ContextMenuItem {
    pub fn new(_id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            icon: None,
            end_content: None,
            disabled: false,
            divider: false,
            destructive: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn end_content(mut self, content: impl Into<SharedString>) -> Self {
        self.end_content = Some(content.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl Into<SharedString>) -> Self {
        self.end_content(content)
    }

    pub fn shortcut(self, shortcut: impl Into<SharedString>) -> Self {
        self.end_content(shortcut)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    pub fn divider(mut self, divider: bool) -> Self {
        self.divider = divider;
        self
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn separator() -> Self {
        Self {
            label: "".into(),
            description: None,
            icon: None,
            end_content: None,
            disabled: true,
            divider: true,
            destructive: false,
            on_click: None,
        }
    }
}

#[derive(IntoElement)]
pub struct ContextMenu {
    position: Point<Pixels>,
    items: Vec<ContextMenuItem>,
    min_width: Pixels,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    dismissing: bool,
    style: StyleRefinement,
}

impl ContextMenu {
    pub fn new(position: Point<Pixels>) -> Self {
        Self {
            position,
            items: Vec::new(),
            min_width: px(160.0),
            on_close: None,
            dismissing: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn dismissing(mut self, dismissing: bool) -> Self {
        self.dismissing = dismissing;
        self
    }

    pub fn item(mut self, item: ContextMenuItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: Vec<ContextMenuItem>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn menu_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    #[allow(non_snake_case)]
    pub fn menuWidth(self, width: Pixels) -> Self {
        self.menu_width(width)
    }

    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl Styled for ContextMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let position = self.position;
        let min_width = self.min_width;
        let on_close_handler = self.on_close.clone();
        let user_style = self.style;
        let dismissing = self.dismissing;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        div()
            .absolute()
            .inset_0()
            .when(on_close_handler.is_some(), |this: Div| {
                let handler = on_close_handler.clone().unwrap();
                let handler2 = handler.clone();
                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    handler(window, cx);
                })
                .on_mouse_down(MouseButton::Right, move |_, window, cx| {
                    handler2(window, cx);
                })
            })
            .child(
                div()
                    .absolute()
                    .occlude()
                    .left(position.x)
                    .top(position.y)
                    .min_w(min_width)
                    .bg(theme.tokens.popover)
                    .rounded(theme.tokens.radius_lg)
                    .shadow(theme.tokens.shadow_md.to_vec())
                    .p(px(4.0))
                    .gap(px(2.0))
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, _| {})
                    .children(self.items.into_iter().map(|item| {
                        if item.label.is_empty() && item.divider {
                            return div()
                                .h(px(1.0))
                                .my(px(4.0))
                                .bg(theme.tokens.border)
                                .into_any_element();
                        }

                        let on_close = self.on_close.clone();
                        let handler = item.on_click.clone();
                        let disabled = item.disabled;
                        let destructive = item.destructive;

                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(8.0))
                            .py(px(6.0))
                            .rounded(theme.tokens.radius_md)
                            .text_size(px(14.0))
                            .line_height(px(20.0))
                            .cursor(if disabled {
                                CursorStyle::Arrow
                            } else {
                                CursorStyle::PointingHand
                            })
                            .when(disabled, |this: Div| {
                                this.text_color(theme.tokens.muted_foreground).opacity(0.5)
                            })
                            .when(!disabled, |this: Div| {
                                this.text_color(if destructive {
                                    theme.tokens.destructive
                                } else {
                                    theme.tokens.popover_foreground
                                })
                                .hover(move |style| style.bg(overlay_hover))
                            })
                            .when(!disabled && handler.is_some(), |this: Div| {
                                let handler = handler.unwrap();
                                let on_close = on_close.clone();
                                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                    handler(window, cx);
                                    if let Some(close_handler) = &on_close {
                                        close_handler(window, cx);
                                    }
                                })
                            })
                            .when_some(item.icon, |this: Div, icon| {
                                this.child(
                                    div()
                                        .size(px(20.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .child(Icon::new(icon.to_string()).size(px(16.0)).color(
                                            if disabled {
                                                theme.tokens.muted_foreground
                                            } else if destructive {
                                                theme.tokens.destructive
                                            } else {
                                                theme.tokens.popover_foreground
                                            },
                                        )),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(1.0))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .line_height(px(20.0))
                                            .text_color(if disabled {
                                                theme.tokens.muted_foreground
                                            } else if destructive {
                                                theme.tokens.destructive
                                            } else {
                                                theme.tokens.popover_foreground
                                            })
                                            .child(item.label),
                                    )
                                    .when_some(item.description, |this, description| {
                                        this.child(
                                            div()
                                                .text_size(px(12.0))
                                                .line_height(px(16.0))
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(description),
                                        )
                                    }),
                            )
                            .when_some(item.end_content, |this, content| {
                                this.child(
                                    div()
                                        .ml_auto()
                                        .text_size(px(12.0))
                                        .line_height(px(16.0))
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(content),
                                )
                            })
                            .into_any_element()
                    }))
                    .with_animation(
                        if self.dismissing {
                            "ctx-menu-exit"
                        } else {
                            "ctx-menu-enter"
                        },
                        Animation::new(Duration::from_millis(if self.dismissing {
                            100
                        } else {
                            120
                        }))
                        .with_easing(if self.dismissing {
                            easings::ease_in_cubic as fn(f32) -> f32
                        } else {
                            easings::ease_out_cubic as fn(f32) -> f32
                        }),
                        move |el, delta| {
                            if dismissing {
                                el.opacity(1.0 - delta).mt(px(4.0 * delta))
                            } else {
                                el.opacity(delta).mt(px(4.0 * (1.0 - delta)))
                            }
                        },
                    ),
            )
    }
}
