use crate::components::slider::{SliderAxis, SliderSize, snap_slider_value, valid_slider_step};
use crate::{astryx, theme::use_theme};
use kael::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ActiveThumb {
    None,
    Start,
    End,
}

type RangeChangeHandler = Rc<dyn Fn(f32, f32, &mut Window, &mut App) + 'static>;

fn handle_range_thumb_key(
    state: &Entity<RangeSliderState>,
    thumb: ActiveThumb,
    key: &str,
    on_change: Option<&RangeChangeHandler>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if !matches!(key, "left" | "down" | "right" | "up" | "home" | "end") {
        return false;
    }

    state.update(cx, |state, cx| {
        let previous = state.range();
        match (thumb, key) {
            (ActiveThumb::Start, "left" | "down") => state.decrement_start(cx),
            (ActiveThumb::Start, "right" | "up") => state.increment_start(cx),
            (ActiveThumb::Start, "home") => state.set_start_value(state.min, cx),
            (ActiveThumb::Start, "end") => state.set_start_value(state.end_value, cx),
            (ActiveThumb::End, "left" | "down") => state.decrement_end(cx),
            (ActiveThumb::End, "right" | "up") => state.increment_end(cx),
            (ActiveThumb::End, "home") => state.set_end_value(state.start_value, cx),
            (ActiveThumb::End, "end") => state.set_end_value(state.max, cx),
            _ => {}
        }
        if state.range() != previous
            && let Some(handler) = on_change
        {
            handler(state.start_value, state.end_value, window, cx);
        }
    });
    true
}

fn handle_range_thumb_accessibility_action(
    state: &Entity<RangeSliderState>,
    thumb: ActiveThumb,
    request: &AccessibilityActionRequest,
    on_change: Option<&RangeChangeHandler>,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        let previous = state.range();
        match (thumb, request.action) {
            (ActiveThumb::Start, AccessibilityAction::Increment) => state.increment_start(cx),
            (ActiveThumb::Start, AccessibilityAction::Decrement) => state.decrement_start(cx),
            (ActiveThumb::End, AccessibilityAction::Increment) => state.increment_end(cx),
            (ActiveThumb::End, AccessibilityAction::Decrement) => state.decrement_end(cx),
            (ActiveThumb::Start, AccessibilityAction::SetValue) => {
                if let Some(AccessibilityActionPayload::NumericValue(value)) =
                    request.payload.as_ref()
                    && value.is_finite()
                {
                    state.set_start_value(*value as f32, cx);
                }
            }
            (ActiveThumb::End, AccessibilityAction::SetValue) => {
                if let Some(AccessibilityActionPayload::NumericValue(value)) =
                    request.payload.as_ref()
                    && value.is_finite()
                {
                    state.set_end_value(*value as f32, cx);
                }
            }
            _ => {}
        }

        if state.range() != previous
            && let Some(handler) = on_change
        {
            handler(state.start_value, state.end_value, window, cx);
        }
    });
}

pub struct RangeSliderState {
    min: f32,
    max: f32,
    start_value: f32,
    end_value: f32,
    step: f32,
    start_focus_handle: FocusHandle,
    end_focus_handle: FocusHandle,
    active_thumb: ActiveThumb,
}

impl RangeSliderState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            start_value: 25.0,
            end_value: 75.0,
            step: 1.0,
            start_focus_handle: cx.focus_handle(),
            end_focus_handle: cx.focus_handle(),
            active_thumb: ActiveThumb::None,
        }
    }

    pub fn min(&self) -> f32 {
        self.min
    }

    pub fn set_min(&mut self, min: f32, cx: &mut Context<Self>) {
        if !min.is_finite() {
            return;
        }
        let previous = (self.min, self.max, self.start_value, self.end_value);
        self.min = min;
        if self.max < min {
            self.max = min;
        }
        self.start_value = self.start_value.clamp(self.min, self.end_value);
        self.end_value = self.end_value.clamp(self.start_value, self.max);
        if previous != (self.min, self.max, self.start_value, self.end_value) {
            cx.notify();
        }
    }

    pub fn max(&self) -> f32 {
        self.max
    }

    pub fn set_max(&mut self, max: f32, cx: &mut Context<Self>) {
        if !max.is_finite() {
            return;
        }
        let previous = (self.min, self.max, self.start_value, self.end_value);
        self.max = max;
        if self.min > max {
            self.min = max;
        }
        self.end_value = self.end_value.clamp(self.start_value, self.max);
        self.start_value = self.start_value.clamp(self.min, self.end_value);
        if previous != (self.min, self.max, self.start_value, self.end_value) {
            cx.notify();
        }
    }

    pub fn start_value(&self) -> f32 {
        self.start_value
    }

    pub fn end_value(&self) -> f32 {
        self.end_value
    }

    pub fn range(&self) -> (f32, f32) {
        (self.start_value, self.end_value)
    }

    pub fn set_start_value(&mut self, value: f32, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        let stepped = snap_slider_value(value, self.min, self.end_value, self.step);

        if (self.start_value - stepped).abs() > f32::EPSILON {
            self.start_value = stepped;
            cx.notify();
        }
    }

    pub fn set_end_value(&mut self, value: f32, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        let stepped = snap_slider_value(value, self.start_value, self.max, self.step);

        if (self.end_value - stepped).abs() > f32::EPSILON {
            self.end_value = stepped;
            cx.notify();
        }
    }

    pub fn set_range(&mut self, start: f32, end: f32, cx: &mut Context<Self>) {
        if !start.is_finite() || !end.is_finite() {
            return;
        }
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        let stepped_start = snap_slider_value(start, self.min, self.max, self.step);
        let stepped_end = snap_slider_value(end, stepped_start, self.max, self.step);

        let changed = (self.start_value - stepped_start).abs() > f32::EPSILON
            || (self.end_value - stepped_end).abs() > f32::EPSILON;

        if changed {
            self.start_value = stepped_start;
            self.end_value = stepped_end;
            cx.notify();
        }
    }

    pub fn step(&self) -> f32 {
        self.step
    }

    pub fn set_step(&mut self, step: f32, cx: &mut Context<Self>) {
        if let Some(step) = valid_slider_step(step) {
            self.step = step;
            let start = snap_slider_value(self.start_value, self.min, self.max, step);
            let end = snap_slider_value(self.end_value, start, self.max, step);
            self.start_value = start;
            self.end_value = end;
            cx.notify();
        }
    }

    fn increment_start(&mut self, cx: &mut Context<Self>) {
        self.set_start_value(self.start_value + self.step, cx);
    }

    fn decrement_start(&mut self, cx: &mut Context<Self>) {
        self.set_start_value(self.start_value - self.step, cx);
    }

    fn increment_end(&mut self, cx: &mut Context<Self>) {
        self.set_end_value(self.end_value + self.step, cx);
    }

    fn decrement_end(&mut self, cx: &mut Context<Self>) {
        self.set_end_value(self.end_value - self.step, cx);
    }

    fn start_percentage(&self) -> f32 {
        if self.max == self.min {
            return 0.0;
        }
        ((self.start_value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    fn end_percentage(&self) -> f32 {
        if self.max == self.min {
            return 0.0;
        }
        ((self.end_value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    fn value_from_position(&self, position: Point<Pixels>, bounds: Bounds<Pixels>) -> f32 {
        let track_width = bounds.size.width;
        if track_width <= px(0.0) {
            return self.min;
        }

        let relative_x = (position.x - bounds.left()).clamp(px(0.0), track_width);
        let percentage = (relative_x / track_width).clamp(0.0, 1.0);
        self.min + percentage * (self.max - self.min)
    }

    fn value_from_position_vertical(&self, position: Point<Pixels>, bounds: Bounds<Pixels>) -> f32 {
        let track_height = bounds.size.height;
        if track_height <= px(0.0) {
            return self.min;
        }

        let relative_y = (position.y - bounds.top()).clamp(px(0.0), track_height);
        let percentage = 1.0 - (relative_y / track_height).clamp(0.0, 1.0);
        self.min + percentage * (self.max - self.min)
    }

    fn update_from_position(
        &mut self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let new_value = self.value_from_position(position, bounds);

        match self.active_thumb {
            ActiveThumb::Start => self.set_start_value(new_value, cx),
            ActiveThumb::End => self.set_end_value(new_value, cx),
            ActiveThumb::None => {
                let start_dist = (new_value - self.start_value).abs();
                let end_dist = (new_value - self.end_value).abs();

                if start_dist <= end_dist {
                    self.active_thumb = ActiveThumb::Start;
                    self.set_start_value(new_value, cx);
                } else {
                    self.active_thumb = ActiveThumb::End;
                    self.set_end_value(new_value, cx);
                }
            }
        }
    }

    fn update_from_position_vertical(
        &mut self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let new_value = self.value_from_position_vertical(position, bounds);

        match self.active_thumb {
            ActiveThumb::Start => self.set_start_value(new_value, cx),
            ActiveThumb::End => self.set_end_value(new_value, cx),
            ActiveThumb::None => {
                let start_dist = (new_value - self.start_value).abs();
                let end_dist = (new_value - self.end_value).abs();

                if start_dist <= end_dist {
                    self.active_thumb = ActiveThumb::Start;
                    self.set_start_value(new_value, cx);
                } else {
                    self.active_thumb = ActiveThumb::End;
                    self.set_end_value(new_value, cx);
                }
            }
        }
    }
}

impl Focusable for RangeSliderState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.start_focus_handle.clone()
    }
}

impl Render for RangeSliderState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct RangeSlider {
    state: Entity<RangeSliderState>,
    instance_id: Option<SharedString>,
    size: SliderSize,
    axis: SliderAxis,
    disabled: bool,
    show_values: bool,
    accessibility_label: SharedString,
    on_change: Option<RangeChangeHandler>,
    style: StyleRefinement,
}

impl RangeSlider {
    pub fn new(state: Entity<RangeSliderState>) -> Self {
        Self {
            state,
            instance_id: None,
            size: SliderSize::Md,
            axis: SliderAxis::Horizontal,
            disabled: false,
            show_values: false,
            accessibility_label: "Range".into(),
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    /// Assign a stable identity to this rendered slider instance.
    ///
    /// Use distinct IDs when presenting one [`RangeSliderState`] in multiple
    /// places so each pair of thumbs stays independently addressable.
    pub fn id(mut self, id: impl Into<SharedString>) -> Self {
        self.instance_id = Some(id.into());
        self
    }

    pub fn size(mut self, size: SliderSize) -> Self {
        self.size = size;
        self
    }

    pub fn horizontal(mut self) -> Self {
        self.axis = SliderAxis::Horizontal;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.axis = SliderAxis::Vertical;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn show_values(mut self, show: bool) -> Self {
        self.show_values = show;
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(f32, f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for RangeSlider {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RangeSlider {
    fn render_horizontal(
        self,
        window: &mut Window,
        theme: crate::theme::Theme,
        start_focus_handle: FocusHandle,
        end_focus_handle: FocusHandle,
        is_focused: bool,
        start_percentage: f32,
        end_percentage: f32,
        start_value: f32,
        end_value: f32,
        track_height: Pixels,
        thumb_width: Pixels,
        thumb_height: Pixels,
        track_bg: Hsla,
        active_bg: Hsla,
        thumb_bg: Hsla,
        focus_ring: BoxShadow,
        user_style: StyleRefinement,
        start_accessibility: AccessibilityAttributes,
        end_accessibility: AccessibilityAttributes,
        instance_id: SharedString,
    ) -> Div {
        let track_bounds = Rc::new(Cell::new(Bounds::default()));
        div()
            .flex()
            .items_center()
            .gap_3()
            .w_full()
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .when(self.show_values, |this| {
                this.child(
                    div()
                        .min_w(px(40.0))
                        .text_center()
                        .text_sm()
                        .text_color(theme.tokens.foreground)
                        .child(format!("{:.0}", start_value)),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h(thumb_height)
                    .flex()
                    .items_center()
                    .when(is_focused && !self.disabled, |this| {
                        this.shadow(smallvec::smallvec![focus_ring])
                    })
                    .rounded(theme.tokens.radius_md)
                    .child(
                        canvas_with_prepaint(
                            {
                                let track_bounds = track_bounds.clone();
                                move |bounds, _, _| {
                                    track_bounds.set(bounds);
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(track_height)
                            .rounded_full()
                            .bg(track_bg)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .left(relative(start_percentage))
                                    .top_0()
                                    .h_full()
                                    .w(relative(end_percentage - start_percentage))
                                    .bg(active_bg),
                            ),
                    )
                    .child({
                        let state_clone = self.state.clone();
                        let on_change_thumb = self.on_change.clone();
                        let track_bounds = track_bounds.clone();

                        div()
                            .id((instance_id.clone(), 0usize))
                            .accessibility(start_accessibility)
                            .when(!self.disabled, |this| {
                                this.track_focus(
                                    &start_focus_handle.clone().tab_index(0).tab_stop(true),
                                )
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if handle_range_thumb_key(
                                            &state,
                                            ActiveThumb::Start,
                                            event.keystroke.key.as_str(),
                                            on_change.as_ref(),
                                            window,
                                            cx,
                                        ) {
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                }
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    let bind = |this: Stateful<Div>, action| {
                                        let state = state.clone();
                                        let on_change = on_change.clone();
                                        this.on_accessibility_action(
                                            action,
                                            move |request, window, cx| {
                                                handle_range_thumb_accessibility_action(
                                                    &state,
                                                    ActiveThumb::Start,
                                                    request,
                                                    on_change.as_ref(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                    };
                                    bind(
                                        bind(
                                            bind(this, AccessibilityAction::Increment),
                                            AccessibilityAction::Decrement,
                                        ),
                                        AccessibilityAction::SetValue,
                                    )
                                }
                            })
                            .absolute()
                            .left(relative(start_percentage))
                            .top_0()
                            .ml(-(thumb_width / 2.0))
                            .w(thumb_width)
                            .h(thumb_height)
                            .rounded(thumb_height / 2.0)
                            .bg(thumb_bg)
                            .when(!self.disabled, {
                                let shadow = theme.tokens.shadow_sm.clone();
                                move |this| {
                                    this.shadow(shadow.to_vec())
                                        .cursor(CursorStyle::PointingHand)
                                }
                            })
                            .when(!self.disabled, |this| {
                                this.on_mouse_down(
                                    MouseButton::Left,
                                    window.listener_for(
                                        &state_clone,
                                        move |state, e: &MouseDownEvent, window, cx| {
                                            window.focus(&start_focus_handle);
                                            state.active_thumb = ActiveThumb::Start;
                                            let previous = state.range();
                                            state.update_from_position(
                                                e.position,
                                                track_bounds.get(),
                                                cx,
                                            );

                                            if state.range() != previous
                                                && let Some(ref handler) = on_change_thumb
                                            {
                                                handler(
                                                    state.start_value,
                                                    state.end_value,
                                                    window,
                                                    cx,
                                                );
                                            }

                                            cx.stop_propagation();
                                        },
                                    ),
                                )
                            })
                    })
                    .child({
                        let state_clone = self.state.clone();
                        let on_change_thumb = self.on_change.clone();
                        let track_bounds = track_bounds.clone();

                        div()
                            .id((instance_id.clone(), 1usize))
                            .accessibility(end_accessibility)
                            .when(!self.disabled, |this| {
                                this.track_focus(
                                    &end_focus_handle.clone().tab_index(0).tab_stop(true),
                                )
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if handle_range_thumb_key(
                                            &state,
                                            ActiveThumb::End,
                                            event.keystroke.key.as_str(),
                                            on_change.as_ref(),
                                            window,
                                            cx,
                                        ) {
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                }
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    let bind = |this: Stateful<Div>, action| {
                                        let state = state.clone();
                                        let on_change = on_change.clone();
                                        this.on_accessibility_action(
                                            action,
                                            move |request, window, cx| {
                                                handle_range_thumb_accessibility_action(
                                                    &state,
                                                    ActiveThumb::End,
                                                    request,
                                                    on_change.as_ref(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                    };
                                    bind(
                                        bind(
                                            bind(this, AccessibilityAction::Increment),
                                            AccessibilityAction::Decrement,
                                        ),
                                        AccessibilityAction::SetValue,
                                    )
                                }
                            })
                            .absolute()
                            .left(relative(end_percentage))
                            .top_0()
                            .ml(-(thumb_width / 2.0))
                            .w(thumb_width)
                            .h(thumb_height)
                            .rounded(thumb_height / 2.0)
                            .bg(thumb_bg)
                            .when(!self.disabled, {
                                let shadow = theme.tokens.shadow_sm.clone();
                                move |this| {
                                    this.shadow(shadow.to_vec())
                                        .cursor(CursorStyle::PointingHand)
                                }
                            })
                            .when(!self.disabled, |this| {
                                this.on_mouse_down(
                                    MouseButton::Left,
                                    window.listener_for(
                                        &state_clone,
                                        move |state, e: &MouseDownEvent, window, cx| {
                                            window.focus(&end_focus_handle);
                                            state.active_thumb = ActiveThumb::End;
                                            let previous = state.range();
                                            state.update_from_position(
                                                e.position,
                                                track_bounds.get(),
                                                cx,
                                            );

                                            if state.range() != previous
                                                && let Some(ref handler) = on_change_thumb
                                            {
                                                handler(
                                                    state.start_value,
                                                    state.end_value,
                                                    window,
                                                    cx,
                                                );
                                            }

                                            cx.stop_propagation();
                                        },
                                    ),
                                )
                            })
                    })
                    .when(!self.disabled, |this| {
                        let state_bar = self.state.clone();
                        let on_change_bar = self.on_change.clone();
                        let track_bounds_bar = track_bounds.clone();

                        this.on_mouse_down(
                            MouseButton::Left,
                            window.listener_for(
                                &state_bar,
                                move |state, e: &MouseDownEvent, window, cx| {
                                    let previous = state.range();
                                    state.update_from_position(
                                        e.position,
                                        track_bounds_bar.get(),
                                        cx,
                                    );

                                    if state.range() != previous
                                        && let Some(ref handler) = on_change_bar
                                    {
                                        handler(state.start_value, state.end_value, window, cx);
                                    }
                                },
                            ),
                        )
                        .on_mouse_move({
                            let state_move = self.state.clone();
                            let on_change_move = self.on_change.clone();
                            let track_bounds_move = track_bounds.clone();

                            window.listener_for(
                                &state_move,
                                move |state, e: &MouseMoveEvent, window, cx| {
                                    if state.active_thumb != ActiveThumb::None {
                                        let previous = state.range();
                                        state.update_from_position(
                                            e.position,
                                            track_bounds_move.get(),
                                            cx,
                                        );

                                        if state.range() != previous
                                            && let Some(ref handler) = on_change_move
                                        {
                                            handler(state.start_value, state.end_value, window, cx);
                                        }
                                    }
                                },
                            )
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            window.listener_for(
                                &self.state,
                                move |state, _: &MouseUpEvent, _, _cx| {
                                    state.active_thumb = ActiveThumb::None;
                                },
                            ),
                        )
                    }),
            )
            .when(self.show_values, |this| {
                this.child(
                    div()
                        .min_w(px(40.0))
                        .text_center()
                        .text_sm()
                        .text_color(theme.tokens.foreground)
                        .child(format!("{:.0}", end_value)),
                )
            })
    }

    fn render_vertical(
        self,
        window: &mut Window,
        theme: crate::theme::Theme,
        start_focus_handle: FocusHandle,
        end_focus_handle: FocusHandle,
        is_focused: bool,
        start_percentage: f32,
        end_percentage: f32,
        start_value: f32,
        end_value: f32,
        track_height: Pixels,
        thumb_width: Pixels,
        thumb_height: Pixels,
        track_bg: Hsla,
        active_bg: Hsla,
        thumb_bg: Hsla,
        focus_ring: BoxShadow,
        user_style: StyleRefinement,
        start_accessibility: AccessibilityAttributes,
        end_accessibility: AccessibilityAttributes,
        instance_id: SharedString,
    ) -> Div {
        let track_bounds = Rc::new(Cell::new(Bounds::default()));
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_3()
            .h_full()
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .when(self.show_values, |this| {
                this.child(
                    div()
                        .min_h(px(24.0))
                        .text_center()
                        .text_sm()
                        .text_color(theme.tokens.foreground)
                        .child(format!("{:.0}", end_value)),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w(thumb_width)
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(is_focused && !self.disabled, |this| {
                        this.shadow(smallvec::smallvec![focus_ring])
                    })
                    .rounded(theme.tokens.radius_md)
                    .child(
                        canvas_with_prepaint(
                            {
                                let track_bounds = track_bounds.clone();
                                move |bounds, _, _| {
                                    track_bounds.set(bounds);
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .relative()
                            .h_full()
                            .w(track_height)
                            .rounded_full()
                            .bg(track_bg)
                            .overflow_hidden()
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .bottom(relative(start_percentage))
                                    .w_full()
                                    .h(relative(end_percentage - start_percentage))
                                    .bg(active_bg),
                            ),
                    )
                    .child({
                        let state_clone = self.state.clone();
                        let on_change_thumb = self.on_change.clone();
                        let track_bounds = track_bounds.clone();

                        div()
                            .id((instance_id.clone(), 0usize))
                            .accessibility(start_accessibility)
                            .when(!self.disabled, |this| {
                                this.track_focus(
                                    &start_focus_handle.clone().tab_index(0).tab_stop(true),
                                )
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if handle_range_thumb_key(
                                            &state,
                                            ActiveThumb::Start,
                                            event.keystroke.key.as_str(),
                                            on_change.as_ref(),
                                            window,
                                            cx,
                                        ) {
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                }
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    let bind = |this: Stateful<Div>, action| {
                                        let state = state.clone();
                                        let on_change = on_change.clone();
                                        this.on_accessibility_action(
                                            action,
                                            move |request, window, cx| {
                                                handle_range_thumb_accessibility_action(
                                                    &state,
                                                    ActiveThumb::Start,
                                                    request,
                                                    on_change.as_ref(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                    };
                                    bind(
                                        bind(
                                            bind(this, AccessibilityAction::Increment),
                                            AccessibilityAction::Decrement,
                                        ),
                                        AccessibilityAction::SetValue,
                                    )
                                }
                            })
                            .absolute()
                            .left_0()
                            .bottom(relative(start_percentage))
                            .mb(-(thumb_height / 2.0))
                            .w(thumb_width)
                            .h(thumb_height)
                            .rounded(thumb_width / 2.0)
                            .bg(thumb_bg)
                            .when(!self.disabled, {
                                let shadow = theme.tokens.shadow_sm.clone();
                                move |this| {
                                    this.shadow(shadow.to_vec())
                                        .cursor(CursorStyle::PointingHand)
                                }
                            })
                            .when(!self.disabled, |this| {
                                this.on_mouse_down(
                                    MouseButton::Left,
                                    window.listener_for(
                                        &state_clone,
                                        move |state, e: &MouseDownEvent, window, cx| {
                                            window.focus(&start_focus_handle);
                                            state.active_thumb = ActiveThumb::Start;
                                            let previous = state.range();
                                            state.update_from_position_vertical(
                                                e.position,
                                                track_bounds.get(),
                                                cx,
                                            );

                                            if state.range() != previous
                                                && let Some(ref handler) = on_change_thumb
                                            {
                                                handler(
                                                    state.start_value,
                                                    state.end_value,
                                                    window,
                                                    cx,
                                                );
                                            }

                                            cx.stop_propagation();
                                        },
                                    ),
                                )
                            })
                    })
                    .child({
                        let state_clone = self.state.clone();
                        let on_change_thumb = self.on_change.clone();
                        let track_bounds = track_bounds.clone();

                        div()
                            .id((instance_id.clone(), 1usize))
                            .accessibility(end_accessibility)
                            .when(!self.disabled, |this| {
                                this.track_focus(
                                    &end_focus_handle.clone().tab_index(0).tab_stop(true),
                                )
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if handle_range_thumb_key(
                                            &state,
                                            ActiveThumb::End,
                                            event.keystroke.key.as_str(),
                                            on_change.as_ref(),
                                            window,
                                            cx,
                                        ) {
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                }
                            })
                            .when(!self.disabled, {
                                let state = self.state.clone();
                                let on_change = self.on_change.clone();
                                move |this| {
                                    let bind = |this: Stateful<Div>, action| {
                                        let state = state.clone();
                                        let on_change = on_change.clone();
                                        this.on_accessibility_action(
                                            action,
                                            move |request, window, cx| {
                                                handle_range_thumb_accessibility_action(
                                                    &state,
                                                    ActiveThumb::End,
                                                    request,
                                                    on_change.as_ref(),
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )
                                    };
                                    bind(
                                        bind(
                                            bind(this, AccessibilityAction::Increment),
                                            AccessibilityAction::Decrement,
                                        ),
                                        AccessibilityAction::SetValue,
                                    )
                                }
                            })
                            .absolute()
                            .left_0()
                            .bottom(relative(end_percentage))
                            .mb(-(thumb_height / 2.0))
                            .w(thumb_width)
                            .h(thumb_height)
                            .rounded(thumb_width / 2.0)
                            .bg(thumb_bg)
                            .when(!self.disabled, {
                                let shadow = theme.tokens.shadow_sm.clone();
                                move |this| {
                                    this.shadow(shadow.to_vec())
                                        .cursor(CursorStyle::PointingHand)
                                }
                            })
                            .when(!self.disabled, |this| {
                                this.on_mouse_down(
                                    MouseButton::Left,
                                    window.listener_for(
                                        &state_clone,
                                        move |state, e: &MouseDownEvent, window, cx| {
                                            window.focus(&end_focus_handle);
                                            state.active_thumb = ActiveThumb::End;
                                            let previous = state.range();
                                            state.update_from_position_vertical(
                                                e.position,
                                                track_bounds.get(),
                                                cx,
                                            );

                                            if state.range() != previous
                                                && let Some(ref handler) = on_change_thumb
                                            {
                                                handler(
                                                    state.start_value,
                                                    state.end_value,
                                                    window,
                                                    cx,
                                                );
                                            }

                                            cx.stop_propagation();
                                        },
                                    ),
                                )
                            })
                    })
                    .when(!self.disabled, |this| {
                        let state_bar = self.state.clone();
                        let on_change_bar = self.on_change.clone();
                        let track_bounds_bar = track_bounds.clone();

                        this.on_mouse_down(
                            MouseButton::Left,
                            window.listener_for(
                                &state_bar,
                                move |state, e: &MouseDownEvent, window, cx| {
                                    let previous = state.range();
                                    state.update_from_position_vertical(
                                        e.position,
                                        track_bounds_bar.get(),
                                        cx,
                                    );

                                    if state.range() != previous
                                        && let Some(ref handler) = on_change_bar
                                    {
                                        handler(state.start_value, state.end_value, window, cx);
                                    }
                                },
                            ),
                        )
                        .on_mouse_move({
                            let state_move = self.state.clone();
                            let on_change_move = self.on_change.clone();
                            let track_bounds_move = track_bounds.clone();

                            window.listener_for(
                                &state_move,
                                move |state, e: &MouseMoveEvent, window, cx| {
                                    if state.active_thumb != ActiveThumb::None {
                                        let previous = state.range();
                                        state.update_from_position_vertical(
                                            e.position,
                                            track_bounds_move.get(),
                                            cx,
                                        );

                                        if state.range() != previous
                                            && let Some(ref handler) = on_change_move
                                        {
                                            handler(state.start_value, state.end_value, window, cx);
                                        }
                                    }
                                },
                            )
                        })
                        .on_mouse_up(
                            MouseButton::Left,
                            window.listener_for(
                                &self.state,
                                move |state, _: &MouseUpEvent, _, _cx| {
                                    state.active_thumb = ActiveThumb::None;
                                },
                            ),
                        )
                    }),
            )
            .when(self.show_values, |this| {
                this.child(
                    div()
                        .min_h(px(24.0))
                        .text_center()
                        .text_sm()
                        .text_color(theme.tokens.foreground)
                        .child(format!("{:.0}", start_value)),
                )
            })
    }
}

impl RenderOnce for RangeSlider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let state = self.state.read(cx);
        let start_focus_handle = state.start_focus_handle.clone();
        let end_focus_handle = state.end_focus_handle.clone();
        let start_is_focused = start_focus_handle.is_focused(window);
        let end_is_focused = end_focus_handle.is_focused(window);
        let is_focused = start_is_focused || end_is_focused;
        let start_percentage = state.start_percentage();
        let end_percentage = state.end_percentage();
        let start_value = state.start_value;
        let end_value = state.end_value;
        let min = state.min;
        let max = state.max;
        let step = state.step;
        let entity_id = self.state.entity_id().as_u64();
        let axis_key = match self.axis {
            SliderAxis::Horizontal => "horizontal",
            SliderAxis::Vertical => "vertical",
        };
        let size_key = match self.size {
            SliderSize::Sm => "sm",
            SliderSize::Md => "md",
            SliderSize::Lg => "lg",
        };
        let instance_id = self.instance_id.clone().unwrap_or_else(|| {
            format!(
                "range-slider-{entity_id}-{axis_key}-{size_key}-{}",
                if self.disabled { "disabled" } else { "enabled" }
            )
            .into()
        });
        let mut start_state = AccessibilityState::NONE;
        let mut end_state = AccessibilityState::NONE;
        if self.disabled {
            start_state |= AccessibilityState::DISABLED;
            end_state |= AccessibilityState::DISABLED;
        }
        if start_is_focused {
            start_state |= AccessibilityState::FOCUSED;
        }
        if end_is_focused {
            end_state |= AccessibilityState::FOCUSED;
        }
        let mut start_accessibility = AccessibilityAttributes::new(AccessibilityRole::Slider)
            .label(format!("{} minimum", self.accessibility_label))
            .value(AccessibilityValue::Range {
                current: start_value as f64,
                min: min as f64,
                max: end_value as f64,
                step: Some(step as f64),
            })
            .states(start_state);
        let mut end_accessibility = AccessibilityAttributes::new(AccessibilityRole::Slider)
            .label(format!("{} maximum", self.accessibility_label))
            .value(AccessibilityValue::Range {
                current: end_value as f64,
                min: start_value as f64,
                max: max as f64,
                step: Some(step as f64),
            })
            .states(end_state);
        if !self.disabled {
            let actions = vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Increment,
                AccessibilityAction::Decrement,
                AccessibilityAction::SetValue,
            ];
            start_accessibility = start_accessibility.actions(actions.clone());
            end_accessibility = end_accessibility.actions(actions);
        }

        let track_height = self.size.track_height();
        let thumb_width = self.size.thumb_width();
        let thumb_height = self.size.thumb_height();

        let (track_bg, active_bg, thumb_bg) = if self.disabled {
            (
                theme.tokens.muted.opacity(0.3),
                theme.tokens.primary.opacity(0.3),
                theme.tokens.primary.opacity(0.3),
            )
        } else {
            (
                theme.tokens.muted,
                theme.tokens.primary,
                theme.tokens.primary,
            )
        };

        let focus_ring = astryx::focus_ring_outer(theme.tokens.primary);
        let user_style = self.style.clone();

        match self.axis {
            SliderAxis::Horizontal => self.render_horizontal(
                window,
                theme,
                start_focus_handle,
                end_focus_handle,
                is_focused,
                start_percentage,
                end_percentage,
                start_value,
                end_value,
                track_height,
                thumb_width,
                thumb_height,
                track_bg,
                active_bg,
                thumb_bg,
                focus_ring,
                user_style,
                start_accessibility,
                end_accessibility,
                instance_id,
            ),
            SliderAxis::Vertical => self.render_vertical(
                window,
                theme,
                start_focus_handle,
                end_focus_handle,
                is_focused,
                start_percentage,
                end_percentage,
                start_value,
                end_value,
                track_height,
                thumb_width,
                thumb_height,
                track_bg,
                active_bg,
                thumb_bg,
                focus_ring,
                user_style,
                start_accessibility,
                end_accessibility,
                instance_id,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RangeSlider, RangeSliderState};
    use kael::{
        AccessibilityAction, AccessibilityRole, AccessibilityState, AppContext, Context, Entity,
        IntoElement, ParentElement, Render, TestAppContext, Window, div,
    };

    struct RangeSliderHost {
        default: Entity<RangeSliderState>,
        large: Entity<RangeSliderState>,
        disabled: Entity<RangeSliderState>,
    }

    impl Render for RangeSliderHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(RangeSlider::new(self.default.clone()).id("range-default"))
                .child(RangeSlider::new(self.large.clone()).id("range-large"))
                .child(
                    RangeSlider::new(self.disabled.clone())
                        .id("range-disabled")
                        .disabled(true),
                )
        }
    }

    #[kael::test]
    fn reversed_ranges_are_sorted_and_snapped_from_minimum(cx: &mut TestAppContext) {
        let state = cx.new(RangeSliderState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_min(10.0, cx);
                state.set_max(30.0, cx);
                state.set_step(3.0, cx);
                state.set_range(27.0, 12.0, cx);
            });
        });
        assert_eq!(cx.read(|cx| state.read(cx).range()), (13.0, 28.0));
    }

    #[kael::test]
    fn invalid_steps_and_values_do_not_poison_state(cx: &mut TestAppContext) {
        let state = cx.new(RangeSliderState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                let before = state.range();
                state.set_step(0.0, cx);
                state.set_range(f32::NAN, 50.0, cx);
                assert_eq!(state.step(), 1.0);
                assert_eq!(state.range(), before);
            });
        });
    }

    #[kael::test]
    fn rendered_instances_have_distinct_enabled_semantics(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let default = cx.new(RangeSliderState::new);
        let large = cx.new(RangeSliderState::new);
        let disabled = cx.new(RangeSliderState::new);
        let (_host, window) = cx.add_window_view({
            let default = default.clone();
            let large = large.clone();
            let disabled = disabled.clone();
            move |_, _| RangeSliderHost {
                default,
                large,
                disabled,
            }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            let sliders = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| node.role == AccessibilityRole::Slider)
                .collect::<Vec<_>>();
            assert_eq!(sliders.len(), 6);
            assert_eq!(
                sliders
                    .iter()
                    .filter(|node| node.states.contains(AccessibilityState::DISABLED))
                    .count(),
                2
            );
            for node in sliders
                .iter()
                .filter(|node| !node.states.contains(AccessibilityState::DISABLED))
            {
                assert!(node.actions.contains(&AccessibilityAction::Increment));
                assert!(
                    window
                        .has_accessibility_action_handler(node.id, AccessibilityAction::Increment,)
                );
            }
        });
    }
}
