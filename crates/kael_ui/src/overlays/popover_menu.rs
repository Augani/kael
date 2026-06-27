//! Popover menu component with positioned menu items.

use crate::components::icon::Icon;
use crate::theme::Theme;
use kael::prelude::FluentBuilder;
use kael::*;
use std::rc::Rc;

pub struct PopoverMenuItem {
    pub id: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub icon: Option<SharedString>,
    pub end_content: Option<SharedString>,
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    pub disabled: bool,
}

impl PopoverMenuItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            icon: None,
            end_content: None,
            on_click: None,
            disabled: false,
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

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(IntoElement)]
pub struct PopoverMenu {
    position: Point<Pixels>,
    items: Vec<PopoverMenuItem>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl PopoverMenu {
    pub fn new(position: Point<Pixels>, items: Vec<PopoverMenuItem>) -> Self {
        Self {
            position,
            items,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl Styled for PopoverMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PopoverMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let on_close_backdrop = self.on_close.clone();
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if let Some(ref handler) = on_close_backdrop {
                    handler(window, cx);
                }
            })
            .child(
                deferred(
                    anchored().snap_to_window().position(self.position).child(
                        div().occlude().child(
                            div()
                                .min_w(px(200.0))
                                .max_w(px(300.0))
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .bg(theme.tokens.popover)
                                .text_color(theme.tokens.popover_foreground)
                                .rounded(theme.tokens.radius_lg)
                                .shadow(theme.tokens.shadow_md.to_vec())
                                .p(px(4.0))
                                .map(|this| {
                                    let mut div = this;
                                    div.style().refine(&user_style);
                                    div
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .children(self.items.into_iter().map(|item| {
                                    let on_click = item.on_click;
                                    let disabled = item.disabled;

                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .px(px(8.0))
                                        .py(px(6.0))
                                        .rounded(theme.tokens.radius_md)
                                        .cursor(if disabled {
                                            CursorStyle::Arrow
                                        } else {
                                            CursorStyle::PointingHand
                                        })
                                        .when(!disabled, |this| {
                                            this.hover(move |style| style.bg(overlay_hover))
                                        })
                                        .when(disabled, |this| this.opacity(0.5))
                                        .when_some(item.icon, |this, icon_name| {
                                            this.child(
                                                div()
                                                    .size(px(20.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .flex_shrink_0()
                                                    .child(
                                                        Icon::new(icon_name)
                                                            .size(px(16.0))
                                                            .color(theme.tokens.foreground),
                                                    ),
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
                                                        .text_size(px(14.0))
                                                        .line_height(px(20.0))
                                                        .child(item.label),
                                                )
                                                .when_some(
                                                    item.description,
                                                    |this, description| {
                                                        this.child(
                                                            div()
                                                                .text_size(px(12.0))
                                                                .line_height(px(16.0))
                                                                .text_color(
                                                                    theme.tokens.muted_foreground,
                                                                )
                                                                .child(description),
                                                        )
                                                    },
                                                ),
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
                                        .when(!disabled && on_click.is_some(), |this| {
                                            this.on_mouse_down(
                                                MouseButton::Left,
                                                move |_, window, cx| {
                                                    if let Some(ref handler) = on_click {
                                                        handler(window, cx);
                                                    }
                                                    cx.stop_propagation();
                                                },
                                            )
                                        })
                                })),
                        ),
                    ),
                )
                .with_priority(1),
            )
    }
}
