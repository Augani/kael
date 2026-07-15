//! NavItem component - ASTRYX navigation row primitive.

use crate::{navigation::nav_icon::NavIcon, styled_ext::StyledExt, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

#[derive(IntoElement)]
pub struct NavItem {
    id: ElementId,
    label: SharedString,
    icon: Option<SharedString>,
    badge: Option<SharedString>,
    selected: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl NavItem {
    #[track_caller]
    pub fn new(label: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        let label = label.into();
        Self {
            id: ElementId::Name(
                format!(
                    "nav-item:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: if label.trim().is_empty() {
                "Navigation item".into()
            } else {
                label
            },
            icon: None,
            badge: None,
            selected: false,
            disabled: false,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for NavItem {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NavItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let disabled = self.disabled;
        let selected = self.selected;
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        let is_control = self.on_click.is_some();
        let handler = self.on_click.filter(|_| !disabled);
        let mut accessibility_state = AccessibilityState::NONE;
        if selected {
            accessibility_state |= AccessibilityState::SELECTED;
        }
        if disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        let role = if is_control || disabled {
            AccessibilityRole::Button
        } else {
            AccessibilityRole::ListItem
        };
        let mut accessibility = AccessibilityAttributes::new(role)
            .label(self.label.to_string())
            .states(accessibility_state);
        if handler.is_some() {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        div()
            .id(self.id)
            .accessibility(accessibility)
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(36.0))
            .px(px(8.0))
            .rounded(theme.tokens.radius_md)
            .bg(if selected {
                theme.tokens.accent
            } else {
                transparent_black()
            })
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled && handler.is_some(), |this| {
                this.focusable()
                    .tab_index(0)
                    .tab_stop(true)
                    .focus_visible(|style| style.inset_ring(theme.tokens.ring, px(2.0)))
                    .cursor_pointer()
                    .hover(move |style| {
                        style.bg(if selected {
                            theme.tokens.accent
                        } else {
                            overlay_hover
                        })
                    })
            })
            .when_some(handler, |this, handler| {
                let on_key = handler.clone();
                this.on_click(move |_, window, cx| {
                    handler(window, cx);
                })
                .on_key_down(move |event, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        on_key(window, cx);
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                })
            })
            .when_some(self.icon, |this, icon| {
                this.child(NavIcon::new(icon).selected(self.selected))
            })
            .child(
                div()
                    .flex_1()
                    .text_size(px(14.0))
                    .line_height(px(20.0))
                    .font_weight(if self.selected {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(theme.tokens.foreground)
                    .child(StyledText::new(self.label).accessibility_hidden(true)),
            )
            .when_some(self.badge, |this, badge| {
                this.child(
                    div()
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded_full()
                        .bg(theme.tokens.muted)
                        .text_size(px(11.0))
                        .line_height(px(14.0))
                        .text_color(theme.tokens.muted_foreground)
                        .child(StyledText::new(badge).accessibility_hidden(true)),
                )
            })
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
    fn empty_labels_use_a_meaningful_default() {
        assert_eq!(NavItem::new(" ").label.as_ref(), "Navigation item");
    }
}
