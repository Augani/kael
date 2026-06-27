//! List component - grouped ASTRYX item rows.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListVariant {
    #[default]
    Plain,
    Bordered,
    Inset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListStyle {
    #[default]
    None,
    Disc,
    Circle,
    Decimal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ListDensity {
    Compact,
    #[default]
    Balanced,
    Spacious,
}

impl ListDensity {
    fn gap(self) -> Pixels {
        match self {
            Self::Compact => px(0.0),
            Self::Balanced => px(2.0),
            Self::Spacious => px(6.0),
        }
    }

    fn padding(self) -> Pixels {
        match self {
            Self::Compact => px(2.0),
            Self::Balanced => px(4.0),
            Self::Spacious => px(6.0),
        }
    }
}

#[derive(IntoElement)]
pub struct List {
    variant: ListVariant,
    density: ListDensity,
    list_style: ListStyle,
    start: usize,
    has_dividers: bool,
    header: Option<AnyElement>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl List {
    pub fn new() -> Self {
        Self {
            variant: ListVariant::Plain,
            density: ListDensity::default(),
            list_style: ListStyle::None,
            start: 1,
            has_dividers: false,
            header: None,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: ListVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn density(mut self, density: ListDensity) -> Self {
        self.density = density;
        self
    }

    pub fn list_style(mut self, list_style: ListStyle) -> Self {
        self.list_style = list_style;
        self
    }

    #[allow(non_snake_case)]
    pub fn listStyle(self, list_style: ListStyle) -> Self {
        self.list_style(list_style)
    }

    pub fn start(mut self, start: usize) -> Self {
        self.start = start.max(1);
        self
    }

    pub fn has_dividers(mut self, has_dividers: bool) -> Self {
        self.has_dividers = has_dividers;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasDividers(self, has_dividers: bool) -> Self {
        self.has_dividers(has_dividers)
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(|child| child.into_any_element()));
        self
    }

    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }
}

impl Default for List {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for List {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for List {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let list_style = self.list_style;
        let density = self.density;
        let children = self.children;
        let child_count = children.len();
        let has_dividers = self.has_dividers;
        let start = self.start;

        let list = div()
            .flex()
            .flex_col()
            .gap(if has_dividers { px(0.0) } else { density.gap() })
            .when(self.variant != ListVariant::Plain, |this| {
                this.bg(theme.tokens.card)
                    .border_1()
                    .border_color(theme.tokens.border)
                    .rounded(theme.tokens.radius_lg)
                    .p(density.padding())
            })
            .when(self.variant == ListVariant::Inset, |this| {
                this.shadow(theme.tokens.shadow_sm.to_vec())
            })
            .children(children.into_iter().enumerate().map(move |(idx, child)| {
                let row = match list_style {
                    ListStyle::None => child,
                    ListStyle::Disc | ListStyle::Circle | ListStyle::Decimal => div()
                        .flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(
                            div()
                                .w(px(16.0))
                                .flex()
                                .justify_center()
                                .text_align(TextAlign::Right)
                                .text_color(theme.tokens.foreground)
                                .when(
                                    matches!(list_style, ListStyle::Disc | ListStyle::Circle),
                                    |this| {
                                        this.pt(px(7.0)).child(
                                            div()
                                                .size(px(6.0))
                                                .rounded_full()
                                                .when(list_style == ListStyle::Disc, |this| {
                                                    this.bg(theme.tokens.foreground)
                                                })
                                                .when(list_style == ListStyle::Circle, |this| {
                                                    this.border_1()
                                                        .border_color(theme.tokens.foreground)
                                                }),
                                        )
                                    },
                                )
                                .when(list_style == ListStyle::Decimal, |this| {
                                    this.text_size(px(14.0))
                                        .line_height(px(20.0))
                                        .justify_end()
                                        .child(format!("{}.", start + idx))
                                }),
                        )
                        .child(div().flex_1().min_w(px(0.0)).child(child))
                        .into_any_element(),
                };

                div()
                    .when(has_dividers && idx + 1 < child_count, |this| {
                        this.border_b_1().border_color(theme.tokens.border)
                    })
                    .child(row)
                    .into_any_element()
            }))
            .when(list_style != ListStyle::None, |this| {
                this.gap(if has_dividers {
                    px(0.0)
                } else {
                    density.gap().max(px(4.0))
                })
            });

        div()
            .flex()
            .flex_col()
            .when_some(self.header, |this, header| {
                this.child(div().mb(px(8.0)).child(header))
            })
            .child(list)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
