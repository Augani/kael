use crate::{
    div, px, AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, Component, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
};
use std::rc::Rc;

type ChangeListener<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of a tab trigger passed to a custom renderer.
pub struct TabRenderState<T> {
    /// The application value represented by this tab.
    pub value: T,
    /// The visible label configured for this tab.
    pub label: SharedString,
    /// Zero-based index of the tab inside the set.
    pub index: usize,
    /// Total number of tabs in the set.
    pub tab_count: usize,
    /// Whether this tab is currently selected.
    pub selected: bool,
    /// Whether this tab currently owns keyboard focus.
    pub focused: bool,
}

type TabRenderer<T> = Rc<dyn Fn(TabRenderState<T>, &Window, &App) -> AnyElement>;

/// Construct a controlled tabs primitive.
#[track_caller]
pub fn tabs<T, I>(id: impl Into<ElementId>, value: T, items: I) -> Tabs<T>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = TabItem<T>>,
{
    Tabs::new(id.into(), value, items.into_iter().collect())
}

/// A single tab item with a label and panel body.
pub struct TabItem<T> {
    /// The application value represented by this tab.
    pub value: T,
    /// The visible label for the tab trigger.
    pub label: SharedString,
    panel: AnyElement,
}

impl<T> TabItem<T> {
    /// Create a tab item from a value, label, and panel body.
    pub fn new(value: T, label: impl Into<SharedString>, panel: impl IntoElement) -> Self {
        Self {
            value,
            label: label.into(),
            panel: panel.into_element().into_any_element(),
        }
    }
}

/// A controlled tabs primitive with caller-owned tab bodies and panels.
pub struct Tabs<T> {
    element_id: ElementId,
    value: T,
    items: Vec<TabItem<T>>,
    on_change: Option<ChangeListener<T>>,
    tab_renderer: Option<TabRenderer<T>>,
}

impl<T> Tabs<T>
where
    T: Clone + PartialEq + 'static,
{
    fn new(element_id: ElementId, value: T, items: Vec<TabItem<T>>) -> Self {
        Self {
            element_id,
            value,
            items,
            on_change: None,
            tab_renderer: None,
        }
    }

    /// Register a callback invoked with the next selected tab value.
    pub fn on_change(mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Render each tab trigger with caller-owned visuals.
    pub fn render_tabs_with(
        mut self,
        renderer: impl Fn(TabRenderState<T>, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.tab_renderer = Some(Rc::new(renderer));
        self
    }
}

impl<T> RenderOnce for Tabs<T>
where
    T: Clone + PartialEq + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Tabs {
            element_id,
            value,
            items,
            on_change,
            tab_renderer,
        } = self;

        let tabs_id = element_id.to_string();
        let item_values = items.iter().map(|item| item.value.clone()).collect::<Vec<_>>();
        let tab_count = item_values.len();
        let selected_index = items
            .iter()
            .position(|item| item.value == value)
            .unwrap_or(0);

        let mut tab_list = div()
            .flex()
            .gap_2()
            .tab_group()
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group));

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("tabs-list-{}", tabs_id);
            tab_list = tab_list.debug_selector(move || selector);
        }

        let mut selected_panel: Option<(SharedString, AnyElement)> = None;

        for (index, item) in items.into_iter().enumerate() {
            let is_selected = index == selected_index;
            if is_selected {
                selected_panel = Some((item.label.clone(), item.panel));
            }

            let previous = if tab_count <= 1 {
                index
            } else if index == 0 {
                tab_count - 1
            } else {
                index - 1
            };
            let next = if tab_count <= 1 || index + 1 == tab_count {
                0
            } else {
                index + 1
            };

            let tab_focus = window
                .use_keyed_state(
                    ElementId::named_usize(format!("{}-tab-focus", tabs_id), index),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            let previous_focus = window
                .use_keyed_state(
                    ElementId::named_usize(format!("{}-tab-focus", tabs_id), previous),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            let next_focus = window
                .use_keyed_state(
                    ElementId::named_usize(format!("{}-tab-focus", tabs_id), next),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            let first_focus = window
                .use_keyed_state(
                    ElementId::named_usize(format!("{}-tab-focus", tabs_id), 0),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            let last_focus = window
                .use_keyed_state(
                    ElementId::named_usize(format!("{}-tab-focus", tabs_id), tab_count.saturating_sub(1)),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();

            let tab_id = ElementId::named_usize(format!("{}-tab", tabs_id), index);
            let current_value = item.value.clone();
            let previous_value = item_values[previous].clone();
            let next_value = item_values[next].clone();
            let first_value = item_values.first().cloned();
            let last_value = item_values.last().cloned();
            let accessibility_label = item.label.clone();
            let selected_value_for_click = value.clone();
            let selected_value_for_key = value.clone();

            let mut tab = div()
                .id(tab_id)
                .track_focus(&tab_focus)
                .tab_index(index as isize)
                .cursor_pointer()
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Tab)
                        .states(if is_selected {
                            AccessibilityState::SELECTED
                        } else {
                            AccessibilityState::NONE
                        })
                        .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click])
                        .label(accessibility_label.to_string()),
                )
                .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

            let click_listener = on_change.clone();
            tab = tab.on_click({
                let tab_focus = tab_focus.clone();
                let current_value = current_value.clone();
                move |_, window, cx| {
                    window.focus(&tab_focus);
                    if current_value != selected_value_for_click {
                        if let Some(listener) = click_listener.clone() {
                            listener(&current_value, window, cx);
                        }
                    }
                }
            });

            let key_listener = on_change.clone();
            let tab_focus_for_key = tab_focus.clone();
            tab = tab.on_key_down(move |event, window, cx| {
                if event.keystroke.modifiers.modified() || tab_count == 0 {
                    return;
                }

                let target = match event.keystroke.key.as_str() {
                    "left" | "up" => Some((previous_value.clone(), previous_focus.clone())),
                    "right" | "down" => Some((next_value.clone(), next_focus.clone())),
                    "home" => first_value.clone().map(|value| (value, first_focus.clone())),
                    "end" => last_value.clone().map(|value| (value, last_focus.clone())),
                    "space" | "enter" => {
                        Some((current_value.clone(), tab_focus_for_key.clone()))
                    }
                    _ => None,
                };

                let Some((next_value, next_focus)) = target else {
                    return;
                };

                window.focus(&next_focus);
                if next_value != selected_value_for_key {
                    if let Some(listener) = key_listener.clone() {
                        listener(&next_value, window, cx);
                    }
                }
                window.prevent_default();
            });

            let tab_body = if let Some(renderer) = &tab_renderer {
                renderer(
                    TabRenderState {
                        value: item.value.clone(),
                        label: item.label.clone(),
                        index,
                        tab_count,
                        selected: is_selected,
                        focused: tab_focus.is_focused(window),
                    },
                    window,
                    cx,
                )
            } else {
                default_tab_body(item.label.clone(), is_selected)
            };

            #[cfg(any(test, feature = "test-support"))]
            {
                let selector = format!("tabs-tab-{}-{}", tabs_id, index);
                tab = tab.debug_selector(move || selector);
            }

            tab_list = tab_list.child(tab.child(tab_body));
        }

        let mut root = div().flex().flex_col().gap_2().child(tab_list);

        if let Some((label, panel)) = selected_panel {
            let mut panel_element = div()
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::TabPanel)
                        .states(AccessibilityState::SELECTED)
                        .label(label.to_string()),
                )
                .child(panel);

            #[cfg(any(test, feature = "test-support"))]
            {
                let selector = format!("tabs-panel-{}", tabs_id);
                panel_element = panel_element.debug_selector(move || selector);
            }

            root = root.child(panel_element);
        }

        root
    }
}

fn default_tab_body(label: SharedString, selected: bool) -> AnyElement {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(if selected {
            crate::rgb(0x1d4ed8)
        } else {
            crate::rgb(0xcbd5e1)
        })
        .bg(if selected {
            crate::rgb(0xdbeafe)
        } else {
            crate::rgb(0xffffff)
        })
        .text_color(if selected {
            crate::rgb(0x1e3a8a)
        } else {
            crate::rgb(0x334155)
        })
        .child(label)
        .into_any_element()
}

impl<T> IntoElement for Tabs<T>
where
    T: Clone + PartialEq + 'static,
{
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Modifiers, Render, TestAppContext};

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum TestTab {
        Overview,
        Activity,
        Billing,
    }

    impl TestTab {
        fn label(self) -> &'static str {
            match self {
                TestTab::Overview => "Overview",
                TestTab::Activity => "Activity",
                TestTab::Billing => "Billing",
            }
        }
    }

    struct TabsView {
        selected: TestTab,
    }

    struct CustomTabsView {
        selected: TestTab,
    }

    impl Render for TabsView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            tabs(
                "workspace",
                self.selected,
                [
                    TabItem::new(
                        TestTab::Overview,
                        "Overview",
                        div()
                            .debug_selector(|| "panel-overview".to_string())
                            .child("Overview panel"),
                    ),
                    TabItem::new(
                        TestTab::Activity,
                        "Activity",
                        div()
                            .debug_selector(|| "panel-activity".to_string())
                            .child("Activity panel"),
                    ),
                    TabItem::new(
                        TestTab::Billing,
                        "Billing",
                        div()
                            .debug_selector(|| "panel-billing".to_string())
                            .child("Billing panel"),
                    ),
                ],
            )
            .on_change(cx.listener(|this, next, _, cx| {
                this.selected = *next;
                cx.notify();
            }))
        }
    }

    impl Render for CustomTabsView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            tabs(
                "custom_tabs",
                self.selected,
                [
                    TabItem::new(TestTab::Overview, "Overview", div().child("Overview")),
                    TabItem::new(TestTab::Activity, "Activity", div().child("Activity")),
                ],
            )
            .render_tabs_with(|state, _, _| {
                let selector = format!(
                    "tabs-custom-{}-{}-{}",
                    state.label,
                    if state.selected { "selected" } else { "unselected" },
                    if state.focused { "focused" } else { "blurred" },
                );
                div().debug_selector(move || selector).child(state.label).into_any_element()
            })
            .on_change(cx.listener(|this, next, _, cx| {
                this.selected = *next;
                cx.notify();
            }))
        }
    }

    #[crate::test]
    fn tabs_click_and_arrow_change_selection(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| TabsView {
            selected: TestTab::Overview,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("panel-overview").is_some());

        let activity_tab = window.debug_bounds("tabs-tab-workspace-1").unwrap();
        window.simulate_click(activity_tab.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Tab
                        && node.label.as_deref() == Some(TestTab::Activity.label())
                })
                .unwrap();
            assert!(node.states.contains(AccessibilityState::SELECTED));
        });
        assert!(window.debug_bounds("panel-activity").is_some());

        window.simulate_keystrokes("right");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::TabPanel
                        && node.label.as_deref() == Some(TestTab::Billing.label())
                })
                .unwrap();
            assert!(node.states.contains(AccessibilityState::SELECTED));
        });
        assert!(window.debug_bounds("panel-billing").is_some());
    }

    #[crate::test]
    fn tabs_render_tabs_with_receives_selection_and_focus(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| CustomTabsView {
            selected: TestTab::Overview,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("tabs-custom-Overview-selected-blurred")
            .is_some());

        let activity_tab = window.debug_bounds("tabs-tab-custom_tabs-1").unwrap();
        window.simulate_click(activity_tab.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("tabs-custom-Activity-selected-focused")
            .is_some());
    }
}