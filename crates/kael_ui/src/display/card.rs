//! Card - Content container with header, body, and footer sections.

use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};

pub struct Card {
    id: Option<ElementId>,
    header: Option<AnyElement>,
    content: Option<AnyElement>,
    footer: Option<AnyElement>,
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
            header: None,
            content: None,
            footer: None,
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

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
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

        let shadow_sm = theme.tokens.shadow_sm.clone();
        let shadow_md = theme.tokens.shadow_md.clone();
        let transition_base = theme.tokens.transition_base;

        let mut base = div()
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_lg)
            .shadow(shadow_sm.to_vec())
            .overflow_hidden();

        if let Some(header) = self.header {
            base = base.child(
                div()
                    .px(px(24.0))
                    .py(px(16.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(header),
            );
        }

        if let Some(content) = self.content {
            base = base.child(div().px(px(24.0)).py(px(16.0)).child(content));
        }

        if let Some(footer) = self.footer {
            base = base.child(
                div()
                    .px(px(24.0))
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
