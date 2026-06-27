//! Section component - ASTRYX-style surface region.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SectionVariant {
    #[default]
    Section,
    Transparent,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionDivider {
    Top,
    Bottom,
    Start,
    End,
}

#[derive(IntoElement)]
pub struct Section {
    variant: SectionVariant,
    padding: Pixels,
    dividers: Vec<SectionDivider>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl Section {
    pub fn new() -> Self {
        Self {
            variant: SectionVariant::Section,
            padding: px(16.0),
            dividers: Vec::new(),
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: SectionVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = padding;
        self
    }

    pub fn divider(mut self, divider: SectionDivider) -> Self {
        self.dividers.push(divider);
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for Section {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Section {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Section {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let bg = match self.variant {
            SectionVariant::Section => theme.tokens.card,
            SectionVariant::Transparent => transparent_black(),
            SectionVariant::Muted => theme.tokens.muted,
        };
        let border = theme.tokens.border;
        let dividers = self.dividers.clone();

        div()
            .bg(bg)
            .p(self.padding)
            .when(dividers.contains(&SectionDivider::Top), |this| {
                this.border_t_1().border_color(border)
            })
            .when(dividers.contains(&SectionDivider::Bottom), |this| {
                this.border_b_1().border_color(border)
            })
            .when(dividers.contains(&SectionDivider::Start), |this| {
                this.border_l_1().border_color(border)
            })
            .when(dividers.contains(&SectionDivider::End), |this| {
                this.border_r_1().border_color(border)
            })
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
    }
}
