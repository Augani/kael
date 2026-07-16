use crate::{
    components::{button::ButtonVariant, icon_button::IconButton},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

const MIN_PANE_SIZE: f32 = 50.0;
const DIVIDER_SIZE: Pixels = px(1.0);
const DIVIDER_HIT_AREA: Pixels = px(12.0);
const KEYBOARD_RESIZE_STEP: f32 = 0.02;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SplitDirection {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
pub enum SplitPaneEvent {
    Resized { ratio: f32 },
    PaneCollapsed { pane: CollapsiblePane },
    PaneExpanded { pane: CollapsiblePane },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CollapsiblePane {
    First,
    Second,
}

pub struct SplitPaneState {
    ratio: f32,
    direction: SplitDirection,
    min_first: f32,
    max_first: f32,
    min_second: f32,
    max_second: f32,
    is_dragging: bool,
    collapsed_pane: Option<CollapsiblePane>,
    ratio_before_collapse: f32,
    bounds: Bounds<Pixels>,
    focus_handle: FocusHandle,
}

impl SplitPaneState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            ratio: 0.5,
            direction: SplitDirection::Horizontal,
            min_first: MIN_PANE_SIZE,
            max_first: f32::MAX,
            min_second: MIN_PANE_SIZE,
            max_second: f32::MAX,
            is_dragging: false,
            collapsed_pane: None,
            ratio_before_collapse: 0.5,
            bounds: Bounds::default(),
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    pub fn set_ratio(&mut self, ratio: f32, cx: &mut Context<Self>) {
        if !ratio.is_finite() {
            return;
        }
        let clamped = self.clamp_ratio(ratio);
        if (self.ratio - clamped).abs() > f32::EPSILON {
            self.ratio = clamped;
            cx.notify();
        }
    }

    pub fn direction(&self) -> SplitDirection {
        self.direction
    }

    pub fn set_direction(&mut self, direction: SplitDirection, cx: &mut Context<Self>) {
        self.direction = direction;
        cx.notify();
    }

    pub fn set_min_first(&mut self, min: f32, cx: &mut Context<Self>) {
        if !min.is_finite() {
            return;
        }
        self.min_first = min.max(0.0);
        self.ratio = self.clamp_ratio(self.ratio);
        cx.notify();
    }

    pub fn set_max_first(&mut self, max: f32, cx: &mut Context<Self>) {
        if !max.is_finite() {
            return;
        }
        self.max_first = max.max(self.min_first);
        self.ratio = self.clamp_ratio(self.ratio);
        cx.notify();
    }

    pub fn set_min_second(&mut self, min: f32, cx: &mut Context<Self>) {
        if !min.is_finite() {
            return;
        }
        self.min_second = min.max(0.0);
        self.ratio = self.clamp_ratio(self.ratio);
        cx.notify();
    }

    pub fn set_max_second(&mut self, max: f32, cx: &mut Context<Self>) {
        if !max.is_finite() {
            return;
        }
        self.max_second = max.max(self.min_second);
        self.ratio = self.clamp_ratio(self.ratio);
        cx.notify();
    }

    pub fn is_collapsed(&self) -> bool {
        self.collapsed_pane.is_some()
    }

    pub fn collapsed_pane(&self) -> Option<CollapsiblePane> {
        self.collapsed_pane
    }

    pub fn collapse(&mut self, pane: CollapsiblePane, cx: &mut Context<Self>) {
        if self.collapsed_pane.is_some() {
            return;
        }
        self.ratio_before_collapse = self.ratio;
        self.collapsed_pane = Some(pane);
        cx.emit(SplitPaneEvent::PaneCollapsed { pane });
        cx.notify();
    }

    pub fn expand(&mut self, cx: &mut Context<Self>) {
        if let Some(pane) = self.collapsed_pane.take() {
            self.ratio = self.ratio_before_collapse;
            cx.emit(SplitPaneEvent::PaneExpanded { pane });
            cx.notify();
        }
    }

    pub fn toggle_collapse(&mut self, pane: CollapsiblePane, cx: &mut Context<Self>) {
        if self.collapsed_pane == Some(pane) {
            self.expand(cx);
        } else {
            self.collapse(pane, cx);
        }
    }

    fn clamp_ratio(&self, ratio: f32) -> f32 {
        let total_size = self.total_size();
        if total_size <= px(0.0) {
            return ratio.clamp(0.0, 1.0);
        }

        let min_first_ratio = (px(self.min_first) / total_size).clamp(0.0, 1.0);
        let max_first_ratio = (px(self.max_first) / total_size).clamp(0.0, 1.0);
        let min_second_ratio = (px(self.min_second) / total_size).clamp(0.0, 1.0);
        let max_second_ratio = (px(self.max_second) / total_size).clamp(0.0, 1.0);

        let lower_bound = min_first_ratio.max(1.0 - max_second_ratio);
        let upper_bound = max_first_ratio.min(1.0 - min_second_ratio);

        ratio.clamp(lower_bound.min(upper_bound), upper_bound.max(lower_bound))
    }

    fn total_size(&self) -> Pixels {
        match self.direction {
            SplitDirection::Horizontal => self.bounds.size.width,
            SplitDirection::Vertical => self.bounds.size.height,
        }
    }

    fn update_from_position(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let total_size = self.total_size();
        if total_size <= px(0.0) {
            return false;
        }

        let relative_pos = match self.direction {
            SplitDirection::Horizontal => position.x - self.bounds.left(),
            SplitDirection::Vertical => position.y - self.bounds.top(),
        };

        let new_ratio = (relative_pos / total_size).clamp(0.0, 1.0);
        let old_ratio = self.ratio;
        self.set_ratio(new_ratio, cx);

        if (self.ratio - old_ratio).abs() > f32::EPSILON {
            cx.emit(SplitPaneEvent::Resized { ratio: self.ratio });
            true
        } else {
            false
        }
    }
}

impl Focusable for SplitPaneState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<SplitPaneEvent> for SplitPaneState {}

impl Render for SplitPaneState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct SplitPane {
    state: Entity<SplitPaneState>,
    first_child: Option<AnyElement>,
    second_child: Option<AnyElement>,
    direction: Option<SplitDirection>,
    show_collapse_buttons: bool,
    on_resize: Option<Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl SplitPane {
    pub fn new(state: Entity<SplitPaneState>) -> Self {
        Self {
            state,
            first_child: None,
            second_child: None,
            direction: None,
            show_collapse_buttons: false,
            on_resize: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn horizontal(state: Entity<SplitPaneState>) -> Self {
        Self::new(state).direction(SplitDirection::Horizontal)
    }

    pub fn vertical(state: Entity<SplitPaneState>) -> Self {
        Self::new(state).direction(SplitDirection::Vertical)
    }

    pub fn direction(mut self, direction: SplitDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    pub fn first(mut self, child: impl IntoElement) -> Self {
        self.first_child = Some(child.into_any_element());
        self
    }

    pub fn second(mut self, child: impl IntoElement) -> Self {
        self.second_child = Some(child.into_any_element());
        self
    }

    pub fn show_collapse_buttons(mut self, show: bool) -> Self {
        self.show_collapse_buttons = show;
        self
    }

    pub fn on_resize(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_resize = Some(Rc::new(handler));
        self
    }
}

impl Styled for SplitPane {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SplitPane {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        if let Some(direction) = self.direction {
            self.state.update(cx, |state, _| {
                state.direction = direction;
            });
        }
        let state = self.state.read(cx);
        let direction = self.direction.unwrap_or(state.direction);
        let ratio = state.ratio;
        let collapsed_pane = state.collapsed_pane;
        let user_style = self.style;

        let (first_size, second_size) = match collapsed_pane {
            Some(CollapsiblePane::First) => (relative(0.0), relative(1.0)),
            Some(CollapsiblePane::Second) => (relative(1.0), relative(0.0)),
            None => (relative(ratio), relative(1.0 - ratio)),
        };

        let state_for_canvas = self.state.clone();
        let state_for_drag = self.state.clone();
        let state_for_move = self.state.clone();
        let state_for_up = self.state.clone();
        let on_resize_drag = self.on_resize.clone();
        let on_resize_move = self.on_resize.clone();

        let is_horizontal = direction == SplitDirection::Horizontal;
        let is_dragging = state.is_dragging;
        let focus_handle = state.focus_handle(cx);
        let divider_focused = focus_handle.is_focused(window);
        let entity_id = self.state.entity_id().as_u64();
        let mut divider_accessibility_state = AccessibilityState::NONE;
        if divider_focused {
            divider_accessibility_state |= AccessibilityState::FOCUSED;
        }
        let divider_accessibility = AccessibilityAttributes::new(AccessibilityRole::Slider)
            .label(if is_horizontal {
                "Resize left and right panes"
            } else {
                "Resize top and bottom panes"
            })
            .value(AccessibilityValue::Number((ratio * 100.0) as f64))
            .states(divider_accessibility_state)
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Increment,
                AccessibilityAction::Decrement,
            ]);
        let state_for_keys = self.state.clone();
        let state_for_increment = self.state.clone();
        let state_for_decrement = self.state.clone();
        let on_resize_keys = self.on_resize.clone();
        let on_resize_increment = self.on_resize.clone();
        let on_resize_decrement = self.on_resize.clone();

        let divider = div()
            .id(("split-pane-divider", entity_id))
            .track_focus(&focus_handle.tab_index(0).tab_stop(true))
            .accessibility(divider_accessibility)
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(kael::transparent_black())
            .when(is_horizontal, |this| {
                this.w(DIVIDER_HIT_AREA).h_full().cursor_col_resize()
            })
            .when(!is_horizontal, |this| {
                this.h(DIVIDER_HIT_AREA).w_full().cursor_row_resize()
            })
            .when(collapsed_pane.is_none(), |this| {
                this.hover(|s| s.bg(theme.tokens.accent.opacity(0.35)))
            })
            .when(is_dragging || divider_focused, |this| {
                this.bg(theme.tokens.accent.opacity(0.45))
            })
            .on_accessibility_action(AccessibilityAction::Increment, move |_, window, cx| {
                state_for_increment.update(cx, |state, cx| {
                    let old_ratio = state.ratio;
                    state.set_ratio(old_ratio + KEYBOARD_RESIZE_STEP, cx);
                    if (state.ratio - old_ratio).abs() > f32::EPSILON {
                        cx.emit(SplitPaneEvent::Resized { ratio: state.ratio });
                        if let Some(handler) = on_resize_increment.as_ref() {
                            handler(state.ratio, window, cx);
                        }
                    }
                });
            })
            .on_accessibility_action(AccessibilityAction::Decrement, move |_, window, cx| {
                state_for_decrement.update(cx, |state, cx| {
                    let old_ratio = state.ratio;
                    state.set_ratio(old_ratio - KEYBOARD_RESIZE_STEP, cx);
                    if (state.ratio - old_ratio).abs() > f32::EPSILON {
                        cx.emit(SplitPaneEvent::Resized { ratio: state.ratio });
                        if let Some(handler) = on_resize_decrement.as_ref() {
                            handler(state.ratio, window, cx);
                        }
                    }
                });
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let delta = match (is_horizontal, event.keystroke.key.as_str()) {
                    (true, "left") | (false, "up") => Some(-KEYBOARD_RESIZE_STEP),
                    (true, "right") | (false, "down") => Some(KEYBOARD_RESIZE_STEP),
                    _ => None,
                };
                let Some(delta) = delta else {
                    return;
                };
                state_for_keys.update(cx, |state, cx| {
                    let old_ratio = state.ratio;
                    state.set_ratio(old_ratio + delta, cx);
                    if (state.ratio - old_ratio).abs() > f32::EPSILON {
                        cx.emit(SplitPaneEvent::Resized { ratio: state.ratio });
                        if let Some(handler) = on_resize_keys.as_ref() {
                            handler(state.ratio, window, cx);
                        }
                    }
                });
                cx.stop_propagation();
                window.prevent_default();
            })
            .when(collapsed_pane.is_none(), |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(
                        &state_for_drag,
                        move |state, e: &MouseDownEvent, window, cx| {
                            state.is_dragging = true;
                            cx.notify();

                            if state.update_from_position(e.position, cx) {
                                if let Some(ref handler) = on_resize_drag {
                                    handler(state.ratio, window, cx);
                                }
                            }
                        },
                    ),
                )
            })
            .child(
                div()
                    .rounded_full()
                    .bg(if is_dragging || divider_focused {
                        theme.tokens.primary
                    } else {
                        theme.tokens.border
                    })
                    .when(is_horizontal, |this| this.h_full().w(DIVIDER_SIZE))
                    .when(!is_horizontal, |this| this.w_full().h(DIVIDER_SIZE)),
            );

        let container_mouse_move = window.listener_for(
            &state_for_move,
            move |state, e: &MouseMoveEvent, window, cx| {
                if state.is_dragging && state.update_from_position(e.position, cx) {
                    if let Some(ref handler) = on_resize_move {
                        handler(state.ratio, window, cx);
                    }
                }
            },
        );

        let container_mouse_up =
            window.listener_for(&state_for_up, move |state, _: &MouseUpEvent, _, cx| {
                state.is_dragging = false;
                cx.notify();
            });

        let mut container = div()
            .flex()
            .size_full()
            .overflow_hidden()
            .when(is_horizontal, |this| this.flex_row())
            .when(!is_horizontal, |this| this.flex_col())
            .on_mouse_move(container_mouse_move)
            .on_mouse_up(MouseButton::Left, container_mouse_up);

        container = container.map(|mut this| {
            this.style().refine(&user_style);
            this
        });

        let is_first_collapsed = collapsed_pane == Some(CollapsiblePane::First);
        let is_second_collapsed = collapsed_pane == Some(CollapsiblePane::Second);

        let first_pane = div()
            .flex_shrink_0()
            .overflow_hidden()
            .when(is_horizontal && !is_first_collapsed, |this| {
                this.h_full().w(first_size)
            })
            .when(!is_horizontal && !is_first_collapsed, |this| {
                this.w_full().h(first_size)
            })
            .when(is_first_collapsed, |this| this.size_0())
            .when_some(self.first_child, |this, child| this.child(child));

        let second_pane = div()
            .flex_shrink_0()
            .overflow_hidden()
            .when(is_horizontal && !is_second_collapsed, |this| {
                this.h_full().w(second_size)
            })
            .when(!is_horizontal && !is_second_collapsed, |this| {
                this.w_full().h(second_size)
            })
            .when(is_second_collapsed, |this| this.size_0())
            .when_some(self.second_child, |this, child| this.child(child));

        let collapse_buttons = if self.show_collapse_buttons && collapsed_pane.is_none() {
            let state_first = self.state.clone();
            let state_second = self.state.clone();

            Some(
                div()
                    .absolute()
                    .when(is_horizontal, |this| {
                        this.top(relative(0.5))
                            .left(relative(ratio))
                            .mt(-px(27.0))
                            .ml(-px(13.0))
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                    })
                    .when(!is_horizontal, |this| {
                        this.left(relative(0.5))
                            .top(relative(ratio))
                            .ml(-px(27.0))
                            .mt(-px(13.0))
                            .flex()
                            .flex_row()
                            .gap(px(2.0))
                    })
                    .child(
                        IconButton::new(if is_horizontal {
                            "chevron-left"
                        } else {
                            "chevron-up"
                        })
                        .id(("split-collapse-first", entity_id))
                        .label(if is_horizontal {
                            "Collapse left pane"
                        } else {
                            "Collapse top pane"
                        })
                        .variant(ButtonVariant::Outline)
                        .size(px(26.0))
                        .icon_size(px(14.0))
                        .on_click(move |_, _, cx| {
                            state_first.update(cx, |state, cx| {
                                state.collapse(CollapsiblePane::First, cx);
                            });
                        }),
                    )
                    .child(
                        IconButton::new(if is_horizontal {
                            "chevron-right"
                        } else {
                            "chevron-down"
                        })
                        .id(("split-collapse-second", entity_id))
                        .label(if is_horizontal {
                            "Collapse right pane"
                        } else {
                            "Collapse bottom pane"
                        })
                        .variant(ButtonVariant::Outline)
                        .size(px(26.0))
                        .icon_size(px(14.0))
                        .on_click(move |_, _, cx| {
                            state_second.update(cx, |state, cx| {
                                state.collapse(CollapsiblePane::Second, cx);
                            });
                        }),
                    ),
            )
        } else if self.show_collapse_buttons && collapsed_pane.is_some() {
            let state_expand = self.state.clone();

            Some(
                div()
                    .absolute()
                    .when(is_horizontal, |this| {
                        if collapsed_pane == Some(CollapsiblePane::First) {
                            this.top(px(4.0)).left(px(4.0))
                        } else {
                            this.top(px(4.0)).right(px(4.0))
                        }
                    })
                    .when(!is_horizontal, |this| {
                        if collapsed_pane == Some(CollapsiblePane::First) {
                            this.left(px(4.0)).top(px(4.0))
                        } else {
                            this.left(px(4.0)).bottom(px(4.0))
                        }
                    })
                    .child(
                        IconButton::new(match (is_horizontal, collapsed_pane) {
                            (true, Some(CollapsiblePane::First)) => "chevron-right",
                            (true, _) => "chevron-left",
                            (false, Some(CollapsiblePane::First)) => "chevron-down",
                            (false, _) => "chevron-up",
                        })
                        .id(("split-expand-pane", entity_id))
                        .label("Expand collapsed pane")
                        .variant(ButtonVariant::Outline)
                        .size(px(28.0))
                        .icon_size(px(16.0))
                        .on_click(move |_, _, cx| {
                            state_expand.update(cx, |state, cx| {
                                state.expand(cx);
                            });
                        }),
                    ),
            )
        } else {
            None
        };

        container
            .relative()
            .child(first_pane)
            .when(collapsed_pane.is_none(), |this| this.child(divider))
            .child(second_pane)
            .when_some(collapse_buttons, |this, buttons| this.child(buttons))
            .child(
                canvas_with_prepaint(
                    move |bounds, _, cx| {
                        state_for_canvas.update(cx, |state, _| {
                            state.bounds = bounds;
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{SplitDirection, SplitPane, SplitPaneState};
    use kael::{
        div, AppContext, Context, IntoElement, ParentElement, Render, Styled, TestAppContext,
        Window,
    };

    struct VerticalHost {
        state: kael::Entity<SplitPaneState>,
    }

    impl Render for VerticalHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                SplitPane::vertical(self.state.clone())
                    .first(div().child("Top"))
                    .second(div().child("Bottom")),
            )
        }
    }

    #[kael::test]
    fn ignores_non_finite_constraints_and_ratios(cx: &mut TestAppContext) {
        let state = cx.new(SplitPaneState::new);
        state.update(cx, |state, cx| {
            state.set_ratio(0.7, cx);
            state.set_ratio(f32::NAN, cx);
            state.set_min_first(f32::INFINITY, cx);
            state.set_max_second(f32::NEG_INFINITY, cx);
        });

        assert_eq!(cx.read(|cx| state.read(cx).ratio()), 0.7);
    }

    #[kael::test]
    fn vertical_builder_syncs_drag_axis_and_exposes_adjustment_actions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(SplitPaneState::new);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| VerticalHost { state }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(state.read(cx).direction(), SplitDirection::Vertical);
            let divider = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == kael::AccessibilityRole::Slider)
                .expect("split pane should expose its divider");
            assert!(divider
                .actions
                .contains(&kael::AccessibilityAction::Increment));
            assert!(divider
                .actions
                .contains(&kael::AccessibilityAction::Decrement));
        });
    }
}
