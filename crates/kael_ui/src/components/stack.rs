//! Stack component - unified horizontal/vertical flex layout.

use crate::layout::{Align, Justify};
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StackDirection {
    Horizontal,
    #[default]
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StackItemSize {
    #[default]
    Static,
    Fill,
}

#[derive(IntoElement)]
pub struct StackItem {
    size: StackItemSize,
    align_self: Option<Align>,
    child: AnyElement,
    style: StyleRefinement,
}

impl StackItem {
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            size: StackItemSize::Static,
            align_self: None,
            child: child.into_any_element(),
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: StackItemSize) -> Self {
        self.size = size;
        self
    }

    pub fn fill(self) -> Self {
        self.size(StackItemSize::Fill)
    }

    pub fn align_self(mut self, align: Align) -> Self {
        self.align_self = Some(align);
        self
    }
}

impl Styled for StackItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StackItem {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .when(self.size == StackItemSize::Fill, |this| this.flex_1())
            .when(self.size == StackItemSize::Static, |this| {
                this.flex_shrink_0()
            })
            .map(|this| apply_align_self(this, self.align_self))
            .child(self.child)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct Stack {
    direction: StackDirection,
    align: Option<Align>,
    justify: Option<Justify>,
    gap: Option<Pixels>,
    wrap: bool,
    width: Option<Pixels>,
    height: Option<Pixels>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Stack {
    pub fn new() -> Self {
        Self {
            direction: StackDirection::Vertical,
            align: None,
            justify: None,
            gap: None,
            wrap: false,
            width: None,
            height: None,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn direction(mut self, direction: StackDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn horizontal(self) -> Self {
        self.direction(StackDirection::Horizontal)
    }

    pub fn vertical(self) -> Self {
        self.direction(StackDirection::Vertical)
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = Some(justify);
        self
    }

    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        self.gap = Some(gap.into());
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn height(mut self, height: impl Into<Pixels>) -> Self {
        self.height = Some(height.into());
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl Default for Stack {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Stack {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Stack {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group))
            .flex()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .when(self.direction == StackDirection::Horizontal, |this| {
                this.flex_row()
            })
            .when(self.direction == StackDirection::Vertical, |this| {
                this.flex_col()
            })
            .when(self.wrap, |this| this.flex_wrap())
            .when_some(self.gap, |this, gap| this.gap(gap))
            .when_some(self.width, |this, width| this.w(width))
            .when_some(self.height, |this, height| this.h(height))
            .map(|this| apply_align(this, self.align))
            .map(|this| apply_justify(this, self.justify))
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}

fn apply_align(div: Div, align: Option<Align>) -> Div {
    match align {
        Some(Align::Start) => div.items_start(),
        Some(Align::Center) => div.items_center(),
        Some(Align::End) => div.items_end(),
        Some(Align::Stretch) => div.items_stretch(),
        None => div,
    }
}

fn apply_align_self(div: Div, align: Option<Align>) -> Div {
    match align {
        Some(Align::Start) => div.self_start(),
        Some(Align::Center) => div.self_center(),
        Some(Align::End) => div.self_end(),
        Some(Align::Stretch) => div.self_stretch(),
        None => div,
    }
}

fn apply_justify(div: Div, justify: Option<Justify>) -> Div {
    match justify {
        Some(Justify::Start) => div.justify_start(),
        Some(Justify::Center) => div.justify_center(),
        Some(Justify::End) => div.justify_end(),
        Some(Justify::Between) => div.justify_between(),
        Some(Justify::Around) => div.justify_around(),
        Some(Justify::Evenly) => div.justify_evenly(),
        None => div,
    }
}
