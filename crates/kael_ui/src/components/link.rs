//! Link component - ASTRYX-style inline or block link surface.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LinkVariant {
    #[default]
    Inline,
    Subtle,
    Block,
}

#[derive(IntoElement)]
pub struct Link {
    label: SharedString,
    variant: LinkVariant,
    disabled: bool,
    external: bool,
    underline: bool,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Link {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            variant: LinkVariant::Inline,
            disabled: false,
            external: false,
            underline: false,
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
        let theme = Theme::of(cx);
        let label = self.label;
        let disabled = self.disabled;
        let user_style = self.style;

        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .font_family(theme.tokens.font_family.clone())
            .text_size(px(14.0))
            .line_height(px(20.0))
            .text_color(if disabled {
                theme.tokens.muted_foreground
            } else {
                match self.variant {
                    LinkVariant::Inline => theme.tokens.primary,
                    LinkVariant::Subtle => theme.tokens.foreground,
                    LinkVariant::Block => theme.tokens.foreground,
                }
            })
            .when(self.underline, |this| this.underline())
            .when(self.variant == LinkVariant::Block, |this| {
                this.flex()
                    .w_full()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(theme.tokens.radius_md)
            })
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(move |style| style.text_color(theme.tokens.primary).underline())
            })
            .when_some(self.on_click.filter(|_| !disabled), |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                    handler(window, cx);
                })
            })
            .child(label)
            .when(self.external, |this| {
                this.child(div().text_size(px(12.0)).child("↗"))
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
