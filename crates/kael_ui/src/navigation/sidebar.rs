//! Sidebar navigation component with collapsible functionality.

use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, prelude::*, *};
use std::panic::Location;
use std::{rc::Rc, sync::Arc};

actions!(
    sidebar,
    [ToggleSidebar, FocusNext, FocusPrevious, ActivateItem]
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarVariant {
    Fixed,
    #[default]
    Collapsible,
    Overlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarPosition {
    #[default]
    Left,
    Right,
}

#[derive(Clone)]
pub struct SidebarItem<T: Clone> {
    pub id: T,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub badge: Option<SharedString>,
    pub disabled: bool,
    pub separator: bool,
}

impl<T: Clone> SidebarItem<T> {
    pub fn new(id: T, label: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            badge: None,
            disabled: false,
            separator: false,
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn separator(mut self, separator: bool) -> Self {
        self.separator = separator;
        self
    }
}

#[derive(Clone, IntoElement)]
pub struct Sidebar<T: Clone + PartialEq + 'static> {
    id: ElementId,
    label: SharedString,
    items: Vec<SidebarItem<T>>,
    selected_id: Option<T>,
    variant: SidebarVariant,
    position: SidebarPosition,
    expanded_width: Pixels,
    collapsed_width: Pixels,
    is_expanded: bool,
    show_toggle_button: bool,
    on_select: Option<Arc<dyn Fn(&T, &mut Window, &mut App) + Send + Sync + 'static>>,
    on_toggle: Option<Arc<dyn Fn(bool, &mut Window, &mut App) + Send + Sync + 'static>>,
    focus_handle: FocusHandle,
    style: StyleRefinement,
}

impl<T: Clone + PartialEq + 'static> Sidebar<T> {
    #[track_caller]
    pub fn new(cx: &mut App) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "sidebar:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: "Sidebar navigation".into(),
            items: Vec::new(),
            selected_id: None,
            variant: SidebarVariant::default(),
            position: SidebarPosition::default(),
            expanded_width: px(260.0),
            collapsed_width: px(48.0),
            is_expanded: true,
            show_toggle_button: true,
            on_select: None,
            on_toggle: None,
            focus_handle: cx.focus_handle(),
            style: StyleRefinement::default(),
        }
    }

    pub fn items(mut self, items: Vec<SidebarItem<T>>) -> Self {
        self.items = items;
        self
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn selected_id(mut self, id: T) -> Self {
        self.selected_id = Some(id);
        self
    }

    pub fn variant(mut self, variant: SidebarVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn position(mut self, position: SidebarPosition) -> Self {
        self.position = position;
        self
    }

    pub fn expanded_width(mut self, width: impl Into<Pixels>) -> Self {
        self.expanded_width = width.into();
        self
    }

    pub fn collapsed_width(mut self, width: impl Into<Pixels>) -> Self {
        self.collapsed_width = width.into();
        self
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.is_expanded = expanded;
        self
    }

    pub fn show_toggle_button(mut self, show: bool) -> Self {
        self.show_toggle_button = show;
        self
    }

    pub fn on_select<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_select = Some(Arc::new(f));
        self
    }

    pub fn on_toggle<F>(mut self, f: F) -> Self
    where
        F: Fn(bool, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_toggle = Some(Arc::new(f));
        self
    }

    fn current_width(&self) -> Pixels {
        if self.is_expanded {
            self.expanded_width
        } else {
            self.collapsed_width
        }
    }
}

impl<T: Clone + PartialEq + 'static> Styled for Sidebar<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + 'static> RenderOnce for Sidebar<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let current_width = self.current_width();
        let is_collapsible = self.variant == SidebarVariant::Collapsible;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

        let on_toggle_for_button = self.on_toggle.clone();
        let on_toggle_for_keyboard = self.on_toggle.clone();

        // Extract all data we need before moving self.style
        let variant = self.variant;
        let position = self.position;
        let show_toggle_button = self.show_toggle_button;
        let is_expanded = self.is_expanded;
        let selected_id = self.selected_id.clone();
        let sidebar_id = self.id.clone();
        let sidebar_label = self.label.clone();
        let enabled_indices: Rc<Vec<usize>> = Rc::new(
            self.items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    (!item.separator && !item.disabled && self.on_select.is_some()).then_some(index)
                })
                .collect(),
        );
        let tab_entry_index = selected_id
            .as_ref()
            .and_then(|selected| {
                self.items.iter().enumerate().find_map(|(index, item)| {
                    (&item.id == selected && enabled_indices.contains(&index)).then_some(index)
                })
            })
            .or_else(|| enabled_indices.first().copied());
        let item_focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..self.items.len())
                .map(|index| {
                    window
                        .use_keyed_state(
                            ElementId::NamedChild(
                                Box::new(sidebar_id.clone()),
                                format!("focus-{index}").into(),
                            ),
                            cx,
                            |_, cx| cx.focus_handle(),
                        )
                        .read(cx)
                        .clone()
                })
                .collect(),
        );

        // Render all items before moving self
        let mut item_elements = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.separator {
                item_elements.push(
                    div()
                        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Separator))
                        .w_full()
                        .h(px(1.0))
                        .bg(theme.tokens.border.opacity(0.5))
                        .my(px(8.0))
                        .into_any_element(),
                );
            } else {
                let is_selected = matches!(selected_id.as_ref(), Some(id) if id == &item.id);
                let focus_handle = item_focus_handles[index].clone();
                let is_focused = focus_handle.is_focused(window);

                let item_element = self.render_sidebar_item(
                    item,
                    index,
                    is_selected,
                    is_focused,
                    is_expanded,
                    &theme,
                    overlay_hover,
                    &sidebar_id,
                    focus_handle,
                    item_focus_handles.clone(),
                    enabled_indices.clone(),
                    tab_entry_index,
                );
                item_elements.push(item_element);
            }
        }

        let user_style = self.style;

        let mut sidebar = div()
            .id(sidebar_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::List)
                    .label(sidebar_label.to_string()),
            )
            .tab_group()
            .flex()
            .flex_col()
            .h_full()
            .bg(transparent_black())
            .w(current_width);

        sidebar = match variant {
            SidebarVariant::Overlay => sidebar
                .absolute()
                .shadow(theme.tokens.shadow_lg.to_vec())
                .when(position == SidebarPosition::Right, |s| s.right_0())
                .when(position == SidebarPosition::Left, |s| s.left_0()),
            _ => sidebar,
        };

        let header = if show_toggle_button && is_collapsible {
            let toggle_id = ElementId::NamedChild(Box::new(sidebar_id), "toggle".into());
            let toggle_label = if is_expanded {
                "Collapse sidebar"
            } else {
                "Expand sidebar"
            };
            let can_toggle = on_toggle_for_button.is_some();
            let mut state = if is_expanded {
                AccessibilityState::EXPANDED
            } else {
                AccessibilityState::COLLAPSED
            };
            if !can_toggle {
                state |= AccessibilityState::DISABLED;
            }
            let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
                .label(toggle_label)
                .states(state);
            if can_toggle {
                accessibility = accessibility
                    .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
            }
            let toggle_button = div()
                .id(toggle_id)
                .accessibility(accessibility)
                .flex()
                .items_center()
                .justify_center()
                .w_full()
                .h(px(48.0))
                .when(can_toggle, |this| {
                    this.focusable()
                        .tab_index(0)
                        .tab_stop(true)
                        .cursor(CursorStyle::PointingHand)
                        .hover(move |style| style.bg(overlay_hover))
                        .focus_visible(move |style| style.bg(overlay_hover))
                })
                .when(!can_toggle, |this| this.opacity(0.5))
                .when_some(on_toggle_for_button, |this, on_toggle| {
                    let on_key = on_toggle.clone();
                    this.on_click(move |_, window, cx| {
                        on_toggle(!is_expanded, window, cx);
                    })
                    .on_key_down(move |event, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            on_key(!is_expanded, window, cx);
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    })
                })
                .child(
                    Icon::new(if is_expanded {
                        "chevron-left"
                    } else {
                        "chevron-right"
                    })
                    .size(px(16.0))
                    .color(theme.tokens.muted_foreground),
                );

            Some(toggle_button)
        } else {
            None
        };

        let mut content = div()
            .flex()
            .flex_col()
            .flex_1()
            .gap(px(2.0))
            .px(px(8.0))
            .py(px(8.0));

        content = content.children(item_elements);

        // Extract focus_handle before using self
        let focus_handle = self.focus_handle.clone();

        sidebar = sidebar
            .track_focus(&focus_handle)
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.key.as_str() == "escape"
                    && is_collapsible
                    && is_expanded
                    && let Some(on_toggle) = on_toggle_for_keyboard.clone()
                {
                    on_toggle(false, window, cx);
                }
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            });

        sidebar.children(
            vec![
                header.map(|h| h.into_any_element()),
                Some(content.into_any_element()),
            ]
            .into_iter()
            .flatten(),
        )
    }
}

impl<T: Clone + PartialEq + 'static> Sidebar<T> {
    fn render_sidebar_item(
        &self,
        item: &SidebarItem<T>,
        _index: usize,
        is_selected: bool,
        is_focused: bool,
        sidebar_expanded: bool,
        theme: &crate::theme::Theme,
        overlay_hover: Hsla,
        sidebar_id: &ElementId,
        focus_handle: FocusHandle,
        focus_handles: Rc<Vec<FocusHandle>>,
        enabled_indices: Rc<Vec<usize>>,
        tab_entry_index: Option<usize>,
    ) -> AnyElement {
        let on_select = self.on_select.clone();
        let can_select = !item.disabled && on_select.is_some();
        let item_id = ElementId::NamedChild(
            Box::new(sidebar_id.clone()),
            format!("item-{_index}").into(),
        );
        let mut state = AccessibilityState::NONE;
        if is_selected {
            state |= AccessibilityState::SELECTED;
        }
        if item.disabled {
            state |= AccessibilityState::DISABLED;
        }
        if is_focused {
            state |= AccessibilityState::FOCUSED;
        }
        let role = if on_select.is_some() {
            AccessibilityRole::Button
        } else {
            AccessibilityRole::ListItem
        };
        let mut accessibility = AccessibilityAttributes::new(role)
            .label(item.label.to_string())
            .states(state);
        if can_select {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let mut item_container = div()
            .id(item_id)
            .accessibility(accessibility)
            .flex()
            .items_center()
            .w_full()
            .h(px(32.0))
            .px(px(8.0))
            .rounded(theme.tokens.radius_md)
            .transition(theme.tokens.transition_fast)
            .cursor(if can_select {
                CursorStyle::PointingHand
            } else {
                CursorStyle::Arrow
            });

        if is_selected {
            item_container = item_container
                .bg(theme.tokens.accent)
                .text_color(theme.tokens.foreground);
        } else if is_focused {
            item_container = item_container
                .bg(overlay_hover)
                .text_color(theme.tokens.foreground);
        } else if can_select {
            item_container = item_container
                .text_color(theme.tokens.foreground)
                .hover(move |style| style.bg(overlay_hover));
        }

        if item.disabled {
            item_container = item_container.opacity(0.5).cursor(CursorStyle::Arrow);
        }

        if can_select {
            let on_select = on_select.unwrap();
            let on_key = on_select.clone();
            let item_id_for_click = item.id.clone();
            let item_id_for_key = item.id.clone();
            let focus_on_mouse = focus_handle.clone();
            item_container = item_container
                .track_focus(
                    &focus_handle
                        .tab_index(if tab_entry_index == Some(_index) {
                            0
                        } else {
                            -1
                        })
                        .tab_stop(tab_entry_index == Some(_index)),
                )
                .focus_visible(move |style| style.bg(overlay_hover))
                .on_mouse_down(MouseButton::Left, move |_, window, _| {
                    window.focus(&focus_on_mouse);
                })
                .on_click(move |_, window, cx| {
                    on_select(&item_id_for_click, window, cx);
                })
                .on_key_down(move |event, window, cx| {
                    if let Some(target) = sidebar_navigation_target(
                        event.keystroke.key.as_str(),
                        _index,
                        enabled_indices.as_slice(),
                    ) {
                        window.focus(&focus_handles[target]);
                        cx.stop_propagation();
                        window.prevent_default();
                    } else if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        on_key(&item_id_for_key, window, cx);
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                });
        }

        let mut children = Vec::new();

        if let Some(icon) = &item.icon {
            let icon_element = Icon::new(icon.clone())
                .size(px(16.0))
                .color(theme.tokens.muted_foreground);

            children.push(icon_element.into_any_element());
        }

        if sidebar_expanded {
            let label_element = div()
                .flex_1()
                .ml(px(8.0))
                .text_size(px(14.0))
                .line_height(px(20.0))
                .font_family(theme.tokens.font_family.clone())
                .font_weight(if is_selected {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if is_selected {
                    theme.tokens.foreground
                } else if item.disabled {
                    theme.tokens.muted_foreground
                } else {
                    theme.tokens.foreground
                })
                .child(StyledText::new(item.label.clone()).accessibility_hidden(true));

            children.push(label_element.into_any_element());

            if let Some(badge) = &item.badge {
                let badge_element = div()
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(theme.tokens.radius_sm)
                    .bg(theme.tokens.muted)
                    .text_size(px(10.0))
                    .line_height(px(14.0))
                    .font_family(theme.tokens.font_family.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.tokens.muted_foreground)
                    .child(StyledText::new(badge.clone()).accessibility_hidden(true));

                children.push(badge_element.into_any_element());
            }
        }

        item_container.children(children).into_any_element()
    }
}

fn sidebar_navigation_target(key: &str, current: usize, enabled: &[usize]) -> Option<usize> {
    if enabled.is_empty() {
        return None;
    }
    let position = enabled
        .iter()
        .position(|candidate| *candidate == current)
        .unwrap_or(0);
    match key {
        "down" | "arrowdown" => Some(enabled[(position + 1) % enabled.len()]),
        "up" | "arrowup" => Some(enabled[(position + enabled.len() - 1) % enabled.len()]),
        "home" => enabled.first().copied(),
        "end" => enabled.last().copied(),
        _ => None,
    }
}

pub fn init_sidebar(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-b", ToggleSidebar, None),
    ]);
}

#[cfg(test)]
mod tests {
    use super::sidebar_navigation_target;

    #[test]
    fn keyboard_navigation_wraps_and_skips_disabled_or_separator_rows() {
        let enabled = [0, 2, 5];
        assert_eq!(sidebar_navigation_target("down", 0, &enabled), Some(2));
        assert_eq!(sidebar_navigation_target("down", 5, &enabled), Some(0));
        assert_eq!(sidebar_navigation_target("up", 0, &enabled), Some(5));
        assert_eq!(sidebar_navigation_target("home", 5, &enabled), Some(0));
        assert_eq!(sidebar_navigation_target("end", 0, &enabled), Some(5));
    }
}
