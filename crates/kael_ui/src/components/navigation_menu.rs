//! Navigation menu component - Hierarchical navigation with expand/collapse state.

use kael::{prelude::FluentBuilder as _, *};
use std::collections::HashSet;
use std::hash::Hash;
use std::panic::Location;
use std::rc::Rc;

use crate::components::icon::Icon;
use crate::components::icon_button::IconButton;
use crate::components::icon_source::IconSource;
use crate::components::text::{Text, TextVariant};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NavigationMenuOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone)]
pub struct NavigationMenuItem<T: Clone = SharedString> {
    pub id: T,
    pub label: SharedString,
    pub icon: Option<IconSource>,
    pub disabled: bool,
    pub children: Vec<NavigationMenuItem<T>>,
}

impl<T: Clone> NavigationMenuItem<T> {
    pub fn new(id: T, label: impl Into<SharedString>) -> Self {
        Self {
            id,
            label: label.into(),
            icon: None,
            disabled: false,
            children: Vec::new(),
        }
    }

    pub fn with_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn with_children(mut self, children: Vec<NavigationMenuItem<T>>) -> Self {
        self.children = children;
        self
    }

    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }
}

#[derive(IntoElement)]
pub struct NavigationMenu<T: Clone + PartialEq + Eq + Hash + 'static> {
    id: ElementId,
    orientation: NavigationMenuOrientation,
    items: Vec<NavigationMenuItem<T>>,
    selected_id: Option<T>,
    expanded_ids: Vec<T>,
    on_select: Option<Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>>,
    on_toggle: Option<Rc<dyn Fn(&T, bool, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> NavigationMenu<T> {
    /// Create a new navigation menu
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "navigation-menu:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            orientation: NavigationMenuOrientation::default(),
            items: Vec::new(),
            selected_id: None,
            expanded_ids: Vec::new(),
            on_select: None,
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    /// Set the menu orientation
    pub fn orientation(mut self, orientation: NavigationMenuOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Add a menu item
    pub fn item(mut self, item: NavigationMenuItem<T>) -> Self {
        self.items.push(item);
        self
    }

    /// Add multiple menu items
    pub fn items(mut self, items: Vec<NavigationMenuItem<T>>) -> Self {
        self.items = items;
        self
    }

    /// Set the selected item ID
    pub fn selected_id(mut self, id: T) -> Self {
        self.selected_id = Some(id);
        self
    }

    /// Set the expanded item IDs
    pub fn expanded_ids(mut self, ids: Vec<T>) -> Self {
        self.expanded_ids = ids;
        self
    }

    /// Set the selection handler
    pub fn on_select<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &mut Window, &mut App) + 'static,
    {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Set the toggle (expand/collapse) handler
    pub fn on_toggle<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, bool, &mut Window, &mut App) + 'static,
    {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> Default for NavigationMenu<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> Styled for NavigationMenu<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + Eq + Hash + 'static> RenderOnce for NavigationMenu<T> {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let orientation = self.orientation;

        let expanded_set: HashSet<T> = self.expanded_ids.into_iter().collect();
        let selected_id = self.selected_id;
        let on_select = self.on_select;
        let on_toggle = self.on_toggle;
        let user_style = self.style;
        let id = self.id;
        let id_key = id.to_string();

        div()
            .id(id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Tree).label("Navigation"),
            )
            .flex()
            .when(
                orientation == NavigationMenuOrientation::Horizontal,
                |this| this.flex_row().items_center().gap(px(4.0)),
            )
            .when(orientation == NavigationMenuOrientation::Vertical, |this| {
                this.flex_col().gap(px(2.0))
            })
            .children(
                self.items
                    .into_iter()
                    .enumerate()
                    .map(move |(index, item)| {
                        render_menu_item(
                            item,
                            orientation,
                            theme,
                            0,
                            id_key.clone(),
                            index.to_string(),
                            &expanded_set,
                            &selected_id,
                            &on_select,
                            &on_toggle,
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

/// Render a single menu item recursively
fn render_menu_item<T: Clone + PartialEq + Eq + Hash + 'static>(
    item: NavigationMenuItem<T>,
    orientation: NavigationMenuOrientation,
    theme: &crate::theme::Theme,
    depth: usize,
    root_id: String,
    path: String,
    expanded_set: &HashSet<T>,
    selected_id: &Option<T>,
    on_select: &Option<Rc<dyn Fn(&T, &mut Window, &mut App) + 'static>>,
    on_toggle: &Option<Rc<dyn Fn(&T, bool, &mut Window, &mut App) + 'static>>,
) -> impl IntoElement {
    let has_children = item.has_children();
    let disabled = item.disabled;
    let is_expanded = expanded_set.contains(&item.id);
    let is_selected = selected_id.as_ref() == Some(&item.id);
    let indent = px(depth as f32 * 16.0);
    let item_label = item.label.clone();
    let select_id = ElementId::Name(format!("{root_id}-select-{path}").into());
    let toggle_id = ElementId::Name(format!("{root_id}-toggle-{path}").into());
    let item_theme = theme.clone();
    let select_handler = on_select.clone();
    let select_item_id = item.id.clone();

    div()
        .relative()
        .flex()
        .flex_col()
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .py(px(8.0))
                .pl(when(
                    orientation == NavigationMenuOrientation::Vertical && depth > 0,
                    indent + px(8.0),
                    px(8.0),
                ))
                .rounded(theme.tokens.radius_sm)
                .transition(theme.tokens.transition_fast)
                .text_size(px(14.0))
                .when(is_selected, |this: Div| this.bg(theme.tokens.accent))
                .when(!is_selected && !disabled, |this: Div| {
                    this.hover(|style| style.bg(theme.tokens.accent.opacity(0.1)))
                })
                .when(has_children, |this: Div| {
                    let item_id = item.id.clone();
                    let on_toggle = on_toggle.clone();
                    let is_expanded_copy = is_expanded;

                    this.child(
                        IconButton::new(if is_expanded {
                            "arrow-down"
                        } else {
                            "arrow-right"
                        })
                        .id(toggle_id)
                        .label(format!(
                            "{} {}",
                            if is_expanded { "Collapse" } else { "Expand" },
                            item_label
                        ))
                        .size(px(28.0))
                        .icon_size(px(12.0))
                        .no_background(true)
                        .disabled(disabled || on_toggle.is_none())
                        .when_some(on_toggle, |this, on_toggle| {
                            this.on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                on_toggle(&item_id, !is_expanded_copy, window, cx);
                            })
                        }),
                    )
                })
                .when(!has_children, |this: Div| this.child(div().w(px(28.0))))
                .child(
                    button(select_id)
                        .role(AccessibilityRole::TreeItem)
                        .label(item.label.clone())
                        .when(disabled || select_handler.is_none(), |this| this.disabled())
                        .when_some(select_handler, |this, on_select| {
                            this.on_click(move |_, window, cx| {
                                on_select(&select_item_id, window, cx);
                            })
                        })
                        .render_with(move |state, _, _| {
                            div()
                                .flex()
                                .flex_1()
                                .w_full()
                                .items_center()
                                .gap(px(8.0))
                                .rounded(item_theme.tokens.radius_sm)
                                .cursor(if disabled {
                                    CursorStyle::Arrow
                                } else {
                                    CursorStyle::PointingHand
                                })
                                .when(state.focused && !disabled, |this| {
                                    this.shadow(smallvec::smallvec![
                                        crate::astryx::focus_ring_outer(item_theme.tokens.ring)
                                    ])
                                })
                                .when_some(item.icon.clone(), |this: Div, icon| {
                                    this.child(Icon::new(icon).size(px(16.0)).color(
                                        if is_selected {
                                            item_theme.tokens.accent_foreground
                                        } else if disabled {
                                            item_theme.tokens.muted_foreground
                                        } else {
                                            item_theme.tokens.foreground
                                        },
                                    ))
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .when(disabled, |this: Div| this.opacity(0.5))
                                        .child(
                                            Text::new(item.label.clone())
                                                .variant(TextVariant::Body)
                                                .accessibility_hidden(true)
                                                .weight(if is_selected {
                                                    FontWeight::SEMIBOLD
                                                } else {
                                                    FontWeight::NORMAL
                                                })
                                                .color(if is_selected {
                                                    item_theme.tokens.accent_foreground
                                                } else if disabled {
                                                    item_theme.tokens.muted_foreground
                                                } else {
                                                    item_theme.tokens.foreground
                                                }),
                                        ),
                                )
                                .into_any_element()
                        }),
                ),
        )
        .when(has_children && is_expanded, |this: Div| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .when(
                        orientation == NavigationMenuOrientation::Horizontal,
                        |this: Div| {
                            this.absolute()
                                .top_full()
                                .left_0()
                                .mt(px(4.0))
                                .min_w(px(200.0))
                                .bg(theme.tokens.popover)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .rounded(theme.tokens.radius_lg)
                                .shadow(theme.tokens.shadow_lg.to_vec())
                                .p(px(4.0))
                        },
                    )
                    .when(
                        orientation == NavigationMenuOrientation::Vertical,
                        |this: Div| this.mt(px(2.0)),
                    )
                    .children(item.children.into_iter().enumerate().map(|(index, child)| {
                        render_menu_item(
                            child,
                            orientation,
                            theme,
                            depth + 1,
                            root_id.clone(),
                            format!("{path}-{index}"),
                            expanded_set,
                            selected_id,
                            on_select,
                            on_toggle,
                        )
                    })),
            )
        })
}

/// Helper function for conditional values
fn when<T>(condition: bool, true_value: T, false_value: T) -> T {
    if condition {
        true_value
    } else {
        false_value
    }
}

#[cfg(test)]
mod tests {
    use super::{NavigationMenu, NavigationMenuItem, NavigationMenuOrientation};
    use kael::{
        AccessibilityAction, AccessibilityActionRequest, AccessibilityRole, Context, IntoElement,
        Render, SharedString, TestAppContext, Window,
    };
    use std::{cell::RefCell, rc::Rc};

    struct NavigationMenuHost {
        selected: Rc<RefCell<Option<SharedString>>>,
        toggled: Rc<RefCell<Option<(SharedString, bool)>>>,
    }

    impl Render for NavigationMenuHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let selected = self.selected.clone();
            let toggled = self.toggled.clone();
            NavigationMenu::<SharedString>::new()
                .orientation(NavigationMenuOrientation::Vertical)
                .items(vec![
                    NavigationMenuItem::new("overview".into(), "Overview"),
                    NavigationMenuItem::new("workspace".into(), "Workspace").with_children(vec![
                        NavigationMenuItem::new("components".into(), "Components"),
                    ]),
                ])
                .on_select(move |id, _, _| *selected.borrow_mut() = Some(id.clone()))
                .on_toggle(move |id, expanded, _, _| {
                    *toggled.borrow_mut() = Some((id.clone(), expanded));
                })
        }
    }

    #[kael::test]
    fn ui_thread_callbacks_route_select_and_expand_actions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let selected = Rc::new(RefCell::new(None));
        let toggled = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            let selected = selected.clone();
            let toggled = toggled.clone();
            move |_, _| NavigationMenuHost { selected, toggled }
        });

        let (overview_id, workspace_toggle_id) = window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let overview = tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::TreeItem
                        && node.label.as_deref() == Some("Overview")
                })
                .expect("selectable destination should be accessible");
            assert!(overview.actions.contains(&AccessibilityAction::Click));
            let workspace_toggle = tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Expand Workspace")
                })
                .expect("parent destination should expose its expansion control");
            assert!(workspace_toggle
                .actions
                .contains(&AccessibilityAction::Click));
            (overview.id, workspace_toggle.id)
        });

        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                overview_id,
                AccessibilityAction::Click,
            ));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                workspace_toggle_id,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();

        assert_eq!(
            selected.borrow().as_ref().map(|id| id.as_ref()),
            Some("overview")
        );
        assert_eq!(
            toggled
                .borrow()
                .as_ref()
                .map(|(id, expanded)| (id.as_ref(), *expanded)),
            Some(("workspace", true))
        );
    }
}
