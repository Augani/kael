//! MobileNav component - compact ASTRYX navigation shell.

use crate::{components::icon::Icon, navigation::nav_item::NavItem, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

#[derive(IntoElement)]
pub struct MobileNavToggle {
    id: ElementId,
    open: bool,
    label: Option<SharedString>,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl MobileNavToggle {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "mobile-nav-toggle:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            open: false,
            label: None,
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.label = if label.trim().is_empty() {
            None
        } else {
            Some(label)
        };
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl Default for MobileNavToggle {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for MobileNavToggle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MobileNavToggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.open;
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        let handler = self.on_toggle;
        let label = self.label.unwrap_or_else(|| {
            if open {
                "Close navigation".into()
            } else {
                "Open navigation".into()
            }
        });
        let mut state = if open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if handler.is_none() {
            state |= AccessibilityState::DISABLED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(label.to_string())
            .states(state);
        if handler.is_some() {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        div()
            .id(self.id)
            .accessibility(accessibility)
            .relative()
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.tokens.radius_md)
            .when(handler.is_some(), |this| {
                this.focusable()
                    .tab_index(0)
                    .tab_stop(true)
                    .cursor_pointer()
                    .hover(move |style| style.bg(overlay_hover))
                    .focus_visible(move |style| style.bg(overlay_hover))
            })
            .when_some(handler, |this, handler| {
                let on_key = handler.clone();
                this.on_click(move |_, window, cx| {
                    handler(!open, window, cx);
                })
                .on_key_down(move |event, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        on_key(!open, window, cx);
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                })
            })
            .child(
                Icon::new(if open { "x" } else { "menu" })
                    .size(px(16.0))
                    .color(theme.tokens.foreground),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct MobileNav {
    title: SharedString,
    open: bool,
    items: Vec<NavItem>,
    on_toggle: Option<Box<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl MobileNav {
    pub fn new(title: impl Into<SharedString>) -> Self {
        let title = title.into();
        Self {
            title: if title.trim().is_empty() {
                "Navigation".into()
            } else {
                title
            },
            open: false,
            items: Vec::new(),
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn item(mut self, item: NavItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl Styled for MobileNav {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MobileNav {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let open = self.open;
        let user_style = self.style;
        let title = self.title;

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label(title.to_string()),
            )
            .flex()
            .flex_col()
            .bg(theme.tokens.popover)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(48.0))
                    .px(px(8.0))
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .text_size(px(15.0))
                            .line_height(px(20.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .child(StyledText::new(title).accessibility_hidden(true)),
                    )
                    .child(
                        MobileNavToggle::new()
                            .open(open)
                            .when_some(self.on_toggle, |toggle, handler| toggle.on_toggle(handler)),
                    ),
            )
            .when(open, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .p(px(8.0))
                        .children(self.items),
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
    fn empty_labels_and_titles_use_meaningful_defaults() {
        assert!(MobileNavToggle::new().label(" ").label.is_none());
        assert!(MobileNavToggle::new().open(true).label("").label.is_none());
        assert_eq!(MobileNav::new(" ").title.as_ref(), "Navigation");
    }
}
