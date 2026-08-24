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

const CONTEXT_MENU_MAX_HEIGHT: Pixels = px(360.0);

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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let position = self.position;
        let min_width = self.min_width;
        let on_close_handler = self.on_close.clone();
        let user_style = self.style;
        let dismissing = self.dismissing;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        let context_menu_id = self.id;
        let menu_id = ElementId::NamedChild(Box::new(context_menu_id.clone()), "menu".into());

        let interactive_indices: Rc<Vec<usize>> = Rc::new(
            self.items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    (!item.disabled && item.on_click.is_some()).then_some(index)
                })
                .collect(),
        );
        let item_focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..self.items.len())
                .map(|index| {
                    window
                        .use_keyed_state(
                            ElementId::NamedChild(
                                Box::new(menu_id.clone()),
                                format!("item-{index}").into(),
                            ),
                            cx,
                            |_, cx| cx.focus_handle(),
                        )
                        .read(cx)
                        .clone()
                })
                .collect(),
        );
        let menu_focus_handle = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(menu_id.clone()), "focus".into()),
                cx,
                |_, cx| cx.focus_handle(),
            )
            .read(cx)
            .clone();
        let menu_scroll_handle = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(menu_id.clone()), "scroll".into()),
                cx,
                |_, _| ScrollHandle::new(),
            )
            .read(cx)
            .clone();
        let tracked_menu_focus = menu_focus_handle
            .clone()
            .tab_index(0)
            .tab_stop(interactive_indices.is_empty());
        // Hosts may keep focus in their own canvas (e.g. a spreadsheet grid)
        // and route menu keys through their own handler; only take focus when
        // the window has no other focus owner, or when a menu item already
        // owns it from a previous frame.
        let menu_owns_focus = item_focus_handles
            .iter()
            .chain(std::iter::once(&menu_focus_handle))
            .any(|handle| handle.is_focused(window));
        if !menu_owns_focus && window.focused(cx).is_none() {
            if let Some(index) = interactive_indices.first() {
                window.focus(&item_focus_handles[*index]);
            } else {
                window.focus(&menu_focus_handle);
            }
        }

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
                anchored().snap_to_window().position(position).child(
                    div()
                        .id(menu_id.clone())
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Menu)
                                .label("Context menu"),
                        )
                        .tab_group()
                        .occlude()
                        .track_focus(&tracked_menu_focus)
                        .min_w(min_width)
                        .max_h(CONTEXT_MENU_MAX_HEIGHT)
                        .overflow_y_scroll()
                        .track_scroll(&menu_scroll_handle)
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
                        .when(on_close_handler.is_some(), |this| {
                            let on_close = on_close_handler.clone().unwrap();
                            this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key.as_str() == "escape"
                                    && !event.keystroke.modifiers.modified()
                                {
                                    on_close(window, cx);
                                    cx.stop_propagation();
                                    window.prevent_default();
                                }
                            })
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
                                            this.text_color(theme.tokens.muted_foreground)
                                                .opacity(0.5)
                                        })
                                        .when(!disabled && handler.is_some(), |this| {
                                            this.track_focus(&item_focus_handles[item_index])
                                                .text_color(if destructive {
                                                    theme.tokens.destructive
                                                } else {
                                                    theme.tokens.popover_foreground
                                                })
                                                .hover(move |style| style.bg(overlay_hover))
                                                .focus_visible(move |style| style.bg(overlay_hover))
                                        })
                                        .when(!disabled && handler.is_some(), |this| {
                                            let on_close = on_close.clone();
                                            let handles_for_key = item_focus_handles.clone();
                                            let indices_for_key = interactive_indices.clone();
                                            let scroll_for_key = menu_scroll_handle.clone();
                                            this.on_key_down(
                                                move |event: &KeyDownEvent, window, cx| {
                                                    if event.keystroke.modifiers.modified() {
                                                        return;
                                                    }
                                                    let key = event.keystroke.key.as_str();
                                                    if matches!(key, "enter" | "space") {
                                                        // Div emits the menu-item click on key-up.
                                                        cx.stop_propagation();
                                                        window.prevent_default();
                                                        return;
                                                    }
                                                    let current = indices_for_key
                                                        .iter()
                                                        .position(|candidate| {
                                                            *candidate == item_index
                                                        })
                                                        .unwrap_or(0);
                                                    let target = match key {
                                                        "down" if !indices_for_key.is_empty() => {
                                                            Some(
                                                                (current + 1)
                                                                    % indices_for_key.len(),
                                                            )
                                                        }
                                                        "up" if !indices_for_key.is_empty() => {
                                                            Some(
                                                                (current + indices_for_key.len()
                                                                    - 1)
                                                                    % indices_for_key.len(),
                                                            )
                                                        }
                                                        "home" if !indices_for_key.is_empty() => {
                                                            Some(0)
                                                        }
                                                        "end" if !indices_for_key.is_empty() => {
                                                            Some(indices_for_key.len() - 1)
                                                        }
                                                        _ => None,
                                                    };
                                                    if let Some(target) = target {
                                                        scroll_for_key.scroll_to_item(
                                                            indices_for_key[target],
                                                        );
                                                        window.focus(
                                                            &handles_for_key
                                                                [indices_for_key[target]],
                                                        );
                                                        window.refresh();
                                                        cx.stop_propagation();
                                                        window.prevent_default();
                                                    } else if key == "escape" {
                                                        if let Some(on_close) = &on_close {
                                                            on_close(window, cx);
                                                        }
                                                        cx.stop_propagation();
                                                        window.prevent_default();
                                                    }
                                                },
                                            )
                                        })
                                        .when(!disabled && handler.is_some(), |this| {
                                            let handler = handler.unwrap();
                                            let on_close = on_close.clone();
                                            this.on_click(move |_, window, cx| {
                                                handler(window, cx);
                                                if let Some(close_handler) = &on_close {
                                                    close_handler(window, cx);
                                                }
                                            })
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
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
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
                                                .when_some(
                                                    item.description,
                                                    |this, description| {
                                                        this.child(
                                                            div()
                                                                .text_size(px(12.0))
                                                                .line_height(px(16.0))
                                                                .overflow_hidden()
                                                                .text_ellipsis()
                                                                .whitespace_nowrap()
                                                                .text_color(
                                                                    theme.tokens.muted_foreground,
                                                                )
                                                                .child(
                                                                    StyledText::new(description)
                                                                        .accessibility_hidden(true),
                                                                ),
                                                        )
                                                    },
                                                ),
                                        )
                                        .when_some(item.end_content, |this, content| {
                                            this.child(
                                                div()
                                                    .ml_auto()
                                                    .flex_shrink_0()
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
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct ContextMenuHost;

    impl Render for ContextMenuHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            ContextMenu::new(point(px(12.0), px(16.0)))
                .item(ContextMenuItem::new("open", "Open").on_click(|_, _| {}))
                .into_any_element()
        }
    }

    struct ContextMenuActivationHost {
        activations: Rc<Cell<usize>>,
        disabled_activations: Rc<Cell<usize>>,
        closes: Rc<Cell<usize>>,
    }

    impl Render for ContextMenuActivationHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let activations = self.activations.clone();
            let disabled_activations = self.disabled_activations.clone();
            let closes = self.closes.clone();
            ContextMenu::new(point(px(12.0), px(16.0)))
                .id("activation-context-menu")
                .item(
                    ContextMenuItem::new("open", "Open")
                        .on_click(move |_, _| activations.set(activations.get() + 1)),
                )
                .item(
                    ContextMenuItem::new("disabled", "Disabled")
                        .disabled(true)
                        .on_click(move |_, _| {
                            disabled_activations.set(disabled_activations.get() + 1);
                        }),
                )
                .on_close(move |_, _| closes.set(closes.get() + 1))
                .into_any_element()
        }
    }

    #[::core::prelude::v1::test]
    fn keyboard_enter_activates_context_menu_item_once() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let activations = Rc::new(Cell::new(0));
        let disabled_activations = Rc::new(Cell::new(0));
        let closes = Rc::new(Cell::new(0));
        let (_host, window) = cx.add_window_view({
            let activations = activations.clone();
            let disabled_activations = disabled_activations.clone();
            let closes = closes.clone();
            move |_, _| ContextMenuActivationHost {
                activations,
                disabled_activations,
                closes,
            }
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("tab");
        window.update(|window, cx| window.draw(cx).clear());

        window.simulate_keystrokes("enter");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("valid keystroke"),
        });

        assert_eq!(activations.get(), 1, "Enter must activate exactly once");
        assert_eq!(closes.get(), 1, "Enter must close exactly once");

        activations.set(0);
        closes.set(0);
        window.simulate_keystrokes("space");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("space").expect("valid keystroke"),
        });
        assert_eq!(activations.get(), 1, "Space must activate exactly once");
        assert_eq!(closes.get(), 1, "Space must close exactly once");

        let (open_center, disabled_center) = window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let center = |label: &str| {
                let bounds = tree
                    .nodes
                    .values()
                    .find(|node| {
                        node.role == AccessibilityRole::MenuItem
                            && node.label.as_deref() == Some(label)
                    })
                    .and_then(|node| node.bounds.as_ref())
                    .expect("menu item should have rendered bounds");
                point(
                    px((bounds.x + bounds.width / 2.0) as f32),
                    px((bounds.y + bounds.height / 2.0) as f32),
                )
            };
            (center("Open"), center("Disabled"))
        });

        activations.set(0);
        closes.set(0);
        window.simulate_mouse_down(open_center, MouseButton::Left, Modifiers::default());
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_mouse_up(open_center, MouseButton::Left, Modifiers::default());
        assert_eq!(
            activations.get(),
            1,
            "a pointer click across a redraw must activate exactly once"
        );
        assert_eq!(closes.get(), 1, "a pointer click must close exactly once");

        activations.set(0);
        closes.set(0);
        window.simulate_click(disabled_center, Modifiers::default());
        assert_eq!(disabled_activations.get(), 0);
        assert_eq!(activations.get(), 0);
        assert_eq!(closes.get(), 0, "a disabled item must not close the menu");

        window.simulate_keystrokes("escape");
        assert_eq!(activations.get(), 0, "Escape must not activate an item");
        assert_eq!(closes.get(), 1, "Escape must still close the menu");
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

    struct ContextMenuKeyboardHost {
        closes: Rc<Cell<usize>>,
    }

    impl Render for ContextMenuKeyboardHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let closes = self.closes.clone();
            ContextMenu::new(point(px(12.0), px(16.0)))
                .id("keyboard-context-menu")
                .item(ContextMenuItem::new("first", "First").on_click(|_, _| {}))
                .item(ContextMenuItem::new("disabled", "Disabled").disabled(true))
                .item(ContextMenuItem::new("last", "Last").on_click(|_, _| {}))
                .on_close(move |_, _| closes.set(closes.get() + 1))
                .into_any_element()
        }
    }

    #[::core::prelude::v1::test]
    fn keyboard_navigation_opens_focused_skips_disabled_and_wraps() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let closes = Rc::new(Cell::new(0));
        let (_host, window) = cx.add_window_view({
            let closes = closes.clone();
            move |_, _| ContextMenuKeyboardHost { closes }
        });

        let focused_label = |window: &mut Window, cx: &mut App| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            tree.nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::MenuItem
                        && node.states.contains(AccessibilityState::FOCUSED)
                })
                .and_then(|node| node.label.clone())
        };

        // Opening the menu must focus the first enabled item, not rely on Tab.
        assert_eq!(
            window.update(focused_label).as_deref(),
            Some("First"),
            "the first enabled item must receive focus when the menu opens"
        );

        // Down must skip the disabled item.
        window.simulate_keystrokes("cmd-down");
        assert_eq!(
            window.update(focused_label).as_deref(),
            Some("First"),
            "modified arrows must remain available to the host"
        );
        window.simulate_keystrokes("down");
        assert_eq!(
            window.update(focused_label).as_deref(),
            Some("Last"),
            "arrow navigation must skip disabled items"
        );

        // Down at the end wraps to the first item.
        window.simulate_keystrokes("down");
        assert_eq!(
            window.update(focused_label).as_deref(),
            Some("First"),
            "arrow navigation must wrap"
        );

        // Up from the first item wraps to the last item.
        window.simulate_keystrokes("up");
        assert_eq!(
            window.update(focused_label).as_deref(),
            Some("Last"),
            "up from the first item must wrap to the last item"
        );

        // Home/End move to the first/last enabled item.
        window.simulate_keystrokes("home");
        assert_eq!(window.update(focused_label).as_deref(), Some("First"));
        window.simulate_keystrokes("end");
        assert_eq!(window.update(focused_label).as_deref(), Some("Last"));

        closes.set(0);
        window.simulate_keystrokes("escape");
        assert_eq!(closes.get(), 1, "Escape must close the menu");
    }

    #[::core::prelude::v1::test]
    fn menu_near_the_right_and_bottom_edge_stays_inside_the_viewport() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_host, window) = cx.add_window_view(|_, _| {
            struct EdgeMenuHost;
            impl Render for EdgeMenuHost {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    ContextMenu::new(point(px(780.0), px(580.0)))
                        .id("edge-context-menu")
                        .item(ContextMenuItem::new("open", "Open").on_click(|_, _| {}))
                        .into_any_element()
                }
            }
            EdgeMenuHost
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let menu_bounds = tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Menu)
                .and_then(|node| node.bounds.as_ref())
                .expect("menu should have rendered bounds");
            let viewport = window.viewport_size();
            assert!(
                menu_bounds.x + menu_bounds.width <= (f32::from(viewport.width) + 0.5) as f64,
                "menu must stay inside the right viewport edge: {:?} vs {viewport:?}",
                menu_bounds
            );
            assert!(
                menu_bounds.y + menu_bounds.height <= (f32::from(viewport.height) + 0.5) as f64,
                "menu must stay inside the bottom viewport edge: {:?} vs {viewport:?}",
                menu_bounds
            );
        });
    }

    #[::core::prelude::v1::test]
    fn keyboard_focus_scrolls_long_menu_items_into_view() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_host, window) = cx.add_window_view(|_, _| {
            struct LongMenuHost;
            impl Render for LongMenuHost {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    ContextMenu::new(point(px(12.0), px(16.0)))
                        .id("long-context-menu")
                        .items(
                            (0..24)
                                .map(|index| {
                                    ContextMenuItem::new(
                                        format!("item-{index}"),
                                        format!("Item {index}"),
                                    )
                                    .on_click(|_, _| {})
                                })
                                .collect(),
                        )
                        .into_any_element()
                }
            }
            LongMenuHost
        });
        window.update(|window, cx| window.draw(cx).clear());

        window.simulate_keystrokes("end");
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let menu = tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Menu)
                .and_then(|node| node.bounds.as_ref())
                .expect("menu bounds");
            let item = tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::MenuItem
                        && node.label.as_deref() == Some("Item 23")
                })
                .expect("last item");
            assert!(item.states.contains(AccessibilityState::FOCUSED));
            let item = item.bounds.as_ref().expect("last item bounds");
            assert!(
                item.y >= menu.y - 0.5 && item.y + item.height <= menu.y + menu.height + 0.5,
                "focused item must be inside the scroll viewport: {item:?} vs {menu:?}"
            );
        });
    }
}
