//! Menu system for dropdown and context menus.

use crate::{
    components::{
        icon::Icon,
        icon_source::IconSource,
        text::{body, caption},
    },
    theme::{Theme, use_theme},
};
use kael::{InteractiveElement, prelude::FluentBuilder as _, *};
use std::collections::HashMap;
use std::panic::Location;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum MenuItemKind {
    Action,
    Checkbox { checked: bool },
    Radio { checked: bool },
    Submenu,
    Separator,
}

impl MenuItemKind {
    /// Stable kind key for content-safe diagnostics.
    pub fn to_text(&self) -> &'static str {
        match self {
            MenuItemKind::Action => "action",
            MenuItemKind::Checkbox { .. } => "checkbox",
            MenuItemKind::Radio { .. } => "radio",
            MenuItemKind::Submenu => "submenu",
            MenuItemKind::Separator => "separator",
        }
    }

    /// Returns true when this item kind carries checked state.
    pub fn is_checked(&self) -> bool {
        matches!(
            self,
            MenuItemKind::Checkbox { checked: true } | MenuItemKind::Radio { checked: true }
        )
    }
}

#[derive(Clone)]
pub struct MenuItem {
    pub id: SharedString,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub shortcut: Option<SharedString>,
    pub kind: MenuItemKind,
    pub disabled: bool,
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            kind: MenuItemKind::Action,
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn separator() -> Self {
        Self {
            id: SharedString::from("separator"),
            label: SharedString::from(""),
            icon: None,
            shortcut: None,
            kind: MenuItemKind::Separator,
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn checkbox(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            kind: MenuItemKind::Checkbox { checked },
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn radio(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            kind: MenuItemKind::Radio { checked },
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn submenu(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            kind: MenuItemKind::Submenu,
            disabled: false,
            on_click: None,
            children: Vec::new(),
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn with_children(mut self, children: Vec<MenuItem>) -> Self {
        self.children = children;
        self
    }

    /// Stable kind key for content-safe diagnostics.
    pub fn kind_key(&self) -> &'static str {
        self.kind.to_text()
    }

    /// Returns true when this item carries checked state.
    pub fn is_checked(&self) -> bool {
        self.kind.is_checked()
    }

    /// Returns true when this item has an icon.
    pub fn has_icon(&self) -> bool {
        self.icon.is_some()
    }

    /// Returns true when this item displays a shortcut.
    pub fn has_shortcut(&self) -> bool {
        self.shortcut.is_some()
    }

    /// Returns true when this item has a click handler.
    pub fn has_click_handler(&self) -> bool {
        self.on_click.is_some()
    }

    /// Returns true when this item is disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Returns the item id length without exposing the id.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns the item label length without exposing the label.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Returns the direct child count.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Counts this item and all descendants.
    pub fn total_item_count(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(MenuItem::total_item_count)
            .sum::<usize>()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "menu_item(kind={}, checked={}, disabled={}, has_icon={}, has_shortcut={}, has_handler={}, id_len_bytes={}, label_len_bytes={}, children={}, total_items={})",
            self.kind_key(),
            self.is_checked(),
            self.is_disabled(),
            self.has_icon(),
            self.has_shortcut(),
            self.has_click_handler(),
            self.id_len_bytes(),
            self.label_len_bytes(),
            self.child_count(),
            self.total_item_count()
        )
    }
}

#[derive(IntoElement)]
pub struct Menu {
    id: ElementId,
    items: Vec<MenuItem>,
    min_width: Pixels,
    max_height: Option<Pixels>,
    auto_focus: bool,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

struct MenuRuntime {
    checked: HashMap<SharedString, bool>,
}

impl Menu {
    #[track_caller]
    pub fn new(items: Vec<MenuItem>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "menu:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            items,
            min_width: px(200.0),
            max_height: Some(px(400.0)),
            auto_focus: false,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = width;
        self
    }

    /// Set a stable identity when multiple menus are rendered from one callsite.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn max_height(mut self, height: Option<Pixels>) -> Self {
        self.max_height = height;
        self
    }

    /// Focus the first enabled item when this menu is first presented.
    ///
    /// This is intended for transient menus. Static menus should keep the
    /// default so rendering them does not unexpectedly move keyboard focus.
    pub fn auto_focus(mut self, auto_focus: bool) -> Self {
        self.auto_focus = auto_focus;
        self
    }

    /// Handle dismissal requests such as Escape.
    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Returns the direct top-level item count.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Counts all items, including submenu descendants.
    pub fn total_item_count(&self) -> usize {
        self.items.iter().map(MenuItem::total_item_count).sum()
    }

    /// Counts top-level and nested action-like items.
    pub fn actionable_item_count(&self) -> usize {
        count_items_matching(&self.items, |item| {
            !matches!(item.kind, MenuItemKind::Separator | MenuItemKind::Submenu)
        })
    }

    /// Counts disabled items.
    pub fn disabled_item_count(&self) -> usize {
        count_items_matching(&self.items, MenuItem::is_disabled)
    }

    /// Counts submenu items.
    pub fn submenu_count(&self) -> usize {
        count_items_matching(&self.items, |item| {
            matches!(item.kind, MenuItemKind::Submenu)
        })
    }

    /// Counts separators.
    pub fn separator_count(&self) -> usize {
        count_items_matching(&self.items, |item| {
            matches!(item.kind, MenuItemKind::Separator)
        })
    }

    /// Counts items with shortcut labels.
    pub fn shortcut_count(&self) -> usize {
        count_items_matching(&self.items, MenuItem::has_shortcut)
    }

    /// Returns true when a max-height cap is configured.
    pub fn has_max_height(&self) -> bool {
        self.max_height.is_some()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "menu(items={}, total_items={}, actionable={}, disabled={}, submenus={}, separators={}, shortcuts={}, has_max_height={})",
            self.item_count(),
            self.total_item_count(),
            self.actionable_item_count(),
            self.disabled_item_count(),
            self.submenu_count(),
            self.separator_count(),
            self.shortcut_count(),
            self.has_max_height()
        )
    }
}

impl Styled for Menu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Menu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let menu_id = self.id;
        let interactive_indices: Rc<Vec<usize>> = Rc::new(
            self.items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| menu_item_enabled(item).then_some(index))
                .collect(),
        );
        let focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..self.items.len())
                .map(|index| {
                    window
                        .use_keyed_state(
                            ElementId::NamedChild(
                                Box::new(menu_id.clone()),
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
        if self.auto_focus
            && !focus_handles.iter().any(|handle| handle.is_focused(window))
            && let Some(index) = interactive_indices.first()
        {
            window.focus(&focus_handles[*index]);
        }
        let initial_checked: HashMap<_, _> = self
            .items
            .iter()
            .filter_map(|item| match item.kind {
                MenuItemKind::Checkbox { checked } | MenuItemKind::Radio { checked } => {
                    Some((item.id.clone(), checked))
                }
                _ => None,
            })
            .collect();
        let current_stateful_ids: Vec<_> = initial_checked.keys().cloned().collect();
        let runtime = window.use_keyed_state(
            ElementId::NamedChild(Box::new(menu_id.clone()), "selection".into()),
            cx,
            move |_, _| MenuRuntime {
                checked: initial_checked,
            },
        );
        runtime.update(cx, |runtime, _| {
            runtime
                .checked
                .retain(|id, _| current_stateful_ids.contains(id));
            for item in &self.items {
                match item.kind {
                    MenuItemKind::Checkbox { checked } | MenuItemKind::Radio { checked } => {
                        runtime.checked.entry(item.id.clone()).or_insert(checked);
                    }
                    _ => {}
                }
            }
        });
        let radio_ids = Rc::new(
            self.items
                .iter()
                .filter(|item| matches!(item.kind, MenuItemKind::Radio { .. }))
                .map(|item| item.id.clone())
                .collect::<Vec<_>>(),
        );
        let theme = Theme::of(cx).clone();
        let mut items = Vec::with_capacity(self.items.len());
        for (index, item) in self.items.into_iter().enumerate() {
            let item_id = ElementId::NamedChild(
                Box::new(menu_id.clone()),
                format!("{}-{index}", item.id).into(),
            );
            let expanded = window.use_keyed_state(
                ElementId::NamedChild(Box::new(item_id.clone()), "expanded".into()),
                cx,
                |_, _| false,
            );
            let is_expanded = *expanded.read(cx);
            items.push((index, item_id, item, expanded, is_expanded));
        }

        div()
            .id(menu_id)
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Menu).label("Menu"))
            .tab_group()
            .min_w(self.min_width)
            .when_some(self.max_height, |div, h| div.max_h(h))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .bg(theme.tokens.popover)
            .rounded(theme.tokens.radius_lg)
            .shadow(theme.tokens.shadow_md.to_vec())
            .p(px(4.0))
            .children(
                items
                    .into_iter()
                    .map(|(index, id, item, expanded, is_expanded)| {
                        render_menu_item(
                            id,
                            item,
                            expanded,
                            is_expanded,
                            self.min_width,
                            runtime.clone(),
                            radio_ids.clone(),
                            index,
                            focus_handles.clone(),
                            interactive_indices.clone(),
                            self.on_close.clone(),
                            window,
                            cx,
                        )
                    }),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn render_menu_item(
    id: ElementId,
    item: MenuItem,
    expanded: Entity<bool>,
    is_expanded: bool,
    parent_width: Pixels,
    runtime: Entity<MenuRuntime>,
    radio_ids: Rc<Vec<SharedString>>,
    index: usize,
    focus_handles: Rc<Vec<FocusHandle>>,
    interactive_indices: Rc<Vec<usize>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let theme = use_theme();
    let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);

    match item.kind {
        MenuItemKind::Separator => div()
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Separator))
            .h(px(1.0))
            .bg(theme.tokens.border)
            .my(px(4.0))
            .mx(px(0.0))
            .into_any_element(),
        _ => {
            let is_stateful = matches!(
                item.kind,
                MenuItemKind::Checkbox { .. } | MenuItemKind::Radio { .. }
            );
            let is_checked = runtime
                .read(cx)
                .checked
                .get(&item.id)
                .copied()
                .unwrap_or_else(|| item.is_checked());
            let has_submenu = matches!(item.kind, MenuItemKind::Submenu);
            let has_children = has_submenu && !item.children.is_empty();
            let is_expanded = has_children && is_expanded;
            let enabled = menu_item_enabled(&item);
            let mut state = AccessibilityState::NONE;
            if item.disabled || !enabled {
                state |= AccessibilityState::DISABLED;
            }
            if is_checked {
                state |= AccessibilityState::CHECKED;
            }
            if has_submenu {
                state |= if is_expanded {
                    AccessibilityState::EXPANDED
                } else {
                    AccessibilityState::COLLAPSED
                };
            }
            let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::MenuItem)
                .label(item.label.to_string())
                .states(state);
            if enabled {
                accessibility = accessibility
                    .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
            }
            let activation: Option<Rc<dyn Fn(&mut Window, &mut App)>> = if has_children {
                let expanded = expanded.clone();
                Some(Rc::new(move |_, cx| {
                    expanded.update(cx, |open, cx| {
                        *open = !*open;
                        cx.notify();
                    });
                }))
            } else if is_stateful && !item.disabled {
                let runtime = runtime.clone();
                let item_id = item.id.clone();
                let is_radio = matches!(item.kind, MenuItemKind::Radio { .. });
                let radio_ids = radio_ids.clone();
                let original_handler = item.on_click.clone();
                Some(Rc::new(move |window, cx| {
                    runtime.update(cx, |runtime, cx| {
                        if is_radio {
                            for id in radio_ids.iter() {
                                runtime.checked.insert(id.clone(), id == &item_id);
                            }
                        } else if let Some(checked) = runtime.checked.get_mut(&item_id) {
                            *checked = !*checked;
                        }
                        cx.notify();
                    });
                    if let Some(handler) = original_handler.as_ref() {
                        handler(window, cx);
                    }
                }))
            } else {
                item.on_click.clone().filter(|_| !item.disabled)
            };
            let children = item.children;
            let submenu_id = ElementId::NamedChild(Box::new(id.clone()), "submenu".into());
            let focus_handle = focus_handles[index].clone();
            let focus_on_mouse = focus_handle.clone();
            let handles_for_key = focus_handles.clone();
            let indices_for_key = interactive_indices.clone();
            let activation_for_key = activation.clone();
            let expanded_for_key = expanded.clone();
            let close_for_key = on_close.clone();
            let is_focused = focus_handle.is_focused(window);

            div()
                .relative()
                .child(
                    div()
                        .id(id)
                        .accessibility(accessibility)
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(theme.tokens.radius_md)
                        .text_size(px(14.0))
                        .line_height(px(20.0))
                        .transition(theme.tokens.transition_fast)
                        .cursor(if enabled {
                            CursorStyle::PointingHand
                        } else {
                            CursorStyle::Arrow
                        })
                        .when(!enabled, |div| div.opacity(0.5))
                        .when(enabled, |div| {
                            div.track_focus(
                                &focus_handle
                                    .tab_index(if interactive_indices.first() == Some(&index) {
                                        0
                                    } else {
                                        -1
                                    })
                                    .tab_stop(interactive_indices.first() == Some(&index)),
                            )
                            .hover(move |style| style.bg(overlay_hover))
                            .focus_visible(move |style| style.bg(overlay_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                move |_, window, _| {
                                    window.focus(&focus_on_mouse);
                                },
                            )
                        })
                        .when(is_focused, |div| div.bg(overlay_hover))
                        .when_some(activation, |div, handler| {
                            div.on_click(move |_, window, cx| {
                                handler(window, cx);
                            })
                        })
                        .when(enabled, |div| {
                            div.on_key_down(move |event, window, cx| {
                                let key = event.keystroke.key.as_str();
                                if let Some(target) =
                                    menu_navigation_target(key, index, indices_for_key.as_slice())
                                {
                                    window.focus(&handles_for_key[target]);
                                } else if matches!(key, "enter" | "space") {
                                    if let Some(handler) = activation_for_key.as_ref() {
                                        handler(window, cx);
                                    }
                                } else if matches!(key, "right" | "arrowright") && has_children {
                                    if !is_expanded {
                                        expanded_for_key.update(cx, |open, cx| {
                                            *open = true;
                                            cx.notify();
                                        });
                                    }
                                } else if matches!(key, "left" | "arrowleft") && is_expanded {
                                    expanded_for_key.update(cx, |open, cx| {
                                        *open = false;
                                        cx.notify();
                                    });
                                } else if key == "escape" {
                                    if let Some(handler) = close_for_key.as_ref() {
                                        handler(window, cx);
                                    }
                                } else {
                                    return;
                                }
                                cx.stop_propagation();
                                window.prevent_default();
                            })
                        })
                        .child(
                            div()
                                .size(px(20.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .when(is_checked, |div| {
                                    div.child(
                                        Icon::new("check")
                                            .size(px(14.0))
                                            .color(theme.tokens.foreground),
                                    )
                                }),
                        )
                        .when_some(item.icon, |this, icon| {
                            this.child(
                                div()
                                    .size(px(20.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_shrink_0()
                                    .child(Icon::new(icon).size(px(16.0)).color(
                                        if item.disabled {
                                            theme.tokens.muted_foreground
                                        } else {
                                            theme.tokens.foreground
                                        },
                                    )),
                            )
                        })
                        .child(
                            div().flex_1().child(
                                body(item.label).accessibility_hidden(true).color(
                                    if item.disabled {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    },
                                ),
                            ),
                        )
                        .when_some(item.shortcut, |div, shortcut| {
                            div.child(
                                caption(shortcut)
                                    .color(theme.tokens.muted_foreground)
                                    .no_wrap(),
                            )
                        })
                        .when(has_submenu, |div| {
                            div.child(
                                Icon::new("chevron-right")
                                    .size(px(14.0))
                                    .color(theme.tokens.muted_foreground),
                            )
                        }),
                )
                .when(is_expanded, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left(parent_width - px(4.0))
                            .top(px(-4.0))
                            .occlude()
                            .child(
                                Menu::new(children)
                                    .id(submenu_id)
                                    .auto_focus(true)
                                    .when_some(on_close, |menu, handler| {
                                        menu.on_close(move |window, cx| handler(window, cx))
                                    }),
                            ),
                    )
                })
                .into_any_element()
        }
    }
}

fn menu_item_enabled(item: &MenuItem) -> bool {
    if item.disabled || matches!(item.kind, MenuItemKind::Separator) {
        return false;
    }
    matches!(
        item.kind,
        MenuItemKind::Checkbox { .. } | MenuItemKind::Radio { .. }
    ) || item.on_click.is_some()
        || matches!(item.kind, MenuItemKind::Submenu) && !item.children.is_empty()
}

fn menu_navigation_target(key: &str, current: usize, enabled: &[usize]) -> Option<usize> {
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

#[derive(Clone)]
pub struct MenuBarItem {
    pub id: SharedString,
    pub label: SharedString,
    pub menu_items: Vec<MenuItem>,
    pub disabled: bool,
}

impl MenuBarItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            menu_items: Vec::new(),
            disabled: false,
        }
    }

    pub fn with_items(mut self, items: Vec<MenuItem>) -> Self {
        self.menu_items = items;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Returns the menu id length without exposing the id.
    pub fn id_len_bytes(&self) -> usize {
        self.id.len()
    }

    /// Returns the menu label length without exposing the label.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Returns the direct item count under this menu bar item.
    pub fn item_count(&self) -> usize {
        self.menu_items.len()
    }

    /// Counts all items under this menu bar item.
    pub fn total_item_count(&self) -> usize {
        self.menu_items.iter().map(MenuItem::total_item_count).sum()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "menu_bar_item(id_len_bytes={}, label_len_bytes={}, items={}, total_items={})",
            self.id_len_bytes(),
            self.label_len_bytes(),
            self.item_count(),
            self.total_item_count()
        )
    }
}

pub struct MenuBar {
    id: ElementId,
    items: Vec<MenuBarItem>,
    active_menu: Option<usize>,
    item_bounds: Vec<Bounds<Pixels>>,
}

impl MenuBar {
    #[track_caller]
    pub fn new(items: Vec<MenuBarItem>) -> Self {
        let caller = Location::caller();
        let item_bounds = vec![Bounds::default(); items.len()];
        Self {
            id: ElementId::Name(
                format!(
                    "menu-bar:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            items,
            active_menu: None,
            item_bounds,
        }
    }

    /// Set a stable identity when multiple menu bars originate at one callsite.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Returns the top-level menu count.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Counts all nested menu items.
    pub fn total_menu_item_count(&self) -> usize {
        self.items.iter().map(MenuBarItem::total_item_count).sum()
    }

    /// Returns true when a menu is currently active.
    pub fn has_active_menu(&self) -> bool {
        self.active_menu.is_some()
    }

    /// Returns true when the active menu index points at an existing menu.
    pub fn active_menu_is_valid(&self) -> bool {
        self.active_menu
            .is_some_and(|index| index < self.items.len())
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "menu_bar(items={}, total_menu_items={}, has_active_menu={}, active_menu_valid={})",
            self.item_count(),
            self.total_menu_item_count(),
            self.has_active_menu(),
            self.active_menu_is_valid()
        )
    }
}

fn menu_items_with_close_fallback(
    items: &[MenuItem],
    menu_bar: &Entity<MenuBar>,
    return_focus: &FocusHandle,
) -> Vec<MenuItem> {
    items
        .iter()
        .cloned()
        .map(|mut item| {
            item.children = menu_items_with_close_fallback(&item.children, menu_bar, return_focus);
            let closes_menu = matches!(
                item.kind,
                MenuItemKind::Action | MenuItemKind::Checkbox { .. } | MenuItemKind::Radio { .. }
            );
            if !item.disabled && closes_menu {
                let original_handler = item.on_click.clone();
                let item_id = item.id.clone();
                let menu_bar = menu_bar.clone();
                let return_focus = return_focus.clone();
                item.on_click = Some(Rc::new(move |window, cx| {
                    menu_bar.update(cx, |menu_bar, cx| {
                        menu_bar.toggle_item_if_stateful(&item_id);
                        menu_bar.active_menu = None;
                        cx.notify();
                    });
                    window.focus(&return_focus);
                    if let Some(handler) = original_handler.as_ref() {
                        handler(window, cx);
                    }
                }));
            }
            item
        })
        .collect()
}

fn toggle_stateful_item(items: &mut [MenuItem], target: &SharedString) -> bool {
    if let Some(index) = items.iter().position(|item| &item.id == target) {
        match items[index].kind {
            MenuItemKind::Checkbox { ref mut checked } => *checked = !*checked,
            MenuItemKind::Radio { .. } => {
                for item in items.iter_mut() {
                    if let MenuItemKind::Radio { ref mut checked } = item.kind {
                        *checked = false;
                    }
                }
                if let MenuItemKind::Radio { ref mut checked } = items[index].kind {
                    *checked = true;
                }
            }
            _ => {}
        }
        return true;
    }

    items
        .iter_mut()
        .any(|item| toggle_stateful_item(&mut item.children, target))
}

impl MenuBar {
    fn toggle_item_if_stateful(&mut self, target: &SharedString) {
        for menu in &mut self.items {
            if toggle_stateful_item(&mut menu.menu_items, target) {
                return;
            }
        }
    }
}

fn menu_bar_navigation_target(key: &str, current: usize, enabled: &[usize]) -> Option<usize> {
    if enabled.is_empty() {
        return None;
    }
    let position = enabled
        .iter()
        .position(|candidate| *candidate == current)
        .unwrap_or(0);
    match key {
        "right" | "arrowright" => Some(enabled[(position + 1) % enabled.len()]),
        "left" | "arrowleft" => Some(enabled[(position + enabled.len() - 1) % enabled.len()]),
        "home" => enabled.first().copied(),
        "end" => enabled.last().copied(),
        _ => None,
    }
}

impl Render for MenuBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let menu_bar_id = self.id.clone();
        let menu_bar = cx.entity();
        let enabled_indices: Rc<Vec<usize>> = Rc::new(
            self.items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    (!item.disabled && !item.menu_items.is_empty()).then_some(index)
                })
                .collect(),
        );
        let focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..self.items.len())
                .map(|index| {
                    window
                        .use_keyed_state(
                            ElementId::NamedChild(
                                Box::new(menu_bar_id.clone()),
                                format!("trigger-focus-{index}").into(),
                            ),
                            cx,
                            |_, cx| cx.focus_handle(),
                        )
                        .read(cx)
                        .clone()
                })
                .collect(),
        );

        div()
            .id(menu_bar_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Menu).label("Application menu"),
            )
            .tab_group()
            .flex()
            .items_center()
            .h(px(32.0))
            .px(px(8.0))
            .gap(px(2.0))
            .bg(theme.tokens.background)
            .border_b_1()
            .border_color(theme.tokens.border)
            .children(self.items.iter().enumerate().map(|(idx, item)| {
                let is_active = self.active_menu == Some(idx);
                let label = item.label.clone();
                let item_id = ElementId::NamedChild(Box::new(menu_bar_id.clone()), item.id.clone());
                let popup_id = ElementId::NamedChild(Box::new(item_id.clone()), "popup".into());
                let focus_handle = focus_handles[idx].clone();
                let menu_items =
                    menu_items_with_close_fallback(&item.menu_items, &menu_bar, &focus_handle);
                let enabled = !item.disabled && !menu_items.is_empty();
                let focus_on_mouse = focus_handle.clone();
                let focus_on_close = focus_handle.clone();
                let handles_for_key = focus_handles.clone();
                let indices_for_key = enabled_indices.clone();
                let menu_bar_for_close = menu_bar.clone();
                let is_focused = focus_handle.is_focused(window);
                let state = if !enabled {
                    AccessibilityState::DISABLED
                } else if is_active {
                    AccessibilityState::EXPANDED
                } else {
                    AccessibilityState::COLLAPSED
                };
                let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::MenuItem)
                    .label(label.to_string())
                    .states(state);
                if enabled {
                    accessibility = accessibility
                        .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
                }

                div()
                    .relative()
                    .child(
                        div()
                            .id(item_id)
                            .accessibility(accessibility)
                            .px(px(12.0))
                            .py(px(6.0))
                            .rounded(theme.tokens.radius_md)
                            .transition(theme.tokens.transition_fast)
                            .cursor(if enabled {
                                CursorStyle::PointingHand
                            } else {
                                CursorStyle::Arrow
                            })
                            .when(!enabled, |div| div.opacity(0.5))
                            .when(enabled, |div| {
                                div.track_focus(
                                    &focus_handle
                                        .tab_index(if enabled_indices.first() == Some(&idx) {
                                            0
                                        } else {
                                            -1
                                        })
                                        .tab_stop(enabled_indices.first() == Some(&idx)),
                                )
                                .on_mouse_down(
                                    MouseButton::Left,
                                    move |_, window, _| {
                                        window.focus(&focus_on_mouse);
                                    },
                                )
                            })
                            .when(is_active, |div| div.bg(theme.tokens.accent))
                            .when(is_focused && !is_active, |div| div.bg(theme.tokens.muted))
                            .when(!is_active && enabled, |div| {
                                div.hover(|style| style.bg(theme.tokens.muted))
                            })
                            .focus_visible(|style| style.bg(theme.tokens.muted))
                            .when(enabled, |div| {
                                div.on_click(cx.listener(move |this, _, _window, cx| {
                                    this.active_menu = if this.active_menu == Some(idx) {
                                        None
                                    } else {
                                        Some(idx)
                                    };
                                    cx.notify();
                                }))
                                .on_key_down(cx.listener(
                                    move |this, event: &KeyDownEvent, window, cx| {
                                        match event.keystroke.key.as_str() {
                                            "enter" | "space" => {
                                                this.active_menu = if this.active_menu == Some(idx)
                                                {
                                                    None
                                                } else {
                                                    Some(idx)
                                                };
                                            }
                                            "down" | "arrowdown" => this.active_menu = Some(idx),
                                            "left" | "arrowleft" | "right" | "arrowright"
                                            | "home" | "end" => {
                                                let Some(target) = menu_bar_navigation_target(
                                                    event.keystroke.key.as_str(),
                                                    idx,
                                                    indices_for_key.as_slice(),
                                                ) else {
                                                    return;
                                                };
                                                window.focus(&handles_for_key[target]);
                                                if this.active_menu.is_some() {
                                                    this.active_menu = Some(target);
                                                }
                                            }
                                            "escape" => this.active_menu = None,
                                            _ => return,
                                        }
                                        cx.notify();
                                        cx.stop_propagation();
                                        window.prevent_default();
                                    },
                                ))
                            })
                            .child(
                                body(label)
                                    .accessibility_hidden(true)
                                    .color(theme.tokens.foreground),
                            )
                            .child({
                                let menu_bar = menu_bar.clone();
                                canvas_with_prepaint(
                                    move |bounds, _, cx| {
                                        menu_bar.update(cx, |menu_bar, _| {
                                            if let Some(slot) = menu_bar.item_bounds.get_mut(idx) {
                                                *slot = bounds;
                                            }
                                        });
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full()
                            }),
                    )
                    .when(is_active && enabled, |this| {
                        let bounds = self.item_bounds.get(idx).copied().unwrap_or_default();
                        let mut anchor = anchored().snap_to_window_with_margin(Edges::all(px(8.0)));
                        if bounds.size.width > px(0.0) {
                            anchor = anchor
                                .anchor(Corner::TopLeft)
                                .position(bounds.corner(Corner::BottomLeft));
                        }
                        this.child(
                            deferred(
                                anchor.child(
                                    div().occlude().child(
                                        Menu::new(menu_items)
                                            .id(popup_id)
                                            .min_width(px(220.0))
                                            .auto_focus(true)
                                            .on_close(move |window, cx| {
                                                menu_bar_for_close.update(cx, |menu_bar, cx| {
                                                    menu_bar.active_menu = None;
                                                    cx.notify();
                                                });
                                                window.focus(&focus_on_close);
                                            }),
                                    ),
                                ),
                            )
                            .with_priority(1),
                        )
                    })
            }))
    }
}

#[derive(IntoElement)]
pub struct ContextMenu {
    id: ElementId,
    items: Vec<MenuItem>,
    position: Point<Pixels>,
}

impl ContextMenu {
    #[track_caller]
    pub fn new(items: Vec<MenuItem>, position: Point<Pixels>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "navigation-context-menu:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            items,
            position,
        }
    }

    /// Returns the direct item count.
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Counts all items, including submenu descendants.
    pub fn total_item_count(&self) -> usize {
        self.items.iter().map(MenuItem::total_item_count).sum()
    }

    /// Counts actionable, non-separator, non-submenu items.
    pub fn actionable_item_count(&self) -> usize {
        count_items_matching(&self.items, |item| {
            !matches!(item.kind, MenuItemKind::Separator | MenuItemKind::Submenu)
        })
    }

    /// Counts disabled items.
    pub fn disabled_item_count(&self) -> usize {
        count_items_matching(&self.items, MenuItem::is_disabled)
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "context_menu(items={}, total_items={}, actionable={}, disabled={})",
            self.item_count(),
            self.total_item_count(),
            self.actionable_item_count(),
            self.disabled_item_count()
        )
    }
}

fn count_items_matching(
    items: &[MenuItem],
    predicate: impl Fn(&MenuItem) -> bool + std::marker::Copy,
) -> usize {
    items
        .iter()
        .map(|item| usize::from(predicate(item)) + count_items_matching(&item.children, predicate))
        .sum()
}

impl RenderOnce for ContextMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let menu_id = ElementId::NamedChild(Box::new(self.id), "menu".into());

        anchored()
            .snap_to_window_with_margin(px(8.0))
            .anchor(Corner::TopLeft)
            .position(self.position)
            .child(
                Menu::new(self.items)
                    .id(menu_id)
                    .min_width(px(200.0))
                    .max_height(Some(px(400.0)))
                    .border_1()
                    .border_color(theme.tokens.border)
                    .shadow(theme.tokens.shadow_lg.to_vec()),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn menu_item_summary_is_content_safe() {
        let item = MenuItem::submenu("private.project", "Secret Project")
            .with_shortcut("cmd-shift-s")
            .with_children(vec![
                MenuItem::checkbox("private.project.sync", "Sync Private Project", true)
                    .on_click(|_, _| {}),
                MenuItem::separator(),
            ]);

        assert_eq!(item.kind_key(), "submenu");
        assert!(item.has_shortcut());
        assert_eq!(item.child_count(), 2);
        assert_eq!(item.total_item_count(), 3);

        let summary = item.to_text();
        assert!(summary.contains("kind=submenu"));
        assert!(summary.contains("children=2"));
        assert!(summary.contains("total_items=3"));
        assert!(!summary.contains("private.project"));
        assert!(!summary.contains("Secret Project"));
        assert!(!summary.contains("Sync Private Project"));
        assert!(!summary.contains("cmd-shift-s"));
    }

    #[::core::prelude::v1::test]
    fn menu_summary_is_content_safe() {
        let menu = Menu::new(vec![
            MenuItem::new("private.open", "Open Secret Workspace")
                .with_shortcut("cmd-o")
                .on_click(|_, _| {}),
            MenuItem::checkbox("private.sync", "Sync Private Workspace", true).disabled(true),
            MenuItem::submenu("private.recent", "Recent Private Workspaces").with_children(vec![
                MenuItem::new("private.recent.alpha", "Alpha Workspace"),
                MenuItem::separator(),
            ]),
        ]);

        assert_eq!(menu.item_count(), 3);
        assert_eq!(menu.total_item_count(), 5);
        assert_eq!(menu.actionable_item_count(), 3);
        assert_eq!(menu.disabled_item_count(), 1);
        assert_eq!(menu.submenu_count(), 1);
        assert_eq!(menu.separator_count(), 1);
        assert_eq!(menu.shortcut_count(), 1);
        assert!(menu.has_max_height());

        let summary = menu.to_text();
        assert!(summary.contains("items=3"));
        assert!(summary.contains("total_items=5"));
        assert!(summary.contains("actionable=3"));
        assert!(!summary.contains("private.open"));
        assert!(!summary.contains("Open Secret Workspace"));
        assert!(!summary.contains("Recent Private Workspaces"));
        assert!(!summary.contains("cmd-o"));
    }

    #[::core::prelude::v1::test]
    fn menu_bar_and_context_menu_summary_is_content_safe() {
        let file = MenuBarItem::new("private.file", "Private File").with_items(vec![
            MenuItem::new("private.file.open", "Open Private File"),
            MenuItem::separator(),
        ]);
        let menu_bar = MenuBar::new(vec![file.clone()]);

        assert_eq!(file.item_count(), 2);
        assert_eq!(file.total_item_count(), 2);
        assert_eq!(menu_bar.item_count(), 1);
        assert_eq!(menu_bar.total_menu_item_count(), 2);
        assert!(!menu_bar.has_active_menu());

        let item_summary = file.to_text();
        let bar_summary = menu_bar.to_text();
        assert!(item_summary.contains("items=2"));
        assert!(bar_summary.contains("items=1"));
        assert!(!item_summary.contains("private.file"));
        assert!(!item_summary.contains("Private File"));
        assert!(!bar_summary.contains("Open Private File"));

        let context_menu = ContextMenu::new(
            vec![
                MenuItem::new("private.context.copy", "Copy Private Value"),
                MenuItem::checkbox("private.context.pin", "Pin Private Value", false)
                    .disabled(true),
            ],
            point(px(640.0), px(320.0)),
        );

        assert_eq!(context_menu.item_count(), 2);
        assert_eq!(context_menu.total_item_count(), 2);
        assert_eq!(context_menu.actionable_item_count(), 2);
        assert_eq!(context_menu.disabled_item_count(), 1);

        let summary = context_menu.to_text();
        assert!(summary.contains("items=2"));
        assert!(summary.contains("disabled=1"));
        assert!(!summary.contains("private.context"));
        assert!(!summary.contains("Copy Private Value"));
        assert!(!summary.contains("640"));
        assert!(!summary.contains("320"));
    }

    #[::core::prelude::v1::test]
    fn menu_bar_stateful_items_toggle_without_external_handlers() {
        let mut items = vec![
            MenuItem::checkbox("wrap", "Word Wrap", true),
            MenuItem::radio("theme-light", "Light", true),
            MenuItem::radio("theme-dark", "Dark", false),
            MenuItem::submenu("more", "More")
                .with_children(vec![MenuItem::checkbox("minimap", "Minimap", false)]),
        ];

        assert!(toggle_stateful_item(&mut items, &"wrap".into()));
        assert!(!items[0].is_checked());

        assert!(toggle_stateful_item(&mut items, &"theme-dark".into()));
        assert!(!items[1].is_checked());
        assert!(items[2].is_checked());

        assert!(toggle_stateful_item(&mut items, &"minimap".into()));
        assert!(items[3].children[0].is_checked());
        assert!(!toggle_stateful_item(&mut items, &"missing".into()));
    }

    #[::core::prelude::v1::test]
    fn menu_keyboard_navigation_wraps_and_skips_inert_rows() {
        let enabled = [0, 2, 5];
        assert_eq!(menu_navigation_target("down", 0, &enabled), Some(2));
        assert_eq!(menu_navigation_target("arrowdown", 5, &enabled), Some(0));
        assert_eq!(menu_navigation_target("up", 0, &enabled), Some(5));
        assert_eq!(menu_navigation_target("home", 5, &enabled), Some(0));
        assert_eq!(menu_navigation_target("end", 0, &enabled), Some(5));
        assert_eq!(menu_navigation_target("enter", 0, &enabled), None);
    }

    #[::core::prelude::v1::test]
    fn menu_bar_keyboard_navigation_wraps_and_skips_disabled_triggers() {
        let enabled = [0, 2, 4];
        assert_eq!(menu_bar_navigation_target("right", 0, &enabled), Some(2));
        assert_eq!(
            menu_bar_navigation_target("arrowright", 4, &enabled),
            Some(0)
        );
        assert_eq!(menu_bar_navigation_target("left", 0, &enabled), Some(4));
        assert_eq!(menu_bar_navigation_target("home", 4, &enabled), Some(0));
        assert_eq!(menu_bar_navigation_target("end", 0, &enabled), Some(4));
    }
}
