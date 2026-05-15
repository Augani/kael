use super::local_history::{
    ensure_local_undo_redo_bindings, local_undo_redo_key_context, WindowValueHistory,
};
use crate::{
    div, px, AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, AnyElement, App, Component, Context, Div, ElementId, FocusHandle,
    InteractiveElement, IntoElement, ParentElement, Redo, RenderOnce, SharedString,
    StatefulInteractiveElement, Styled, Undo, Window,
};
use std::rc::Rc;

type ChangeListener = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

struct CheckboxState {
    focus_handle: FocusHandle,
    checked: bool,
    indeterminate: bool,
    history: WindowValueHistory<bool>,
    on_change: Option<ChangeListener>,
}

impl CheckboxState {
    fn new(
        focus_handle: FocusHandle,
        history: WindowValueHistory<bool>,
        checked: bool,
        indeterminate: bool,
        on_change: Option<ChangeListener>,
    ) -> Self {
        Self {
            focus_handle,
            checked,
            indeterminate,
            history,
            on_change,
        }
    }

    fn sync_from_props(
        &mut self,
        checked: bool,
        indeterminate: bool,
        on_change: Option<ChangeListener>,
    ) {
        if self.checked != checked || self.indeterminate != indeterminate {
            self.checked = checked;
            self.indeterminate = indeterminate;
            self.history.clear();
        }

        self.on_change = on_change;
    }

    fn next_checked(&self) -> bool {
        if self.indeterminate {
            true
        } else {
            !self.checked
        }
    }

    fn commit_checked(&mut self, next: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.indeterminate && self.checked == next {
            return;
        }

        let previous = self.checked;
        self.checked = next;
        self.indeterminate = false;
        self.history.record(previous, next);

        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_checked(self.next_checked(), window, cx);
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.history.undo() else {
            return;
        };

        self.checked = previous;
        self.indeterminate = false;
        if let Some(listener) = self.on_change.clone() {
            listener(&previous, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.history.redo() else {
            return;
        };

        self.checked = next;
        self.indeterminate = false;
        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }
}

#[non_exhaustive]
/// Snapshot of checkbox state passed to a custom renderer.
pub struct CheckboxRenderState {
    /// Whether the checkbox is currently checked.
    pub checked: bool,
    /// Whether the checkbox is currently indeterminate.
    pub indeterminate: bool,
    /// The visible label configured for the checkbox, if any.
    pub label: Option<SharedString>,
    /// Whether the checkbox currently owns keyboard focus.
    pub focused: bool,
    /// Whether the checkbox is disabled.
    pub disabled: bool,
}

type CheckboxCustomRenderer = Rc<dyn Fn(CheckboxRenderState, &Window, &App) -> AnyElement>;

/// Construct a controlled checkbox form control.
#[track_caller]
pub fn checkbox(id: impl Into<ElementId>, checked: bool) -> Checkbox {
    Checkbox::new(id.into(), checked)
}

/// A controlled checkbox form control.
pub struct Checkbox {
    element_id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    indeterminate: bool,
    disabled: bool,
    on_change: Option<ChangeListener>,
    custom_renderer: Option<CheckboxCustomRenderer>,
}

impl Checkbox {
    #[track_caller]
    fn new(element_id: ElementId, checked: bool) -> Self {
        Self {
            element_id,
            checked,
            label: None,
            indeterminate: false,
            disabled: false,
            on_change: None,
            custom_renderer: None,
        }
    }

    /// Set the visible label for the checkbox.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Render the checkbox in an indeterminate state.
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        self
    }

    /// Register a callback invoked with the next checked state after user interaction.
    pub fn on_change(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Disable the checkbox, preventing user interaction.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Render the checkbox indicator and content with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(CheckboxRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Checkbox {
            element_id,
            checked,
            label,
            indeterminate,
            disabled,
            on_change,
            custom_renderer,
        } = self;

        ensure_local_undo_redo_bindings(cx);

        let undo_manager = window.undo_manager();
        let initial_on_change = on_change.clone();

        let state = window.use_keyed_state(element_id.clone(), cx, move |_, cx| {
            let focus_handle = cx.focus_handle();
            let history =
                WindowValueHistory::new(undo_manager.clone(), &focus_handle, "Checkbox toggle");
            CheckboxState::new(
                focus_handle,
                history,
                checked,
                indeterminate,
                initial_on_change.clone(),
            )
        });
        state.update(cx, |state, _| {
            state.sync_from_props(checked, indeterminate, on_change.clone());
        });

        let (focus_handle, checked, indeterminate, can_undo, can_redo) = {
            let snapshot = state.read(cx);
            (
                snapshot.focus_handle.clone(),
                snapshot.checked,
                snapshot.indeterminate,
                snapshot.history.can_undo(),
                snapshot.history.can_redo(),
            )
        };

        let accessibility_label = label.as_ref().map(ToString::to_string);

        #[cfg(any(test, feature = "test-support"))]
        let debug_selector_id = element_id.to_string();

        let mut states = AccessibilityState::NONE;
        if indeterminate {
            states |= AccessibilityState::INDETERMINATE;
        } else if checked {
            states |= AccessibilityState::CHECKED;
        }

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::CheckBox)
            .states(states)
            .value(AccessibilityValue::Toggle(checked))
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Toggle,
            ]);
        if let Some(label) = accessibility_label {
            accessibility = accessibility.label(label);
        }

        let mut root = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(true)
            .key_context(local_undo_redo_key_context())
            .accessibility(accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

        if disabled {
            root = root.cursor_default();
        } else {
            root = root.cursor_pointer();
        }

        if can_undo {
            root = root.on_action({
                let state = state.clone();
                move |_: &Undo, window, cx| {
                    state.update(cx, |state, cx| {
                        state.undo(window, cx);
                    });
                }
            });
        }
        if can_redo {
            root = root.on_action({
                let state = state.clone();
                move |_: &Redo, window, cx| {
                    state.update(cx, |state, cx| {
                        state.redo(window, cx);
                    });
                }
            });
        }

        root = root
            .on_click({
                let state = state.clone();
                move |event, window, cx| {
                    if disabled {
                        return;
                    }
                    if !event.standard_click() {
                        return;
                    }

                    state.update(cx, |state, cx| {
                        state.toggle(window, cx);
                    });
                }
            })
            .on_key_down({
                let state = state.clone();
                move |event, window, cx| {
                    if disabled {
                        return;
                    }
                    if event.keystroke.modifiers.modified() {
                        return;
                    }

                    if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                        state.update(cx, |state, cx| {
                            state.toggle(window, cx);
                        });
                        window.prevent_default();
                    }
                }
            });

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("checkbox-{}", debug_selector_id);
            root = root.debug_selector(move || selector);
        }

        let focused = focus_handle.is_focused(window);
        let body = if let Some(renderer) = &custom_renderer {
            renderer(
                CheckboxRenderState {
                    checked,
                    indeterminate,
                    label: label.clone(),
                    focused,
                    disabled,
                },
                window,
                cx,
            )
        } else {
            default_checkbox_body(checked, indeterminate, label.clone())
        };

        root.child(body)
    }
}

fn default_checkbox_body(
    checked: bool,
    indeterminate: bool,
    label: Option<SharedString>,
) -> AnyElement {
    let mut body = div()
        .flex()
        .items_center()
        .gap_2()
        .child(default_checkbox_indicator(checked, indeterminate));

    if let Some(label) = label {
        body = body.child(div().child(label));
    }

    body.into_any_element()
}

fn default_checkbox_indicator(checked: bool, indeterminate: bool) -> Div {
    let box_fill = if checked || indeterminate {
        crate::rgb(0x1d4ed8)
    } else {
        crate::rgb(0xffffff)
    };
    let border_color = if checked || indeterminate {
        crate::rgb(0x1d4ed8)
    } else {
        crate::rgb(0x94a3b8)
    };

    let mut indicator = div()
        .w(px(18.0))
        .h(px(18.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(border_color)
        .bg(box_fill)
        .flex()
        .items_center()
        .justify_center();

    if indeterminate {
        indicator = indicator.child(
            div()
                .w(px(10.0))
                .h(px(2.0))
                .rounded(px(1.0))
                .bg(crate::rgb(0xffffff)),
        );
    } else if checked {
        indicator = indicator.child(
            div()
                .w(px(8.0))
                .h(px(8.0))
                .rounded(px(2.0))
                .bg(crate::rgb(0xffffff)),
        );
    }

    indicator
}

impl IntoElement for Checkbox {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{div, Context, Modifiers, Render, TestAppContext, Undo};

    struct CheckboxView {
        checked: bool,
    }

    struct CustomCheckboxView {
        checked: bool,
    }

    struct MultiCheckboxView {
        first: bool,
        second: bool,
    }

    impl Render for CheckboxView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            checkbox("agree", self.checked)
                .label("Agree")
                .on_change(cx.listener(|this, checked, _, cx| {
                    this.checked = *checked;
                    cx.notify();
                }))
        }
    }

    impl Render for CustomCheckboxView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            checkbox("agree_custom", self.checked)
                .label("Agree")
                .render_with(|state, _, _| {
                    let selector = format!(
                        "checkbox-custom-{}-{}-{}-{}",
                        if state.checked {
                            "checked"
                        } else {
                            "unchecked"
                        },
                        if state.indeterminate {
                            "indeterminate"
                        } else {
                            "determinate"
                        },
                        if state.focused { "focused" } else { "blurred" },
                        if state.label.is_some() {
                            "with-label"
                        } else {
                            "no-label"
                        },
                    );
                    div()
                        .debug_selector(move || selector)
                        .child(state.label.unwrap_or_else(|| "missing".into()))
                        .into_any_element()
                })
                .on_change(cx.listener(|this, checked, _, cx| {
                    this.checked = *checked;
                    cx.notify();
                }))
        }
    }

    impl Render for MultiCheckboxView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .child(
                    checkbox("first", self.first)
                        .label("First")
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.first = *checked;
                            cx.notify();
                        })),
                )
                .child(
                    checkbox("second", self.second)
                        .label("Second")
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.second = *checked;
                            cx.notify();
                        })),
                )
        }
    }

    #[crate::test]
    fn checkbox_click_and_space_toggle_value(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| CheckboxView { checked: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("checkbox-agree").unwrap();
        window.simulate_click(bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::CheckBox)
                .unwrap();
            assert!(node.states.contains(AccessibilityState::CHECKED));
        });

        window.simulate_keystrokes("space");
        window.update(|window, cx| {
            window.draw(cx).clear();

            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::CheckBox)
                .unwrap();
            assert!(!node.states.contains(AccessibilityState::CHECKED));
        });
    }

    #[crate::test]
    fn checkbox_undo_redo_tracks_focused_value(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| CheckboxView { checked: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("checkbox-agree").unwrap();
        window.simulate_click(bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(view.read(cx).checked);
        });

        window.simulate_keystrokes("cmd-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!view.read(cx).checked);
        });

        window.simulate_keystrokes("cmd-shift-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(view.read(cx).checked);
        });
    }

    #[crate::test]
    fn undo_availability_and_dispatch_follow_focused_checkbox(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| MultiCheckboxView {
            first: false,
            second: false,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!window.is_action_available(&Undo, cx));
        });

        let first_bounds = window.debug_bounds("checkbox-first").unwrap();
        let second_bounds = window.debug_bounds("checkbox-second").unwrap();

        window.simulate_click(first_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert!(view.first);
            assert!(!view.second);
            assert!(window.is_action_available(&Undo, cx));
        });

        window.simulate_click(second_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert!(view.first);
            assert!(view.second);
            assert!(window.is_action_available(&Undo, cx));
        });

        window.simulate_keystrokes("cmd-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert!(view.first);
            assert!(!view.second);
            assert!(!window.is_action_available(&Undo, cx));
            assert!(window.is_action_available(&Redo, cx));
        });

        window.simulate_keystrokes("shift-tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window.is_action_available(&Undo, cx));
        });

        window.simulate_keystrokes("cmd-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let view = view.read(cx);
            assert!(!view.first);
            assert!(!view.second);
            assert!(!window.is_action_available(&Undo, cx));
        });
    }

    #[crate::test]
    fn checkbox_render_with_receives_label_and_focus(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| CustomCheckboxView { checked: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("checkbox-custom-unchecked-determinate-blurred-with-label")
            .is_some());

        let bounds = window.debug_bounds("checkbox-agree_custom").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("checkbox-custom-checked-determinate-focused-with-label")
            .is_some());
    }
}
