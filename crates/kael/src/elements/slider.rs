use super::local_history::{
    WindowValueHistory, ensure_local_undo_redo_bindings, local_undo_redo_key_context,
    register_focused_action_handler_when,
};
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, App, BorderStyle, Bounds, CursorStyle, DispatchPhase, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, InteractiveElement, Interactivity,
    IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Redo, Styled, Undo, Window, fill, outline, point, px, relative, size,
};
use std::{cell::RefCell, rc::Rc};

type ChangeListener = std::rc::Rc<dyn Fn(&f64, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq)]
struct SliderDragState {
    start_value: f64,
}

#[derive(Debug)]
struct SliderPersistentState {
    value: f64,
    history: WindowValueHistory<f64>,
    drag_state: Option<SliderDragState>,
}

impl SliderPersistentState {
    fn new(history: WindowValueHistory<f64>, value: f64) -> Self {
        Self {
            value,
            history,
            drag_state: None,
        }
    }

    fn sync_from_props(&mut self, value: f64) {
        if slider_values_equal(self.value, value) {
            return;
        }

        // Keep an in-progress drag authoritative: a controlled caller that
        // dispatches on_change and feeds the (possibly quantized/clamped)
        // model value back into this prop must not cancel the live gesture.
        // The prop is re-adopted on the first frame after the drag ends.
        if self.drag_state.is_some() {
            return;
        }

        self.value = value;
        self.drag_state = None;
        self.history.clear();
    }

    fn begin_drag(
        &mut self,
        value: f64,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.drag_state.get_or_insert(SliderDragState {
            start_value: self.value,
        });
        self.value = value;
        if let Some(listener) = on_change.clone() {
            listener(&value, window, cx);
        }
    }

    fn update_drag(
        &mut self,
        value: f64,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.drag_state.is_none() {
            return;
        }

        self.value = value;
        if let Some(listener) = on_change.clone() {
            listener(&value, window, cx);
        }
    }

    fn finish_drag(
        &mut self,
        value: f64,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(drag_state) = self.drag_state.take() else {
            return;
        };

        self.value = value;
        if let Some(listener) = on_change.clone() {
            listener(&value, window, cx);
        }
        if !slider_values_equal(drag_state.start_value, value) {
            self.history.record(drag_state.start_value, value);
        }
    }

    fn commit_value(
        &mut self,
        value: f64,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.drag_state = None;
        if !slider_values_equal(self.value, value) {
            let previous = self.value;
            self.value = value;
            self.history.record(previous, value);
        }
        if let Some(listener) = on_change.clone() {
            listener(&value, window, cx);
        }
    }

    fn undo(&mut self, on_change: &Option<ChangeListener>, window: &mut Window, cx: &mut App) {
        self.drag_state = None;
        let Some(previous) = self.history.undo() else {
            return;
        };

        self.value = previous;
        if let Some(listener) = on_change.clone() {
            listener(&previous, window, cx);
        }
    }

    fn redo(&mut self, on_change: &Option<ChangeListener>, window: &mut Window, cx: &mut App) {
        self.drag_state = None;
        let Some(next) = self.history.redo() else {
            return;
        };

        self.value = next;
        if let Some(listener) = on_change.clone() {
            listener(&next, window, cx);
        }
    }

    fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }
}

#[non_exhaustive]
/// Snapshot of slider state passed to a custom renderer.
pub struct SliderRenderState {
    /// The current slider value.
    pub value: f64,
    /// The normalized minimum bound.
    pub min: f64,
    /// The normalized maximum bound.
    pub max: f64,
    /// Current position expressed as a value between 0.0 and 1.0.
    pub percentage: f64,
    /// Whether the slider is currently being dragged.
    pub dragging: bool,
    /// Whether the slider currently owns keyboard focus.
    pub focused: bool,
    /// Whether the slider is disabled.
    pub disabled: bool,
}

impl SliderRenderState {
    /// Returns true when the current value is at the normalized minimum.
    pub fn is_at_min(&self) -> bool {
        self.percentage <= f64::EPSILON
    }

    /// Returns true when the current value is at the normalized maximum.
    pub fn is_at_max(&self) -> bool {
        (1.0 - self.percentage).abs() <= f64::EPSILON
    }

    /// Coarse slider position class for content-safe diagnostics.
    pub fn position_class(&self) -> &'static str {
        if self.is_at_min() {
            "min"
        } else if self.is_at_max() {
            "max"
        } else if self.percentage < 0.5 {
            "lower"
        } else if self.percentage > 0.5 {
            "upper"
        } else {
            "middle"
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "slider_render_state(position_class={}, at_min={}, at_max={}, dragging={}, focused={}, disabled={})",
            self.position_class(),
            self.is_at_min(),
            self.is_at_max(),
            self.dragging,
            self.focused,
            self.disabled
        )
    }
}

type SliderCustomRenderer = Rc<dyn Fn(SliderRenderState, Bounds<Pixels>, &mut Window, &mut App)>;

/// Construct a controlled slider form control.
#[track_caller]
pub fn slider(id: impl Into<ElementId>, value: f64) -> Slider {
    Slider::new(id.into(), value)
}

/// A controlled slider form control.
pub struct Slider {
    interactivity: Interactivity,
    element_id: ElementId,
    value: f64,
    min: f64,
    max: f64,
    step: Option<f64>,
    discrete: bool,
    vertical: bool,
    disabled: bool,
    on_change: Option<ChangeListener>,
    custom_renderer: Option<SliderCustomRenderer>,
}

#[doc(hidden)]
pub struct SliderPrepaintState {
    hitbox: Hitbox,
}

impl Slider {
    #[track_caller]
    fn new(element_id: ElementId, value: f64) -> Self {
        let mut interactivity = Interactivity::new();
        interactivity.element_id = Some(element_id.clone());
        interactivity.focusable = true;
        interactivity.tab_stop = true;
        interactivity.key_context = Some(local_undo_redo_key_context());
        interactivity.base_style.size.width = Some(relative(1.0).into());
        interactivity.base_style.size.height = Some(px(24.0).into());

        Self {
            interactivity,
            element_id,
            value,
            min: 0.0,
            max: 100.0,
            step: Some(1.0),
            discrete: false,
            vertical: false,
            disabled: false,
            on_change: None,
            custom_renderer: None,
        }
    }

    /// Set the minimum slider value.
    pub fn min(mut self, min: f64) -> Self {
        self.min = min;
        self
    }

    /// Set the maximum slider value.
    pub fn max(mut self, max: f64) -> Self {
        self.max = max;
        self
    }

    /// Set the slider increment used for keyboard input and snapping.
    pub fn step(mut self, step: f64) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            Some(step)
        } else {
            self.step
        };
        self
    }

    /// Snap slider interactions to discrete values.
    pub fn discrete(mut self) -> Self {
        self.discrete = true;
        self
    }

    /// Render the slider vertically instead of horizontally.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self.interactivity.base_style.size.width = Some(px(24.0).into());
        self.interactivity.base_style.size.height = Some(px(160.0).into());
        self
    }

    /// Disable the slider, preventing user interaction.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Register a callback invoked with the next slider value after user interaction.
    pub fn on_change(mut self, listener: impl Fn(&f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(listener));
        self
    }

    /// Render the slider track and thumb with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(SliderRenderState, Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }

    fn clamped_range(&self) -> (f64, f64) {
        if self.min <= self.max {
            (self.min, self.max)
        } else {
            (self.max, self.min)
        }
    }

    fn clamped_value(&self) -> f64 {
        clamp_to_range(self.value, self.clamped_range())
    }
}

impl Element for Slider {
    type RequestLayoutState = ();
    type PrepaintState = SliderPrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout_id = self.interactivity.request_layout(
            global_id,
            inspector_id,
            window,
            cx,
            |style, window, cx| window.request_layout(style, None, cx),
        );
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let focus_handle = self
            .interactivity
            .tracked_focus_handle
            .clone()
            .expect("slider requires a focus handle");
        window.set_focus_handle(&focus_handle, cx);
        window.next_frame.tab_stops.insert(&focus_handle);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        SliderPrepaintState { hitbox }
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let global_id = global_id.expect("slider requires a global id");
        ensure_local_undo_redo_bindings(cx);
        window.set_key_context(local_undo_redo_key_context());

        let focus_handle = self
            .interactivity
            .tracked_focus_handle
            .clone()
            .expect("slider requires a focus handle");
        let undo_manager = window.undo_manager();
        let drag_state = window.with_element_state(
            &global_id,
            |state: Option<Rc<RefCell<SliderPersistentState>>>, _| {
                let state = state.unwrap_or_else(|| {
                    let history = WindowValueHistory::new(
                        undo_manager.clone(),
                        &focus_handle,
                        "Slider change",
                    );
                    Rc::new(RefCell::new(SliderPersistentState::new(
                        history,
                        self.clamped_value(),
                    )))
                });
                (state.clone(), state)
            },
        );
        drag_state
            .borrow_mut()
            .sync_from_props(self.clamped_value());

        if prepaint.hitbox.is_hovered(window) && !self.disabled {
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }

        let value = drag_state.borrow().value;
        let is_focused = focus_handle.is_focused(window);
        let is_dragging = drag_state.borrow().is_dragging();
        let (range_min, range_max) = self.clamped_range();

        if let Some(renderer) = &self.custom_renderer {
            let percentage = slider_fraction(value, (range_min, range_max));
            renderer(
                SliderRenderState {
                    value,
                    min: range_min,
                    max: range_max,
                    percentage,
                    dragging: is_dragging,
                    focused: is_focused,
                    disabled: self.disabled,
                },
                bounds,
                window,
                cx,
            );
        } else {
            let track_bounds = slider_track_bounds(bounds, self.vertical);
            let thumb_bounds =
                slider_thumb_bounds(bounds, self.vertical, value, (range_min, range_max));

            window.paint_quad(fill(track_bounds, crate::rgb(0xe2e8f0)).corner_radii(px(999.0)));
            let active_bounds = slider_active_bounds(track_bounds, thumb_bounds, self.vertical);
            window.paint_quad(fill(active_bounds, crate::rgb(0x1d4ed8)).corner_radii(px(999.0)));
            window.paint_quad(
                fill(thumb_bounds, crate::rgb(0xffffff))
                    .corner_radii(px(999.0))
                    .border_widths(px(1.0))
                    .border_color(if is_focused {
                        crate::rgb(0x1d4ed8)
                    } else {
                        crate::rgb(0x94a3b8)
                    }),
            );

            if is_focused {
                window.paint_quad(
                    outline(bounds, crate::rgb(0x93c5fd), BorderStyle::default())
                        .corner_radii(px(999.0)),
                );
            }
        }

        let on_change_mouse_down = self.on_change.clone();
        let on_change_mouse_move = self.on_change.clone();
        let on_change_mouse_up = self.on_change.clone();
        let on_change_key = self.on_change.clone();
        let on_change_undo = self.on_change.clone();
        let on_change_redo = self.on_change.clone();
        let vertical = self.vertical;
        let min = range_min;
        let max = range_max;
        let step = self.step;
        let discrete = self.discrete;
        let can_undo = drag_state.borrow().history.can_undo();
        let can_redo = drag_state.borrow().history.can_redo();

        let slider_disabled = self.disabled;
        let hitbox = prepaint.hitbox.clone();
        let mouse_focus_handle = focus_handle.clone();
        let mouse_drag_state = drag_state.clone();
        register_focused_action_handler_when::<Undo>(window, can_undo, focus_handle.clone(), {
            let state = drag_state.clone();
            move |_, window, cx| {
                state.borrow_mut().undo(&on_change_undo, window, cx);
                window.refresh();
            }
        });
        register_focused_action_handler_when::<Redo>(window, can_redo, focus_handle.clone(), {
            let state = drag_state.clone();
            move |_, window, cx| {
                state.borrow_mut().redo(&on_change_redo, window, cx);
                window.refresh();
            }
        });
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if slider_disabled {
                return;
            }
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !hitbox.is_hovered(window)
            {
                return;
            }

            window.focus(&mouse_focus_handle);
            let value = slider_value_for_position(
                event.position,
                bounds,
                vertical,
                (min, max),
                step,
                discrete,
            );
            mouse_drag_state
                .borrow_mut()
                .begin_drag(value, &on_change_mouse_down, window, cx);
            window.refresh();
        });

        let move_drag_state = drag_state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }

            if !event.dragging() {
                return;
            }

            if !move_drag_state.borrow().is_dragging() {
                return;
            }

            let value = slider_value_for_position(
                event.position,
                bounds,
                vertical,
                (min, max),
                step,
                discrete,
            );
            move_drag_state
                .borrow_mut()
                .update_drag(value, &on_change_mouse_move, window, cx);
            window.refresh();
        });

        let up_drag_state = drag_state.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }

            if !up_drag_state.borrow().is_dragging() {
                return;
            }

            let value = slider_value_for_position(
                event.position,
                bounds,
                vertical,
                (min, max),
                step,
                discrete,
            );
            up_drag_state
                .borrow_mut()
                .finish_drag(value, &on_change_mouse_up, window, cx);
            window.refresh();
        });

        let key_focus_handle = focus_handle;
        let key_state = drag_state;
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if slider_disabled {
                return;
            }
            if phase != DispatchPhase::Bubble
                || !key_focus_handle.is_focused(window)
                || event.keystroke.modifiers.modified()
            {
                return;
            }

            let current_value = key_state.borrow().value;
            let delta = match event.keystroke.key.as_str() {
                "left" if !vertical => Some(-step.unwrap_or(1.0)),
                "right" if !vertical => Some(step.unwrap_or(1.0)),
                "down" if vertical => Some(-step.unwrap_or(1.0)),
                "up" if vertical => Some(step.unwrap_or(1.0)),
                "pagedown" => Some(-step.unwrap_or(1.0) * 10.0),
                "pageup" => Some(step.unwrap_or(1.0) * 10.0),
                _ => None,
            };

            if let Some(delta) = delta {
                let next = quantize_slider_value(current_value + delta, (min, max), step, discrete);
                key_state
                    .borrow_mut()
                    .commit_value(next, &on_change_key, window, cx);
                window.refresh();
                window.prevent_default();
            }
        });

        let accessibility_id = window.next_anonymous_accessibility_id();
        window.register_accessibility_node_at(
            AccessibilityAttributes::new(AccessibilityRole::Slider)
                .states(if is_focused {
                    AccessibilityState::FOCUSED
                } else {
                    AccessibilityState::NONE
                })
                .value(AccessibilityValue::Range {
                    current: value,
                    min,
                    max,
                    step,
                })
                .actions(vec![
                    AccessibilityAction::Focus,
                    AccessibilityAction::Increment,
                    AccessibilityAction::Decrement,
                ])
                .to_node(accessibility_id),
            bounds,
        );
    }
}

impl IntoElement for Slider {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Slider {
    fn style(&mut self) -> &mut crate::StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Slider {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

fn clamp_to_range(value: f64, range: (f64, f64)) -> f64 {
    value.clamp(range.0, range.1)
}

fn slider_values_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9
}

fn slider_fraction(value: f64, range: (f64, f64)) -> f64 {
    let span = range.1 - range.0;
    if span.abs() <= f64::EPSILON {
        0.0
    } else {
        ((value - range.0) / span).clamp(0.0, 1.0)
    }
}

fn quantize_slider_value(value: f64, range: (f64, f64), step: Option<f64>, discrete: bool) -> f64 {
    let value = clamp_to_range(value, range);
    if !discrete {
        return if let Some(step) = step {
            if step > 0.0 {
                let steps = ((value - range.0) / step).round();
                clamp_to_range(range.0 + steps * step, range)
            } else {
                value
            }
        } else {
            value
        };
    }

    if let Some(step) = step {
        if step > 0.0 {
            let steps = ((value - range.0) / step).round();
            return clamp_to_range(range.0 + steps * step, range);
        }
    }

    value.round().clamp(range.0, range.1)
}

fn slider_track_bounds(bounds: Bounds<Pixels>, vertical: bool) -> Bounds<Pixels> {
    if vertical {
        Bounds::new(
            point(bounds.center().x - px(2.0), bounds.top() + px(8.0)),
            size(px(4.0), bounds.size.height - px(16.0)),
        )
    } else {
        Bounds::new(
            point(bounds.left() + px(8.0), bounds.center().y - px(2.0)),
            size(bounds.size.width - px(16.0), px(4.0)),
        )
    }
}

fn slider_thumb_bounds(
    bounds: Bounds<Pixels>,
    vertical: bool,
    value: f64,
    range: (f64, f64),
) -> Bounds<Pixels> {
    let track = slider_track_bounds(bounds, vertical);
    let fraction = slider_fraction(value, range) as f32;
    let thumb_size = size(px(14.0), px(14.0));

    if vertical {
        let top = track.bottom() - px(7.0) - track.size.height * fraction;
        Bounds::new(point(bounds.center().x - px(7.0), top), thumb_size)
    } else {
        let left = track.left() - px(7.0) + track.size.width * fraction;
        Bounds::new(point(left, bounds.center().y - px(7.0)), thumb_size)
    }
}

fn slider_active_bounds(
    track: Bounds<Pixels>,
    thumb: Bounds<Pixels>,
    vertical: bool,
) -> Bounds<Pixels> {
    if vertical {
        Bounds::from_corners(
            point(track.left(), thumb.center().y),
            point(track.right(), track.bottom()),
        )
    } else {
        Bounds::from_corners(
            point(track.left(), track.top()),
            point(thumb.center().x, track.bottom()),
        )
    }
}

fn slider_value_for_position(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    vertical: bool,
    range: (f64, f64),
    step: Option<f64>,
    discrete: bool,
) -> f64 {
    let track = slider_track_bounds(bounds, vertical);
    let fraction = if vertical {
        ((track.bottom() - position.y) / track.size.height) as f64
    } else {
        ((position.x - track.left()) / track.size.width) as f64
    }
    .clamp(0.0, 1.0);

    let value = range.0 + (range.1 - range.0) * fraction;
    quantize_slider_value(value, range, step, discrete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Modifiers, MouseButton, ParentElement, Render, TestAppContext, div};
    use std::cell::Cell;

    struct SliderView {
        value: f64,
    }

    struct CustomSliderView {
        value: f64,
        snapshot: Rc<Cell<Option<(f64, f64, bool)>>>,
    }

    impl Render for SliderView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(240.0))
                .h(px(24.0))
                .debug_selector(|| "slider-frame".to_string())
                .child(
                    slider("volume", self.value)
                        .min(0.0)
                        .max(100.0)
                        .step(1.0)
                        .on_change(cx.listener(|this, value, _, cx| {
                            this.value = *value;
                            cx.notify();
                        })),
                )
        }
    }

    impl Render for CustomSliderView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let snapshot = self.snapshot.clone();
            div().w(px(240.0)).h(px(24.0)).child(
                slider("volume_custom", self.value)
                    .min(0.0)
                    .max(100.0)
                    .render_with(move |state, _, _, _| {
                        snapshot.set(Some((state.value, state.percentage, state.focused)));
                    }),
            )
        }
    }

    #[test]
    fn quantize_slider_value_clamps_to_range() {
        assert_eq!(
            quantize_slider_value(-10.0, (0.0, 100.0), Some(5.0), false),
            0.0
        );
        assert_eq!(
            quantize_slider_value(110.0, (0.0, 100.0), Some(5.0), false),
            100.0
        );
    }

    #[test]
    fn quantize_slider_value_respects_step() {
        assert_eq!(
            quantize_slider_value(13.0, (0.0, 100.0), Some(5.0), false),
            15.0
        );
        assert_eq!(
            quantize_slider_value(12.0, (0.0, 100.0), Some(5.0), true),
            10.0
        );
    }

    #[test]
    fn slider_value_for_position_inverts_vertical_axis() {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(24.0), px(120.0)));
        let top = slider_value_for_position(
            point(px(12.0), px(8.0)),
            bounds,
            true,
            (0.0, 100.0),
            Some(1.0),
            false,
        );
        let bottom = slider_value_for_position(
            point(px(12.0), px(112.0)),
            bounds,
            true,
            (0.0, 100.0),
            Some(1.0),
            false,
        );

        assert!(top > bottom);
    }

    #[crate::test]
    fn slider_drag_undo_redo_coalesces_to_one_history_entry(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| SliderView { value: 0.0 });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("slider-frame").unwrap();
        let track = slider_track_bounds(bounds, false);
        let start = point(track.left(), track.center().y);
        let middle = track.center();
        let end = point(track.right(), track.center().y);

        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(middle, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, 100.0);
        });

        window.simulate_keystrokes("secondary-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, 0.0);
        });

        window.simulate_keystrokes("secondary-shift-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, 100.0);
        });
    }

    #[crate::test]
    fn slider_render_with_receives_value_and_percentage(cx: &mut TestAppContext) {
        let snapshot = Rc::new(Cell::new(None));
        let snapshot_ref = snapshot.clone();
        let (_view, mut window) = cx.add_window_view(|_, _| CustomSliderView {
            value: 25.0,
            snapshot: snapshot_ref,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(snapshot.get(), Some((25.0, 0.25, false)));
    }

    #[test]
    fn slider_render_state_summary_is_content_safe() {
        let state = SliderRenderState {
            value: 42.5,
            min: 10.0,
            max: 90.0,
            percentage: 0.40625,
            dragging: true,
            focused: true,
            disabled: false,
        };

        assert_eq!(state.position_class(), "lower");
        assert!(!state.is_at_min());
        assert!(!state.is_at_max());

        let summary = state.to_text();
        assert!(summary.contains("position_class=lower"));
        assert!(summary.contains("at_min=false"));
        assert!(summary.contains("at_max=false"));
        assert!(summary.contains("dragging=true"));
        assert!(summary.contains("focused=true"));
        assert!(summary.contains("disabled=false"));
        assert!(!summary.contains("42.5"));
        assert!(!summary.contains("10.0"));
        assert!(!summary.contains("90.0"));
        assert!(!summary.contains("0.40625"));
    }
}
