use super::local_history::{
    WindowValueHistory, ensure_local_undo_redo_bindings, local_undo_redo_key_context,
    register_focused_action_handler_when,
};
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, App, BorderStyle, Bounds, CursorStyle, DispatchPhase, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, Interactivity, IntoElement,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, Redo, Undo, Window, fill, outline, point, px, relative, size,
};
use std::{cell::RefCell, rc::Rc};

type ChangeListener = Rc<dyn Fn(&Pixels, &mut Window, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq)]
struct SplitterDragState {
    start_value: Pixels,
    start_position: Point<Pixels>,
}

#[derive(Debug)]
struct SplitterPersistentState {
    value: Pixels,
    history: WindowValueHistory<Pixels>,
    drag_state: Option<SplitterDragState>,
}

impl SplitterPersistentState {
    fn new(history: WindowValueHistory<Pixels>, value: Pixels) -> Self {
        Self {
            value,
            history,
            drag_state: None,
        }
    }

    fn sync_from_props(&mut self, value: Pixels) {
        if self.value == value {
            return;
        }

        self.value = value;
        self.drag_state = None;
        self.history.clear();
    }

    fn begin_drag(&mut self, position: Point<Pixels>) {
        self.drag_state.get_or_insert(SplitterDragState {
            start_value: self.value,
            start_position: position,
        });
    }

    fn update_drag(
        &mut self,
        value: Pixels,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.drag_state.is_none() {
            return;
        }

        if self.value != value {
            self.value = value;
            if let Some(listener) = on_change.clone() {
                listener(&value, window, cx);
            }
        }
    }

    fn finish_drag(
        &mut self,
        value: Pixels,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(drag_state) = self.drag_state.take() else {
            return;
        };

        if self.value != value {
            self.value = value;
            if let Some(listener) = on_change.clone() {
                listener(&value, window, cx);
            }
        }

        if drag_state.start_value != self.value {
            self.history.record(drag_state.start_value, self.value);
        }
    }

    fn commit_value(
        &mut self,
        value: Pixels,
        on_change: &Option<ChangeListener>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.drag_state = None;
        if self.value != value {
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

    fn drag_state(&self) -> Option<SplitterDragState> {
        self.drag_state
    }

    fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
/// Snapshot of splitter state passed to a custom renderer.
pub struct SplitterRenderState {
    /// The current splitter position.
    pub value: Pixels,
    /// The minimum splitter position.
    pub min: Pixels,
    /// The maximum splitter position.
    pub max: Pixels,
    /// Whether the visible splitter rule is vertical.
    pub vertical: bool,
    /// Current position expressed as a value between 0.0 and 1.0.
    pub percentage: f64,
    /// Whether the splitter is currently being dragged.
    pub dragging: bool,
    /// Whether the splitter currently owns keyboard focus.
    pub focused: bool,
}

type SplitterCustomRenderer =
    Rc<dyn Fn(SplitterRenderState, Bounds<Pixels>, &mut Window, &mut App)>;

/// Construct a controlled splitter primitive for resizable panes.
#[track_caller]
pub fn splitter(id: impl Into<ElementId>, value: Pixels) -> Splitter {
    Splitter::new(id.into(), value)
}

/// A controlled splitter primitive with caller-owned visuals.
pub struct Splitter {
    interactivity: Interactivity,
    element_id: ElementId,
    value: Pixels,
    min: Pixels,
    max: Pixels,
    step: Option<Pixels>,
    discrete: bool,
    vertical: bool,
    on_change: Option<ChangeListener>,
    custom_renderer: Option<SplitterCustomRenderer>,
}

#[doc(hidden)]
pub struct SplitterPrepaintState {
    hitbox: Hitbox,
}

impl Splitter {
    #[track_caller]
    fn new(element_id: ElementId, value: Pixels) -> Self {
        let mut interactivity = Interactivity::new();
        interactivity.element_id = Some(element_id.clone());
        interactivity.focusable = true;
        interactivity.tab_stop = true;
        interactivity.key_context = Some(local_undo_redo_key_context());
        interactivity.base_style.size.width = Some(px(8.0).into());
        interactivity.base_style.size.height = Some(relative(1.0).into());

        Self {
            interactivity,
            element_id,
            value,
            min: px(0.0),
            max: px(1000.0),
            step: Some(px(1.0)),
            discrete: false,
            vertical: true,
            on_change: None,
            custom_renderer: None,
        }
    }

    /// Set the minimum splitter position.
    pub fn min(mut self, min: Pixels) -> Self {
        self.min = min;
        self
    }

    /// Set the maximum splitter position.
    pub fn max(mut self, max: Pixels) -> Self {
        self.max = max;
        self
    }

    /// Set the keyboard and drag snap increment.
    pub fn step(mut self, step: Pixels) -> Self {
        self.step = (step > Pixels::ZERO).then_some(step);
        self
    }

    /// Snap drag updates down to the nearest step instead of rounding.
    pub fn discrete(mut self) -> Self {
        self.discrete = true;
        self
    }

    /// Render a horizontal splitter rule that moves vertically.
    pub fn horizontal(mut self) -> Self {
        self.vertical = false;
        self.interactivity.base_style.size.width = Some(relative(1.0).into());
        self.interactivity.base_style.size.height = Some(px(8.0).into());
        self
    }

    /// Register a callback invoked with the newly requested splitter position.
    pub fn on_change(
        mut self,
        listener: impl Fn(&Pixels, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Render the splitter with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(SplitterRenderState, Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }

    fn clamped_range(&self) -> (Pixels, Pixels) {
        if self.min <= self.max {
            (self.min, self.max)
        } else {
            (self.max, self.min)
        }
    }

    fn clamped_value(&self) -> Pixels {
        clamp_splitter_value(self.value, self.clamped_range())
    }
}

impl Element for Splitter {
    type RequestLayoutState = ();
    type PrepaintState = SplitterPrepaintState;

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
            .expect("splitter requires a focus handle");
        window.set_focus_handle(&focus_handle, cx);
        window.next_frame.tab_stops.insert(&focus_handle);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);

        SplitterPrepaintState { hitbox }
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
        let global_id = global_id.expect("splitter requires a global id");
        ensure_local_undo_redo_bindings(cx);
        window.set_key_context(local_undo_redo_key_context());

        let focus_handle = self
            .interactivity
            .tracked_focus_handle
            .clone()
            .expect("splitter requires a focus handle");
        let undo_manager = window.undo_manager();
        let persistent_state = window.with_element_state(
            &global_id,
            |state: Option<Rc<RefCell<SplitterPersistentState>>>, _| {
                let state = state.unwrap_or_else(|| {
                    let history = WindowValueHistory::new(
                        undo_manager.clone(),
                        &focus_handle,
                        "Splitter resize",
                    );
                    Rc::new(RefCell::new(SplitterPersistentState::new(
                        history,
                        self.clamped_value(),
                    )))
                });
                (state.clone(), state)
            },
        );
        persistent_state
            .borrow_mut()
            .sync_from_props(self.clamped_value());

        if prepaint.hitbox.is_hovered(window) {
            window.set_cursor_style(splitter_cursor(self.vertical), &prepaint.hitbox);
        }

        let value = persistent_state.borrow().value;
        let is_focused = focus_handle.is_focused(window);
        let is_dragging = persistent_state.borrow().is_dragging();
        let (range_min, range_max) = self.clamped_range();
        let percentage = splitter_fraction(value, (range_min, range_max));
        let render_state = SplitterRenderState {
            value,
            min: range_min,
            max: range_max,
            vertical: self.vertical,
            percentage,
            dragging: is_dragging,
            focused: is_focused,
        };

        if let Some(renderer) = &self.custom_renderer {
            renderer(render_state, bounds, window, cx);
        } else {
            paint_default_splitter(render_state, bounds, window);
        }

        let on_change_mouse_move = self.on_change.clone();
        let on_change_mouse_up = self.on_change.clone();
        let on_change_key = self.on_change.clone();
        let on_change_undo = self.on_change.clone();
        let on_change_redo = self.on_change.clone();
        let vertical = self.vertical;
        let step = self.step;
        let discrete = self.discrete;
        let min = range_min;
        let max = range_max;
        let can_undo = persistent_state.borrow().history.can_undo();
        let can_redo = persistent_state.borrow().history.can_redo();

        register_focused_action_handler_when::<Undo>(window, can_undo, focus_handle.clone(), {
            let state = persistent_state.clone();
            move |_, window, cx| {
                state.borrow_mut().undo(&on_change_undo, window, cx);
                window.refresh();
            }
        });
        register_focused_action_handler_when::<Redo>(window, can_redo, focus_handle.clone(), {
            let state = persistent_state.clone();
            move |_, window, cx| {
                state.borrow_mut().redo(&on_change_redo, window, cx);
                window.refresh();
            }
        });

        let mouse_focus_handle = focus_handle.clone();
        let mouse_state = persistent_state.clone();
        let hitbox = prepaint.hitbox.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !hitbox.is_hovered(window)
            {
                return;
            }

            window.focus(&mouse_focus_handle);
            mouse_state.borrow_mut().begin_drag(event.position);
            window.refresh();
        });

        let move_state = persistent_state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }

            let Some(drag_state) = move_state.borrow().drag_state() else {
                return;
            };

            let next = splitter_value_for_drag(
                event.position,
                drag_state,
                vertical,
                (min, max),
                step,
                discrete,
            );
            move_state
                .borrow_mut()
                .update_drag(next, &on_change_mouse_move, window, cx);
            window.refresh();
        });

        let up_state = persistent_state.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }

            let Some(drag_state) = up_state.borrow().drag_state() else {
                return;
            };

            let next = splitter_value_for_drag(
                event.position,
                drag_state,
                vertical,
                (min, max),
                step,
                discrete,
            );
            up_state
                .borrow_mut()
                .finish_drag(next, &on_change_mouse_up, window, cx);
            window.refresh();
        });

        let key_focus_handle = focus_handle;
        let key_state = persistent_state;
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || !key_focus_handle.is_focused(window)
                || event.keystroke.modifiers.modified()
            {
                return;
            }

            let current_value = key_state.borrow().value;
            let base_step = step.unwrap_or(px(16.0));
            let delta = match event.keystroke.key.as_str() {
                "left" if vertical => Some(-base_step),
                "right" if vertical => Some(base_step),
                "up" if !vertical => Some(-base_step),
                "down" if !vertical => Some(base_step),
                "pageup" => Some(-base_step * 10.0),
                "pagedown" => Some(base_step * 10.0),
                _ => None,
            };

            let next = if let Some(delta) = delta {
                Some(quantize_splitter_value(
                    current_value + delta,
                    (min, max),
                    step,
                    discrete,
                ))
            } else {
                match event.keystroke.key.as_str() {
                    "home" => Some(min),
                    "end" => Some(max),
                    _ => None,
                }
            };

            if let Some(next) = next {
                key_state
                    .borrow_mut()
                    .commit_value(next, &on_change_key, window, cx);
                window.refresh();
                window.prevent_default();
            }
        });

        window.register_accessibility_node(
            AccessibilityAttributes::new(AccessibilityRole::Separator)
                .states(if is_focused {
                    AccessibilityState::FOCUSED
                } else {
                    AccessibilityState::NONE
                })
                .value(AccessibilityValue::Range {
                    current: value.0 as f64,
                    min: range_min.0 as f64,
                    max: range_max.0 as f64,
                    step: step.map(|step| step.0 as f64),
                })
                .actions(vec![
                    AccessibilityAction::Focus,
                    AccessibilityAction::Increment,
                    AccessibilityAction::Decrement,
                ])
                .to_node(crate::AccessibilityId::new()),
        );
    }
}

impl IntoElement for Splitter {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

fn splitter_cursor(vertical: bool) -> CursorStyle {
    if vertical {
        CursorStyle::ResizeColumn
    } else {
        CursorStyle::ResizeRow
    }
}

fn clamp_splitter_value(value: Pixels, range: (Pixels, Pixels)) -> Pixels {
    value.max(range.0).min(range.1)
}

fn splitter_fraction(value: Pixels, range: (Pixels, Pixels)) -> f64 {
    let span = range.1 - range.0;
    if span <= Pixels::ZERO {
        0.0
    } else {
        ((value.0 - range.0.0) / span.0).clamp(0.0, 1.0) as f64
    }
}

fn quantize_splitter_value(
    value: Pixels,
    range: (Pixels, Pixels),
    step: Option<Pixels>,
    discrete: bool,
) -> Pixels {
    let clamped = clamp_splitter_value(value, range);
    match step {
        Some(step) if step > Pixels::ZERO => {
            let offset = clamped - range.0;
            let steps = if discrete {
                (offset.0 / step.0).floor()
            } else {
                (offset.0 / step.0).round()
            };
            clamp_splitter_value(range.0 + step * steps, range)
        }
        _ => clamped,
    }
}

fn splitter_value_for_drag(
    position: Point<Pixels>,
    drag_state: SplitterDragState,
    vertical: bool,
    range: (Pixels, Pixels),
    step: Option<Pixels>,
    discrete: bool,
) -> Pixels {
    let delta = if vertical {
        position.x - drag_state.start_position.x
    } else {
        position.y - drag_state.start_position.y
    };
    quantize_splitter_value(drag_state.start_value + delta, range, step, discrete)
}

fn splitter_track_bounds(bounds: Bounds<Pixels>, vertical: bool) -> Bounds<Pixels> {
    if vertical {
        Bounds::new(
            point(bounds.center().x - px(1.0), bounds.top() + px(4.0)),
            size(px(2.0), bounds.size.height - px(8.0)),
        )
    } else {
        Bounds::new(
            point(bounds.left() + px(4.0), bounds.center().y - px(1.0)),
            size(bounds.size.width - px(8.0), px(2.0)),
        )
    }
}

fn splitter_grip_bounds(bounds: Bounds<Pixels>, vertical: bool) -> Bounds<Pixels> {
    if vertical {
        Bounds::new(
            point(bounds.center().x - px(2.0), bounds.center().y - px(12.0)),
            size(px(4.0), px(24.0)),
        )
    } else {
        Bounds::new(
            point(bounds.center().x - px(12.0), bounds.center().y - px(2.0)),
            size(px(24.0), px(4.0)),
        )
    }
}

fn paint_default_splitter(state: SplitterRenderState, bounds: Bounds<Pixels>, window: &mut Window) {
    let accent = if state.dragging || state.focused {
        crate::rgb(0x1d4ed8)
    } else {
        crate::rgb(0x94a3b8)
    };
    let track = splitter_track_bounds(bounds, state.vertical);
    let grip = splitter_grip_bounds(bounds, state.vertical);

    window.paint_quad(fill(track, accent).corner_radii(px(999.0)));
    window.paint_quad(fill(grip, accent).corner_radii(px(999.0)));

    if state.focused {
        window.paint_quad(
            outline(bounds, crate::rgb(0x93c5fd), BorderStyle::default()).corner_radii(px(999.0)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::div::InteractiveElement;
    use crate::{
        Context, Modifiers, MouseButton, ParentElement, Render, Styled, TestAppContext, div,
    };
    use std::{cell::Cell, rc::Rc};

    struct SplitterView {
        value: Pixels,
    }

    struct CustomSplitterView {
        value: Pixels,
        snapshot: Rc<Cell<Option<(f64, bool, bool)>>>,
    }

    impl Render for SplitterView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(8.0))
                .h(px(120.0))
                .debug_selector(|| "splitter-frame".to_string())
                .child(
                    splitter("pane_splitter", self.value)
                        .min(px(0.0))
                        .max(px(240.0))
                        .step(px(1.0))
                        .on_change(cx.listener(|this, value, _, cx| {
                            this.value = *value;
                            cx.notify();
                        })),
                )
        }
    }

    impl Render for CustomSplitterView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let snapshot = self.snapshot.clone();
            div()
                .w(px(8.0))
                .h(px(120.0))
                .debug_selector(|| "custom-splitter-frame".to_string())
                .child(
                    splitter("custom_splitter", self.value)
                        .min(px(0.0))
                        .max(px(240.0))
                        .render_with(move |state, _, _, _| {
                            snapshot.set(Some((
                                state.value.0 as f64,
                                state.vertical,
                                state.focused,
                            )));
                        }),
                )
        }
    }

    #[test]
    fn quantize_splitter_value_clamps_and_snaps() {
        assert_eq!(
            quantize_splitter_value(px(-10.0), (px(0.0), px(100.0)), Some(px(5.0)), false),
            px(0.0)
        );
        assert_eq!(
            quantize_splitter_value(px(17.0), (px(0.0), px(100.0)), Some(px(5.0)), false),
            px(15.0)
        );
        assert_eq!(
            quantize_splitter_value(px(19.0), (px(0.0), px(100.0)), Some(px(5.0)), true),
            px(15.0)
        );
    }

    #[crate::test]
    fn splitter_drag_undo_redo_coalesces_to_one_history_entry(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| SplitterView { value: px(40.0) });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("splitter-frame").unwrap();
        let start = bounds.center();
        let end = point(bounds.center().x + px(60.0), bounds.center().y);

        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, px(100.0));
        });

        window.simulate_keystrokes("secondary-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, px(40.0));
        });

        window.simulate_keystrokes("secondary-shift-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, px(100.0));
        });
    }

    #[crate::test]
    fn splitter_keyboard_moves_vertical_splitter(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| SplitterView { value: px(40.0) });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("splitter-frame").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.simulate_keystrokes("right pagedown home end");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, px(240.0));
        });
    }

    #[crate::test]
    fn splitter_render_with_receives_value_and_focus(cx: &mut TestAppContext) {
        let snapshot = Rc::new(Cell::new(None));
        let snapshot_ref = snapshot.clone();
        let (_view, mut window) = cx.add_window_view(|_, _| CustomSplitterView {
            value: px(64.0),
            snapshot: snapshot_ref,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("custom-splitter-frame").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let (value, vertical, focused) = snapshot.get().unwrap();
        assert_eq!(value, 64.0);
        assert!(vertical);
        assert!(focused);
    }
}
