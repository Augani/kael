//! OverflowList component - visible item row with a truthful overflow reveal.

use crate::theme::Theme;
use kael::{InteractiveElement as _, prelude::FluentBuilder as _, *};
use std::panic::Location;

#[derive(IntoElement)]
pub struct OverflowList {
    id: ElementId,
    items: Vec<AnyElement>,
    max_visible: usize,
    gap: Pixels,
    style: StyleRefinement,
}

impl OverflowList {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "overflow-list:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            items: Vec::new(),
            max_visible: 3,
            gap: px(6.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn item(mut self, item: impl IntoElement) -> Self {
        self.items.push(item.into_any_element());
        self
    }

    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible;
        self
    }

    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        self.gap = gap.into();
        self
    }
}

impl Default for OverflowList {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for OverflowList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for OverflowList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let overflow = self.items.len().saturating_sub(self.max_visible);
        let user_style = self.style;
        let mut visible_items = self.items;
        let split = visible_items.len().min(self.max_visible);
        let overflowed_items = visible_items.split_off(split);
        let badge_id = ElementId::NamedChild(Box::new(self.id.clone()), "reveal".into());
        let badge_focus = window
            .use_keyed_state(badge_id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let open_state = window.use_keyed_state(
            ElementId::NamedChild(Box::new(self.id.clone()), "open".into()),
            cx,
            |_, _| false,
        );
        let is_open = *open_state.read(cx);
        let open_for_toggle = open_state.clone();
        let open_for_menu = open_state.clone();
        let viewport = window.viewport_size();
        let badge_focus_for_menu = badge_focus.clone();
        let badge_bounds = window.use_keyed_state(
            ElementId::NamedChild(Box::new(self.id.clone()), "bounds".into()),
            cx,
            |_, _| None,
        );
        let measured_bounds = *badge_bounds.read(cx);
        let mut badge_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        badge_state |= if badge_focus.is_focused(window) {
            AccessibilityState::FOCUSED
        } else {
            AccessibilityState::NONE
        };

        div()
            .flex()
            .items_center()
            .gap(self.gap)
            .overflow_hidden()
            .children(visible_items)
            .when(overflow > 0, |this| {
                this.child(
                    div()
                        .relative()
                        .child(
                            canvas_with_prepaint(
                                move |bounds, _, cx| {
                                    badge_bounds.update(cx, |state, _| *state = Some(bounds));
                                },
                                |_, (), _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        )
                        .child(
                            div()
                                .id(badge_id)
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Button)
                                        .label(format!("Show {overflow} more items"))
                                        .states(badge_state)
                                        .actions(vec![
                                            AccessibilityAction::Focus,
                                            AccessibilityAction::Click,
                                        ]),
                                )
                                .track_focus(&badge_focus.tab_index(0).tab_stop(true))
                                .flex_shrink_0()
                                .px(px(7.0))
                                .py(px(2.0))
                                .rounded_full()
                                .bg(theme.tokens.muted)
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .text_color(theme.tokens.muted_foreground)
                                .cursor(CursorStyle::PointingHand)
                                .hover(|style| {
                                    style.bg(crate::astryx::overlay_hover(
                                        theme.tokens.background.l < 0.5,
                                    ))
                                })
                                .on_key_down({
                                    let open_for_keys = open_for_toggle.clone();
                                    move |event: &KeyDownEvent, window, cx| match event
                                        .keystroke
                                        .key
                                        .as_str()
                                    {
                                        "enter" | "space"
                                            if !event.keystroke.modifiers.modified() =>
                                        {
                                            // Div emits the click on key-up;
                                            // only suppress native scrolling
                                            // on key-down.
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                        "escape" if is_open => {
                                            open_for_keys.update(cx, |open, cx| {
                                                *open = false;
                                                cx.notify();
                                            });
                                            window.refresh();
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                        _ => {}
                                    }
                                })
                                .on_click({
                                    let open_for_click = open_for_toggle.clone();
                                    move |_, window, cx| {
                                        open_for_click.update(cx, |open, cx| {
                                            *open = !*open;
                                            cx.notify();
                                        });
                                        window.refresh();
                                    }
                                })
                                .child(format!("+{overflow}")),
                        ),
                )
                .when(is_open, |this| {
                    this.when_some(measured_bounds, |this, badge_bounds| {
                        let open_for_backdrop = open_for_menu.clone();
                        let open_for_escape = open_for_menu.clone();
                        let focus_after_backdrop = badge_focus_for_menu.clone();
                        let focus_after_escape = badge_focus_for_menu.clone();
                        this.child(deferred(
                            anchored()
                                .snap_to_window()
                                .position(Point::default())
                                .child(
                                    // The deferred backdrop must cover the
                                    // viewport, not merely the compact row's
                                    // layout bounds, so a click anywhere
                                    // outside the reveal reliably dismisses it.
                                    div()
                                        .w(viewport.width)
                                        .h(viewport.height)
                                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                            open_for_backdrop.update(cx, |open, cx| {
                                                *open = false;
                                                cx.notify();
                                            });
                                            window.focus(&focus_after_backdrop);
                                            window.refresh();
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        })
                                        .on_key_down(move |event: &KeyDownEvent, window, cx| {
                                            if event.keystroke.key == "escape"
                                                && !event.keystroke.modifiers.modified()
                                            {
                                                open_for_escape.update(cx, |open, cx| {
                                                    *open = false;
                                                    cx.notify();
                                                });
                                                window.focus(&focus_after_escape);
                                                window.refresh();
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            }
                                        })
                                        .child(
                                            anchored()
                                                .snap_to_window()
                                                .position(point(
                                                    badge_bounds.left(),
                                                    badge_bounds.bottom() + px(4.0),
                                                ))
                                                .child(
                                                    div()
                                                        .id("overflow-list-menu")
                                                        .accessibility(
                                                            AccessibilityAttributes::new(
                                                                AccessibilityRole::List,
                                                            )
                                                            .label("Overflowed items"),
                                                        )
                                                        .occlude()
                                                        .flex()
                                                        .flex_col()
                                                        .gap(px(4.0))
                                                        .p(px(8.0))
                                                        .max_h(px(300.0))
                                                        .overflow_y_scroll()
                                                        .bg(theme.tokens.popover)
                                                        .text_color(theme.tokens.popover_foreground)
                                                        .rounded(theme.tokens.radius_lg)
                                                        .shadow(theme.tokens.shadow_md.to_vec())
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            |_, _, cx| {
                                                                cx.stop_propagation();
                                                            },
                                                        )
                                                        .children(overflowed_items),
                                                ),
                                        ),
                                ),
                        ))
                    })
                })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::TestAppContext;
    use std::cell::Cell;
    use std::rc::Rc;

    struct OverflowHost {
        activations: Rc<Cell<usize>>,
    }

    impl Render for OverflowHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let activations = self.activations.clone();
            let mut list = OverflowList::new().id("host-overflow-list").max_visible(2);
            for index in 0..5 {
                let activations = activations.clone();
                list = list.item(
                    div()
                        .id(SharedString::from(format!("overflow-item-{index}")))
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::ListItem)
                                .label(format!("Item {index}")),
                        )
                        .px(px(6.0))
                        .child(format!("Item {index}"))
                        .when(index >= 2, |this| {
                            let activations = activations.clone();
                            this.accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Button)
                                    .label(format!("Activate item {index}"))
                                    .actions(vec![AccessibilityAction::Click]),
                            )
                            .on_click(move |_, _, _| {
                                activations.set(activations.get() + 1);
                            })
                        }),
                );
            }
            list
        }
    }

    #[::core::prelude::v1::test]
    fn overflowed_items_are_reachable_through_the_reveal() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let activations = Rc::new(Cell::new(0));
        let (_host, window) = cx.add_window_view({
            let activations = activations.clone();
            move |_, _| OverflowHost { activations }
        });
        window.update(|window, cx| window.draw(cx).clear());

        window.update(|window, _cx| {
            let tree = window.accessibility_tree();
            assert!(
                tree.nodes
                    .values()
                    .any(|node| node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Show 3 more items")),
                "the badge must advertise what it reveals"
            );
            assert!(
                !tree
                    .nodes
                    .values()
                    .any(|node| node.label.as_deref() == Some("Activate item 4")),
                "overflowed items must not be interactive while hidden"
            );
        });

        // Keyboard: Tab reaches the badge, Enter opens the reveal.
        window.simulate_keystrokes("tab");
        window.simulate_keystrokes("enter");
        // Key-down is consumed for native scrolling, but activation belongs
        // to the framework's standard key-up click path.
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(
                !window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .any(|node| node.label.as_deref() == Some("Activate item 4"))
            );
        });
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("valid keystroke"),
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert!(
                tree.nodes
                    .values()
                    .any(|node| node.label.as_deref() == Some("Activate item 4")),
                "opening the reveal must expose the overflowed items"
            );
            let badge = tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Show 3 more items"))
                .expect("badge present");
            assert!(badge.states.contains(AccessibilityState::EXPANDED));
        });

        // Click-away dismissal must cover the full viewport, not just the
        // compact list row. It also restores focus to the reveal button.
        window.simulate_click(point(px(700.0), px(500.0)), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert!(
                !tree
                    .nodes
                    .values()
                    .any(|node| node.label.as_deref() == Some("Activate item 4")),
                "a distant backdrop click must dismiss the reveal"
            );
            let badge = tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Show 3 more items"))
                .expect("badge present");
            assert!(badge.states.contains(AccessibilityState::COLLAPSED));
            assert!(badge.states.contains(AccessibilityState::FOCUSED));
        });

        // Reopen for the pointer activation check below.
        window.simulate_keystrokes("enter");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("valid keystroke"),
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
        });

        // Pointer: click an overflowed item through the reveal.
        let item_center = window.update(|window, _cx| {
            let tree = window.accessibility_tree();
            let bounds = tree
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Activate item 4"))
                .and_then(|node| node.bounds.as_ref())
                .expect("revealed item has bounds");
            point(
                px((bounds.x + bounds.width / 2.0) as f32),
                px((bounds.y + bounds.height / 2.0) as f32),
            )
        });
        window.simulate_click(item_center, Modifiers::default());
        assert_eq!(activations.get(), 1, "revealed item must be clickable");

        // Escape closes the reveal.
        window.simulate_keystrokes("escape");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert!(
                !tree
                    .nodes
                    .values()
                    .any(|node| node.label.as_deref() == Some("Activate item 4")),
                "Escape must hide the overflowed items again"
            );
        });
    }
}
