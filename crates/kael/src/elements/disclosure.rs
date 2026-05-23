use crate::{
    div, px, AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, Component, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
};
use std::rc::Rc;

type ChangeListener = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of disclosure trigger state passed to a custom renderer.
pub struct DisclosureRenderState {
    /// Whether the disclosure is currently expanded.
    pub open: bool,
    /// The visible label configured for the trigger, if any.
    pub label: Option<SharedString>,
    /// Whether the trigger currently owns keyboard focus.
    pub focused: bool,
}

type DisclosureRenderer = Rc<dyn Fn(DisclosureRenderState, &Window, &App) -> AnyElement>;

/// Construct a controlled disclosure primitive.
#[track_caller]
pub fn disclosure(id: impl Into<ElementId>, open: bool) -> Disclosure {
    Disclosure::new(id.into(), open)
}

/// A controlled disclosure primitive with caller-owned trigger visuals and panel content.
pub struct Disclosure {
    element_id: ElementId,
    open: bool,
    label: Option<SharedString>,
    panel: Option<AnyElement>,
    on_change: Option<ChangeListener>,
    custom_renderer: Option<DisclosureRenderer>,
}

impl Disclosure {
    fn new(element_id: ElementId, open: bool) -> Self {
        Self {
            element_id,
            open,
            label: None,
            panel: None,
            on_change: None,
            custom_renderer: None,
        }
    }

    /// Set the visible label for the disclosure trigger.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the disclosure panel content.
    pub fn panel(mut self, panel: impl IntoElement) -> Self {
        self.panel = Some(panel.into_element().into_any_element());
        self
    }

    /// Register a callback invoked with the next open state after user interaction.
    pub fn on_change(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Render the disclosure trigger with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(DisclosureRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }
}

impl RenderOnce for Disclosure {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Disclosure {
            element_id,
            open,
            label,
            panel,
            on_change,
            custom_renderer,
        } = self;

        let disclosure_id = element_id.to_string();
        let focus_handle = window
            .use_keyed_state(
                ElementId::named_usize(format!("{}-focus", disclosure_id), 0),
                cx,
                |_, cx| cx.focus_handle(),
            )
            .read(cx)
            .clone();

        let mut trigger_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .states(if open {
                AccessibilityState::EXPANDED
            } else {
                AccessibilityState::COLLAPSED
            })
            .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        if let Some(label) = label.as_ref() {
            trigger_accessibility = trigger_accessibility.label(label.to_string());
        }

        let mut trigger = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(true)
            .cursor_pointer()
            .accessibility(trigger_accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

        if let Some(listener) = on_change {
            let click_listener = listener.clone();
            let open_for_click = open;
            let open_for_key = open;
            let focus_for_click = focus_handle.clone();
            trigger = trigger
                .on_click(move |_, window, cx| {
                    window.focus(&focus_for_click);
                    click_listener(&!open_for_click, window, cx);
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }

                    if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                        listener(&!open_for_key, window, cx);
                        window.prevent_default();
                    }
                });
        }

        let trigger_body = if let Some(renderer) = custom_renderer {
            renderer(
                DisclosureRenderState {
                    open,
                    label: label.clone(),
                    focused: focus_handle.is_focused(window),
                },
                window,
                cx,
            )
        } else {
            default_disclosure_body(label.clone(), open)
        };

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("disclosure-{}", disclosure_id);
            trigger = trigger.debug_selector(move || selector);
        }

        let mut root = div().flex().flex_col().gap_2().child(trigger.child(trigger_body));

        if open {
            if let Some(panel) = panel {
                let mut panel_element = div()
                    .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group).states(
                        AccessibilityState::EXPANDED,
                    ))
                    .child(panel);
                #[cfg(any(test, feature = "test-support"))]
                {
                    let selector = format!("disclosure-panel-{}", disclosure_id);
                    panel_element = panel_element.debug_selector(move || selector);
                }
                root = root.child(panel_element);
            }
        }

        root
    }
}

fn default_disclosure_body(label: Option<SharedString>, open: bool) -> AnyElement {
    let icon = if open { "-" } else { "+" };
    div()
        .flex()
        .items_center()
        .gap_2()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(crate::rgb(0xcbd5e1))
        .bg(crate::rgb(0xffffff))
        .text_color(crate::rgb(0x0f172a))
        .child(icon)
        .child(label.unwrap_or_else(|| SharedString::from("Disclosure")))
        .into_any_element()
}

impl IntoElement for Disclosure {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Modifiers, Render, TestAppContext};

    struct DisclosureView {
        open: bool,
    }

    struct CustomDisclosureView {
        open: bool,
    }

    impl Render for DisclosureView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            disclosure("filters", self.open)
                .label("Filters")
                .panel(div().debug_selector(|| "filters-panel".to_string()).child("Options"))
                .on_change(cx.listener(|this, open, _, cx| {
                    this.open = *open;
                    cx.notify();
                }))
        }
    }

    impl Render for CustomDisclosureView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            disclosure("advanced", self.open)
                .label("Advanced")
                .panel(div().child("Advanced panel"))
                .render_with(|state, _, _| {
                    let selector = format!(
                        "disclosure-custom-{}-{}-{}",
                        state.label.unwrap_or_else(|| SharedString::from("missing")),
                        if state.open { "open" } else { "closed" },
                        if state.focused { "focused" } else { "blurred" },
                    );
                    div().debug_selector(move || selector).child("Advanced").into_any_element()
                })
                .on_change(cx.listener(|this, open, _, cx| {
                    this.open = *open;
                    cx.notify();
                }))
        }
    }

    #[crate::test]
    fn disclosure_click_and_keyboard_toggle_open(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| DisclosureView { open: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("disclosure-filters").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Filters")
                })
                .unwrap();
            assert!(node.states.contains(AccessibilityState::EXPANDED));
        });

        window.simulate_keystrokes("space");
        window.update(|window, cx| {
            window.draw(cx).clear();

            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Filters")
                })
                .unwrap();
            assert!(node.states.contains(AccessibilityState::COLLAPSED));
        });
    }

    #[crate::test]
    fn disclosure_render_with_receives_open_focus_and_label(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| CustomDisclosureView { open: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("disclosure-custom-Advanced-closed-blurred")
            .is_some());

        let bounds = window.debug_bounds("disclosure-advanced").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("disclosure-custom-Advanced-open-focused")
            .is_some());
    }
}