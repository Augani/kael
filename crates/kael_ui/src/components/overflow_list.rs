//! OverflowList component - visible item row with compact overflow counter.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct OverflowList {
    items: Vec<AnyElement>,
    max_visible: usize,
    gap: Pixels,
    style: StyleRefinement,
}

impl OverflowList {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            max_visible: 3,
            gap: px(6.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn item(mut self, item: impl IntoElement) -> Self {
        self.items.push(item.into_any_element());
        self
    }

    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible;
        self
    }

    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        self.gap = gap.into();
        self
    }
}

impl Default for OverflowList {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for OverflowList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for OverflowList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let overflow = self.items.len().saturating_sub(self.max_visible);
        let user_style = self.style;

        div()
            .flex()
            .items_center()
            .gap(self.gap)
            .overflow_hidden()
            .children(self.items.into_iter().take(self.max_visible))
            .when(overflow > 0, |this| {
                this.child(
                    div()
                        .flex_shrink_0()
                        .px(px(7.0))
                        .py(px(2.0))
                        .rounded_full()
                        .bg(theme.tokens.muted)
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(theme.tokens.muted_foreground)
                        .child(format!("+{overflow}")),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
