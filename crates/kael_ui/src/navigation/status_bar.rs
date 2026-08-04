//! Status bar component with customizable sections.

use crate::{
    components::{
        badge::{Badge, BadgeVariant},
        icon::Icon,
        icon_source::IconSource,
        text::caption,
    },
    theme::{Theme, use_theme},
};
use kael::{InteractiveElement, prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

#[derive(Clone)]
pub struct StatusItem {
    pub icon: Option<IconSource>,
    pub text: Option<SharedString>,
    pub badge: Option<SharedString>,
    pub badge_variant: BadgeVariant,
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    pub disabled: bool,
    pub tooltip: Option<SharedString>,
}

impl StatusItem {
    pub fn text(text: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            text: Some(text.into()),
            badge: None,
            badge_variant: BadgeVariant::Default,
            on_click: None,
            disabled: false,
            tooltip: None,
        }
    }

    pub fn icon(icon: impl Into<IconSource>) -> Self {
        Self {
            icon: Some(icon.into()),
            text: None,
            badge: None,
            badge_variant: BadgeVariant::Default,
            on_click: None,
            disabled: false,
            tooltip: None,
        }
    }

    pub fn icon_text(icon: impl Into<IconSource>, text: impl Into<SharedString>) -> Self {
        Self {
            icon: Some(icon.into()),
            text: Some(text.into()),
            badge: None,
            badge_variant: BadgeVariant::Default,
            on_click: None,
            disabled: false,
            tooltip: None,
        }
    }

    pub fn badge(text: impl Into<SharedString>, tooltip: impl Into<SharedString>) -> Self {
        Self {
            icon: None,
            text: None,
            badge: Some(text.into()),
            badge_variant: BadgeVariant::Default,
            on_click: None,
            disabled: false,
            tooltip: Some(tooltip.into()),
        }
    }

    pub fn icon_badge(icon: impl Into<IconSource>, badge: impl Into<SharedString>) -> Self {
        Self {
            icon: Some(icon.into()),
            text: None,
            badge: Some(badge.into()),
            badge_variant: BadgeVariant::Default,
            on_click: None,
            disabled: false,
            tooltip: None,
        }
    }

    pub fn badge_variant(mut self, variant: BadgeVariant) -> Self {
        self.badge_variant = variant;
        self
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarSection {
    Left,
    Center,
    Right,
}

pub struct StatusBar {
    id: ElementId,
    left_items: Vec<StatusItem>,
    center_items: Vec<StatusItem>,
    right_items: Vec<StatusItem>,
    height: Pixels,
    style: StyleRefinement,
}

impl StatusBar {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "status-bar:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            left_items: Vec::new(),
            center_items: Vec::new(),
            right_items: Vec::new(),
            height: px(28.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn left(mut self, items: Vec<StatusItem>) -> Self {
        self.left_items = items;
        self
    }

    pub fn center(mut self, items: Vec<StatusItem>) -> Self {
        self.center_items = items;
        self
    }

    pub fn right(mut self, items: Vec<StatusItem>) -> Self {
        self.right_items = items;
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        let height = f32::from(height);
        self.height = px(if height.is_finite() && height > 0.0 {
            height
        } else {
            28.0
        });
        self
    }

    pub fn add_left(mut self, item: StatusItem) -> Self {
        self.left_items.push(item);
        self
    }

    pub fn add_center(mut self, item: StatusItem) -> Self {
        self.center_items.push(item);
        self
    }

    pub fn add_right(mut self, item: StatusItem) -> Self {
        self.right_items.push(item);
        self
    }
}

impl Styled for StatusBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Default for StatusBar {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Render for StatusBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style.clone();
        let id_key = self.id.to_string();

        div()
            .id(self.id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Toolbar).label("Status bar"),
            )
            .flex()
            .items_center()
            .justify_between()
            .h(self.height)
            .px(px(12.0))
            .py(px(6.0))
            .gap(px(12.0))
            .bg(theme.tokens.card)
            .border_t_1()
            .border_color(theme.tokens.border)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(div().flex().items_center().gap(px(12.0)).children(
                self.left_items.iter().enumerate().map(|(index, item)| {
                    render_status_item(
                        ElementId::Name(format!("{id_key}-left-{index}").into()),
                        item.clone(),
                    )
                }),
            ))
            .child(div().flex().items_center().gap(px(12.0)).children(
                self.center_items.iter().enumerate().map(|(index, item)| {
                    render_status_item(
                        ElementId::Name(format!("{id_key}-center-{index}").into()),
                        item.clone(),
                    )
                }),
            ))
            .child(div().flex().items_center().gap(px(12.0)).children(
                self.right_items.iter().enumerate().map(|(index, item)| {
                    render_status_item(
                        ElementId::Name(format!("{id_key}-right-{index}").into()),
                        item.clone(),
                    )
                }),
            ))
    }
}

fn render_status_item(id: ElementId, item: StatusItem) -> impl IntoElement {
    let theme = use_theme();
    let has_action = !item.disabled && item.on_click.is_some();
    let handler = item.on_click.filter(|_| !item.disabled);
    let label = item
        .tooltip
        .clone()
        .or_else(|| item.text.clone())
        .or_else(|| item.badge.clone())
        .unwrap_or_else(|| SharedString::from("Status action"));
    let mut state = AccessibilityState::NONE;
    if item.disabled {
        state |= AccessibilityState::DISABLED;
    }
    let role = if handler.is_some() {
        AccessibilityRole::Button
    } else {
        AccessibilityRole::Group
    };
    let mut accessibility = AccessibilityAttributes::new(role)
        .label(label.to_string())
        .states(state);
    if handler.is_some() {
        accessibility =
            accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
    }

    div()
        .id(id)
        .accessibility(accessibility)
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .py(px(4.0))
        .rounded(theme.tokens.radius_sm)
        .transition(theme.tokens.transition_fast)
        .when(has_action, |div| {
            div.focusable()
                .tab_index(0)
                .tab_stop(true)
                .cursor(CursorStyle::PointingHand)
                .hover(|style| style.bg(theme.tokens.muted))
                .focus_visible(|style| style.bg(theme.tokens.muted))
        })
        .when(item.disabled, |div| div.opacity(0.5))
        .when_some(item.tooltip, |div, tooltip| div.tooltip(tooltip))
        .when_some(handler, |div, handler| {
            let on_key = handler.clone();
            div.on_click(move |_, window, cx| {
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
        .when_some(item.icon, |div, icon| {
            div.child(Icon::new(icon).size(px(14.0)).color(if item.disabled {
                theme.tokens.muted_foreground
            } else {
                theme.tokens.foreground
            }))
        })
        .when_some(item.text, |div, text| {
            div.child(
                caption(text)
                    .accessibility_hidden(true)
                    .color(if item.disabled {
                        theme.tokens.muted_foreground
                    } else {
                        theme.tokens.foreground
                    }),
            )
        })
        .when_some(item.badge, |div, badge_text| {
            div.child(
                Badge::new(badge_text)
                    .variant(item.badge_variant)
                    .accessibility_hidden(true),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_height_uses_the_safe_default() {
        assert_eq!(StatusBar::new().height(px(f32::NAN)).height, px(28.0));
        assert_eq!(StatusBar::new().height(px(-1.0)).height, px(28.0));
    }
}
