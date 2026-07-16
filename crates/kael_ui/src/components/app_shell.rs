//! AppShell component - ASTRYX-style application frame.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AppShellBreakpoint {
    Sm,
    Md,
    Lg,
    #[default]
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AppShellVariant {
    Wash,
    Surface,
    Section,
    #[default]
    Elevated,
}

#[derive(IntoElement)]
pub struct AppShell {
    top: Option<AnyElement>,
    side: Option<AnyElement>,
    content: Option<AnyElement>,
    footer: Option<AnyElement>,
    sidebar_width: Pixels,
    variant: AppShellVariant,
    breakpoint: AppShellBreakpoint,
    style: StyleRefinement,
}

impl AppShell {
    pub fn new() -> Self {
        Self {
            top: None,
            side: None,
            content: None,
            footer: None,
            sidebar_width: px(260.0),
            variant: AppShellVariant::Elevated,
            breakpoint: AppShellBreakpoint::None,
            style: StyleRefinement::default(),
        }
    }

    pub fn top(mut self, top: impl IntoElement) -> Self {
        self.top = Some(top.into_any_element());
        self
    }

    pub fn side(mut self, side: impl IntoElement) -> Self {
        self.side = Some(side.into_any_element());
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

    pub fn sidebar_width(mut self, width: impl Into<Pixels>) -> Self {
        self.sidebar_width = width.into();
        self
    }

    pub fn variant(mut self, variant: AppShellVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn breakpoint(mut self, breakpoint: AppShellBreakpoint) -> Self {
        self.breakpoint = breakpoint;
        self
    }
}

impl Default for AppShell {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for AppShell {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AppShell {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let variant = self.variant;
        let breakpoint = self.breakpoint;

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.0))
            .bg(match variant {
                AppShellVariant::Surface => theme.tokens.card,
                _ => theme.tokens.background,
            })
            .text_color(theme.tokens.foreground)
            .when_some(self.top, |this, top| {
                this.child(div().flex_shrink_0().child(top))
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .when(breakpoint != AppShellBreakpoint::None, |this| {
                        this.min_w(px(0.0))
                    })
                    .when_some(self.side, |this, side| {
                        this.child(
                            div()
                                .w(self.sidebar_width)
                                .h_full()
                                .min_h(px(0.0))
                                .flex_shrink_0()
                                .overflow_hidden()
                                .when(variant == AppShellVariant::Section, |this| {
                                    this.border_r_1().border_color(theme.tokens.border)
                                })
                                .child(side),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .min_h(px(0.0))
                            .overflow_hidden()
                            .when(variant == AppShellVariant::Elevated, |this| {
                                this.m(px(8.0))
                                    .bg(theme.tokens.card)
                                    .rounded(theme.tokens.radius_lg)
                            })
                            .child(self.content.unwrap_or_else(|| div().into_any_element())),
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
