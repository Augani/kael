//! Link component - ASTRYX-style inline or block link surface.

use crate::{
    components::icon::{Icon, IconSize},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LinkVariant {
    #[default]
    Inline,
    Subtle,
    Block,
}

#[derive(IntoElement)]
pub struct Link {
    id: ElementId,
    label: SharedString,
    variant: LinkVariant,
    disabled: bool,
    external: bool,
    underline: bool,
    href: Option<SharedString>,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Link {
    #[track_caller]
    pub fn new(label: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "link:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: label.into(),
            variant: LinkVariant::Inline,
            disabled: false,
            external: false,
            underline: false,
            href: None,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: LinkVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn external(mut self, external: bool) -> Self {
        self.external = external;
        self
    }

    pub fn underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    /// Open a URL when the link is activated.
    pub fn href(mut self, href: impl Into<SharedString>) -> Self {
        let href = href.into();
        self.href = (!href.trim().is_empty()).then_some(href);
        self
    }

    #[allow(non_snake_case)]
    pub fn hasUnderline(self, underline: bool) -> Self {
        self.underline(underline)
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl Styled for Link {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Link {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let label: SharedString = if self.label.trim().is_empty() {
            "Link".into()
        } else {
            self.label
        };
        let href = self.href;
        let on_click = self.on_click;
        let disabled = self.disabled || (href.is_none() && on_click.is_none());
        let user_style = self.style;
        let variant = self.variant;
        let underline = self.underline;
        let external = self.external;

        button(self.id)
            .role(AccessibilityRole::Link)
            .label(label.clone())
            .when(external, |this| {
                this.description("Opens in an external browser")
            })
            .when(disabled, |this| this.disabled())
            .when(!disabled, |this| {
                this.on_click(move |_, window, cx| {
                    if let Some(href) = href.as_ref() {
                        let _ = cx.open_url(href.as_ref());
                    }
                    if let Some(handler) = on_click.as_ref() {
                        handler(window, cx);
                    }
                })
            })
            .render_with(move |state, _, _| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .font_family(theme.tokens.font_family.clone())
                    .text_size(px(14.0))
                    .line_height(px(20.0))
                    .text_color(if state.disabled {
                        theme.tokens.muted_foreground
                    } else {
                        match variant {
                            LinkVariant::Inline => theme.tokens.primary,
                            LinkVariant::Subtle | LinkVariant::Block => theme.tokens.foreground,
                        }
                    })
                    .when(underline, |this| this.underline())
                    .when(variant == LinkVariant::Block, |this| {
                        this.w_full()
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(theme.tokens.radius_md)
                    })
                    .when(state.focused && !state.disabled, |this| {
                        this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                            theme.tokens.ring
                        )])
                    })
                    .when(!state.disabled, |this| {
                        this.cursor_pointer()
                            .hover(move |style| style.text_color(theme.tokens.primary).underline())
                    })
                    .child(StyledText::new(label.clone()).accessibility_hidden(true))
                    .when(external, |this| {
                        this.child(
                            Icon::new("external-link")
                                .size(IconSize::Custom(px(12.0)))
                                .color(if state.disabled {
                                    theme.tokens.muted_foreground
                                } else {
                                    theme.tokens.primary
                                }),
                        )
                    })
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div.into_any_element()
                    })
            })
    }
}
