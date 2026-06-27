//! FormLayout component - spatial layout container for fields.

use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FormLayoutDirection {
    #[default]
    Vertical,
    Horizontal,
    HorizontalLabels,
}

#[derive(IntoElement)]
pub struct FormLayout {
    direction: FormLayoutDirection,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl FormLayout {
    pub fn new() -> Self {
        Self {
            direction: FormLayoutDirection::Vertical,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn direction(mut self, direction: FormLayoutDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for FormLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for FormLayout {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FormLayout {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let child_count = self.children.len().max(1);

        div()
            .when(self.direction == FormLayoutDirection::Vertical, |this| {
                this.flex().flex_col().gap(px(16.0))
            })
            .when(self.direction == FormLayoutDirection::Horizontal, |this| {
                this.grid().grid_cols(child_count as u16).gap(px(16.0))
            })
            .when(
                self.direction == FormLayoutDirection::HorizontalLabels,
                |this| this.flex().flex_col().gap(px(12.0)),
            )
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}
