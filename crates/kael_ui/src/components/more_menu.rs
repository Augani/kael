//! MoreMenu component - compact overflow menu trigger.

use crate::{
    components::icon_button::IconButton,
    navigation::menu::{Menu, MenuItem},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct MoreMenu {
    items: Vec<MenuItem>,
    label: SharedString,
    disabled: bool,
    style: StyleRefinement,
}

impl MoreMenu {
    pub fn new(items: Vec<MenuItem>) -> Self {
        Self {
            items,
            label: "More actions".into(),
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl Styled for MoreMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MoreMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .items_start()
            .gap(px(8.0))
            .child(
                IconButton::new("ellipsis")
                    .size(px(32.0))
                    .icon_size(px(16.0))
                    .disabled(self.disabled),
            )
            .child(
                div()
                    .min_w(px(180.0))
                    .rounded(theme.tokens.radius_lg)
                    .child(Menu::new(self.items)),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
