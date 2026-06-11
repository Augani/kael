use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::spring::{SpringPoint, SpringPreset};

const FRAME_INTERVAL: Duration = Duration::from_millis(8);

pub struct DraggableSpringState {
    offset: SpringPoint,
    pan: PanGesture,
    snap_points: Vec<Point<f32>>,
    is_dragging: bool,
    animating: bool,
    drag_origin: Point<f32>,
    grab_offset: Point<f32>,
    release_velocity: Point<f32>,
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
        }
    }

    pub fn set_snap_points(&mut self, snap_points: Vec<Point<f32>>) {
        self.snap_points = snap_points;
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

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.pan.reset();
        self.pan.on_event(&PlatformInput::MouseDown(event.clone()));
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
        self.schedule_tick(cx);
    }

    fn schedule_tick(&self, cx: &mut Context<Self>) {
        cx.spawn(async |this, cx| {
            cx.background_executor().timer(FRAME_INTERVAL).await;

            _ = this.update(cx, |state, cx| {
                if !state.animating {
                    return;
                }

                if state.is_dragging {
                    state.animating = false;
                    return;
                }

                let moving = state.offset.tick_with_real_dt();
                if moving {
                    state.schedule_tick(cx);
                } else {
                    state.animating = false;
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for DraggableSpringState {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(IntoElement)]
pub struct DraggableSpring {
    id: ElementId,
    state: Entity<DraggableSpringState>,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl DraggableSpring {
    pub fn new(id: impl Into<ElementId>, state: Entity<DraggableSpringState>) -> Self {
        Self {
            id: id.into(),
            state,
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
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
        let state = self.state.read(cx);
        let offset = state.offset();

        let down_state = self.state.clone();
        let move_state = self.state.clone();
        let up_state = self.state.clone();

        div()
            .id(self.id)
            .relative()
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
            .map(|this: Stateful<Div>| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}
