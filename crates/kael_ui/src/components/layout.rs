//! Layout component - ASTRYX-style page layout frame.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct LayoutHeader {
    child: AnyElement,
    has_divider: bool,
    style: StyleRefinement,
}

impl LayoutHeader {
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
            has_divider: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn has_divider(mut self, has_divider: bool) -> Self {
        self.has_divider = has_divider;
        self
    }
}

impl Styled for LayoutHeader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LayoutHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex_shrink_0()
            .px(px(16.0))
            .py(px(12.0))
            .when(self.has_divider, |this| {
                this.border_b_1().border_color(theme.tokens.border)
            })
            .child(self.child)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct LayoutContent {
    child: AnyElement,
    padding: Pixels,
    scrollable: bool,
    style: StyleRefinement,
}

impl LayoutContent {
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
            padding: px(16.0),
            scrollable: true,
            style: StyleRefinement::default(),
        }
    }

    pub fn padding(mut self, padding: impl Into<Pixels>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }
}

impl Styled for LayoutContent {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LayoutContent {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;

        div()
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .p(self.padding)
            .when(!self.scrollable, |this| this.overflow_hidden())
            .child(self.child)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct LayoutFooter {
    child: AnyElement,
    has_divider: bool,
    style: StyleRefinement,
}

impl LayoutFooter {
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
            has_divider: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn has_divider(mut self, has_divider: bool) -> Self {
        self.has_divider = has_divider;
        self
    }
}

impl Styled for LayoutFooter {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LayoutFooter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex_shrink_0()
            .px(px(16.0))
            .py(px(12.0))
            .when(self.has_divider, |this| {
                this.border_t_1().border_color(theme.tokens.border)
            })
            .child(self.child)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct LayoutPanel {
    child: AnyElement,
    width: Pixels,
    has_divider: bool,
    style: StyleRefinement,
}

impl LayoutPanel {
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            child: child.into_any_element(),
            width: px(240.0),
            has_divider: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = width.into();
        self
    }

    pub fn has_divider(mut self, has_divider: bool) -> Self {
        self.has_divider = has_divider;
        self
    }
}

impl Styled for LayoutPanel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LayoutPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .w(self.width)
            .h_full()
            .flex_shrink_0()
            .when(self.has_divider, |this| {
                this.border_r_1().border_color(theme.tokens.border)
            })
            .child(self.child)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct Layout {
    header: Option<AnyElement>,
    panel: Option<AnyElement>,
    content: Option<AnyElement>,
    footer: Option<AnyElement>,
    content_width: Option<Pixels>,
    gap: Pixels,
    style: StyleRefinement,
}

impl Layout {
    pub fn new() -> Self {
        Self {
            header: None,
            panel: None,
            content: None,
            footer: None,
            content_width: None,
            gap: px(0.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    pub fn panel(mut self, panel: impl IntoElement) -> Self {
        self.panel = Some(panel.into_any_element());
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    pub fn content_width(mut self, width: impl Into<Pixels>) -> Self {
        self.content_width = Some(width.into());
        self
    }

    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        self.gap = gap.into();
        self
    }
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Layout {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Layout {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.0))
            .bg(theme.tokens.background)
            .gap(self.gap)
            .when_some(self.header, |this, header| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .border_b_1()
                        .border_color(theme.tokens.border)
                        .child(header),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .when_some(self.panel, |this, panel| {
                        this.child(
                            div()
                                .h_full()
                                .border_r_1()
                                .border_color(theme.tokens.border)
                                .child(panel),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .overflow_hidden()
                            .child(
                                div()
                                    .size_full()
                                    .when_some(self.content_width, |this, width| {
                                        this.max_w(width).mx_auto()
                                    })
                                    .child(
                                        self.content.unwrap_or_else(|| div().into_any_element()),
                                    ),
                            ),
                    ),
            )
            .when_some(self.footer, |this, footer| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .border_t_1()
                        .border_color(theme.tokens.border)
                        .child(footer),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
