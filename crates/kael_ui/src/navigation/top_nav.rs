//! TopNav component - ASTRYX-style horizontal navigation bar.

use crate::{
    navigation::{nav_icon::NavIcon, nav_item::NavItem},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

pub type TopNavItem = NavItem;
pub type TopNavMenu = crate::navigation::nav_menu::NavMenu;

#[derive(IntoElement)]
pub struct TopNavHeading {
    heading: SharedString,
    superheading: Option<SharedString>,
    logo: Option<SharedString>,
    style: StyleRefinement,
}

impl TopNavHeading {
    pub fn new(heading: impl Into<SharedString>) -> Self {
        let heading = heading.into();
        Self {
            heading: if heading.trim().is_empty() {
                "Navigation".into()
            } else {
                heading
            },
            superheading: None,
            logo: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn superheading(mut self, superheading: impl Into<SharedString>) -> Self {
        let superheading = superheading.into();
        self.superheading = (!superheading.trim().is_empty()).then_some(superheading);
        self
    }

    pub fn logo(mut self, logo: impl Into<SharedString>) -> Self {
        let logo = logo.into();
        self.logo = (!logo.trim().is_empty()).then_some(logo);
        self
    }
}

impl Styled for TopNavHeading {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TopNavHeading {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Primary navigation"),
            )
            .tab_group()
            .flex()
            .items_center()
            .gap(px(8.0))
            .min_w(px(0.0))
            .when_some(self.logo, |this, logo| this.child(NavIcon::new(logo)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w(px(0.0))
                    .when_some(self.superheading, |this, superheading| {
                        this.child(
                            div()
                                .text_size(px(11.0))
                                .line_height(px(14.0))
                                .text_color(theme.tokens.muted_foreground)
                                .child(superheading),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(15.0))
                            .line_height(px(20.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .child(self.heading),
                    ),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct TopNav {
    brand: Option<SharedString>,
    leading_icon: Option<SharedString>,
    items: Vec<NavItem>,
    trailing: Vec<AnyElement>,
    style: StyleRefinement,
}

impl TopNav {
    pub fn new() -> Self {
        Self {
            brand: None,
            leading_icon: None,
            items: Vec::new(),
            trailing: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn brand(mut self, brand: impl Into<SharedString>) -> Self {
        let brand = brand.into();
        self.brand = (!brand.trim().is_empty()).then_some(brand);
        self
    }

    pub fn leading_icon(mut self, icon: impl Into<SharedString>) -> Self {
        let icon = icon.into();
        self.leading_icon = (!icon.trim().is_empty()).then_some(icon);
        self
    }

    pub fn item(mut self, item: NavItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn trailing(mut self, child: impl IntoElement) -> Self {
        self.trailing.push(child.into_any_element());
        self
    }
}

impl Default for TopNav {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for TopNav {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TopNav {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .h(px(48.0))
            .p(px(8.0))
            .bg(transparent_black())
            .when(
                self.brand.is_some() || self.leading_icon.is_some(),
                |this| {
                    this.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .min_w(px(0.0))
                            .when_some(self.leading_icon, |this, icon| {
                                this.child(NavIcon::new(icon))
                            })
                            .when_some(self.brand, |this, brand| {
                                this.child(
                                    div()
                                        .text_size(px(15.0))
                                        .line_height(px(20.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.tokens.foreground)
                                        .child(brand),
                                )
                            }),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .flex_1()
                    .children(self.items),
            )
            .children(self.trailing)
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
    fn empty_heading_and_optional_brand_content_are_normalized() {
        let heading = TopNavHeading::new(" ").superheading("").logo(" ");
        assert_eq!(heading.heading.as_ref(), "Navigation");
        assert!(heading.superheading.is_none());
        assert!(heading.logo.is_none());

        let nav = TopNav::new().brand(" ").leading_icon("");
        assert!(nav.brand.is_none());
        assert!(nav.leading_icon.is_none());
    }
}
