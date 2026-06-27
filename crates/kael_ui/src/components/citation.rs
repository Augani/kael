//! Citation component - inline source attribution.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Debug, Default)]
pub struct CitationSource {
    pub title: Option<SharedString>,
    pub url: Option<SharedString>,
    pub icon: Option<SharedString>,
}

impl CitationSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn url(mut self, url: impl Into<SharedString>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CitationVariant {
    #[default]
    Label,
    Number,
}

pub type CitationProps = Citation;

#[derive(IntoElement)]
pub struct Citation {
    title: SharedString,
    source: Option<SharedString>,
    source_data: Option<CitationSource>,
    number: usize,
    variant: CitationVariant,
    description: Option<SharedString>,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Citation {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            source: None,
            source_data: None,
            number: 1,
            variant: CitationVariant::Label,
            description: None,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn source(mut self, source: impl Into<SharedString>) -> Self {
        self.source = Some(source.into());
        self
    }

    pub fn source_data(mut self, source: CitationSource) -> Self {
        self.source_data = Some(source);
        self
    }

    pub fn citation_source(self, source: CitationSource) -> Self {
        self.source_data(source)
    }

    pub fn number(mut self, number: usize) -> Self {
        self.number = number.max(1);
        self
    }

    pub fn variant(mut self, variant: CitationVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }
}

impl Styled for Citation {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Citation {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let source_title = self
            .source_data
            .as_ref()
            .and_then(|source| source.title.clone())
            .or_else(|| self.source.clone())
            .unwrap_or_else(|| self.title.clone());
        let icon = self
            .source_data
            .as_ref()
            .and_then(|source| source.icon.clone());
        let has_url = self
            .source_data
            .as_ref()
            .and_then(|source| source.url.as_ref())
            .is_some();
        let dark = theme.tokens.background.l < 0.5;
        let accessible_label = format!("Citation {}: {}", self.number, source_title);
        let on_click = self.on_click;
        let description = self.description;

        match self.variant {
            CitationVariant::Number => div()
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .min_w(px(20.0))
                .h(px(20.0))
                .px(px(4.0))
                .rounded_full()
                .bg(theme.tokens.primary.opacity(0.12))
                .text_color(theme.tokens.primary)
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .line_height(px(20.0))
                .cursor(if has_url || on_click.is_some() {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::Arrow
                })
                .when(has_url || on_click.is_some(), |this| {
                    this.hover(|style| style.bg(crate::astryx::overlay_hover(dark)))
                })
                .when_some(on_click, |this, handler| {
                    this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        handler(window, cx);
                    })
                })
                .child(self.number.to_string())
                .when_some(description, |this, description| {
                    this.child(
                        div()
                            .size(px(1.0))
                            .overflow_hidden()
                            .opacity(0.0)
                            .child(description),
                    )
                })
                .child(
                    div()
                        .size(px(1.0))
                        .overflow_hidden()
                        .opacity(0.0)
                        .child(accessible_label),
                )
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                })
                .into_any_element(),
            CitationVariant::Label => div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap(px(4.0))
                .h(px(20.0))
                .max_w(px(240.0))
                .px(px(8.0))
                .rounded(theme.tokens.radius_md)
                .border_1()
                .border_color(theme.tokens.border)
                .bg(transparent_black())
                .text_color(theme.tokens.muted_foreground)
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .line_height(px(20.0))
                .overflow_hidden()
                .cursor(if has_url || on_click.is_some() {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::Arrow
                })
                .when(has_url || on_click.is_some(), |this| {
                    this.hover(|style| {
                        style
                            .bg(crate::astryx::overlay_hover(dark))
                            .text_color(theme.tokens.foreground)
                    })
                })
                .when_some(on_click, |this, handler| {
                    this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        handler(window, cx);
                    })
                })
                .when_some(icon, |this, icon| {
                    this.child(
                        div()
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(theme.tokens.background)
                            .border_1()
                            .border_color(theme.tokens.border)
                            .overflow_hidden()
                            .child(img(icon).size(px(12.0)).object_fit(ObjectFit::Cover)),
                    )
                })
                .child(
                    div()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(source_title),
                )
                .when_some(description, |this, description| {
                    this.child(
                        div()
                            .size(px(1.0))
                            .overflow_hidden()
                            .opacity(0.0)
                            .child(description),
                    )
                })
                .child(
                    div()
                        .size(px(1.0))
                        .overflow_hidden()
                        .opacity(0.0)
                        .child(accessible_label),
                )
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                })
                .into_any_element(),
        }
    }
}
