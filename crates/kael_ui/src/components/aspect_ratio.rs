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
            ratio,
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
