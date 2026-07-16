//! SizeContext component - explicit ASTRYX control-size boundary.

use crate::astryx::ControlSize;
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct SizeContext {
    size: ControlSize,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl SizeContext {
    pub fn new(size: ControlSize) -> Self {
        Self {
            size,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn size(&self) -> ControlSize {
        self.size
    }
}

impl Styled for SizeContext {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SizeContext {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(match self.size {
                ControlSize::Sm => px(4.0),
                ControlSize::Md => px(6.0),
                ControlSize::Lg => px(8.0),
            })
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}
