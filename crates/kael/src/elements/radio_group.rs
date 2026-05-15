use super::local_history::{
    WindowValueHistory, ensure_local_undo_redo_bindings, local_undo_redo_key_context,
};
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, Component, Context, ElementId, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Redo, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Undo,
    Window, div, px,
};
use std::rc::Rc;

type ChangeListener<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

struct RadioGroupState<T> {
    _history_focus_handle: FocusHandle,
    value: T,
    options: Vec<RadioOption<T>>,
    history: WindowValueHistory<T>,
    on_change: Option<ChangeListener<T>>,
}

impl<T> RadioGroupState<T>
where
    T: Clone + PartialEq + 'static,
{
    fn new(
        history_focus_handle: FocusHandle,
        history: WindowValueHistory<T>,
        value: T,
        options: Vec<RadioOption<T>>,
        on_change: Option<ChangeListener<T>>,
    ) -> Self {
        Self {
            _history_focus_handle: history_focus_handle,
            value,
            options,
            history,
            on_change,
        }
    }

    fn sync_from_props(
        &mut self,
        value: T,
        options: Vec<RadioOption<T>>,
        on_change: Option<ChangeListener<T>>,
    ) {
        if self.value != value || self.options != options {
            self.value = value;
            self.options = options;
            self.history.clear();
        }

        self.on_change = on_change;
    }

    fn set_value(&mut self, next: T, window: &mut Window, cx: &mut Context<Self>) {
        if self.value != next {
            let previous = self.value.clone();
            self.value = next.clone();
            self.history.record(previous, next.clone());
        }

        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.history.undo() else {
            return;
        };

        self.value = previous.clone();
        if let Some(listener) = self.on_change.clone() {
            listener(&previous, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.history.redo() else {
            return;
        };

        self.value = next.clone();
        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }
}

#[non_exhaustive]
/// Snapshot of a single radio option passed to a custom renderer.
pub struct RadioItemRenderState<T> {
    /// The option value associated with the rendered item.
    pub value: T,
    /// The visible label for the rendered item.
    pub label: SharedString,
    /// Zero-based index of the option inside the group.
    pub index: usize,
    /// Total number of options in the group.
    pub option_count: usize,
    /// Whether this option is currently selected.
    pub selected: bool,
    /// Whether this option currently owns keyboard focus.
    pub focused: bool,
    /// Whether the radio group is disabled.
    pub disabled: bool,
}

type RadioItemCustomRenderer<T> = Rc<dyn Fn(RadioItemRenderState<T>, &Window, &App) -> AnyElement>;

/// Construct a controlled radio group from a current value and labeled options.
#[track_caller]
pub fn radio_group<T, I, O>(id: impl Into<ElementId>, value: T, options: I) -> RadioGroup<T>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = O>,
    O: Into<RadioOption<T>>,
{
    RadioGroup::new(
        id.into(),
        value,
        options.into_iter().map(Into::into).collect(),
    )
}

#[derive(Clone, Debug, PartialEq)]
/// A single labeled option in a radio group.
pub struct RadioOption<T> {
    /// The value emitted when this option is selected.
    pub value: T,
    /// The visible label for this option.
    pub label: SharedString,
}

impl<T> RadioOption<T> {
    /// Create a new radio option.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}

impl<T, L> From<(T, L)> for RadioOption<T>
where
    L: Into<SharedString>,
{
    fn from((value, label): (T, L)) -> Self {
        Self::new(value, label)
    }
}

/// A controlled radio group form control.
pub struct RadioGroup<T> {
    element_id: ElementId,
    value: T,
    options: Vec<RadioOption<T>>,
    disabled: bool,
    on_change: Option<ChangeListener<T>>,
    custom_renderer: Option<RadioItemCustomRenderer<T>>,
}

impl<T> RadioGroup<T>
where
    T: Clone + PartialEq + 'static,
{
    #[track_caller]
    fn new(element_id: ElementId, value: T, options: Vec<RadioOption<T>>) -> Self {
        Self {
            element_id,
            value,
            options,
            disabled: false,
            on_change: None,
            custom_renderer: None,
        }
    }

    /// Disable the radio group, preventing user interaction.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Register a callback invoked with the newly selected option value.
    pub fn on_change(mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Render each option with caller-owned visuals while the framework keeps behavior and accessibility.
    pub fn render_with(
        mut self,
        renderer: impl Fn(RadioItemRenderState<T>, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }
}

impl<T> RenderOnce for RadioGroup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let RadioGroup {
            element_id,
            value,
            options,
            disabled,
            on_change,
            custom_renderer,
        } = self;

        ensure_local_undo_redo_bindings(cx);
        let undo_manager = window.undo_manager();
        let initial_value = value.clone();
        let initial_options = options.clone();
        let initial_on_change = on_change.clone();

        let state = window.use_keyed_state(element_id.clone(), cx, move |_, cx| {
            let history_focus_handle = cx.focus_handle();
            let history = WindowValueHistory::new(
                undo_manager.clone(),
                &history_focus_handle,
                "Radio selection",
            );
            RadioGroupState::new(
                history_focus_handle,
                history,
                initial_value.clone(),
                initial_options.clone(),
                initial_on_change.clone(),
            )
        });
        state.update(cx, |state, _| {
            state.sync_from_props(value.clone(), options.clone(), on_change.clone());
        });

        let (value, options, can_undo, can_redo) = {
            let snapshot = state.read(cx);
            (
                snapshot.value.clone(),
                snapshot.options.clone(),
                snapshot.history.can_undo(),
                snapshot.history.can_redo(),
            )
        };

        let group_id = element_id.to_string();
        let selected_index = options
            .iter()
            .position(|option| option.value == value)
            .unwrap_or(0);
        let option_values = options
            .iter()
            .map(|option| option.value.clone())
            .collect::<Vec<_>>();
        let option_count = option_values.len();

        let mut root = div()
            .tab_group()
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group));

        #[cfg(any(test, feature = "test-support"))]
        {
            let group_selector = format!("radio-group-{}", group_id);
            root = root.debug_selector(move || group_selector);
        }

        for (index, option) in options.into_iter().enumerate() {
            let is_selected = index == selected_index;
            let option_id = ElementId::named_usize(format!("{}-option", group_id), index);
            let option_focus = window
                .use_keyed_state(
                    ElementId::named_usize(format!("{}-option-focus", group_id), index),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone();
            let previous = if option_count <= 1 {
                index
            } else if index == 0 {
                option_count - 1
            } else {
                index - 1
            };
            let next = if option_count <= 1 || index + 1 == option_count {
                0
            } else {
                index + 1
            };
            let click_value = option.value.clone();
            let key_value_prev = option_values[previous].clone();
            let key_value_next = option_values[next].clone();
            let accessibility_label = option.label.clone();
            let key_state = state.clone();

            let mut option_element = div()
                .id(option_id)
                .track_focus(&option_focus)
                .tab_index(index as isize)
                .key_context(local_undo_redo_key_context())
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::RadioButton)
                        .states(if is_selected {
                            AccessibilityState::SELECTED | AccessibilityState::CHECKED
                        } else {
                            AccessibilityState::NONE
                        })
                        .actions(vec![
                            AccessibilityAction::Focus,
                            AccessibilityAction::Toggle,
                        ])
                        .label(accessibility_label.to_string()),
                )
                .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

            if disabled {
                option_element = option_element.cursor_default();
            } else {
                option_element = option_element.cursor_pointer();
            }

            if can_undo {
                option_element = option_element.on_action({
                    let state = state.clone();
                    move |_: &Undo, window, cx| {
                        state.update(cx, |state, cx| {
                            state.undo(window, cx);
                        });
                    }
                });
            }
            if can_redo {
                option_element = option_element.on_action({
                    let state = state.clone();
                    move |_: &Redo, window, cx| {
                        state.update(cx, |state, cx| {
                            state.redo(window, cx);
                        });
                    }
                });
            }

            let option_content = if let Some(renderer) = &custom_renderer {
                renderer(
                    RadioItemRenderState {
                        value: option.value.clone(),
                        label: option.label.clone(),
                        index,
                        option_count,
                        selected: is_selected,
                        focused: option_focus.is_focused(window),
                        disabled,
                    },
                    window,
                    cx,
                )
            } else {
                default_radio_item(is_selected, option.label.clone())
            };

            option_element = option_element
                .on_click({
                    let state = state.clone();
                    move |_, window, cx| {
                        if disabled {
                            return;
                        }
                        state.update(cx, |state, cx| {
                            state.set_value(click_value.clone(), window, cx);
                        });
                    }
                })
                .on_key_down(move |event, window, cx| {
                    if disabled {
                        return;
                    }
                    if event.keystroke.modifiers.modified() {
                        return;
                    }

                    let next_value = match event.keystroke.key.as_str() {
                        "left" | "up" => Some(key_value_prev.clone()),
                        "right" | "down" => Some(key_value_next.clone()),
                        _ => None,
                    };

                    if let Some(next_value) = next_value {
                        key_state.update(cx, |state, cx| {
                            state.set_value(next_value, window, cx);
                        });
                    }
                });

            #[cfg(any(test, feature = "test-support"))]
            {
                let selector = format!("radio-option-{}-{}", group_id, index);
                option_element = option_element.debug_selector(move || selector);
            }

            root = root.child(option_element.child(option_content));
        }

        root
    }
}

fn default_radio_item(selected: bool, label: SharedString) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(18.0))
                .h(px(18.0))
                .rounded(px(999.0))
                .border_1()
                .border_color(if selected {
                    crate::rgb(0x1d4ed8)
                } else {
                    crate::rgb(0x94a3b8)
                })
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(px(8.0))
                        .h(px(8.0))
                        .rounded(px(999.0))
                        .bg(if selected {
                            crate::rgb(0x1d4ed8)
                        } else {
                            crate::rgba(0x00000000)
                        }),
                ),
        )
        .child(div().child(label))
        .into_any_element()
}

impl<T> IntoElement for RadioGroup<T>
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
    use crate::{Context, Render, TestAppContext, Undo, div};

    struct RadioView {
        value: &'static str,
    }

    struct CustomRadioView {
        value: &'static str,
    }

    impl Render for RadioView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            radio_group(
                "size",
                self.value,
                [("sm", "Small"), ("md", "Medium"), ("lg", "Large")],
            )
            .on_change(cx.listener(|this, value, _, cx| {
                this.value = *value;
                cx.notify();
            }))
        }
    }

    impl Render for CustomRadioView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            radio_group(
                "size",
                self.value,
                [("sm", "Small"), ("md", "Medium"), ("lg", "Large")],
            )
            .render_with(|state, _, _| {
                let selector = format!(
                    "radio-custom-{}-{}-{}",
                    state.index,
                    if state.selected { "selected" } else { "idle" },
                    if state.focused { "focused" } else { "blurred" },
                );
                div()
                    .debug_selector(move || selector)
                    .child(state.label)
                    .into_any_element()
            })
            .on_change(cx.listener(|this, value, _, cx| {
                this.value = *value;
                cx.notify();
            }))
        }
    }

    #[crate::test]
    fn radio_group_click_and_arrow_change_selection(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| RadioView { value: "sm" });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let medium = window.debug_bounds("radio-option-size-1").unwrap();
        window.simulate_click(medium.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let selected = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::RadioButton
                        && node.label.as_deref() == Some("Medium")
                })
                .unwrap();
            assert!(selected.states.contains(AccessibilityState::SELECTED));
        });

        window.simulate_keystrokes("right");
        window.update(|window, cx| {
            window.draw(cx).clear();

            let selected = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::RadioButton
                        && node.label.as_deref() == Some("Large")
                })
                .unwrap();
            assert!(selected.states.contains(AccessibilityState::SELECTED));
        });
    }

    #[crate::test]
    fn radio_group_undo_redo_tracks_shared_history(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| RadioView { value: "sm" });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!window.is_action_available(&Undo, cx));
        });

        let medium = window.debug_bounds("radio-option-size-1").unwrap();
        window.simulate_click(medium.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, "md");
            assert!(window.is_action_available(&Undo, cx));
        });

        window.simulate_keystrokes("cmd-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, "sm");
        });

        window.simulate_keystrokes("cmd-shift-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, "md");
        });
    }

    #[crate::test]
    fn radio_group_render_with_receives_option_metadata_and_focus(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| CustomRadioView { value: "sm" });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let selected = window
            .debug_bounds("radio-custom-0-selected-blurred")
            .unwrap();
        assert!(selected.size.width > px(0.0));

        let medium = window.debug_bounds("radio-option-size-1").unwrap();
        window.simulate_click(medium.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("radio-custom-1-selected-focused")
                .is_some()
        );
    }
}
