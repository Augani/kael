use kael::{prelude::FluentBuilder as _, *};

use crate::animations::DisplayFrameClock;
use crate::spring::{SpringPoint, SpringPreset};

pub struct DraggableSpringState {
    offset: SpringPoint,
    pan: PanGesture,
    snap_points: Vec<Point<f32>>,
    is_dragging: bool,
    animating: bool,
    drag_origin: Point<f32>,
    grab_offset: Point<f32>,
    release_velocity: Point<f32>,
    frame_clock: DisplayFrameClock,
}

impl DraggableSpringState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            offset: SpringPoint::with_preset(0.0, 0.0, SpringPreset::Snappy),
            pan: PanGesture::new(),
            snap_points: Vec::new(),
            is_dragging: false,
            animating: false,
            drag_origin: Point::default(),
            grab_offset: Point::default(),
            release_velocity: Point::default(),
            frame_clock: DisplayFrameClock::default(),
        }
    }

    pub fn set_snap_points(&mut self, snap_points: Vec<Point<f32>>) {
        self.snap_points = snap_points
            .into_iter()
            .filter(|point| point.x.is_finite() && point.y.is_finite())
            .collect();
    }

    pub fn set_preset(&mut self, preset: SpringPreset) {
        self.offset.set_preset(preset);
    }

    pub fn offset(&self) -> Point<f32> {
        let (x, y) = self.offset.value();
        point(x, y)
    }

    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    fn nudge(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        self.cancel_animation();
        self.offset.stop();
        let (x, y) = self.offset.value();
        self.offset.set_value(x + dx, y + dy);
        cx.notify();
    }

    fn reset_position(&mut self, cx: &mut Context<Self>) {
        self.cancel_animation();
        self.offset.stop();
        self.offset.set_value(0.0, 0.0);
        cx.notify();
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.pan.reset();
        self.pan.on_event(&PlatformInput::MouseDown(event.clone()));
        self.cancel_animation();
        self.offset.stop();
        self.is_dragging = true;
        self.release_velocity = Point::default();
        self.drag_origin = point(f32::from(position.x), f32::from(position.y));
        let current = self.offset.value();
        self.grab_offset = point(current.0, current.1);
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !self.is_dragging {
            return;
        }

        if let Some(pan_event) = self.pan.on_event(&PlatformInput::MouseMove(event.clone())) {
            self.release_velocity = point(
                f32::from(pan_event.velocity.x),
                f32::from(pan_event.velocity.y),
            );
            let pointer = point(f32::from(position.x), f32::from(position.y));
            let next_x = self.grab_offset.x + (pointer.x - self.drag_origin.x);
            let next_y = self.grab_offset.y + (pointer.y - self.drag_origin.y);
            self.offset.set_value(next_x, next_y);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if !self.is_dragging {
            return;
        }

        self.is_dragging = false;

        if let Some(pan_event) = self.pan.on_event(&PlatformInput::MouseUp(event.clone())) {
            self.release_velocity = point(
                f32::from(pan_event.velocity.x),
                f32::from(pan_event.velocity.y),
            );
        }
        let release_velocity = self.release_velocity;

        let current = self.offset.value();
        let target = self.nearest_snap(point(current.0, current.1));

        if cx.reduce_motion() {
            self.offset.stop();
            self.offset.set_value(target.x, target.y);
            self.cancel_animation();
            cx.notify();
            return;
        }

        self.offset.set_target(target.x, target.y);
        self.offset.impulse(release_velocity.x, release_velocity.y);

        self.start_animation(cx);
        cx.notify();
    }

    fn nearest_snap(&self, current: Point<f32>) -> Point<f32> {
        if self.snap_points.is_empty() {
            return point(0.0, 0.0);
        }

        let mut best = self.snap_points[0];
        let mut best_distance = f32::MAX;

        for snap in &self.snap_points {
            let dx = current.x - snap.x;
            let dy = current.y - snap.y;
            let distance = dx * dx + dy * dy;
            if distance < best_distance {
                best_distance = distance;
                best = *snap;
            }
        }

        best
    }

    fn start_animation(&mut self, cx: &mut Context<Self>) {
        if self.animating {
            return;
        }
        self.animating = true;
        self.frame_clock.restart();
        cx.notify();
    }

    fn cancel_animation(&mut self) {
        self.animating = false;
        self.frame_clock.stop();
    }
}

fn schedule_spring_frame(state: &Entity<DraggableSpringState>, window: &Window, cx: &mut App) {
    let generation = state.update(cx, |state, _| {
        state
            .animating
            .then(|| state.frame_clock.try_arm())
            .flatten()
    });
    let Some(generation) = generation else {
        return;
    };

    let state = state.downgrade();
    window.on_next_frame(move |window, cx| {
        _ = state.update(cx, |state, cx| {
            let Some(dt) = state.frame_clock.sample(generation) else {
                return;
            };
            if !state.animating || state.is_dragging {
                state.cancel_animation();
                return;
            }
            if !window.animations_enabled() {
                let target = state.offset.target();
                state.offset.set_value(target.0, target.1);
                state.cancel_animation();
                cx.notify();
                return;
            }

            if !state.offset.tick(dt) {
                state.cancel_animation();
            }
            cx.notify();
        });
    });
}

impl Render for DraggableSpringState {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

fn drag_offset_from_action(request: &AccessibilityActionRequest) -> Option<Point<f32>> {
    let AccessibilityActionPayload::Value(value) = request.payload.as_ref()? else {
        return None;
    };
    let values = value
        .split(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-'
        })
        .filter(|part| !part.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if values.len() == 2 && values.iter().all(|value| value.is_finite()) {
        Some(point(values[0], values[1]))
    } else {
        None
    }
}

#[derive(IntoElement)]
pub struct DraggableSpring {
    id: ElementId,
    label: SharedString,
    state: Entity<DraggableSpringState>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl DraggableSpring {
    pub fn new(id: impl Into<ElementId>, state: Entity<DraggableSpringState>) -> Self {
        Self {
            id: id.into(),
            label: "Draggable item".into(),
            state,
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn snap_points(self, snap_points: Vec<Point<f32>>, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, _| state.set_snap_points(snap_points));
        self
    }

    pub fn spring_preset(self, preset: SpringPreset, cx: &mut App) -> Self {
        self.state.update(cx, |state, _| state.set_preset(preset));
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Styled for DraggableSpring {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for DraggableSpring {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for DraggableSpring {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        schedule_spring_frame(&self.state, window, cx);
        let state = self.state.read(cx);
        let offset = state.offset();
        let accessibility_value = format!("x {:.0}, y {:.0}", offset.x, offset.y);

        let down_state = self.state.clone();
        let move_state = self.state.clone();
        let up_state = self.state.clone();
        let key_state = self.state.clone();
        let accessibility_state = self.state.clone();

        div()
            .id(self.id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.label.to_string())
                    .description(
                        "Drag with the pointer, use arrow keys to move, or press Home to reset",
                    )
                    .value(AccessibilityValue::Text(accessibility_value))
                    .actions(vec![
                        AccessibilityAction::Focus,
                        AccessibilityAction::SetValue,
                    ]),
            )
            .relative()
            .focusable()
            .tab_index(0)
            .tab_stop(true)
            .focus_visible(|style| {
                style.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                    crate::theme::Theme::of(cx).tokens.ring,
                )])
            })
            .child(
                div()
                    .ml(px(offset.x))
                    .mt(px(offset.y))
                    .children(self.children),
            )
            .on_mouse_down(
                MouseButton::Left,
                window.listener_for(&down_state, |state, event: &MouseDownEvent, _window, cx| {
                    state.on_mouse_down(event, event.position, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(window.listener_for(
                &move_state,
                |state, event: &MouseMoveEvent, _window, cx| {
                    state.on_mouse_move(event, event.position, cx);
                },
            ))
            .on_mouse_up(
                MouseButton::Left,
                window.listener_for(&up_state, |state, event: &MouseUpEvent, _window, cx| {
                    state.on_mouse_up(event, cx);
                }),
            )
            .on_key_down(move |event, window, cx| {
                let delta = if event.keystroke.modifiers.shift {
                    24.0
                } else {
                    8.0
                };
                let movement = match event.keystroke.key.as_str() {
                    "left" => Some((-delta, 0.0)),
                    "right" => Some((delta, 0.0)),
                    "up" => Some((0.0, -delta)),
                    "down" => Some((0.0, delta)),
                    _ => None,
                };
                if let Some((dx, dy)) = movement {
                    key_state.update(cx, |state, cx| state.nudge(dx, dy, cx));
                    cx.stop_propagation();
                    window.prevent_default();
                } else if event.keystroke.key == "home" {
                    key_state.update(cx, |state, cx| state.reset_position(cx));
                    cx.stop_propagation();
                    window.prevent_default();
                }
            })
            .on_accessibility_action(
                AccessibilityAction::SetValue,
                move |request, _window, cx| {
                    let Some(offset) = drag_offset_from_action(request) else {
                        return;
                    };
                    accessibility_state.update(cx, |state, cx| {
                        state.offset.stop();
                        state.offset.set_value(offset.x, offset.y);
                        cx.notify();
                    });
                },
            )
            .map(|this: Stateful<Div>| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn snap_points_drop_non_finite_coordinates() {
        let mut cx = TestAppContext::single();
        let state = cx.new(DraggableSpringState::new);

        cx.update(|cx| {
            state.update(cx, |state, _| {
                state.set_snap_points(vec![
                    point(20.0, 30.0),
                    point(f32::NAN, 0.0),
                    point(0.0, f32::INFINITY),
                ]);
                assert_eq!(state.snap_points, vec![point(20.0, 30.0)]);
            });
        });
    }

    #[::core::prelude::v1::test]
    fn accessibility_value_parses_two_dimensional_offsets() {
        let request = AccessibilityActionRequest::with_payload(
            AccessibilityId::new(),
            AccessibilityAction::SetValue,
            AccessibilityActionPayload::Value("x -24, y 16".into()),
        );

        assert_eq!(drag_offset_from_action(&request), Some(point(-24.0, 16.0)));
    }
}
