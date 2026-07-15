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
    columns: u16,
    gap: Pixels,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl FormLayout {
    pub fn new() -> Self {
        Self {
            direction: FormLayoutDirection::Vertical,
            columns: 2,
            gap: px(16.0),
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn direction(mut self, direction: FormLayoutDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the number of columns used by horizontal form layouts.
    pub fn columns(mut self, columns: u16) -> Self {
        self.columns = columns.max(1);
        self
    }

    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        self.gap = gap.into();
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
        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Form fields"),
            )
            .w_full()
            .min_w(px(0.0))
            .when(self.direction == FormLayoutDirection::Vertical, |this| {
                this.flex().flex_col().gap(self.gap)
            })
            .when(self.direction == FormLayoutDirection::Horizontal, |this| {
                this.grid().grid_cols(self.columns).gap(self.gap)
            })
            .when(
                self.direction == FormLayoutDirection::HorizontalLabels,
                |this| {
                    this.grid()
                        .grid_cols(2)
                        .gap_x(px(20.0))
                        .gap_y(self.gap)
                        .items_center()
                },
            )
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}
