//! Context menu component for right-click menus.

use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;
use std::time::Duration;

use crate::animations::easings;
use crate::components::icon::Icon;
use crate::theme::Theme;

fn sanitize_menu_position(position: Point<Pixels>) -> Point<Pixels> {
    let coordinate = |value: Pixels| {
        let value = f32::from(value);
        px(if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        })
    };
    point(coordinate(position.x), coordinate(position.y))
}

fn sanitize_menu_width(width: Pixels) -> Pixels {
    let width = f32::from(width);
    if width.is_finite() && width > 0.0 {
        px(width.clamp(120.0, 640.0))
    } else {
        px(160.0)
    }
}

#[derive(Clone)]
pub struct ContextMenuItem {
    id: Option<SharedString>,
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
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: Some(id.into()),
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
            id: None,
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
    id: ElementId,
    position: Point<Pixels>,
    items: Vec<ContextMenuItem>,
    min_width: Pixels,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    dismissing: bool,
    style: StyleRefinement,
}

impl ContextMenu {
    #[track_caller]
    pub fn new(position: Point<Pixels>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "context-menu:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            position: sanitize_menu_position(position),
            items: Vec::new(),
            min_width: px(160.0),
            on_close: None,
            dismissing: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
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
        self.min_width = sanitize_menu_width(width);
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
        let context_menu_id = self.id;
        let menu_id = ElementId::NamedChild(Box::new(context_menu_id.clone()), "menu".into());

        div()
            .id(context_menu_id)
            .absolute()
            .inset_0()
            .when(on_close_handler.is_some(), |this| {
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
                    .id(menu_id.clone())
                    .absolute()
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::Menu).label("Context menu"),
                    )
                    .tab_group()
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
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .children(
                        self.items
                            .into_iter()
                            .enumerate()
                            .map(|(item_index, item)| {
                                if item.label.is_empty() && item.divider {
                                    return div()
                                        .id(ElementId::NamedChild(
                                            Box::new(menu_id.clone()),
                                            format!("separator-{item_index}").into(),
                                        ))
                                        .accessibility(AccessibilityAttributes::new(
                                            AccessibilityRole::Separator,
                                        ))
                                        .h(px(1.0))
                                        .my(px(4.0))
                                        .bg(theme.tokens.border)
                                        .into_any_element();
                                }

                                let on_close = self.on_close.clone();
                                let handler = item.on_click.clone();
                                let disabled = item.disabled;
                                let destructive = item.destructive;
                                let item_id = item.id.unwrap_or_else(|| item.label.clone());
                                let item_id =
                                    ElementId::NamedChild(Box::new(menu_id.clone()), item_id);
                                let mut accessibility_state = AccessibilityState::NONE;
                                if disabled {
                                    accessibility_state |= AccessibilityState::DISABLED;
                                }
                                let mut accessibility =
                                    AccessibilityAttributes::new(AccessibilityRole::MenuItem)
                                        .label(item.label.to_string())
                                        .states(accessibility_state);
                                if let Some(description) = item.description.as_ref() {
                                    accessibility =
                                        accessibility.description(description.to_string());
                                }
                                if !disabled && handler.is_some() {
                                    accessibility = accessibility.actions(vec![
                                        AccessibilityAction::Focus,
                                        AccessibilityAction::Click,
                                    ]);
                                }

                                div()
                                    .id(item_id)
                                    .accessibility(accessibility)
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
                                    .when(disabled, |this| {
                                        this.text_color(theme.tokens.muted_foreground).opacity(0.5)
                                    })
                                    .when(!disabled && handler.is_some(), |this| {
                                        this.focusable()
                                            .tab_index(item_index as isize)
                                            .tab_stop(true)
                                            .text_color(if destructive {
                                                theme.tokens.destructive
                                            } else {
                                                theme.tokens.popover_foreground
                                            })
                                            .hover(move |style| style.bg(overlay_hover))
                                            .focus_visible(move |style| style.bg(overlay_hover))
                                    })
                                    .when(!disabled && on_close.is_some(), |this| {
                                        let on_close = on_close.clone().unwrap();
                                        this.on_key_down(move |event, window, cx| {
                                            if event.keystroke.key.as_str() == "escape" {
                                                on_close(window, cx);
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            }
                                        })
                                    })
                                    .when(!disabled && handler.is_some(), |this| {
                                        let handler = handler.unwrap();
                                        let on_key = handler.clone();
                                        let on_close = on_close.clone();
                                        let on_close_for_key = on_close.clone();
                                        this.on_click(move |_, window, cx| {
                                            handler(window, cx);
                                            if let Some(close_handler) = &on_close {
                                                close_handler(window, cx);
                                            }
                                        })
                                        .on_key_down(
                                            move |event, window, cx| {
                                                if matches!(
                                                    event.keystroke.key.as_str(),
                                                    "enter" | "space"
                                                ) {
                                                    on_key(window, cx);
                                                    if let Some(close_handler) = &on_close_for_key {
                                                        close_handler(window, cx);
                                                    }
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                }
                                            },
                                        )
                                    })
                                    .when_some(item.icon, |this, icon: SharedString| {
                                        this.child(
                                            div()
                                                .size(px(20.0))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .flex_shrink_0()
                                                .child(
                                                    Icon::new(icon.to_string())
                                                        .size(px(16.0))
                                                        .color(if disabled {
                                                            theme.tokens.muted_foreground
                                                        } else if destructive {
                                                            theme.tokens.destructive
                                                        } else {
                                                            theme.tokens.popover_foreground
                                                        }),
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
                                                    .line_height(px(20.0))
                                                    .text_color(if disabled {
                                                        theme.tokens.muted_foreground
                                                    } else if destructive {
                                                        theme.tokens.destructive
                                                    } else {
                                                        theme.tokens.popover_foreground
                                                    })
                                                    .child(
                                                        StyledText::new(item.label)
                                                            .accessibility_hidden(true),
                                                    ),
                                            )
                                            .when_some(item.description, |this, description| {
                                                this.child(
                                                    div()
                                                        .text_size(px(12.0))
                                                        .line_height(px(16.0))
                                                        .text_color(theme.tokens.muted_foreground)
                                                        .child(
                                                            StyledText::new(description)
                                                                .accessibility_hidden(true),
                                                        ),
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
                            }),
                    )
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

#[cfg(test)]
mod tests {
    use super::*;

    struct ContextMenuHost;

    impl Render for ContextMenuHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            ContextMenu::new(point(px(12.0), px(16.0)))
                .item(ContextMenuItem::new("open", "Open").on_click(|_, _| {}))
                .into_any_element()
        }
    }

    #[::core::prelude::v1::test]
    fn accessibility_hierarchy_is_stable_across_frames() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_host, window) = cx.add_window_view(|_, _| ContextMenuHost);

        let hierarchy = |window: &mut Window, cx: &mut App| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let menu = tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Menu)
                .expect("context menu should expose menu semantics");
            let item = tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::MenuItem)
                .expect("context menu should expose item semantics");
            (menu.id, item.id, item.parent)
        };

        let first = window.update(hierarchy);
        let second = window.update(hierarchy);
        assert_eq!(first, second);
        assert_eq!(first.2, Some(first.0));
    }

    #[::core::prelude::v1::test]
    fn invalid_menu_geometry_uses_safe_bounds() {
        let menu = ContextMenu::new(point(px(f32::NAN), px(-20.0))).menu_width(px(f32::INFINITY));
        assert_eq!(menu.position, point(px(0.0), px(0.0)));
        assert_eq!(menu.min_width, px(160.0));
        assert_eq!(
            ContextMenu::new(point(px(0.0), px(0.0)))
                .menu_width(px(1.0))
                .min_width,
            px(120.0)
        );
    }
}
