//! Token component - compact labeled entity chips.

use crate::{
    astryx::{self, ControlSize, Hue},
    components::{icon::Icon, icon_source::IconSource},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TokenColor {
    #[default]
    Default,
    Hue(Hue),
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Cyan,
    Blue,
    Purple,
    Pink,
    Gray,
}

pub type TokenSize = ControlSize;
pub type TokenProps = Token;

#[derive(IntoElement)]
pub struct Token {
    label: SharedString,
    size: ControlSize,
    color: TokenColor,
    icon: Option<IconSource>,
    end_content: Option<AnyElement>,
    description: Option<SharedString>,
    href: Option<SharedString>,
    label_hidden: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_remove: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Token {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            size: ControlSize::Md,
            color: TokenColor::Default,
            icon: None,
            end_content: None,
            description: None,
            href: None,
            label_hidden: false,
            disabled: false,
            on_click: None,
            on_remove: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: TokenColor) -> Self {
        self.color = color;
        self
    }

    pub fn hue(mut self, hue: Hue) -> Self {
        self.color = TokenColor::Hue(hue);
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl IntoElement) -> Self {
        self.end_content(content)
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn href(mut self, href: impl Into<SharedString>) -> Self {
        self.href = Some(href.into());
        self
    }

    pub fn label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    pub fn is_label_hidden(self, hidden: bool) -> Self {
        self.label_hidden(hidden)
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn is_disabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.is_disabled(disabled)
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }

    pub fn on_remove(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onRemove(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_remove(handler)
    }
}

impl Styled for Token {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Token {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let dark = theme.tokens.background.l < 0.5;
        let (bg_color, text_color) = token_colors(self.color, dark, &theme);
        let user_style = self.style;
        let is_interactive = self.on_click.is_some();
        let has_href = self.href.is_some();
        let label = self.label.clone();
        let height = self.size.height() - px(8.0);
        let icon_size = match self.size {
            ControlSize::Sm => px(12.0),
            ControlSize::Md => px(14.0),
            ControlSize::Lg => px(16.0),
        };

        div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .h(height)
            .max_w_full()
            .px(px(8.0))
            .rounded(theme.tokens.radius_sm)
            .bg(bg_color)
            .text_color(text_color)
            .font_family(theme.tokens.font_family.clone())
            .text_size(px(12.0))
            .line_height(relative(1.35))
            .font_weight(FontWeight::MEDIUM)
            .overflow_hidden()
            .when(self.disabled, |this| this.opacity(0.5))
            .when((is_interactive || has_href) && !self.disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(bg_color.blend(astryx::overlay_hover(dark))))
            })
            .when_some(self.on_click.filter(|_| !self.disabled), |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    handler(window, cx);
                })
            })
            .when_some(self.href, |this, href| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(format!("href:{href}"))
                        .opacity(0.0),
                )
            })
            .when_some(self.icon, |this, icon| {
                this.child(
                    div()
                        .size(icon_size)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Icon::new(icon).size(icon_size).color(text_color)),
                )
            })
            .when(!self.label_hidden, |this| {
                this.child(
                    div()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(label.clone()),
                )
            })
            .when(self.label_hidden, |this| {
                this.child(
                    div()
                        .size(px(1.0))
                        .overflow_hidden()
                        .opacity(0.0)
                        .child(label),
                )
            })
            .when_some(self.description, |this, description| {
                this.child(
                    div()
                        .size(px(1.0))
                        .overflow_hidden()
                        .opacity(0.0)
                        .child(description),
                )
            })
            .children(self.end_content)
            .when_some(
                self.on_remove.filter(|_| !self.disabled),
                |this, handler| {
                    this.child(
                        div()
                            .ml(px(2.0))
                            .mr(px(-4.0))
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .hover(|style| style.bg(text_color.opacity(0.12)))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                cx.stop_propagation();
                                handler(window, cx);
                            })
                            .child(Icon::new("x").size(px(12.0)).color(text_color)),
                    )
                },
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn token_colors(color: TokenColor, dark: bool, theme: &Theme) -> (Hsla, Hsla) {
    let hue = match color {
        TokenColor::Default => return (theme.tokens.muted, theme.tokens.foreground),
        TokenColor::Hue(hue) => hue,
        TokenColor::Red => Hue::Red,
        TokenColor::Orange => Hue::Orange,
        TokenColor::Yellow => Hue::Yellow,
        TokenColor::Green => Hue::Green,
        TokenColor::Teal => Hue::Teal,
        TokenColor::Cyan => Hue::Cyan,
        TokenColor::Blue => Hue::Blue,
        TokenColor::Purple => Hue::Purple,
        TokenColor::Pink => Hue::Pink,
        TokenColor::Gray => Hue::Gray,
    };
    let colors = hue.colors(dark);
    (colors.background, colors.text)
}
