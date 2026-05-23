use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, App, BorderStyle, Bounds, CursorStyle, DispatchPhase, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, Interactivity, IntoElement,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, ScrollHandle, Window, fill, outline, point, px, size,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq)]
struct ScrollBarDragState {
    start_logical_offset: Pixels,
    start_position: Point<Pixels>,
}

#[derive(Debug, Default)]
struct ScrollBarPersistentState {
    drag_state: Option<ScrollBarDragState>,
}

impl ScrollBarPersistentState {
    fn begin_drag(&mut self, logical_offset: Pixels, position: Point<Pixels>) {
        self.drag_state = Some(ScrollBarDragState {
            start_logical_offset: logical_offset,
            start_position: position,
        });
    }

    fn end_drag(&mut self) {
        self.drag_state = None;
    }

    fn drag_state(&self) -> Option<ScrollBarDragState> {
        self.drag_state
    }

    fn is_dragging(&self) -> bool {
        self.drag_state.is_some()
    }
}

#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
/// Snapshot of scroll bar state passed to a custom renderer.
pub struct ScrollBarRenderState {
    /// Whether the scroll bar is vertical.
    pub vertical: bool,
    /// The current logical scroll offset, expressed as a positive distance from the start.
    pub logical_offset: Pixels,
    /// The maximum logical scroll offset.
    pub max_offset: Pixels,
    /// The size of the visible viewport along the scrolling axis.
    pub viewport_size: Pixels,
    /// The total content size along the scrolling axis.
    pub content_size: Pixels,
    /// Current thumb position expressed as a value between 0.0 and 1.0.
    pub percentage: f64,
    /// Thumb size expressed as a fraction of the track length.
    pub thumb_ratio: f64,
    /// Whether the scroll bar is currently being dragged.
    pub dragging: bool,
    /// Whether the scroll bar currently owns keyboard focus.
    pub focused: bool,
    /// Visual opacity of the scroll bar (0.0 = hidden, 1.0 = fully visible).
    pub opacity: f32,
}

impl ScrollBarRenderState {
    /// Compute thumb bounds inside the provided scroll bar bounds.
    pub fn thumb_bounds(&self, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
        scroll_bar_thumb_bounds(bounds, *self)
    }
}

type ScrollBarCustomRenderer =
    Rc<dyn Fn(ScrollBarRenderState, Bounds<Pixels>, &mut Window, &mut App)>;

/// Construct a scroll bar primitive bound to a scroll handle.
#[track_caller]
pub fn scroll_bar(id: impl Into<ElementId>, scroll_handle: ScrollHandle) -> ScrollBar {
    ScrollBar::new(id.into(), scroll_handle)
}

/// A focusable scroll bar primitive bound to a scrollable container.
pub struct ScrollBar {
    interactivity: Interactivity,
    element_id: ElementId,
    scroll_handle: ScrollHandle,
    vertical: bool,
    step: Pixels,
    custom_renderer: Option<ScrollBarCustomRenderer>,
}

#[doc(hidden)]
pub struct ScrollBarPrepaintState {
    hitbox: Hitbox,
}

impl ScrollBar {
    #[track_caller]
    fn new(element_id: ElementId, scroll_handle: ScrollHandle) -> Self {
        let mut interactivity = Interactivity::new();
        interactivity.element_id = Some(element_id.clone());
        interactivity.focusable = true;
        interactivity.tab_stop = true;
        interactivity.base_style.size.width = Some(px(12.0).into());
        interactivity.base_style.size.height = Some(px(160.0).into());

        Self {
            interactivity,
            element_id,
            scroll_handle,
            vertical: true,
            step: px(40.0),
            custom_renderer: None,
        }
    }

    /// Render the scroll bar horizontally instead of vertically.
    pub fn horizontal(mut self) -> Self {
        self.vertical = false;
        self.interactivity.base_style.size.width = Some(px(160.0).into());
        self.interactivity.base_style.size.height = Some(px(12.0).into());
        self
    }

    /// Set the keyboard increment used for arrow-key scrolling.
    pub fn step(mut self, step: Pixels) -> Self {
        if step > Pixels::ZERO {
            self.step = step;
        }
        self
    }

    /// Render the scroll track and thumb with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(ScrollBarRenderState, Bounds<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }
}

impl Element for ScrollBar {
    type RequestLayoutState = ();
    type PrepaintState = ScrollBarPrepaintState;

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
            .expect("scroll bar requires a focus handle");
        window.set_focus_handle(&focus_handle, cx);
        window.next_frame.tab_stops.insert(&focus_handle);

        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        ScrollBarPrepaintState { hitbox }
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
        let global_id = global_id.expect("scroll bar requires a global id");
        let focus_handle = self
            .interactivity
            .tracked_focus_handle
            .clone()
            .expect("scroll bar requires a focus handle");
        let persistent_state = window.with_element_state(
            &global_id,
            |state: Option<Rc<RefCell<ScrollBarPersistentState>>>, _| {
                let state = state
                    .unwrap_or_else(|| Rc::new(RefCell::new(ScrollBarPersistentState::default())));
                (state.clone(), state)
            },
        );

        if prepaint.hitbox.is_hovered(window) {
            window.set_cursor_style(CursorStyle::PointingHand, &prepaint.hitbox);
        }

        let is_hovered = prepaint.hitbox.is_hovered(window);
        let render_state = build_scroll_bar_render_state(
            &self.scroll_handle,
            self.vertical,
            focus_handle.is_focused(window),
            persistent_state.borrow().is_dragging(),
            is_hovered,
        );

        if let Some(renderer) = &self.custom_renderer {
            renderer(render_state, bounds, window, cx);
        } else {
            paint_default_scroll_bar(render_state, bounds, window);
        }

        if render_state.opacity > 0.0 && render_state.opacity < 1.0 {
            window.request_animation_frame();
        }

        let hitbox = prepaint.hitbox.clone();
        let mouse_focus_handle = focus_handle.clone();
        let mouse_state = persistent_state.clone();
        let mouse_scroll_handle = self.scroll_handle.clone();
        let vertical = self.vertical;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !hitbox.is_hovered(window)
            {
                return;
            }

            window.focus(&mouse_focus_handle);
            let render_state = build_scroll_bar_render_state(
                &mouse_scroll_handle,
                vertical,
                true,
                mouse_state.borrow().is_dragging(),
                true,
            );
            let thumb_bounds = render_state.thumb_bounds(bounds);
            let current_logical = render_state.logical_offset;

            let next_logical = if thumb_bounds.contains(&event.position) {
                current_logical
            } else {
                logical_offset_for_position(event.position, bounds, render_state, true)
            };

            set_scroll_handle_logical_offset(&mouse_scroll_handle, vertical, next_logical);
            mouse_state
                .borrow_mut()
                .begin_drag(next_logical, event.position);
            window.refresh();
        });

        let move_state = persistent_state.clone();
        let move_scroll_handle = self.scroll_handle.clone();
        let move_focus_handle = focus_handle.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }

            let Some(drag_state) = move_state.borrow().drag_state() else {
                return;
            };

            let render_state = build_scroll_bar_render_state(
                &move_scroll_handle,
                vertical,
                move_focus_handle.is_focused(window),
                true,
                false,
            );
            let next_logical =
                logical_offset_for_drag(event.position, bounds, render_state, drag_state);
            set_scroll_handle_logical_offset(&move_scroll_handle, vertical, next_logical);
            window.refresh();
        });

        let up_state = persistent_state.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }

            if !up_state.borrow().is_dragging() {
                return;
            }

            up_state.borrow_mut().end_drag();
            window.refresh();
        });

        let key_focus_handle = focus_handle;
        let key_scroll_handle = self.scroll_handle.clone();
        let step = self.step;
        window.on_key_event(move |event: &KeyDownEvent, phase, window, _cx| {
            if phase != DispatchPhase::Bubble
                || !key_focus_handle.is_focused(window)
                || event.keystroke.modifiers.modified()
            {
                return;
            }

            let render_state =
                build_scroll_bar_render_state(&key_scroll_handle, vertical, true, false, false);
            let page = if render_state.viewport_size > Pixels::ZERO {
                render_state.viewport_size
            } else {
                step * 10.0
            };
            let current = render_state.logical_offset;
            let next = match event.keystroke.key.as_str() {
                "up" if vertical => Some(clamp_pixels(
                    current - step,
                    Pixels::ZERO,
                    render_state.max_offset,
                )),
                "down" if vertical => Some(clamp_pixels(
                    current + step,
                    Pixels::ZERO,
                    render_state.max_offset,
                )),
                "left" if !vertical => Some(clamp_pixels(
                    current - step,
                    Pixels::ZERO,
                    render_state.max_offset,
                )),
                "right" if !vertical => Some(clamp_pixels(
                    current + step,
                    Pixels::ZERO,
                    render_state.max_offset,
                )),
                "pageup" => Some(clamp_pixels(
                    current - page,
                    Pixels::ZERO,
                    render_state.max_offset,
                )),
                "pagedown" => Some(clamp_pixels(
                    current + page,
                    Pixels::ZERO,
                    render_state.max_offset,
                )),
                "home" => Some(Pixels::ZERO),
                "end" => Some(render_state.max_offset),
                _ => None,
            };

            if let Some(next) = next {
                set_scroll_handle_logical_offset(&key_scroll_handle, vertical, next);
                window.refresh();
                window.prevent_default();
            }
        });

        window.register_accessibility_node(
            AccessibilityAttributes::new(AccessibilityRole::ScrollBar)
                .states(if render_state.focused {
                    AccessibilityState::FOCUSED
                } else {
                    AccessibilityState::NONE
                })
                .value(AccessibilityValue::Range {
                    current: render_state.logical_offset.0 as f64,
                    min: 0.0,
                    max: render_state.max_offset.0 as f64,
                    step: Some(self.step.0 as f64),
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

impl IntoElement for ScrollBar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

const SCROLLBAR_SHOW_DELAY: f64 = 1.0;
const SCROLLBAR_FADE_DURATION: f64 = 0.4;

fn scrollbar_opacity(
    scroll_handle: &ScrollHandle,
    dragging: bool,
    focused: bool,
    hovered: bool,
) -> f32 {
    if dragging || focused || hovered {
        return 1.0;
    }

    let elapsed = scroll_handle.seconds_since_last_scroll();
    if elapsed < SCROLLBAR_SHOW_DELAY {
        1.0
    } else {
        let fade_progress = ((elapsed - SCROLLBAR_SHOW_DELAY) / SCROLLBAR_FADE_DURATION).min(1.0);
        (1.0 - ease_in_out(fade_progress)) as f32
    }
}

fn ease_in_out(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

fn build_scroll_bar_render_state(
    scroll_handle: &ScrollHandle,
    vertical: bool,
    focused: bool,
    dragging: bool,
    hovered: bool,
) -> ScrollBarRenderState {
    let offset = scroll_handle.offset();
    let max_offset = scroll_handle.max_offset();
    let viewport_bounds = scroll_handle.bounds();

    let logical_offset = logical_scroll_offset(offset, vertical);
    let max_axis_offset = logical_axis_size(max_offset, vertical);
    let viewport_size = axis_pixels(viewport_bounds.size, vertical);
    let content_size = viewport_size + max_axis_offset;
    let percentage = if max_axis_offset > Pixels::ZERO {
        (logical_offset.0 / max_axis_offset.0) as f64
    } else {
        0.0
    };
    let thumb_ratio = if content_size > Pixels::ZERO {
        (viewport_size.0 / content_size.0).clamp(0.0, 1.0) as f64
    } else {
        1.0
    };

    let opacity = scrollbar_opacity(scroll_handle, dragging, focused, hovered);

    ScrollBarRenderState {
        vertical,
        logical_offset,
        max_offset: max_axis_offset,
        viewport_size,
        content_size,
        percentage,
        thumb_ratio,
        dragging,
        focused,
        opacity,
    }
}

fn paint_default_scroll_bar(
    state: ScrollBarRenderState,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    if state.opacity <= 0.0 {
        return;
    }

    let track_bounds = scroll_bar_track_bounds(bounds, state.vertical);
    let thumb_bounds = state.thumb_bounds(bounds);

    let track_color = crate::hsla(0., 0., 0.89, 0.6 * state.opacity);
    let thumb_color = crate::hsla(0., 0., 0.58, state.opacity);
    let border_color = if state.focused {
        crate::hsla(220. / 360., 0.77, 0.47, state.opacity)
    } else {
        crate::hsla(215. / 360., 0.16, 0.47, 0.6 * state.opacity)
    };

    window.paint_quad(fill(track_bounds, track_color).corner_radii(px(999.0)));
    window.paint_quad(
        fill(thumb_bounds, thumb_color)
            .corner_radii(px(999.0))
            .border_widths(px(1.0))
            .border_color(border_color),
    );

    if state.focused {
        let focus_color = crate::hsla(213. / 360., 0.94, 0.78, 0.8 * state.opacity);
        window.paint_quad(
            outline(bounds, focus_color, BorderStyle::default()).corner_radii(px(999.0)),
        );
    }
}

fn scroll_bar_track_bounds(bounds: Bounds<Pixels>, vertical: bool) -> Bounds<Pixels> {
    if vertical {
        Bounds::new(
            point(bounds.center().x - px(3.0), bounds.top() + px(4.0)),
            size(px(6.0), bounds.size.height - px(8.0)),
        )
    } else {
        Bounds::new(
            point(bounds.left() + px(4.0), bounds.center().y - px(3.0)),
            size(bounds.size.width - px(8.0), px(6.0)),
        )
    }
}

fn scroll_bar_thumb_bounds(bounds: Bounds<Pixels>, state: ScrollBarRenderState) -> Bounds<Pixels> {
    let track = scroll_bar_track_bounds(bounds, state.vertical);
    let track_length = axis_pixels(track.size, state.vertical);
    let thumb_length = scroll_bar_thumb_length(track_length, state.thumb_ratio as f32);
    let available = (track_length - thumb_length).max(Pixels::ZERO);
    let fraction = if state.max_offset > Pixels::ZERO {
        (state.logical_offset.0 / state.max_offset.0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    if state.vertical {
        let top = track.top() + available * fraction;
        Bounds::new(
            point(track.left(), top),
            size(track.size.width, thumb_length),
        )
    } else {
        let left = track.left() + available * fraction;
        Bounds::new(
            point(left, track.top()),
            size(thumb_length, track.size.height),
        )
    }
}

fn scroll_bar_thumb_length(track_length: Pixels, thumb_ratio: f32) -> Pixels {
    let candidate = track_length * thumb_ratio.clamp(0.0, 1.0);
    candidate.max(px(20.0)).min(track_length)
}

fn logical_offset_for_position(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    state: ScrollBarRenderState,
    center_thumb: bool,
) -> Pixels {
    if state.max_offset <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let track = scroll_bar_track_bounds(bounds, state.vertical);
    let track_length = axis_pixels(track.size, state.vertical);
    let thumb_length = scroll_bar_thumb_length(track_length, state.thumb_ratio as f32);
    let available = (track_length - thumb_length).max(Pixels::ZERO);
    if available <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let pointer = if state.vertical {
        position.y - track.top()
    } else {
        position.x - track.left()
    };
    let anchor = if center_thumb {
        pointer - thumb_length / 2.0
    } else {
        pointer
    };
    let clamped = clamp_pixels(anchor, Pixels::ZERO, available);
    let fraction = clamped.0 / available.0;
    state.max_offset * fraction
}

fn logical_offset_for_drag(
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    state: ScrollBarRenderState,
    drag_state: ScrollBarDragState,
) -> Pixels {
    if state.max_offset <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let track = scroll_bar_track_bounds(bounds, state.vertical);
    let track_length = axis_pixels(track.size, state.vertical);
    let thumb_length = scroll_bar_thumb_length(track_length, state.thumb_ratio as f32);
    let available = (track_length - thumb_length).max(Pixels::ZERO);
    if available <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let delta = if state.vertical {
        position.y - drag_state.start_position.y
    } else {
        position.x - drag_state.start_position.x
    };
    let logical_delta = state.max_offset * (delta.0 / available.0);
    clamp_pixels(
        drag_state.start_logical_offset + logical_delta,
        Pixels::ZERO,
        state.max_offset,
    )
}

fn set_scroll_handle_logical_offset(
    scroll_handle: &ScrollHandle,
    vertical: bool,
    logical_offset: Pixels,
) {
    let mut offset = scroll_handle.offset();
    if vertical {
        offset.y = -logical_offset;
    } else {
        offset.x = -logical_offset;
    }
    scroll_handle.set_offset(offset);
}

fn logical_scroll_offset(offset: Point<Pixels>, vertical: bool) -> Pixels {
    let axis = if vertical { -offset.y } else { -offset.x };
    axis.max(Pixels::ZERO)
}

fn axis_pixels(size: crate::Size<Pixels>, vertical: bool) -> Pixels {
    if vertical { size.height } else { size.width }
}

fn logical_axis_size(size: crate::Size<Pixels>, vertical: bool) -> Pixels {
    if vertical { size.height } else { size.width }
}

fn clamp_pixels(value: Pixels, min: Pixels, max: Pixels) -> Pixels {
    value.max(min).min(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::div::{InteractiveElement, StatefulInteractiveElement};
    use crate::{
        Context, Modifiers, MouseButton, ParentElement, Render, Styled, TestAppContext, div,
    };
    use std::{cell::Cell, rc::Rc};

    struct ScrollBarView {
        handle: ScrollHandle,
    }

    struct CustomScrollBarView {
        handle: ScrollHandle,
        snapshot: Rc<Cell<Option<(f64, f64, bool)>>>,
    }

    impl Render for ScrollBarView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .h(px(160.0))
                .w(px(120.0))
                .child(
                    div()
                        .w(px(100.0))
                        .h(px(160.0))
                        .id("scroll-content-region")
                        .overflow_y_scroll()
                        .track_scroll(&self.handle)
                        .child(
                            div()
                                .w(px(100.0))
                                .h(px(480.0))
                                .debug_selector(|| "scroll-content".to_string()),
                        ),
                )
                .child(
                    div()
                        .h(px(160.0))
                        .debug_selector(|| "scroll-bar-frame".to_string())
                        .child(scroll_bar("content_scroll_bar", self.handle.clone())),
                )
        }
    }

    impl Render for CustomScrollBarView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let snapshot = self.snapshot.clone();
            div()
                .flex()
                .h(px(160.0))
                .w(px(120.0))
                .child(
                    div()
                        .w(px(100.0))
                        .h(px(160.0))
                        .id("custom-scroll-content-region")
                        .overflow_y_scroll()
                        .track_scroll(&self.handle)
                        .child(div().w(px(100.0)).h(px(480.0))),
                )
                .child(div().h(px(160.0)).child(
                    scroll_bar("custom_scroll_bar", self.handle.clone()).render_with(
                        move |state, _, _, _| {
                            snapshot.set(Some((
                                state.percentage,
                                state.thumb_ratio,
                                state.focused,
                            )));
                        },
                    ),
                ))
        }
    }

    #[test]
    fn scroll_bar_thumb_bounds_shrink_with_large_content() {
        let state = ScrollBarRenderState {
            vertical: true,
            logical_offset: px(0.0),
            max_offset: px(320.0),
            viewport_size: px(160.0),
            content_size: px(480.0),
            percentage: 0.0,
            thumb_ratio: 160.0 / 480.0,
            dragging: false,
            focused: false,
            opacity: 1.0,
        };
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(12.0), px(160.0)));
        let thumb = state.thumb_bounds(bounds);

        assert!(thumb.size.height < scroll_bar_track_bounds(bounds, true).size.height);
        assert!(thumb.size.height >= px(20.0));
    }

    #[crate::test]
    fn scroll_bar_drag_updates_bound_scroll_handle(cx: &mut TestAppContext) {
        let handle = ScrollHandle::new();
        let (_view, mut window) = cx.add_window_view(|_, _| ScrollBarView {
            handle: handle.clone(),
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("scroll-bar-frame").unwrap();
        let start = point(bounds.center().x, bounds.top() + px(12.0));
        let end = point(bounds.center().x, bounds.bottom() - px(12.0));

        window.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
        window.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(handle.offset().y < px(-100.0));
            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ScrollBar)
                .unwrap();
            assert!(matches!(node.value, Some(AccessibilityValue::Range { .. })));
        });
    }

    #[crate::test]
    fn scroll_bar_keyboard_updates_bound_scroll_handle(cx: &mut TestAppContext) {
        let handle = ScrollHandle::new();
        let (_view, mut window) = cx.add_window_view(|_, _| ScrollBarView {
            handle: handle.clone(),
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("scroll-bar-frame").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.simulate_keystrokes("pagedown");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(handle.offset().y < px(0.0));
        });
    }

    #[crate::test]
    fn scroll_bar_render_with_receives_thumb_metrics(cx: &mut TestAppContext) {
        let handle = ScrollHandle::new();
        let snapshot = Rc::new(Cell::new(None));
        let snapshot_ref = snapshot.clone();
        let (_view, mut window) = cx.add_window_view(|_, _| CustomScrollBarView {
            handle: handle.clone(),
            snapshot: snapshot_ref,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        handle.set_offset(point(px(0.0), px(-80.0)));
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let (percentage, thumb_ratio, focused) = snapshot.get().unwrap();
        assert!(percentage > 0.0);
        assert!(thumb_ratio > 0.0 && thumb_ratio < 1.0);
        assert!(!focused);
    }
}
