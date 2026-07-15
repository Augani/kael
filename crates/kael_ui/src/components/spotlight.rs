use kael::{prelude::FluentBuilder as _, *};

pub struct SpotlightState {
    mouse_pos: Option<Point<Pixels>>,
    bounds: Option<Bounds<Pixels>>,
}

impl Default for SpotlightState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpotlightState {
    pub fn new() -> Self {
        Self {
            mouse_pos: None,
            bounds: None,
        }
    }
}

#[derive(IntoElement)]
pub struct Spotlight {
    id: ElementId,
    state: Entity<SpotlightState>,
    color: Option<Hsla>,
    spot_size: Pixels,
    intensity: f32,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Spotlight {
    pub fn new(id: impl Into<ElementId>, state: Entity<SpotlightState>) -> Self {
        Self {
            id: id.into(),
            state,
            color: None,
            spot_size: px(300.0),
            intensity: 0.15,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        if f32::from(size).is_finite() && size > px(0.0) {
            self.spot_size = size;
        }
        self
    }

    pub fn intensity(mut self, intensity: f32) -> Self {
        if intensity.is_finite() {
            self.intensity = intensity.clamp(0.0, 1.0);
        }
        self
    }
}

impl Styled for Spotlight {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Spotlight {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Spotlight {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let spot_color = self.color.unwrap_or(hsla(0.6, 0.8, 0.7, 1.0));
        let spot_size = self.spot_size;
        let intensity = self.intensity;
        let mouse_pos = self.state.read(cx).mouse_pos;
        let half_size = px(f32::from(spot_size) / 2.0);
        let user_style = self.style;

        let state_for_move = self.state.clone();
        let state_for_bounds = self.state.clone();
        let state_for_hover = self.state.clone();
        let motion_enabled = !cx.reduce_motion();

        let mut container = div()
            .id(self.id)
            .relative()
            .overflow_hidden()
            .on_mouse_move(move |event: &MouseMoveEvent, _, cx| {
                if !motion_enabled {
                    return;
                }
                state_for_move.update(cx, |s, cx| {
                    if let Some(bounds) = s.bounds {
                        s.mouse_pos = Some(point(
                            event.position.x - bounds.origin.x,
                            event.position.y - bounds.origin.y,
                        ));
                        cx.notify();
                    }
                });
            })
            .on_hover(move |hovered: &bool, _, cx| {
                if !*hovered {
                    state_for_hover.update(cx, |state, cx| {
                        state.mouse_pos = None;
                        cx.notify();
                    });
                }
            })
            .map(|mut el| {
                el.style().refine(&user_style);
                el
            })
            .child(
                canvas_with_prepaint(
                    move |bounds, _, cx| {
                        state_for_bounds.update(cx, |state, _| state.bounds = Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full(),
            );

        for child in self.children {
            container = container.child(child);
        }

        if let Some(pos) = mouse_pos {
            let glow_color = Hsla {
                a: intensity,
                ..spot_color
            };

            let glow = div()
                .absolute()
                .left(pos.x - half_size)
                .top(pos.y - half_size)
                .w(spot_size)
                .h(spot_size)
                .rounded(spot_size)
                .bg(glow_color);

            container = container.child(glow);
        }

        container
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_spot_geometry_keeps_safe_defaults() {
        let mut cx = TestAppContext::single();
        let state = cx.new(|_| SpotlightState::new());
        let spotlight = Spotlight::new("spot", state)
            .size(px(f32::NAN))
            .intensity(f32::NAN);

        assert_eq!(spotlight.spot_size, px(300.0));
        assert_eq!(spotlight.intensity, 0.15);
    }
}
