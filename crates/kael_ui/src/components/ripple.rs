use kael::*;
use std::time::Duration;

use crate::animations::easings;

#[derive(IntoElement)]
pub struct Ripple {
    id: ElementId,
    origin: Point<Pixels>,
    color: Hsla,
    duration: Duration,
    max_size: Pixels,
}

impl Ripple {
    pub fn new(id: impl Into<ElementId>, origin: Point<Pixels>, color: Hsla) -> Self {
        let origin = if f32::from(origin.x).is_finite() && f32::from(origin.y).is_finite() {
            origin
        } else {
            Point::default()
        };
        Self {
            id: id.into(),
            origin,
            color,
            duration: Duration::from_millis(400),
            max_size: px(150.0),
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn max_size(mut self, size: Pixels) -> Self {
        if f32::from(size).is_finite() && size > px(0.0) {
            self.max_size = size;
        }
        self
    }
}

impl RenderOnce for Ripple {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let origin = self.origin;
        let max_size = self.max_size;
        let color = self.color;
        let ripple_id = self.id;
        let animation_id = ElementId::NamedChild(Box::new(ripple_id.clone()), "expand".into());

        div()
            .absolute()
            .overflow_hidden()
            .size_full()
            .top_0()
            .left_0()
            .child(
                div()
                    .id(ripple_id)
                    .absolute()
                    .rounded_full()
                    .bg(color.opacity(0.2))
                    .with_animation(
                        animation_id,
                        Animation::new(self.duration).with_easing(easings::ease_out_cubic),
                        move |el, delta| {
                            let size = max_size * delta;
                            el.size(size)
                                .left(origin.x - size / 2.0)
                                .top(origin.y - size / 2.0)
                                .opacity((1.0 - delta) * 0.6)
                        },
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_geometry_keeps_the_ripple_bounded() {
        let ripple = Ripple::new("ripple", point(px(f32::NAN), px(f32::INFINITY)), white())
            .max_size(px(f32::INFINITY))
            .max_size(px(-20.0));

        assert_eq!(ripple.origin, Point::default());
        assert_eq!(ripple.max_size, px(150.0));
    }
}
