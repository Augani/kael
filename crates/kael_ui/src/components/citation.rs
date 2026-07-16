//! Citation component - inline source attribution.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;

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
    id: ElementId,
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
    #[track_caller]
    pub fn new(title: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "citation:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
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
        let theme = Theme::of(cx).clone();
        let user_style = self.style;
        let source_title = self
            .source_data
            .as_ref()
            .and_then(|source| source.title.clone())
            .or_else(|| self.source.clone())
            .filter(|title| !title.trim().is_empty())
            .or_else(|| (!self.title.trim().is_empty()).then(|| self.title.clone()))
            .unwrap_or_else(|| "Source".into());
        let icon = self
            .source_data
            .as_ref()
            .and_then(|source| source.icon.clone());
        let source_url = self
            .source_data
            .as_ref()
            .and_then(|source| source.url.clone())
            .filter(|url| !url.trim().is_empty());
        let has_url = source_url.is_some();
        let dark = theme.tokens.background.l < 0.5;
        let accessible_label = format!("Citation {}: {}", self.number, source_title);
        let on_click = self.on_click;
        let description = self.description.filter(|text| !text.trim().is_empty());
        let variant = self.variant;
        let number = self.number;
        let has_action = has_url || on_click.is_some();

        button(self.id)
            .role(if has_url {
                AccessibilityRole::Link
            } else {
                AccessibilityRole::Button
            })
            .label(accessible_label)
            .when_some(description, |this, description| {
                this.description(description)
            })
            .when(!has_action, |this| this.disabled())
            .when(has_action, |this| {
                this.on_click(move |_, window, cx| {
                    if let Some(url) = source_url.as_ref() {
                        let _ = cx.open_url(url.as_ref());
                    }
                    if let Some(handler) = on_click.as_ref() {
                        handler(window, cx);
                    }
                })
            })
            .render_with(move |state, _, _| {
                let content = match variant {
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
                        .child(StyledText::new(number.to_string()).accessibility_hidden(true)),
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
                        .when_some(icon.clone(), |this, icon| {
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
                                .child(
                                    StyledText::new(source_title.clone())
                                        .accessibility_hidden(true),
                                ),
                        ),
                };

                content
                    .cursor(if state.disabled {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .when(!state.disabled, |this| {
                        this.hover(|style| {
                            style
                                .bg(crate::astryx::overlay_hover(dark))
                                .text_color(theme.tokens.foreground)
                        })
                    })
                    .when(state.focused && !state.disabled, |this| {
                        this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                            theme.tokens.ring
                        )])
                    })
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div.into_any_element()
                    })
            })
    }
}
