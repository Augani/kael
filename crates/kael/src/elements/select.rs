use super::local_history::{
    WindowValueHistory, ensure_local_undo_redo_bindings, local_undo_redo_key_context,
};
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, AnyElement, App, AppContext, Bounds, Context, DismissEvent, Element,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    InteractiveElement, IntoElement, KeyDownEvent, LayerAnchor, LayerOptions, LayerStack, LayoutId,
    ParentElement, Pixels, Point, Redo, Render, SharedString, StatefulInteractiveElement, Styled,
    Undo, Window, div, point, px, text_input,
};
use std::rc::Rc;

type ChangeListener<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

/// Construct a controlled combo box from the current value and labeled options.
#[track_caller]
pub fn select<T, I, O>(id: impl Into<ElementId>, value: T, options: I) -> Select<T>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = O>,
    O: Into<SelectOption<T>>,
{
    Select::new(
        id.into(),
        value,
        options.into_iter().map(Into::into).collect(),
    )
}

#[derive(Clone, Debug, PartialEq)]
/// A single labeled option in a select control.
pub struct SelectOption<T> {
    /// The value emitted when this option is selected.
    pub value: T,
    /// The visible label for this option.
    pub label: SharedString,
}

impl<T> SelectOption<T> {
    /// Create a new select option.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}

impl<T, L> From<(T, L)> for SelectOption<T>
where
    L: Into<SharedString>,
{
    fn from((value, label): (T, L)) -> Self {
        Self::new(value, label)
    }
}

/// A controlled popup-backed combo box.
pub struct Select<T> {
    element_id: ElementId,
    value: T,
    options: Vec<SelectOption<T>>,
    placeholder: SharedString,
    searchable: bool,
    on_change: Option<ChangeListener<T>>,
    source_location: &'static core::panic::Location<'static>,
}

#[doc(hidden)]
pub struct SelectElementState<T> {
    root: AnyElement,
    state: Entity<SelectState<T>>,
}

struct SelectState<T> {
    focus_handle: FocusHandle,
    layer_stack: Entity<LayerStack>,
    value: T,
    options: Vec<SelectOption<T>>,
    history: WindowValueHistory<T>,
    placeholder: SharedString,
    searchable: bool,
    search_query: SharedString,
    highlighted_index: Option<usize>,
    trigger_bounds: Option<Bounds<Pixels>>,
    on_change: Option<ChangeListener<T>>,
}

struct SelectPopup<T> {
    selector_id: String,
    state: Entity<SelectState<T>>,
    root_focus: FocusHandle,
    search_focus: FocusHandle,
    searchable: bool,
}

impl<T> Select<T>
where
    T: Clone + PartialEq + 'static,
{
    #[track_caller]
    fn new(element_id: ElementId, value: T, options: Vec<SelectOption<T>>) -> Self {
        Self {
            element_id,
            value,
            options,
            placeholder: SharedString::default(),
            searchable: false,
            on_change: None,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the placeholder shown when the current value does not match any option.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Enable in-popup search filtering.
    pub fn searchable(mut self) -> Self {
        self.searchable = true;
        self
    }

    /// Register a callback invoked with the newly selected option value.
    pub fn on_change(mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    fn build_trigger(
        &self,
        selector_id: &str,
        state: Entity<SelectState<T>>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (snapshot, can_undo, can_redo) = {
            let select_state = state.read(cx);
            (
                select_state.snapshot(cx),
                select_state.history.can_undo(),
                select_state.history.can_redo(),
            )
        };
        let focus_handle = snapshot.focus_handle.clone();
        let undo_state = state.clone();
        let open_state = state.clone();
        let click_state = state.clone();
        let key_state = state.clone();
        let redo_state = state;
        let element_id = self.element_id.clone();
        let selector_id = selector_id.to_string();
        let click_selector_id = selector_id.clone();
        #[cfg(any(test, feature = "test-support"))]
        let trigger_selector_id = selector_id.clone();
        let key_selector_id = selector_id;
        let accessibility_value = snapshot.display_text.to_string();

        let mut accessibility_state = if snapshot.is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if focus_handle.is_focused(window) {
            accessibility_state |= AccessibilityState::FOCUSED;
        }

        let mut trigger = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(true)
            .key_context(local_undo_redo_key_context())
            .min_w(px(160.0))
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(if snapshot.is_open {
                crate::rgb(0x1d4ed8)
            } else {
                crate::rgb(0x94a3b8)
            })
            .bg(crate::rgb(0xffffff))
            .cursor_pointer()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::ComboBox)
                    .states(accessibility_state)
                    .value(AccessibilityValue::Text(accessibility_value))
                    .actions(vec![
                        AccessibilityAction::Focus,
                        AccessibilityAction::ShowMenu,
                    ]),
            )
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)))
            .hover(|style| style.bg(crate::rgb(0xf8fafc)));

        if can_undo {
            trigger = trigger.on_action({
                move |_: &Undo, window, cx| {
                    undo_state.update(cx, |state, cx| {
                        state.undo(window, cx);
                    });
                }
            });
        }
        if can_redo {
            trigger = trigger.on_action({
                move |_: &Redo, window, cx| {
                    redo_state.update(cx, |state, cx| {
                        state.redo(window, cx);
                    });
                }
            });
        }

        trigger = trigger
            .on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }

                let popup_state = open_state.clone();
                popup_state.update(cx, |state, cx| {
                    if state.is_open(cx) {
                        state.close_popup(cx);
                    } else {
                        state.open_popup(popup_state.clone(), &click_selector_id, window, cx);
                    }
                });
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }

                match event.keystroke.key.as_str() {
                    "space" | "enter" => {
                        let popup_state = click_state.clone();
                        popup_state.update(cx, |state, cx| {
                            if state.is_open(cx) {
                                state.close_popup(cx);
                            } else {
                                state.open_popup(popup_state.clone(), &key_selector_id, window, cx);
                            }
                        });
                        window.prevent_default();
                    }
                    "down" => {
                        let popup_state = key_state.clone();
                        popup_state.update(cx, |state, cx| {
                            state.move_highlight(
                                1,
                                popup_state.clone(),
                                &key_selector_id,
                                window,
                                cx,
                            );
                        });
                        window.prevent_default();
                    }
                    "up" => {
                        let popup_state = key_state.clone();
                        popup_state.update(cx, |state, cx| {
                            state.move_highlight(
                                -1,
                                popup_state.clone(),
                                &key_selector_id,
                                window,
                                cx,
                            );
                        });
                        window.prevent_default();
                    }
                    _ => {}
                }
            })
            .child(
                div()
                    .text_color(if snapshot.showing_placeholder {
                        crate::rgb(0x64748b)
                    } else {
                        crate::rgb(0x0f172a)
                    })
                    .child(snapshot.display_text),
            )
            .child(
                div()
                    .text_color(crate::rgb(0x64748b))
                    .child(if snapshot.is_open { "^" } else { "v" }),
            );

        #[cfg(any(test, feature = "test-support"))]
        {
            let trigger_selector = format!("select-{}", trigger_selector_id);
            trigger = trigger.debug_selector(move || trigger_selector);
        }

        trigger.into_any_element()
    }
}

impl<T> IntoElement for Select<T>
where
    T: Clone + PartialEq + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T> Element for Select<T>
where
    T: Clone + PartialEq + 'static,
{
    type RequestLayoutState = SelectElementState<T>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let global_id = global_id.expect("select requires a global id");
        let current_view = window.current_view();
        let selector_id = self.element_id.to_string();
        let value = self.value.clone();
        let options = self.options.clone();
        let placeholder = self.placeholder.clone();
        let searchable = self.searchable;
        let on_change = self.on_change.clone();
        let undo_manager = window.undo_manager();

        ensure_local_undo_redo_bindings(cx);

        let state =
            window.with_element_state(global_id, |state: Option<Entity<SelectState<T>>>, _| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let layer_stack = cx.new(|_| LayerStack::new());
                    let state = cx.new(|cx| {
                        let focus_handle = cx.focus_handle();
                        let history = WindowValueHistory::new(
                            undo_manager.clone(),
                            &focus_handle,
                            "Select option",
                        );
                        SelectState::new(
                            focus_handle,
                            layer_stack.clone(),
                            history,
                            value.clone(),
                            options.clone(),
                            placeholder.clone(),
                            searchable,
                            on_change.clone(),
                        )
                    });

                    cx.observe(&state, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();

                    cx.observe(&layer_stack, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();

                    (state.clone(), state)
                }
            });

        state.update(cx, |state, cx| {
            state.sync_from_props(value, options, placeholder, searchable, on_change, cx);
        });

        let trigger = self.build_trigger(&selector_id, state.clone(), window, cx);
        let overlay_origin = state.read(cx).trigger_bounds.map(|bounds| bounds.origin);
        let overlay =
            build_layer_overlay(state.read(cx).layer_stack.clone(), overlay_origin, window);
        let mut root = div()
            .relative()
            .child(trigger)
            .child(overlay)
            .into_any_element();
        let layout_id = root.request_layout(window, cx);

        (layout_id, SelectElementState { root, state })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let selector_id = self.element_id.to_string();
        let state_entity = request_layout.state.clone();

        request_layout.state.update(cx, |state, cx| {
            state.set_trigger_bounds(bounds, state_entity.clone(), &selector_id, window, cx);
        });
        request_layout.root.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        request_layout.root.paint(window, cx);
    }
}

impl<T> SelectState<T>
where
    T: Clone + PartialEq + 'static,
{
    fn new(
        focus_handle: FocusHandle,
        layer_stack: Entity<LayerStack>,
        history: WindowValueHistory<T>,
        value: T,
        options: Vec<SelectOption<T>>,
        placeholder: SharedString,
        searchable: bool,
        on_change: Option<ChangeListener<T>>,
    ) -> Self {
        let mut state = Self {
            focus_handle,
            layer_stack,
            value,
            options,
            history,
            placeholder,
            searchable,
            search_query: SharedString::default(),
            highlighted_index: None,
            trigger_bounds: None,
            on_change,
        };
        state.reset_highlight();
        state
    }

    fn sync_from_props(
        &mut self,
        value: T,
        options: Vec<SelectOption<T>>,
        placeholder: SharedString,
        searchable: bool,
        on_change: Option<ChangeListener<T>>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        let mut reset_history = false;

        if self.value != value {
            self.value = value;
            changed = true;
            reset_history = true;
        }
        if self.options != options {
            self.options = options;
            changed = true;
            reset_history = true;
        }
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            changed = true;
        }
        if self.searchable != searchable {
            self.searchable = searchable;
            changed = true;
        }
        if !self.searchable && !self.search_query.is_empty() {
            self.search_query = SharedString::default();
            changed = true;
        }
        if self.on_change.as_ref().map(Rc::as_ptr) != on_change.as_ref().map(Rc::as_ptr) {
            self.on_change = on_change;
            changed = true;
        }

        if reset_history {
            self.history.clear();
        }

        if changed {
            self.reset_highlight();
            cx.notify();
        }
    }

    fn snapshot(&self, cx: &App) -> SelectSnapshot {
        let selected_label = self.selected_label();
        let showing_placeholder = selected_label.is_none() && !self.placeholder.is_empty();
        SelectSnapshot {
            focus_handle: self.focus_handle.clone(),
            display_text: selected_label.unwrap_or_else(|| self.placeholder.clone()),
            showing_placeholder,
            is_open: self.is_open(cx),
        }
    }

    fn set_trigger_bounds(
        &mut self,
        bounds: Bounds<Pixels>,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.trigger_bounds != Some(bounds);
        self.trigger_bounds = Some(bounds);

        if changed && self.is_open(cx) {
            self.open_popup(state, selector_id, window, cx);
        }
    }

    fn is_open(&self, cx: &App) -> bool {
        !self.layer_stack.read(cx).is_empty()
    }

    fn close_popup(&mut self, cx: &mut Context<Self>) {
        self.layer_stack.update(cx, |stack, cx| {
            stack.clear(cx);
        });
        cx.notify();
    }

    fn open_popup(
        &mut self,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(anchor_position) = self.anchor_position() else {
            return;
        };

        if self.searchable {
            self.search_query = SharedString::default();
        }
        self.reset_highlight();

        let popup = cx.new({
            let selector_id = selector_id.to_string();
            let searchable = self.searchable;
            move |cx| SelectPopup::new(selector_id.clone(), state, searchable, cx)
        });

        let anchor = LayerAnchor::at(anchor_position)
            .offset(point(px(0.0), px(4.0)))
            .snap_to_window();

        self.layer_stack.update(cx, |stack, cx| {
            stack.clear(cx);
            stack.push(
                popup,
                LayerOptions::default()
                    .anchored(anchor)
                    .dismiss_on_click_outside()
                    .dismiss_on_escape()
                    .priority(100),
                window,
                cx,
            );
        });
        cx.notify();
    }

    fn move_highlight(
        &mut self,
        delta: isize,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_open(cx) {
            self.open_popup(state, selector_id, window, cx);
        }

        let filtered = self.filtered_indices();
        self.highlighted_index = next_highlighted_index(&filtered, self.highlighted_index, delta);
        cx.notify();
    }

    fn commit_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.highlighted_index else {
            return;
        };

        self.commit_index(index, window, cx);
    }

    fn commit_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(option) = self.options.get(index).cloned() else {
            return;
        };

        self.close_popup(cx);
        window.focus(&self.focus_handle);
        if self.value != option.value {
            let previous = self.value.clone();
            self.value = option.value.clone();
            self.history.record(previous, option.value.clone());
            self.reset_highlight();
        }
        if let Some(listener) = self.on_change.clone() {
            listener(&option.value, window, cx);
        }
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.history.undo() else {
            return;
        };

        self.value = previous.clone();
        self.reset_highlight();
        if let Some(listener) = self.on_change.clone() {
            listener(&previous, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.history.redo() else {
            return;
        };

        self.value = next.clone();
        self.reset_highlight();
        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }

    fn set_search_query(&mut self, query: SharedString, cx: &mut Context<Self>) {
        self.search_query = query;
        self.reset_highlight();
        cx.notify();
    }

    fn selected_index(&self) -> Option<usize> {
        self.options
            .iter()
            .position(|option| option.value == self.value)
    }

    fn selected_label(&self) -> Option<SharedString> {
        self.selected_index()
            .and_then(|index| self.options.get(index))
            .map(|option| option.label.clone())
    }

    fn filtered_indices(&self) -> Vec<usize> {
        filter_option_indices(&self.options, &self.search_query)
    }

    fn reset_highlight(&mut self) {
        let filtered = self.filtered_indices();
        self.highlighted_index = default_highlighted_index(&filtered, self.selected_index());
    }

    fn anchor_position(&self) -> Option<Point<Pixels>> {
        self.trigger_bounds
            .map(|bounds| point(bounds.left(), bounds.bottom()))
    }

    fn popup_width(&self) -> Pixels {
        self.trigger_bounds
            .map(|bounds| bounds.size.width.max(px(160.0)))
            .unwrap_or(px(160.0))
    }
}

impl<T> SelectPopup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn new(
        selector_id: String,
        state: Entity<SelectState<T>>,
        searchable: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| {
            cx.notify();
        })
        .detach();

        Self {
            selector_id,
            state,
            root_focus: cx.focus_handle(),
            search_focus: cx.focus_handle(),
            searchable,
        }
    }
}

impl<T> EventEmitter<DismissEvent> for SelectPopup<T> where T: Clone + PartialEq + 'static {}

impl<T> Focusable for SelectPopup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        let _ = cx;
        if self.searchable {
            self.search_focus.clone()
        } else {
            self.root_focus.clone()
        }
    }
}

impl<T> Render for SelectPopup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (snapshot, can_undo, can_redo) = {
            let state = self.state.read(cx);
            (
                PopupSnapshot {
                    width: state.popup_width(),
                    searchable: state.searchable,
                    search_query: state.search_query.clone(),
                    highlighted_index: state.highlighted_index,
                    selected_index: state.selected_index(),
                    options: state.options.clone(),
                    filtered_indices: state.filtered_indices(),
                },
                state.history.can_undo(),
                state.history.can_redo(),
            )
        };

        let navigation_state = self.state.clone();
        let commit_state = self.state.clone();
        let close_state = self.state.clone();
        let search_state = self.state.clone();
        let selector_id = self.selector_id.clone();

        let mut panel = div()
            .id(ElementId::named_usize(
                format!("{}-popup", self.selector_id),
                0,
            ))
            .min_w(snapshot.width)
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded(px(10.0))
            .border_1()
            .border_color(crate::rgb(0xcbd5e1))
            .bg(crate::rgb(0xffffff))
            .shadow_lg()
            .capture_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }

                match event.keystroke.key.as_str() {
                    "down" => {
                        let popup_state = navigation_state.clone();
                        popup_state.update(cx, |state, cx| {
                            state.move_highlight(1, popup_state.clone(), &selector_id, window, cx);
                        });
                        window.prevent_default();
                    }
                    "up" => {
                        let popup_state = navigation_state.clone();
                        popup_state.update(cx, |state, cx| {
                            state.move_highlight(-1, popup_state.clone(), &selector_id, window, cx);
                        });
                        window.prevent_default();
                    }
                    "enter" => {
                        commit_state.update(cx, |state, cx| {
                            state.commit_highlighted(window, cx);
                        });
                        window.prevent_default();
                    }
                    "escape" => {
                        close_state.update(cx, |state, cx| {
                            state.close_popup(cx);
                        });
                        window.prevent_default();
                    }
                    _ => {}
                }
            })
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::List));

        #[cfg(any(test, feature = "test-support"))]
        {
            let popup_selector = format!("select-popup-{}", self.selector_id);
            panel = panel.debug_selector(move || popup_selector);
        }

        if snapshot.searchable {
            panel = panel.child(
                div().track_focus(&self.search_focus).child(
                    text_input(
                        ElementId::named_usize(format!("{}-search", self.selector_id), 0),
                        snapshot.search_query,
                    )
                    .placeholder("Search")
                    .on_change(move |query, _, cx| {
                        search_state.update(cx, |state, cx| {
                            state.set_search_query(query, cx);
                        });
                    }),
                ),
            );
        } else {
            panel = panel
                .track_focus(&self.root_focus)
                .focusable()
                .tab_stop(true)
                .key_context(local_undo_redo_key_context());

            if can_undo {
                panel = panel.on_action({
                    let state = self.state.clone();
                    move |_: &Undo, window, cx| {
                        state.update(cx, |state, cx| {
                            state.undo(window, cx);
                        });
                    }
                });
            }
            if can_redo {
                panel = panel.on_action({
                    let state = self.state.clone();
                    move |_: &Redo, window, cx| {
                        state.update(cx, |state, cx| {
                            state.redo(window, cx);
                        });
                    }
                });
            }
        }

        if snapshot.filtered_indices.is_empty() {
            return panel
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(8.0))
                        .text_color(crate::rgb(0x64748b))
                        .child("No options"),
                )
                .into_any_element();
        }

        let mut list = div()
            .flex()
            .flex_col()
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::List));

        for option_index in snapshot.filtered_indices {
            let option = snapshot.options[option_index].clone();
            let is_highlighted = snapshot.highlighted_index == Some(option_index);
            let is_selected = snapshot.selected_index == Some(option_index);
            let commit_state = self.state.clone();
            let selector_id = self.selector_id.clone();

            let mut row = div()
                .id(ElementId::named_usize(
                    format!("{}-option", selector_id),
                    option_index,
                ))
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::ListItem)
                        .label(option.label.to_string())
                        .states(if is_selected {
                            AccessibilityState::SELECTED
                        } else {
                            AccessibilityState::NONE
                        })
                        .actions(vec![AccessibilityAction::Click]),
                )
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .bg(if is_highlighted {
                    crate::rgb(0xe0f2fe)
                } else if is_selected {
                    crate::rgb(0xf8fafc)
                } else {
                    crate::rgba(0x00000000)
                })
                .hover(|style| style.bg(crate::rgb(0xf1f5f9)))
                .on_click(move |event, window, cx| {
                    if !event.standard_click() {
                        return;
                    }

                    commit_state.update(cx, |state, cx| {
                        state.commit_index(option_index, window, cx);
                    });
                })
                .child(div().child(option.label));

            if is_selected {
                row = row.child(div().text_color(crate::rgb(0x1d4ed8)).child("*"));
            }

            #[cfg(any(test, feature = "test-support"))]
            {
                let option_selector =
                    format!("select-option-{}-{}", self.selector_id, option_index);
                row = row.debug_selector(move || option_selector);
            }

            list = list.child(row);
        }

        panel.child(list).into_any_element()
    }
}

struct SelectSnapshot {
    focus_handle: FocusHandle,
    display_text: SharedString,
    showing_placeholder: bool,
    is_open: bool,
}

struct PopupSnapshot<T> {
    width: Pixels,
    searchable: bool,
    search_query: SharedString,
    highlighted_index: Option<usize>,
    selected_index: Option<usize>,
    options: Vec<SelectOption<T>>,
    filtered_indices: Vec<usize>,
}

fn build_layer_overlay(
    stack: Entity<LayerStack>,
    origin: Option<Point<Pixels>>,
    window: &mut Window,
) -> AnyElement {
    let viewport = window.viewport_size();
    let origin = origin.unwrap_or(Point::default());
    div()
        .absolute()
        .top(-origin.y)
        .left(-origin.x)
        .w(viewport.width)
        .h(viewport.height)
        .child(stack)
        .into_any_element()
}

fn filter_option_indices<T>(options: &[SelectOption<T>], query: &str) -> Vec<usize> {
    let needle = query.trim().to_lowercase();
    options
        .iter()
        .enumerate()
        .filter_map(|(index, option)| {
            if needle.is_empty() || option.label.to_string().to_lowercase().contains(&needle) {
                Some(index)
            } else {
                None
            }
        })
        .collect()
}

fn default_highlighted_index(filtered: &[usize], selected_index: Option<usize>) -> Option<usize> {
    selected_index
        .filter(|selected_index| filtered.contains(selected_index))
        .or_else(|| filtered.first().copied())
}

fn next_highlighted_index(
    filtered: &[usize],
    current: Option<usize>,
    delta: isize,
) -> Option<usize> {
    if filtered.is_empty() {
        return None;
    }

    let current_position = current
        .and_then(|current| filtered.iter().position(|index| *index == current))
        .unwrap_or(0);
    let next_position = (current_position as isize + delta).rem_euclid(filtered.len() as isize);
    filtered.get(next_position as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessibilityRole, AccessibilityState, AccessibilityValue, Context, Modifiers, Render,
        TestAppContext, Undo,
    };

    struct SelectView {
        value: &'static str,
    }

    struct PlainSelectView {
        value: &'static str,
    }

    impl Render for SelectView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            select(
                "pet",
                self.value,
                [("cat", "Cat"), ("dog", "Dog"), ("eel", "Eel")],
            )
            .placeholder("Choose a pet")
            .searchable()
            .on_change(cx.listener(|this, value, _, cx| {
                this.value = *value;
                cx.notify();
            }))
        }
    }

    impl Render for PlainSelectView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            select(
                "pet_plain",
                self.value,
                [("cat", "Cat"), ("dog", "Dog"), ("eel", "Eel")],
            )
            .placeholder("Choose a pet")
            .on_change(cx.listener(|this, value, _, cx| {
                this.value = *value;
                cx.notify();
            }))
        }
    }

    #[test]
    fn filter_option_indices_matches_case_insensitive_labels() {
        let options = vec![
            SelectOption::new(1, "Alpha"),
            SelectOption::new(2, "Beta"),
            SelectOption::new(3, "Gamma"),
        ];

        assert_eq!(filter_option_indices(&options, "a"), vec![0, 1, 2]);
        assert_eq!(filter_option_indices(&options, "TA"), vec![1]);
        assert_eq!(filter_option_indices(&options, "z"), Vec::<usize>::new());
    }

    #[test]
    fn next_highlighted_index_wraps_filtered_options() {
        let filtered = vec![1, 4, 7];

        assert_eq!(next_highlighted_index(&filtered, Some(7), 1), Some(1));
        assert_eq!(next_highlighted_index(&filtered, Some(1), -1), Some(7));
        assert_eq!(next_highlighted_index(&filtered, None, 1), Some(4));
    }

    #[crate::test]
    fn select_click_opens_and_commits_option(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| SelectView { value: "cat" });

        window.update(|window, cx| {
            window.draw(cx).clear();

            let combo = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .unwrap();
            assert!(combo.states.contains(AccessibilityState::COLLAPSED));
            assert_eq!(
                combo.value,
                Some(AccessibilityValue::Text("Cat".to_string()))
            );
        });

        let trigger_bounds = window.debug_bounds("select-pet").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let combo = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .unwrap();
            assert!(combo.states.contains(AccessibilityState::EXPANDED));
        });

        assert!(window.debug_bounds("select-popup-pet").is_some());

        let option_bounds = window.debug_bounds("select-option-pet-1").unwrap();
        window.simulate_click(option_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            assert_eq!(view.read(cx).value, "dog");
            let combo = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .unwrap();
            assert!(combo.states.contains(AccessibilityState::COLLAPSED));
            assert_eq!(
                combo.value,
                Some(AccessibilityValue::Text("Dog".to_string()))
            );
        });
    }

    #[crate::test]
    fn select_undo_redo_tracks_shared_history(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| PlainSelectView { value: "cat" });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!window.is_action_available(&Undo, cx));
        });

        let trigger_bounds = window.debug_bounds("select-pet_plain").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let option_bounds = window.debug_bounds("select-option-pet_plain-1").unwrap();
        window.simulate_click(option_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, "dog");
            assert!(window.is_action_available(&Undo, cx));
        });

        window.simulate_keystrokes("cmd-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, "cat");
        });

        window.simulate_keystrokes("cmd-shift-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).value, "dog");
        });
    }
}
