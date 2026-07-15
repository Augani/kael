//! NavMenu component - grouped navigation items.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

pub type NavHeadingMenu = NavMenu;
pub type NavHeadingMenuItem = crate::navigation::nav_item::NavItem;
pub type NavMenuItem = crate::navigation::nav_item::NavItem;

#[derive(IntoElement)]
pub struct NavMenu {
    label: Option<SharedString>,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl NavMenu {
    pub fn new() -> Self {
        Self {
            label: None,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.label = (!label.trim().is_empty()).then_some(label);
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Default for NavMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for NavMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NavMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label(
                    self.label
                        .as_ref()
                        .map_or("Navigation menu", SharedString::as_ref),
                ),
            )
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when_some(self.label, |this, label| {
                this.child(
                    div()
                        .px(px(8.0))
                        .pb(px(2.0))
                        .text_size(px(11.0))
                        .line_height(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.tokens.muted_foreground)
                        .child(StyledText::new(label).accessibility_hidden(true)),
                )
            })
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn empty_group_labels_fall_back_to_the_default() {
        assert!(NavMenu::new().label(" ").label.is_none());
    }
}
