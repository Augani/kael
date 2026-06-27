//! Card - Content container with header, body, and footer sections.

use crate::{
    astryx::Hue,
    theme::{use_theme, Theme},
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CardVariant {
    #[default]
    Default,
    Transparent,
    Muted,
    Blue,
    Cyan,
    Gray,
    Green,
    Orange,
    Pink,
    Purple,
    Red,
    Teal,
    Yellow,
}

impl CardVariant {
    pub(crate) fn background(self, theme: &Theme) -> Hsla {
        let dark = theme.tokens.background.l < 0.5;
        match self {
            Self::Default => theme.tokens.card,
            Self::Transparent => transparent_black(),
            Self::Muted => theme.tokens.secondary,
            Self::Blue => Hue::Blue.colors(dark).background,
            Self::Cyan => Hue::Cyan.colors(dark).background,
            Self::Gray => Hue::Gray.colors(dark).background,
            Self::Green => Hue::Green.colors(dark).background,
            Self::Orange => Hue::Orange.colors(dark).background,
            Self::Pink => Hue::Pink.colors(dark).background,
            Self::Purple => Hue::Purple.colors(dark).background,
            Self::Red => Hue::Red.colors(dark).background,
            Self::Teal => Hue::Teal.colors(dark).background,
            Self::Yellow => Hue::Yellow.colors(dark).background,
        }
    }
}

pub struct Card {
    id: Option<ElementId>,
    children: Vec<AnyElement>,
    header: Option<AnyElement>,
    content: Option<AnyElement>,
    footer: Option<AnyElement>,
    variant: CardVariant,
    padding: Option<Pixels>,
    width: Option<Pixels>,
    height: Option<Pixels>,
    max_width: Option<Pixels>,
    min_height: Option<Pixels>,
    hoverable: bool,
    style: StyleRefinement,
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Card {
    pub fn new() -> Self {
        Self {
            id: None,
            children: Vec::new(),
            header: None,
            content: None,
            footer: None,
            variant: CardVariant::default(),
            padding: None,
            width: None,
            height: None,
            max_width: None,
            min_height: None,
            hoverable: false,
            style: StyleRefinement::default(),
        }
    }

    /// Opt into a subtle lift on hover: the card steps up one shadow level and
    /// rises a single pixel. The eased transition needs a stable [`ElementId`],
    /// so the id is supplied here. Off by default, leaving existing cards
    /// untouched.
    pub fn hoverable(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self.hoverable = true;
        self
    }

    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
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

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn max_width(mut self, max_width: Pixels) -> Self {
        self.max_width = Some(max_width);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxWidth(self, max_width: Pixels) -> Self {
        self.max_width(max_width)
    }

    pub fn min_height(mut self, min_height: Pixels) -> Self {
        self.min_height = Some(min_height);
        self
    }

    #[allow(non_snake_case)]
    pub fn minHeight(self, min_height: Pixels) -> Self {
        self.min_height(min_height)
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for Card {
    type Element = AnyElement;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let user_style = self.style;

        let shadow_md = theme.tokens.shadow_md.clone();
        let transition_base = theme.tokens.transition_base;
        let bg = self.variant.background(&theme);
        let border_color = if self.variant == CardVariant::Transparent {
            transparent_black()
        } else if self.variant == CardVariant::Default {
            theme.tokens.input
        } else {
            transparent_black()
        };

        let mut base = div()
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .rounded(theme.tokens.radius_lg)
            .overflow_hidden()
            .when_some(self.width, |this, width| this.w(width))
            .when_some(self.height, |this, height| this.h(height).overflow_hidden())
            .when_some(self.max_width, |this, max_width| this.max_w(max_width))
            .when_some(self.min_height, |this, min_height| this.min_h(min_height));

        if !self.children.is_empty() {
            base = base.child(
                div()
                    .p(self.padding.unwrap_or(px(16.0)))
                    .children(self.children),
            );
        }

        if let Some(header) = self.header {
            base = base.child(
                div()
                    .px(px(16.0))
                    .py(px(16.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(header),
            );
        }

        if let Some(content) = self.content {
            base = base.child(div().p(px(16.0)).child(content));
        }

        if let Some(footer) = self.footer {
            base = base.child(
                div()
                    .px(px(16.0))
                    .py(px(16.0))
                    .border_t_1()
                    .border_color(theme.tokens.border)
                    .child(footer),
            );
        }

        base = base.map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div
        });

        match self.id {
            Some(id) if self.hoverable => base
                .id(id)
                .transition(transition_base)
                .hover(move |style| style.shadow(shadow_md.to_vec()).translate_y(px(-1.0)))
                .into_any_element(),
            _ => base.into_any_element(),
        }
    }
}
