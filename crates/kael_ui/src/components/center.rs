//! Center component - flexbox centering helper.

use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CenterAxis {
    #[default]
    Both,
    Horizontal,
    Vertical,
}

#[derive(IntoElement)]
pub struct Center {
    axis: CenterAxis,
    inline: bool,
    width: Option<Pixels>,
    height: Option<Pixels>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Center {
    pub fn new() -> Self {
        Self {
            axis: CenterAxis::Both,
            inline: false,
            width: None,
            height: None,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn axis(mut self, axis: CenterAxis) -> Self {
        self.axis = axis;
        self
    }

    pub fn inline(mut self, inline: bool) -> Self {
        self.inline = inline;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for Center {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Center {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Center {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .when(!self.inline, |this| this.w_full())
            .when(
                self.axis == CenterAxis::Both || self.axis == CenterAxis::Vertical,
                |this| this.items_center(),
            )
            .when(
                self.axis == CenterAxis::Both || self.axis == CenterAxis::Horizontal,
                |this| this.justify_center(),
            )
            .when_some(self.width, |this, width| this.w(width))
            .when_some(self.height, |this, height| this.h(height))
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}
