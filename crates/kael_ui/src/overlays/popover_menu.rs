//! Popover menu component with positioned menu items.

use crate::components::icon::Icon;
use crate::theme::Theme;
use kael::prelude::FluentBuilder;
use kael::*;
use std::{cell::Cell, panic::Location, rc::Rc};

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
    id: ElementId,
    position: Point<Pixels>,
    items: Vec<PopoverMenuItem>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl PopoverMenu {
    #[track_caller]
    pub fn new(position: Point<Pixels>, items: Vec<PopoverMenuItem>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "popover-menu:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            position,
            items,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let on_close_backdrop = self.on_close.clone();
        let on_close_menu = self.on_close.clone();
        // A closing menu may receive several native requests before the next
        // frame removes it. All items share one claim for this mounted render.
        let activation_claimed = self.on_close.as_ref().map(|_| Rc::new(Cell::new(false)));
        let backdrop_claimed = activation_claimed.clone();
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        let menu_id = self.id.clone();
        let position = sanitize_position(self.position);
        let items = self.items;
        let interactive_indices: Rc<Vec<usize>> = Rc::new(
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    (!item.disabled && item.on_click.is_some()).then_some(index)
                })
                .collect(),
        );
        let focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..items.len())
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
        let tracked_menu_focus = menu_focus_handle
            .clone()
            .tab_index(0)
            .tab_stop(interactive_indices.is_empty());
        if !focus_handles.iter().any(|handle| handle.is_focused(window)) {
            if let Some(index) = interactive_indices.first() {
                window.focus(&focus_handles[*index]);
            } else {
                window.focus(&menu_focus_handle);
            }
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(menu_id.clone()),
                "backdrop".into(),
            ))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                dismiss_menu(&on_close_backdrop, &backdrop_claimed, window, cx);
            })
            .child(
                deferred(
                    anchored().snap_to_window().position(position).child(
                        div().occlude().child(
                            div()
                                .id(menu_id.clone())
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Menu)
                                        .label("Menu"),
                                )
                                .track_focus(&tracked_menu_focus)
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
                                .when(on_close_menu.is_some(), |this| {
                                    let on_close = on_close_menu.clone();
                                    let activation_claimed = activation_claimed.clone();
                                    this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if event.keystroke.key.as_str() == "escape" {
                                            dismiss_menu(
                                                &on_close,
                                                &activation_claimed,
                                                window,
                                                cx,
                                            );
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                })
                                .children(items.into_iter().enumerate().map(|(index, item)| {
                                    render_popover_menu_item(
                                        index,
                                        item,
                                        menu_id.clone(),
                                        focus_handles.clone(),
                                        interactive_indices.clone(),
                                        on_close_menu.clone(),
                                        activation_claimed.clone(),
                                        overlay_hover,
                                        &theme,
                                        window,
                                    )
                                })),
                        ),
                    ),
                )
                .with_priority(1),
            )
    }
}

fn sanitize_position(position: Point<Pixels>) -> Point<Pixels> {
    fn coordinate(value: Pixels) -> Pixels {
        let value = f32::from(value);
        if value.is_finite() {
            px(value.max(0.0))
        } else {
            px(0.0)
        }
    }

    point(coordinate(position.x), coordinate(position.y))
}

fn dismiss_menu(
    on_close: &Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    activation_claimed: &Option<Rc<Cell<bool>>>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(handler) = on_close.as_ref()
        && !activation_claimed
            .as_ref()
            .is_some_and(|claimed| claimed.replace(true))
    {
        handler(window, cx);
    }
}

fn render_popover_menu_item(
    index: usize,
    item: PopoverMenuItem,
    menu_id: ElementId,
    focus_handles: Rc<Vec<FocusHandle>>,
    interactive_indices: Rc<Vec<usize>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    activation_claimed: Option<Rc<Cell<bool>>>,
    overlay_hover: Hsla,
    theme: &Theme,
    window: &mut Window,
) -> AnyElement {
    let on_click = item.on_click;
    let interactive = !item.disabled && on_click.is_some();
    let focus_handle = focus_handles[index].clone();
    let focus_on_activation = focus_handle.clone();
    let handles_for_key = focus_handles.clone();
    let indices_for_key = interactive_indices.clone();
    let close_key = on_close.clone();
    let close_key_claimed = activation_claimed.clone();
    let activate: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |window, cx| {
        if activation_claimed
            .as_ref()
            .is_some_and(|claimed| claimed.replace(true))
        {
            return;
        }
        window.focus(&focus_on_activation);
        if let Some(handler) = on_click.as_ref() {
            handler(window, cx);
        }
        if let Some(handler) = on_close.as_ref() {
            handler(window, cx);
        }
    });
    let activate_mouse = activate.clone();
    let activate_native = activate.clone();
    let label = item.label.clone();
    let mut state = AccessibilityState::NONE;
    if !interactive {
        state |= AccessibilityState::DISABLED;
    }
    if focus_handle.is_focused(window) {
        state |= AccessibilityState::FOCUSED;
    }
    let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::MenuItem)
        .label(label.to_string())
        .states(state);
    if let Some(description) = item.description.as_ref() {
        accessibility = accessibility.description(description.to_string());
    }
    if interactive {
        accessibility =
            accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
    }

    div()
        .id(ElementId::NamedChild(
            Box::new(menu_id),
            format!("item-{index}").into(),
        ))
        .accessibility(accessibility)
        .when(interactive, |row| {
            row.track_focus(&focus_handle.tab_index(0).tab_stop(true))
        })
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(theme.tokens.radius_md)
        .cursor(if interactive {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .when(interactive, |this| {
            this.hover(move |style| style.bg(overlay_hover))
        })
        .when(!interactive, |this| this.opacity(0.5))
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
                        .child(StyledText::new(label).accessibility_hidden(true)),
                )
                .when_some(item.description, |this, description| {
                    this.child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child(StyledText::new(description).accessibility_hidden(true)),
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
        .when(interactive, |this| {
            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                activate_mouse(window, cx);
                cx.stop_propagation();
            })
            .on_accessibility_action(AccessibilityAction::Click, move |_, window, cx| {
                activate_native(window, cx);
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let current = indices_for_key
                    .iter()
                    .position(|candidate| *candidate == index)
                    .unwrap_or(0);
                let target = match key {
                    "down" if !indices_for_key.is_empty() => {
                        Some((current + 1) % indices_for_key.len())
                    }
                    "up" if !indices_for_key.is_empty() => {
                        Some((current + indices_for_key.len() - 1) % indices_for_key.len())
                    }
                    "home" if !indices_for_key.is_empty() => Some(0),
                    "end" if !indices_for_key.is_empty() => Some(indices_for_key.len() - 1),
                    _ => None,
                };
                if let Some(target) = target {
                    window.focus(&handles_for_key[indices_for_key[target]]);
                    cx.stop_propagation();
                } else if matches!(key, "enter" | "space") {
                    activate(window, cx);
                    cx.stop_propagation();
                } else if key == "escape" {
                    dismiss_menu(&close_key, &close_key_claimed, window, cx);
                    cx.stop_propagation();
                }
            })
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MenuActionHost {
        open: bool,
        closes_on_activation: bool,
        hide_on_close: bool,
        activations: Rc<Cell<[usize; 3]>>,
        closes: Rc<Cell<usize>>,
    }

    impl Render for MenuActionHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if !self.open {
                return div().size_full().into_any_element();
            }
            let items = ["First", "Second", "Disabled"]
                .into_iter()
                .enumerate()
                .map(|(index, label)| {
                    let activations = self.activations.clone();
                    PopoverMenuItem::new(label, label)
                        .disabled(index == 2)
                        .on_click(move |_, _| {
                            let mut counts = activations.get();
                            counts[index] += 1;
                            activations.set(counts);
                        })
                })
                .chain([PopoverMenuItem::new("inert", "Inert")])
                .collect();
            let menu = PopoverMenu::new(point(px(12.0), px(16.0)), items).id("native-menu");
            if self.closes_on_activation {
                let host = cx.weak_entity();
                menu.on_close(move |_, cx| {
                    let _ = host.update(cx, |this, cx| {
                        this.closes.set(this.closes.get() + 1);
                        if this.hide_on_close {
                            this.open = false;
                            cx.notify();
                        }
                    });
                })
                .into_any_element()
            } else {
                menu.into_any_element()
            }
        }
    }

    struct MenuContainer {
        menu: Option<Entity<MenuActionHost>>,
    }

    impl Render for MenuContainer {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().children(self.menu.clone())
        }
    }

    fn item_ids(window: &Window) -> [AccessibilityId; 4] {
        let tree = window.accessibility_tree();
        ["First", "Second", "Disabled", "Inert"].map(|label| {
            let item = tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::MenuItem && node.label.as_deref() == Some(label)
                })
                .expect("menu item should be mounted");
            let enabled = matches!(label, "First" | "Second");
            assert_eq!(item.actions.contains(&AccessibilityAction::Click), enabled);
            assert_eq!(
                window.has_accessibility_action_handler(item.id, AccessibilityAction::Click),
                enabled,
                "advertised native Click must have a handler"
            );
            assert_eq!(item.states.contains(AccessibilityState::DISABLED), !enabled);
            item.id
        })
    }

    #[::core::prelude::v1::test]
    fn native_click_closes_once_across_items_and_retires_handlers() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let activations = Rc::new(Cell::new([0; 3]));
        let closes = Rc::new(Cell::new(0));
        let menu = cx.new({
            let activations = activations.clone();
            let closes = closes.clone();
            move |_| MenuActionHost {
                open: true,
                closes_on_activation: true,
                hide_on_close: true,
                activations,
                closes,
            }
        });
        let weak_menu = menu.downgrade();
        let (container, window) = cx.add_window_view({
            let menu = menu.clone();
            move |_, _| MenuContainer { menu: Some(menu) }
        });
        let ids = window.update(|window, cx| {
            window.draw(cx).clear();
            let ids = item_ids(window);
            for id in [ids[2], ids[3], ids[0], ids[0], ids[1]] {
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Click,
                ));
            }
            ids
        });
        window.run_until_parked();
        assert_eq!(activations.get(), [1, 0, 0]);
        assert_eq!(closes.get(), 1);
        window.update(|window, cx| {
            assert!(!menu.read(cx).open);
            window.draw(cx).clear();
            for id in ids {
                assert!(window.accessibility_tree().get(id).is_none());
                assert!(!window.has_accessibility_action_handler(id, AccessibilityAction::Click));
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Click,
                ));
            }
            container.update(cx, |container, cx| {
                container.menu = None;
                cx.notify();
            });
            window.draw(cx).clear();
        });
        drop(menu);
        window.run_until_parked();
        assert!(
            weak_menu.upgrade().is_none(),
            "retired handlers must not retain the menu view"
        );
        assert_eq!(activations.get(), [1, 0, 0]);
        assert_eq!(closes.get(), 1);
    }

    #[::core::prelude::v1::test]
    fn persistent_native_menu_allows_repeated_actions() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let activations = Rc::new(Cell::new([0; 3]));
        let (_host, window) = cx.add_window_view({
            let activations = activations.clone();
            move |_, _| MenuActionHost {
                open: true,
                closes_on_activation: false,
                hide_on_close: true,
                activations,
                closes: Rc::new(Cell::new(0)),
            }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let ids = item_ids(window);
            for id in [ids[0], ids[0], ids[1], ids[2], ids[3]] {
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Click,
                ));
            }
        });
        window.run_until_parked();
        assert_eq!(activations.get(), [2, 1, 0]);
        // Native activation focuses the same item as pointer activation; the
        // persistent menu keeps that item's ordinary keyboard path usable.
        window.simulate_keystrokes("enter");
        assert_eq!(activations.get(), [2, 2, 0]);
        window.simulate_keystrokes("space");
        assert_eq!(activations.get(), [2, 3, 0]);
    }

    #[::core::prelude::v1::test]
    fn pointer_down_keeps_immediate_activation_and_shares_native_close_guard() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let activations = Rc::new(Cell::new([0; 3]));
        let closes = Rc::new(Cell::new(0));
        let (_host, window) = cx.add_window_view({
            let activations = activations.clone();
            let closes = closes.clone();
            move |_, _| MenuActionHost {
                open: true,
                closes_on_activation: true,
                hide_on_close: true,
                activations,
                closes,
            }
        });
        let (first_center, second) = window.update(|window, cx| {
            window.draw(cx).clear();
            let ids = item_ids(window);
            let bounds = window
                .accessibility_tree()
                .get(ids[0])
                .unwrap()
                .bounds
                .unwrap();
            (
                point(
                    px((bounds.x + bounds.width / 2.0) as f32),
                    px((bounds.y + bounds.height / 2.0) as f32),
                ),
                ids[1],
            )
        });
        window.simulate_mouse_down(first_center, MouseButton::Left, Modifiers::default());
        assert_eq!(
            activations.get(),
            [1, 0, 0],
            "pointer activation must precede mouse-up"
        );
        assert_eq!(closes.get(), 1);
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                second,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
        assert_eq!(activations.get(), [1, 0, 0]);
        assert_eq!(closes.get(), 1);
    }

    fn assert_dismissal_rejects_pending_activation(escape: bool) {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::astryx_neutral()));
        let activations = Rc::new(Cell::new([0; 3]));
        let closes = Rc::new(Cell::new(0));
        let (_host, window) = cx.add_window_view({
            let activations = activations.clone();
            let closes = closes.clone();
            move |_, _| MenuActionHost {
                open: true,
                closes_on_activation: true,
                // A close callback can start an animation or async teardown
                // before removing its menu. The pending frame remains valid.
                hide_on_close: false,
                activations,
                closes,
            }
        });
        let (first, backdrop) = window.update(|window, cx| {
            window.draw(cx).clear();
            let viewport = window.viewport_size();
            (
                item_ids(window)[0],
                point(viewport.width - px(1.0), viewport.height - px(1.0)),
            )
        });
        window.update(|window, cx| {
            let event = if escape {
                PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse("escape").unwrap(),
                    is_held: false,
                })
            } else {
                PlatformInput::MouseDown(MouseDownEvent {
                    position: backdrop,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            };
            window.dispatch_event(event, cx);
            assert_eq!(activations.get(), [0; 3]);
            assert_eq!(closes.get(), 1);
            // Keep the old frame valid to exercise the shared close guard,
            // rather than relying on a completed repaint to reject the request.
            assert!(window.accessibility_tree().get(first).is_some());
            assert!(window.has_accessibility_action_handler(first, AccessibilityAction::Click));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                first,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
        assert_eq!(activations.get(), [0; 3]);
        assert_eq!(closes.get(), 1);
    }

    #[::core::prelude::v1::test]
    fn escape_dismissal_rejects_queued_native_activation() {
        assert_dismissal_rejects_pending_activation(true);
    }

    #[::core::prelude::v1::test]
    fn backdrop_dismissal_rejects_queued_native_activation() {
        assert_dismissal_rejects_pending_activation(false);
    }

    #[::core::prelude::v1::test]
    fn menu_positions_are_finite_and_inside_the_viewport_origin() {
        assert_eq!(
            sanitize_position(point(px(f32::NAN), px(f32::INFINITY))),
            point(px(0.0), px(0.0))
        );
        assert_eq!(
            sanitize_position(point(px(-20.0), px(30.0))),
            point(px(0.0), px(30.0))
        );
    }
}
