use super::local_history::{
    WindowValueHistory, ensure_local_undo_redo_bindings, local_undo_redo_key_context,
};
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, App, Component, Context, ElementId, FocusHandle, InteractiveElement,
    IntoElement, ParentElement, Redo, RenderOnce, SharedString, StatefulInteractiveElement, Styled,
    Undo, Window, div, px,
};
use std::rc::Rc;

type ChangeListener = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

struct ToggleState {
    focus_handle: FocusHandle,
    on: bool,
    history: WindowValueHistory<bool>,
    on_change: Option<ChangeListener>,
}

impl ToggleState {
    fn new(
        focus_handle: FocusHandle,
        history: WindowValueHistory<bool>,
        on: bool,
        on_change: Option<ChangeListener>,
    ) -> Self {
        Self {
            focus_handle,
            on,
            history,
            on_change,
        }
    }

    fn sync_from_props(&mut self, on: bool, on_change: Option<ChangeListener>) {
        if self.on != on {
            self.on = on;
            self.history.clear();
        }

        self.on_change = on_change;
    }

    fn set_value(&mut self, next: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.on != next {
            let previous = self.on;
            self.on = next;
            self.history.record(previous, next);
        }

        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }

    fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_value(!self.on, window, cx);
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.history.undo() else {
            return;
        };

        self.on = previous;
        if let Some(listener) = self.on_change.clone() {
            listener(&previous, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.history.redo() else {
            return;
        };

        self.on = next;
        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }
}

/// Construct a controlled toggle switch form control.
#[track_caller]
pub fn toggle(id: impl Into<ElementId>, on: bool) -> Toggle {
    Toggle::new(id.into(), on)
}

/// A controlled toggle switch form control.
pub struct Toggle {
    element_id: ElementId,
    on: bool,
    label: Option<SharedString>,
    on_change: Option<ChangeListener>,
}

impl Toggle {
    #[track_caller]
    fn new(element_id: ElementId, on: bool) -> Self {
        Self {
            element_id,
            on,
            label: None,
            on_change: None,
        }
    }

    /// Set the visible label for the toggle.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Register a callback invoked with the next on/off state after user interaction.
    pub fn on_change(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Toggle {
            element_id,
            on,
            label,
            on_change,
        } = self;

        ensure_local_undo_redo_bindings(cx);

        let undo_manager = window.undo_manager();
        let initial_on_change = on_change.clone();

        let state = window.use_keyed_state(element_id.clone(), cx, move |_, cx| {
            let focus_handle = cx.focus_handle();
            let history =
                WindowValueHistory::new(undo_manager.clone(), &focus_handle, "Toggle switch");
            ToggleState::new(focus_handle, history, on, initial_on_change.clone())
        });
        state.update(cx, |state, _| {
            state.sync_from_props(on, on_change.clone());
        });

        let (focus_handle, on, can_undo, can_redo) = {
            let snapshot = state.read(cx);
            (
                snapshot.focus_handle.clone(),
                snapshot.on,
                snapshot.history.can_undo(),
                snapshot.history.can_redo(),
            )
        };

        let accessibility_label = label.as_ref().map(ToString::to_string);

        #[cfg(any(test, feature = "test-support"))]
        let debug_selector_id = element_id.to_string();

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Switch)
            .states(if on {
                AccessibilityState::CHECKED
            } else {
                AccessibilityState::NONE
            })
            .value(AccessibilityValue::Toggle(on))
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
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .accessibility(accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

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
            let selector = format!("toggle-{}", debug_selector_id);
            root = root.debug_selector(move || selector);
        }

        let knob = div()
            .w(px(14.0))
            .h(px(14.0))
            .rounded(px(999.0))
            .bg(crate::rgb(0xffffff));

        let mut track = div()
            .w(px(34.0))
            .h(px(20.0))
            .rounded(px(999.0))
            .px(px(3.0))
            .flex()
            .items_center()
            .bg(if on {
                crate::rgb(0x1d4ed8)
            } else {
                crate::rgb(0xcbd5e1)
            });

        track = if on {
            track.justify_end().child(knob)
        } else {
            track.justify_start().child(knob)
        };

        root = root.child(track);

        if let Some(label) = label {
            root = root.child(div().child(label));
        }

        root
    }
}

impl IntoElement for Toggle {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Render, TestAppContext};

    struct ToggleView {
        on: bool,
    }

    impl Render for ToggleView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            toggle("wifi", self.on)
                .label("Wi-Fi")
                .on_change(cx.listener(|this, on, _, cx| {
                    this.on = *on;
                    cx.notify();
                }))
        }
    }

    #[crate::test]
    fn toggle_click_sets_switch_accessibility(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| ToggleView { on: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("toggle-wifi").unwrap();
        window.simulate_click(bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Switch)
                .unwrap();
            assert!(node.states.contains(AccessibilityState::CHECKED));
            assert_eq!(node.value, Some(AccessibilityValue::Toggle(true)));
        });
    }
}
