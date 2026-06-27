//! NavIcon component - centered navigation icon slot.

use crate::{components::icon::Icon, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct NavIcon {
    icon: SharedString,
    selected: bool,
    style: StyleRefinement,
}

impl NavIcon {
    pub fn new(icon: impl Into<SharedString>) -> Self {
        Self {
            icon: icon.into(),
            selected: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

impl Styled for NavIcon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NavIcon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(theme.tokens.primary)
            .text_color(theme.tokens.primary_foreground)
            .child(Icon::new(self.icon).size(px(16.0)))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
