//! Sidebar navigation component with collapsible functionality.

use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, prelude::*, *};
use std::sync::Arc;

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
    focused_index: Option<usize>,
    style: StyleRefinement,
}

impl<T: Clone + PartialEq + 'static> Sidebar<T> {
    pub fn new(cx: &mut App) -> Self {
        Self {
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
            focused_index: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn items(mut self, items: Vec<SidebarItem<T>>) -> Self {
        self.items = items;
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
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
        let focused_index = self.focused_index;

        // Render all items before moving self
        let mut item_elements = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.separator {
                item_elements.push(
                    div()
                        .w_full()
                        .h(px(1.0))
                        .bg(theme.tokens.border.opacity(0.5))
                        .my(px(8.0))
                        .into_any_element(),
                );
            } else {
                let is_selected = matches!(selected_id.as_ref(), Some(id) if id == &item.id);
                let is_focused = Some(index) == focused_index;

                let item_element = self.render_sidebar_item(
                    item,
                    index,
                    is_selected,
                    is_focused,
                    is_expanded,
                    &theme,
                    overlay_hover,
                    cx,
                );
                item_elements.push(item_element);
            }
        }

        let user_style = self.style;

        let mut sidebar = div()
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
            let toggle_button = div()
                .flex()
                .items_center()
                .justify_center()
                .w_full()
                .h(px(48.0))
                .cursor(CursorStyle::PointingHand)
                .hover(move |style| style.bg(overlay_hover))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    if let Some(on_toggle) = on_toggle_for_button.clone() {
                        on_toggle(!is_expanded, window, cx);
                    }
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
                if event.keystroke.key.as_str() == "escape" && is_collapsible && is_expanded {
                    if let Some(on_toggle) = on_toggle_for_keyboard.clone() {
                        on_toggle(false, window, cx);
                    }
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
        _cx: &mut App,
    ) -> AnyElement {
        let on_select = self.on_select.clone();

        let mut item_container = div()
            .flex()
            .items_center()
            .w_full()
            .h(px(32.0))
            .px(px(8.0))
            .rounded(theme.tokens.radius_md)
            .transition(theme.tokens.transition_fast)
            .cursor(if item.disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            });

        if is_selected {
            item_container = item_container
                .bg(theme.tokens.accent)
                .text_color(theme.tokens.foreground);
        } else if is_focused {
            item_container = item_container
                .bg(overlay_hover)
                .text_color(theme.tokens.foreground);
        } else {
            item_container = item_container
                .text_color(theme.tokens.foreground)
                .hover(move |style| style.bg(overlay_hover));
        }

        if item.disabled {
            item_container = item_container.opacity(0.5).cursor(CursorStyle::Arrow);
        }

        if !item.disabled {
            item_container = item_container.on_mouse_down(MouseButton::Left, {
                let item_id = item.id.clone();
                move |_, window, cx| {
                    if let Some(on_select) = on_select.clone() {
                        on_select(&item_id, window, cx);
                    }
                }
            });
        }

        let mut children = Vec::new();

        if let Some(icon) = &item.icon {
            let icon_element = Icon::new(icon.clone())
                .size(px(16.0))
                .color(if item.disabled {
                    theme.tokens.muted_foreground
                } else {
                    theme.tokens.muted_foreground
                });

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
                .child(item.label.clone());

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
                    .child(badge.clone());

                children.push(badge_element.into_any_element());
            }
        }

        item_container.children(children).into_any_element()
    }
}

pub fn init_sidebar(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("ctrl-b", ToggleSidebar, None),
    ]);
}
