//! AspectRatio component - maintains a fixed width/height ratio.

use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AspectRatioShape {
    #[default]
    Rectangle,
    Ellipse,
}

#[derive(IntoElement)]
pub struct AspectRatio {
    ratio: f32,
    shape: AspectRatioShape,
    child: AnyElement,
    style: StyleRefinement,
}

impl AspectRatio {
    pub fn new(ratio: f32, child: impl IntoElement) -> Self {
        Self {
            ratio: if ratio.is_finite() && ratio > 0.0 {
                ratio
            } else {
                1.0
            },
            shape: AspectRatioShape::Rectangle,
            child: child.into_any_element(),
            style: StyleRefinement::default(),
        }
    }

    pub fn shape(mut self, shape: AspectRatioShape) -> Self {
        self.shape = shape;
        self
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::AspectRatio;
    use kael::div;

    #[test]
    fn invalid_ratios_fall_back_to_square() {
        assert_eq!(AspectRatio::new(0.0, div()).ratio, 1.0);
        assert_eq!(AspectRatio::new(-2.0, div()).ratio, 1.0);
        assert_eq!(AspectRatio::new(f32::NAN, div()).ratio, 1.0);
        assert_eq!(AspectRatio::new(16.0 / 9.0, div()).ratio, 16.0 / 9.0);
    }
}

impl Styled for AspectRatio {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AspectRatio {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .relative()
            .w_full()
            .min_h(px(0.0))
            .flex_shrink_0()
            .aspect_ratio(self.ratio)
            .overflow_hidden()
            .when(self.shape == AspectRatioShape::Ellipse, |this| {
                this.rounded_full()
            })
            .child(div().absolute().inset_0().child(self.child))
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}
