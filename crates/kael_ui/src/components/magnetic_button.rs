//! Button whose position subtly shifts toward cursor when nearby.

use kael::{prelude::FluentBuilder as _, *};

pub struct MagneticButtonState {
    mouse_offset: Point<f32>,
    pointer_origin: Option<Point<f32>>,
    is_hovering: bool,
}

impl MagneticButtonState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            mouse_offset: Point::default(),
            pointer_origin: None,
            is_hovering: false,
        }
    }
}

impl Render for MagneticButtonState {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

#[derive(IntoElement)]
pub struct MagneticButton {
    id: ElementId,
    state: Entity<MagneticButtonState>,
    strength: f32,
    #[allow(dead_code)]
    range: Pixels,
    style: StyleRefinement,
    children: Vec<AnyElement>,
}

impl MagneticButton {
    pub fn new(id: impl Into<ElementId>, state: Entity<MagneticButtonState>) -> Self {
        Self {
            id: id.into(),
            state,
            strength: 0.3,
            range: px(100.0),
            style: StyleRefinement::default(),
            children: Vec::new(),
        }
    }

    pub fn strength(mut self, strength: f32) -> Self {
        if strength.is_finite() {
            self.strength = strength.clamp(0.0, 1.0);
        }
        self
    }

    pub fn range(mut self, range: Pixels) -> Self {
        if f32::from(range).is_finite() && range > px(0.0) {
            self.range = range;
        }
        self
    }
}

impl Styled for MagneticButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for MagneticButton {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for MagneticButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let state = self.state.read(cx);
        let strength = self.strength;
        let range = f32::from(self.range);
        let motion_enabled = !cx.reduce_motion();

        let (final_x, final_y) = if motion_enabled && state.is_hovering {
            (
                state.mouse_offset.x * strength * 20.0,
                state.mouse_offset.y * strength * 20.0,
            )
        } else {
            (0.0, 0.0)
        };

        let state_move = self.state.clone();
        let state_hover = self.state.clone();

        div()
            .id(self.id)
            .relative()
            .cursor_pointer()
            .child(
                div()
                    .ml(px(final_x))
                    .mt(px(final_y))
                    .children(self.children),
            )
            .on_mouse_move(move |event: &MouseMoveEvent, _window, cx| {
                if !motion_enabled {
                    return;
                }
                state_move.update(cx, |s, cx| {
                    let pointer = point(f32::from(event.position.x), f32::from(event.position.y));
                    let origin = *s.pointer_origin.get_or_insert(pointer);
                    let mut dx = (pointer.x - origin.x) / range;
                    let mut dy = (pointer.y - origin.y) / range;
                    let magnitude = (dx * dx + dy * dy).sqrt();
                    if magnitude > 1.0 {
                        dx /= magnitude;
                        dy /= magnitude;
                    }
                    s.mouse_offset = point(dx, dy);
                    cx.notify();
                });
            })
            .on_hover(move |hovered: &bool, _window, cx| {
                state_hover.update(cx, |s, cx| {
                    s.is_hovering = *hovered && motion_enabled;
                    if !*hovered {
                        s.mouse_offset = Point::default();
                        s.pointer_origin = None;
                    }
                    cx.notify();
                });
            })
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
    fn invalid_motion_values_keep_safe_defaults() {
        let mut cx = TestAppContext::single();
        let state = cx.new(MagneticButtonState::new);
        let button = MagneticButton::new("magnetic", state)
            .strength(f32::NAN)
            .range(px(-20.0));

        assert_eq!(button.strength, 0.3);
        assert_eq!(button.range, px(100.0));
    }
}
