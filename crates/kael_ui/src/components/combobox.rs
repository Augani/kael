//! Combobox component - Searchable dropdown combining Input and Select functionality.
//!
//! Features:
//! - Searchable input with live filtering
//! - Keyboard navigation (Up/Down arrows, Enter, Escape)
//! - Custom render functions for items
//! - Multiple selection mode
//! - Virtual scrolling for large lists
//! - Clear button to reset selection
//! - Full Styled trait support for customization

use crate::components::{icon::Icon, input::InputSize};
use crate::theme::Theme;
use kael::{prelude::*, *};

actions!(
    combobox,
    [
        ComboboxUp,
        ComboboxDown,
        ComboboxConfirm,
        ComboboxCancel,
        ComboboxClear
    ]
);

const DROPDOWN_MARGIN: Pixels = px(4.0);

/// Events emitted by the Combobox
#[derive(Clone, Debug)]
pub enum ComboboxEvent {
    Change,
    Search,
}

/// Combobox state for managing selections and search
pub struct ComboboxState<T: Clone + 'static> {
    pub selected: Vec<T>,
    pub search_text: String,
    pub is_open: bool,
    pub focused_index: Option<usize>,
    pub scroll_handle: ScrollHandle,
}

impl<T: Clone + 'static> ComboboxState<T> {
    pub fn new() -> Self {
        Self {
            selected: Vec::new(),
            search_text: String::new(),
            is_open: false,
            focused_index: None,
            scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn with_selected(mut self, item: T) -> Self {
        self.selected.push(item);
        self
    }

    pub fn toggle_open(&mut self) {
        self.is_open = !self.is_open;
        if !self.is_open {
            self.search_text.clear();
            self.focused_index = None;
        }
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.search_text.clear();
        self.focused_index = None;
    }

    pub fn select_item(&mut self, item: T, multi_select: bool)
    where
        T: PartialEq,
    {
        self.select_item_by(item, multi_select, PartialEq::eq);
    }

    /// Select or toggle an item using a caller-provided equality predicate.
    ///
    /// This is useful for model types that intentionally do not implement
    /// [`PartialEq`]. Most callers should use [`Self::select_item_eq`].
    pub fn select_item_by(&mut self, item: T, multi_select: bool, equals: impl Fn(&T, &T) -> bool) {
        if multi_select {
            if let Some(index) = self
                .selected
                .iter()
                .position(|selected| equals(selected, &item))
            {
                self.selected.remove(index);
            } else {
                self.selected.push(item);
            }
        } else {
            self.selected = vec![item];
            self.is_open = false;
            self.search_text.clear();
            self.focused_index = None;
        }
    }

    /// Select or toggle an item using [`PartialEq`].
    pub fn select_item_eq(&mut self, item: T, multi_select: bool)
    where
        T: PartialEq,
    {
        self.select_item_by(item, multi_select, PartialEq::eq);
    }

    pub fn clear(&mut self) {
        self.selected.clear();
        self.search_text.clear();
    }

    pub fn is_selected(&self, item: &T) -> bool
    where
        T: PartialEq,
    {
        self.selected.iter().any(|s| s == item)
    }
}

impl<T: Clone + 'static> Default for ComboboxState<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Combobox component with searchable dropdown
pub struct Combobox<T: Clone + 'static> {
    focus_handle: FocusHandle,
    focus_subscription: Option<(AnyWindowHandle, Subscription)>,
    items: Vec<T>,
    option_accessibility_ids: Option<AccessibilityIdRange>,
    state: Entity<ComboboxState<T>>,
    placeholder: SharedString,
    disabled: bool,
    clearable: bool,
    multi_select: bool,
    size: InputSize,
    max_height: Pixels,

    filter_fn: Box<dyn Fn(&T, &str) -> bool>,
    render_item: Box<dyn Fn(&T) -> SharedString>,
    render_selected: Option<Box<dyn Fn(&[T]) -> SharedString>>,

    on_select: Option<Box<dyn Fn(&T, &mut Window, &mut App)>>,
    on_search: Option<Box<dyn Fn(&str, &mut App)>>,

    bounds: Bounds<Pixels>,
    dropdown_bounds: Bounds<Pixels>,

    style: StyleRefinement,
}

impl<T: Clone + PartialEq + 'static> Combobox<T> {
    /// Create a new combobox with items and state entity
    pub fn new(items: Vec<T>, state: &Entity<ComboboxState<T>>, cx: &mut Context<Self>) -> Self {
        let option_accessibility_ids = (!items.is_empty()).then(|| {
            AccessibilityId::reserve_range(items.len() as u64)
                .expect("accessibility identifier space exhausted")
        });
        Self {
            focus_handle: cx.focus_handle(),
            focus_subscription: None,
            items,
            option_accessibility_ids,
            state: state.clone(),
            placeholder: "Search...".into(),
            disabled: false,
            clearable: true,
            multi_select: false,
            size: InputSize::default(),
            max_height: px(300.0),
            filter_fn: Box::new(|_, _| true),
            render_item: Box::new(|_| "Item".into()),
            render_selected: None,
            on_select: None,
            on_search: None,
            bounds: Bounds::default(),
            dropdown_bounds: Bounds::default(),
            style: StyleRefinement::default(),
        }
    }

    /// Set the placeholder text
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Set disabled state
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Enable/disable clear button
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Enable multiple selection mode
    pub fn multi_select(mut self, multi_select: bool) -> Self {
        self.multi_select = multi_select;
        self
    }

    /// Set the control size using the Astryx input height scale.
    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    /// Set maximum dropdown height
    pub fn max_height(mut self, height: impl Into<Pixels>) -> Self {
        self.max_height = height.into();
        self
    }

    /// Set custom filter function
    ///
    /// # Example
    /// ```rust,ignore
    /// combobox.filter_fn(|item, search| {
    ///     item.name.to_lowercase().contains(&search.to_lowercase())
    /// })
    /// ```
    pub fn filter_fn<F>(mut self, f: F) -> Self
    where
        F: Fn(&T, &str) -> bool + 'static,
    {
        self.filter_fn = Box::new(f);
        self
    }

    /// Set custom item render function
    ///
    /// # Example
    /// ```rust,ignore
    /// combobox.render_item(|item| item.name.clone().into())
    /// ```
    pub fn render_item<F>(mut self, f: F) -> Self
    where
        F: Fn(&T) -> SharedString + 'static,
    {
        self.render_item = Box::new(f);
        self
    }

    /// Set custom selected items render function
    pub fn render_selected<F>(mut self, f: F) -> Self
    where
        F: Fn(&[T]) -> SharedString + 'static,
    {
        self.render_selected = Some(Box::new(f));
        self
    }

    /// Set callback when item is selected
    pub fn on_select<F>(mut self, callback: F) -> Self
    where
        F: Fn(&T, &mut Window, &mut App) + 'static,
    {
        self.on_select = Some(Box::new(callback));
        self
    }

    /// Set callback when search text changes
    pub fn on_search<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, &mut App) + 'static,
    {
        self.on_search = Some(Box::new(callback));
        self
    }

    /// Get filtered items based on current search text
    fn filtered_items(&self, cx: &App) -> Vec<(usize, &T)> {
        let state = self.state.read(cx);
        let search = state.search_text.to_lowercase();

        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| (self.filter_fn)(item, &search))
            .collect()
    }

    /// Toggle dropdown open/close
    fn toggle_dropdown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.state.read(cx).is_open {
            self.close_dropdown(cx);
            return;
        }
        let search = self.state.read(cx).search_text.to_lowercase();
        let has_items = self
            .items
            .iter()
            .any(|item| (self.filter_fn)(item, &search));
        window.focus(&self.focus_handle);
        self.state.update(cx, |state, cx| {
            state.is_open = true;
            state.focused_index = has_items.then_some(0);
            state.scroll_handle.set_offset(point(px(0.0), px(0.0)));
            if has_items {
                state.scroll_handle.scroll_to_item(0);
            }
            cx.notify();
        });
    }

    /// Close the dropdown
    fn close_dropdown(&mut self, cx: &mut Context<Self>) {
        self.dropdown_bounds = Bounds::default();
        self.state.update(cx, |state, cx| {
            state.close();
            cx.notify(); // Trigger re-render
        });
    }

    /// Clear all selections
    fn clear_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.state.read(cx).selected.is_empty() {
            return;
        }
        let has_items = self.items.iter().any(|item| (self.filter_fn)(item, ""));
        self.state.update(cx, |state, cx| {
            state.clear();
            if state.is_open {
                state.focused_index = has_items.then_some(0);
            }
            cx.notify(); // Trigger re-render
        });
        self.scroll_focused_into_view(cx);
        cx.emit(ComboboxEvent::Change);
    }

    /// Select an item
    fn select_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(&(original_idx, _)) = self.filtered_items(cx).get(index) {
            self.select_original_item(original_idx, window, cx);
        }
    }

    /// Pointer and native callbacks retain the original item's identity across filtering.
    fn select_original_item(
        &mut self,
        original_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.state.read(cx);
        if self.disabled || !state.is_open {
            return;
        }
        let search = state.search_text.to_lowercase();
        if let Some(item) = self.items.get(original_idx)
            && (self.filter_fn)(item, &search)
        {
            let item = item.clone();
            self.state.update(cx, |state, cx| {
                state.select_item(item.clone(), self.multi_select);
                cx.notify(); // Trigger re-render
            });

            cx.emit(ComboboxEvent::Change);

            if let Some(ref callback) = self.on_select {
                (callback)(&item, window, cx);
            }
        }
    }

    /// Handle up arrow key
    fn combobox_up(&mut self, _: &ComboboxUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if !self.state.read(cx).is_open {
            self.toggle_dropdown(window, cx);
            return;
        }
        let filtered_count = self.filtered_items(cx).len();
        if filtered_count == 0 {
            return;
        }

        self.state.update(cx, |state, cx| {
            state.focused_index = Some(match state.focused_index {
                Some(0) | None => filtered_count - 1,
                Some(current) => current.saturating_sub(1).min(filtered_count - 1),
            });
            cx.notify();
        });
        self.scroll_focused_into_view(cx);
    }

    /// Handle down arrow key
    fn combobox_down(&mut self, _: &ComboboxDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if !self.state.read(cx).is_open {
            self.toggle_dropdown(window, cx);
            return;
        }
        let filtered_count = self.filtered_items(cx).len();
        if filtered_count == 0 {
            return;
        }

        self.state.update(cx, |state, cx| {
            state.focused_index = Some(match state.focused_index {
                Some(current) if current < filtered_count - 1 => current + 1,
                _ => 0,
            });
            cx.notify();
        });
        self.scroll_focused_into_view(cx);
    }

    /// Reveal against this frame's measured rows, including size and wrapped labels.
    fn scroll_focused_into_view(&self, cx: &Context<Self>) {
        let state = self.state.read(cx);
        if !self.disabled
            && state.is_open
            && let Some(position) = state.focused_index
        {
            state.scroll_handle.scroll_to_item(position);
        }
    }

    /// Handle enter key (confirm selection)
    fn combobox_confirm(
        &mut self,
        _: &ComboboxConfirm,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        let state = self.state.read(cx);
        if state.is_open {
            if let Some(idx) = state.focused_index {
                self.select_item(idx, window, cx);
            }
        } else {
            self.toggle_dropdown(window, cx);
        }
    }

    /// Handle escape key (cancel/close)
    fn combobox_cancel(&mut self, _: &ComboboxCancel, _: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && self.state.read(cx).is_open {
            self.close_dropdown(cx);
        }
    }

    /// Handle clear action
    fn combobox_clear(&mut self, _: &ComboboxClear, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_selection(window, cx);
    }
}

impl<T: Clone + 'static> Styled for Combobox<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + PartialEq + 'static> Render for Combobox<T> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.disabled && self.focus_handle.is_focused(window) {
            window.blur();
            if self.state.read(cx).is_open {
                self.close_dropdown(cx);
            }
        }
        let window_handle = window.window_handle();
        if self.focus_subscription.as_ref().map(|(handle, _)| *handle) != Some(window_handle) {
            self.focus_subscription = Some((
                window_handle,
                cx.on_blur(&self.focus_handle, window, |this, _, cx| {
                    if this.state.read(cx).is_open {
                        this.close_dropdown(cx);
                    }
                }),
            ));
        }
        let theme = Theme::of(cx);
        let user_style = self.style.clone();

        let state = self.state.read(cx);
        let is_open = state.is_open && !self.disabled;
        let search_text = state.search_text.clone();
        let focused_idx = state.focused_index;
        let has_selection = !state.selected.is_empty();
        let entity_id = cx.entity().entity_id().as_u64();
        let is_focused = self.focus_handle.is_focused(window);
        let filtered = if is_open {
            self.filtered_items(cx)
        } else {
            Vec::new()
        };

        let display_text: SharedString = if !search_text.is_empty() {
            search_text.clone().into()
        } else if !state.selected.is_empty() {
            if let Some(ref render_fn) = self.render_selected {
                render_fn(&state.selected)
            } else if self.multi_select {
                format!("{} selected", state.selected.len()).into()
            } else {
                (self.render_item)(&state.selected[0])
            }
        } else {
            self.placeholder.clone()
        };

        let bounds = self.bounds;
        let is_searching = !search_text.is_empty();
        let hover_ring = crate::astryx::input_hover_ring(theme.tokens.input);
        let focus_ring = crate::astryx::focus_ring(theme.tokens.primary);
        let (height, padding_x, text_size, icon_size) = match self.size {
            InputSize::Sm => (px(28.0), px(8.0), px(14.0), px(16.0)),
            InputSize::Md => (px(32.0), px(8.0), px(14.0), px(16.0)),
            InputSize::Lg => (px(36.0), px(8.0), px(14.0), px(16.0)),
        };
        let item_padding_y = match self.size {
            InputSize::Sm => px(4.0),
            InputSize::Md => px(6.0),
            InputSize::Lg => px(8.0),
        };

        let mut accessibility_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if self.disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::ComboBox)
            .label("Combobox")
            .placeholder(self.placeholder.to_string())
            .states(accessibility_state);
        if has_selection || is_searching {
            accessibility = accessibility.value(AccessibilityValue::Text(display_text.to_string()));
        }
        if !self.disabled {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                if is_open {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ]);
        }
        if is_open
            && let Some(active_id) =
                focused_idx
                    .and_then(|index| filtered.get(index))
                    .and_then(|(original_index, _)| {
                        self.option_accessibility_ids
                            .and_then(|ids| ids.get(*original_index as u64))
                    })
        {
            accessibility = accessibility.active_descendant(active_id);
        }

        let trigger = div()
            .id(("combobox-trigger", entity_id))
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .h(height)
            .px(padding_x)
            .bg(theme.tokens.card)
            .border_1()
            .border_color(if is_open {
                theme.tokens.primary
            } else {
                theme.tokens.input
            })
            .rounded(theme.tokens.radius_md)
            .text_color(if has_selection && !is_searching {
                theme.tokens.foreground
            } else {
                theme.tokens.muted_foreground
            })
            .text_size(text_size)
            .font_family(theme.tokens.font_family.clone())
            .cursor(if self.disabled {
                CursorStyle::Arrow
            } else {
                CursorStyle::PointingHand
            })
            .transition(theme.tokens.transition_fast)
            .shadow(smallvec::smallvec![crate::astryx::focus_ring(
                kael::transparent_black()
            )])
            .when(is_open && !self.disabled, |div| {
                div.shadow(smallvec::smallvec![focus_ring])
            })
            .when(!self.disabled, |div: Stateful<Div>| {
                div.hover(move |style| {
                    style
                        .border_color(theme.tokens.input)
                        .shadow(smallvec::smallvec![hover_ring])
                })
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !this.disabled {
                        window.focus(&this.focus_handle);
                        if !this.state.read(cx).is_open {
                            this.toggle_dropdown(window, cx);
                        }
                    } else {
                        // A previously painted enabled owner may still have
                        // its default pointer-focus listener until redraw.
                        window.prevent_default();
                    }
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(display_text),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .when(
                        self.clearable && has_selection && !self.disabled,
                        |this_div| {
                            this_div.child(
                                div()
                                    .size(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(theme.tokens.radius_sm)
                                    .cursor(CursorStyle::PointingHand)
                                    .hover(|style| {
                                        style.bg(crate::astryx::overlay_hover(
                                            theme.tokens.background.l < 0.5,
                                        ))
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _event, window, cx| {
                                            this.clear_selection(window, cx);
                                            cx.stop_propagation();
                                        }),
                                    )
                                    .on_accessibility_action(
                                        AccessibilityAction::Click,
                                        cx.listener(|this, _, window, cx| {
                                            this.clear_selection(window, cx);
                                        }),
                                    )
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Button)
                                            .label("Clear selection")
                                            .actions(vec![AccessibilityAction::Click]),
                                    )
                                    .child(
                                        Icon::new("x")
                                            .size(icon_size)
                                            .color(theme.tokens.muted_foreground),
                                    ),
                            )
                        },
                    )
                    .child(
                        div()
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(if is_open {
                                    "chevron-up"
                                } else {
                                    "chevron-down"
                                })
                                .size(icon_size)
                                .color(theme.tokens.muted_foreground),
                            ),
                    ),
            )
            .child({
                let entity = cx.entity().clone();
                canvas_with_prepaint(
                    move |bounds, _, cx| {
                        entity.update(cx, |this, _| {
                            this.bounds = bounds;
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            });

        div()
            .id(("combobox-control", entity_id))
            .relative()
            .w_full()
            .key_context("Combobox")
            .accessibility(accessibility)
            .when(!self.disabled, |this: Stateful<Div>| {
                this.track_focus(&self.focus_handle.clone().tab_index(0).tab_stop(true))
                    .on_action(cx.listener(Combobox::combobox_up))
                    .on_action(cx.listener(Combobox::combobox_down))
                    .on_action(cx.listener(Combobox::combobox_confirm))
                    .on_action(cx.listener(Combobox::combobox_clear))
                    .on_accessibility_action(
                        AccessibilityAction::Focus,
                        cx.listener(|this, _, window, _| {
                            if !this.disabled {
                                window.focus(&this.focus_handle);
                            }
                        }),
                    )
                    .on_accessibility_action(
                        AccessibilityAction::Click,
                        cx.listener(|this, _, window, cx| this.toggle_dropdown(window, cx)),
                    )
                    .on_accessibility_action(
                        AccessibilityAction::Expand,
                        cx.listener(|this, _, window, cx| {
                            if !this.state.read(cx).is_open {
                                this.toggle_dropdown(window, cx);
                            }
                        }),
                    )
                    .on_accessibility_action(
                        AccessibilityAction::Collapse,
                        cx.listener(|this, _, _, cx| {
                            if !this.disabled && this.state.read(cx).is_open {
                                this.close_dropdown(cx);
                            }
                        }),
                    )
            })
            .when(is_open, |this: Stateful<Div>| {
                this.on_action(cx.listener(Combobox::combobox_cancel))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if this.disabled || !this.focus_handle.is_focused(window) {
                        return;
                    }
                    let mut search = this.state.read(cx).search_text.clone();
                    if event.keystroke.key == "backspace" {
                        search.pop();
                    } else if event.keystroke.key.len() == 1
                        && !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.platform
                        && !event.keystroke.modifiers.alt {
                        search.push_str(&event.keystroke.key);
                    } else {
                        return;
                    }
                    let query = search.to_lowercase();
                    let has_items = this.items.iter().any(|item| (this.filter_fn)(item, &query));
                    this.state.update(cx, |state, cx| {
                        state.search_text = search.clone();
                        state.focused_index = has_items.then_some(0);
                        state.scroll_handle.set_offset(point(px(0.0), px(0.0)));
                        cx.notify();
                    });
                    this.scroll_focused_into_view(cx);
                    cx.emit(ComboboxEvent::Search);
                    if let Some(ref callback) = this.on_search {
                        callback(&search, cx);
                    }
                    cx.stop_propagation();
                    window.prevent_default();
                }))
            })
            .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                if !this.disabled && this.state.read(cx).is_open && !this.dropdown_bounds.contains(&event.position) {
                    this.close_dropdown(cx);
                }
            }))
            .child(trigger)
            .when(is_open, |this| {
                this.child(
                    deferred(
                        anchored()
                            .snap_to_window_with_margin(Edges::all(DROPDOWN_MARGIN))
                            .child(
                                div()
                                    .relative()
                                    .occlude()
                                    .w(bounds.size.width)
                                    .child({
                                        let entity = cx.weak_entity();
                                        canvas_with_prepaint(
                                            move |bounds, _, cx| {
                                                _ = entity.update(cx, |this, _| {
                                                    this.dropdown_bounds = bounds;
                                                });
                                            },
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .size_full()
                                    })
                                    .child(
                                        div()
                                            .occlude()
                                            .mt(DROPDOWN_MARGIN)
                                            .bg(theme.tokens.popover)
                                            .border_1()
                                            .border_color(theme.tokens.border)
                                            .rounded(theme.tokens.radius_lg)
                                            .shadow(theme.tokens.shadow_lg.to_vec())
                                            .overflow_hidden()
                                            .child({
                                                div()
                                                    .id(("combobox-dropdown-list", entity_id))
                                                    .accessibility(
                                                        AccessibilityAttributes::new(
                                                            AccessibilityRole::List,
                                                        )
                                                        .label("Options"),
                                                    )
                                                    .max_h(self.max_height)
                                                    .overflow_y_scroll()
                                                    .track_scroll(&self.state.read(cx).scroll_handle)
                                                    .py(px(4.0))
                                                    .when(filtered.is_empty(), |this| {
                                                        this.child(
                                                            div()
                                                                .px(px(12.0))
                                                                .py(px(16.0))
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .text_size(px(13.0))
                                                                .font_family(theme.tokens.font_family.clone())
                                                                .text_color(theme.tokens.muted_foreground)
                                                                .child("No results found")
                                                        )
                                                    })
                                                    .when(!filtered.is_empty(), |this| {
                                                        this.children(
                                                            filtered.iter().enumerate().map(|(display_idx, &(original_idx, item))| {
                                                                let is_focused = focused_idx == Some(display_idx);
                                                                let is_selected = state.is_selected(item);
                                                                let item_text = (self.render_item)(item);
                                                                let mut item_state = AccessibilityState::NONE;
                                                                if is_selected {
                                                                    item_state |= AccessibilityState::SELECTED;
                                                                }

                                                                div()
                                                                    .id(ElementId::NamedInteger(
                                                                        format!("combobox-option-{entity_id}").into(),
                                                                        original_idx as u64,
                                                                    ))
                                                                    .accessibility(
                                                                        AccessibilityAttributes::new(AccessibilityRole::ListItem)
                                                                            .id(self.option_accessibility_ids
                                                                                .and_then(|ids| ids.get(original_idx as u64))
                                                                                .expect("each item has a stable accessibility identity"))
                                                                            .label(item_text.to_string())
                                                                            .states(item_state)
                                                                            .actions(vec![AccessibilityAction::Click]),
                                                                    )
                                                                    .px(px(12.0))
                                                                    .py(item_padding_y)
                                                                    .flex()
                                                                    .items_center()
                                                                    .justify_between()
                                                                    .gap(px(8.0))
                                                                    .rounded(theme.tokens.radius_md)
                                                                    .text_color(theme.tokens.popover_foreground)
                                                                    .bg(if is_focused {
                                                                        crate::astryx::overlay_hover(theme.tokens.background.l < 0.5)
                                                                    } else if is_selected {
                                                                        theme.tokens.primary.opacity(0.12)
                                                                    } else {
                                                                        kael::transparent_black()
                                                                    })
                                                                    .hover(|style| {
                                                                        style.bg(crate::astryx::overlay_hover(
                                                                            theme.tokens.background.l < 0.5,
                                                                        ))
                                                                    })
                                                                    .cursor(CursorStyle::PointingHand)
                                                                    .text_size(text_size)
                                                                    .when(is_selected, |this| {
                                                                        this.font_weight(FontWeight::MEDIUM)
                                                                    })
                                                                    .font_family(theme.tokens.font_family.clone())
                                                                    .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, window, cx| {
                                                                        this.select_original_item(original_idx, window, cx);
                                                                    }))
                                                                    .on_accessibility_action(AccessibilityAction::Click, cx.listener(move |this, _, window, cx| {
                                                                        this.select_original_item(original_idx, window, cx);
                                                                    }))
                                                                    .child(div().flex_1().min_w(px(0.0)).child(item_text))
                                                                    .child(
                                                                        div()
                                                                            .size(px(16.0))
                                                                            .flex()
                                                                            .items_center()
                                                                            .justify_center()
                                                                            .flex_shrink_0()
                                                                            .when(is_selected, |slot| {
                                                                                slot.child(
                                                                                    Icon::new("check")
                                                                                        .size(px(16.0))
                                                                                        .color(theme.tokens.primary)
                                                                                )
                                                                            })
                                                                    )
                                                            })
                                                        )
                                                    })
                                            })
                                    )
                            )
                    )
                    .with_priority(1)
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

/// Initialize combobox key bindings
pub fn init_combobox(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", ComboboxUp, Some("Combobox")),
        KeyBinding::new("down", ComboboxDown, Some("Combobox")),
        KeyBinding::new("enter", ComboboxConfirm, Some("Combobox")),
        KeyBinding::new("escape", ComboboxCancel, Some("Combobox")),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-k", ComboboxClear, Some("Combobox")),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-k", ComboboxClear, Some("Combobox")),
    ]);
}

impl<T: Clone + 'static> Focusable for Combobox<T> {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl<T: Clone + 'static> EventEmitter<ComboboxEvent> for Combobox<T> {}

#[cfg(test)]
mod tests {
    use super::{Combobox, ComboboxEvent, ComboboxState, init_combobox};
    use crate::components::input::InputSize;
    use kael::{
        AccessibilityAction, AccessibilityActionRequest, AccessibilityId, AccessibilityRole,
        AccessibilityState, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
        KeyUpEvent, Keystroke, Modifiers, ParentElement as _, Render, ScrollDelta,
        ScrollWheelEvent, StatefulInteractiveElement as _, Styled as _, TestAppContext,
        VisualTestContext, Window, div, point, px,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    struct ComboboxHost {
        combobox: Entity<Combobox<&'static str>>,
    }

    impl Render for ComboboxHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(240.0)).child(self.combobox.clone())
        }
    }

    fn mounted_combobox() -> (
        Entity<ComboboxState<&'static str>>,
        VisualTestContext,
        Rc<Cell<usize>>,
    ) {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            init_combobox(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(|_| ComboboxState::new());
        let callbacks = Rc::new(Cell::new(0));
        let combobox = cx.new({
            let state = state.clone();
            let callbacks = callbacks.clone();
            move |cx| {
                Combobox::new(vec!["Alpha", "Beta"], &state, cx)
                    .render_item(|item| (*item).into())
                    .on_select(move |_, _, _| callbacks.set(callbacks.get() + 1))
            }
        });
        let (_, window) = cx.add_window_view(move |_, _| ComboboxHost { combobox });
        window.update(|window, cx| window.draw(cx).clear());
        (state, window.clone(), callbacks)
    }

    fn exposed_node(window: &Window, role: AccessibilityRole, label: &str) -> AccessibilityId {
        window
            .accessibility_tree()
            .nodes
            .values()
            .find(|node| node.role == role && node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing {role:?} named {label}"))
            .id
    }

    fn open_with_pointer(window: &mut VisualTestContext) {
        let center = window.update(|window, _| {
            let id = exposed_node(window, AccessibilityRole::ComboBox, "Combobox");
            let bounds = window.accessibility_tree().get(id).unwrap().bounds.unwrap();
            point(
                px((bounds.x + bounds.width / 2.0) as f32),
                px((bounds.y + bounds.height / 2.0) as f32),
            )
        });
        window.simulate_click(center, Modifiers::default());
        window.update(|window, cx| window.draw(cx).clear());
    }

    struct IndexedComboboxHost {
        combobox: Entity<Combobox<usize>>,
        next_focus: kael::FocusHandle,
        cache_control: bool,
    }

    impl Render for IndexedComboboxHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(240.0))
                .child(if self.cache_control {
                    kael::AnyView::from(self.combobox.clone())
                        .cached(kael::StyleRefinement::default().w_full().h(px(32.0)))
                        .into_any_element()
                } else {
                    self.combobox.clone().into_any_element()
                })
                .child(
                    div()
                        .id("after-combobox")
                        .h(px(28.0))
                        .track_focus(&self.next_focus.clone().tab_index(0).tab_stop(true))
                        .accessibility(
                            kael::AccessibilityAttributes::new(AccessibilityRole::Button)
                                .label("Next control")
                                .actions(vec![
                                    AccessibilityAction::Focus,
                                    AccessibilityAction::Click,
                                ]),
                        )
                        .on_click(|_, _, _| {})
                        .child("Next control"),
                )
        }
    }

    #[derive(Default)]
    struct ComboboxCounts {
        selected: RefCell<Vec<usize>>,
        changes: Cell<usize>,
        searches: Cell<usize>,
        search_callbacks: Cell<usize>,
        filter_calls: Cell<usize>,
    }

    struct IndexedComboboxFixture {
        state: Entity<ComboboxState<usize>>,
        combobox: Entity<Combobox<usize>>,
        window: VisualTestContext,
        counts: Rc<ComboboxCounts>,
        next_focus: kael::FocusHandle,
        _subscription: kael::Subscription,
    }

    fn choice_label(index: usize) -> String {
        format!("Choice {index:02} with a longer label that wraps within the options viewport")
    }

    fn indexed_combobox(size: InputSize, multi_select: bool) -> IndexedComboboxFixture {
        indexed_combobox_with_cache(size, multi_select, false)
    }

    fn indexed_combobox_with_cache(
        size: InputSize,
        multi_select: bool,
        cache_control: bool,
    ) -> IndexedComboboxFixture {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            init_combobox(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(|_| ComboboxState::new());
        let counts = Rc::new(ComboboxCounts::default());
        let combobox = cx.new({
            let state = state.clone();
            let counts = counts.clone();
            move |cx| {
                let search_counts = counts.clone();
                let filter_counts = counts.clone();
                Combobox::new((0..40).collect(), &state, cx)
                    .size(size)
                    .max_height(px(180.0))
                    .multi_select(multi_select)
                    .render_item(|index| choice_label(*index).into())
                    .filter_fn(move |index, query| {
                        filter_counts
                            .filter_calls
                            .set(filter_counts.filter_calls.get() + 1);
                        format!("choice {index:02}").contains(query)
                    })
                    .on_select(move |item, _, _| counts.selected.borrow_mut().push(*item))
                    .on_search(move |_, _| {
                        search_counts
                            .search_callbacks
                            .set(search_counts.search_callbacks.get() + 1)
                    })
            }
        });
        let subscription = cx.update(|cx| {
            let counts = counts.clone();
            cx.subscribe(&combobox, move |_, event: &ComboboxEvent, _| match event {
                ComboboxEvent::Change => counts.changes.set(counts.changes.get() + 1),
                ComboboxEvent::Search => counts.searches.set(counts.searches.get() + 1),
            })
        });
        let next_focus = cx.update(|cx| cx.focus_handle());
        let (_, window) = cx.add_window_view({
            let combobox = combobox.clone();
            let next_focus = next_focus.clone();
            move |_, _| IndexedComboboxHost {
                combobox,
                next_focus,
                cache_control,
            }
        });
        window.deactivate_window();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        window.update(|window, cx| {
            assert!(
                window.is_window_active(),
                "focus lifecycle requires an active test window"
            );
            window.draw(cx).clear();
        });
        IndexedComboboxFixture {
            state,
            combobox,
            window: window.clone(),
            counts,
            next_focus,
            _subscription: subscription,
        }
    }

    fn assert_active_option_is_visible(
        combobox: &Entity<Combobox<usize>>,
        state: &Entity<ComboboxState<usize>>,
        window: &Window,
        cx: &kael::App,
    ) -> AccessibilityId {
        let combobox = combobox.read(cx);
        let state = state.read(cx);
        let position = state.focused_index.expect("highlighted option");
        let original_index = combobox.filtered_items(cx)[position].0;
        let id = combobox
            .option_accessibility_ids
            .unwrap()
            .get(original_index as u64)
            .unwrap();
        let tree = window.accessibility_tree();
        let option = tree.get(id).expect("active option must be mounted");
        let bounds = option.bounds.expect("painted option bounds");
        let viewport = state.scroll_handle.bounds();
        assert!(
            bounds.y >= f64::from(f32::from(viewport.top())) - 0.5
                && bounds.y + bounds.height <= f64::from(f32::from(viewport.bottom())) + 0.5,
            "actual option {original_index} bounds {bounds:?} must fit actual viewport {viewport:?}",
        );
        assert_eq!(tree.focused_node_count(), 1);
        let control = tree
            .get(exposed_node(
                window,
                AccessibilityRole::ComboBox,
                "Combobox",
            ))
            .unwrap();
        let list = tree
            .get(exposed_node(window, AccessibilityRole::List, "Options"))
            .unwrap();
        assert!(control.states.contains(AccessibilityState::FOCUSED));
        assert_eq!(control.active_descendant, Some(id));
        assert_eq!(option.parent, Some(list.id));
        assert_eq!(list.parent, Some(control.id));
        let native = tree.to_accesskit_tree_update(None, None);
        let native_control = native
            .nodes
            .iter()
            .find(|(id, _)| id.0 == control.id.0)
            .unwrap();
        assert_eq!(native_control.1.active_descendant().unwrap().0, id.0);
        id
    }

    fn native_action(
        window: &mut VisualTestContext,
        id: AccessibilityId,
        action: AccessibilityAction,
    ) {
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id, action,
            ));
        });
        window.run_until_parked();
        window.update(|window, cx| window.draw(cx).clear());
    }

    #[::core::prelude::v1::test]
    fn native_trigger_actions_open_mounted_combobox() {
        let mut opened = Vec::new();
        for action in [AccessibilityAction::Click, AccessibilityAction::Expand] {
            let (state, mut window, callbacks) = mounted_combobox();
            window.update(|window, _| {
                let id = exposed_node(window, AccessibilityRole::ComboBox, "Combobox");
                assert!(
                    window
                        .accessibility_tree()
                        .get(id)
                        .unwrap()
                        .actions
                        .contains(&action)
                );
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id, action,
                ));
            });
            window.run_until_parked();
            window.update(|window, cx| {
                window.draw(cx).clear();
                opened.push((action, state.read(cx).is_open));
            });
            assert_eq!(callbacks.get(), 0);
        }
        eprintln!("combobox_native_open_results={opened:?}");
        assert!(
            opened.iter().all(|(_, open)| *open),
            "advertised native actions must open the control: {opened:?}"
        );
    }

    #[::core::prelude::v1::test]
    fn closed_mounted_combobox_opens_with_keyboard() {
        let (state, mut window, callbacks) = mounted_combobox();
        window.simulate_keystrokes("tab enter");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        let keyboard_opened = window.update(|window, cx| {
            window.draw(cx).clear();
            state.read(cx).is_open
        });
        if !keyboard_opened {
            open_with_pointer(&mut window);
            assert!(
                window.update(|_, cx| state.read(cx).is_open),
                "pointer positive control must open"
            );
        }
        eprintln!("combobox_keyboard_opened={keyboard_opened}; pointer_positive_control=true");
        assert_eq!(callbacks.get(), 0);
        assert!(
            keyboard_opened,
            "a focused, closed combobox must open with Enter"
        );
    }

    #[::core::prelude::v1::test]
    fn native_option_and_clear_activate_mounted_combobox() {
        let (state, mut window, callbacks) = mounted_combobox();
        open_with_pointer(&mut window);
        assert!(window.update(|_, cx| state.read(cx).is_open));
        window.update(|window, _| {
            let id = exposed_node(window, AccessibilityRole::ListItem, "Beta");
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
        let native_selected = window.update(|window, cx| {
            window.draw(cx).clear();
            state.read(cx).selected == ["Beta"]
        });
        let native_selection_callbacks = callbacks.get();

        // Establish a selection independently so clear is tested even when option activation fails.
        window.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.select_item("Beta", false);
                cx.notify();
            });
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let id = exposed_node(window, AccessibilityRole::Button, "Clear selection");
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
        let native_cleared = window.update(|window, cx| {
            window.draw(cx).clear();
            state.read(cx).selected.is_empty()
        });
        eprintln!(
            "combobox_native_selection={native_selected}; callbacks={native_selection_callbacks}; native_clear={native_cleared}"
        );
        assert!(
            native_selected,
            "native option Click must select the same item as pointer activation"
        );
        assert_eq!(
            native_selection_callbacks, 1,
            "selection callback must run exactly once"
        );
        assert!(
            native_cleared,
            "native clear Click must clear the selection"
        );
    }

    #[::core::prelude::v1::test]
    fn native_disclosure_owns_popup_and_retires_option_actions() {
        let mut fixture = indexed_combobox(InputSize::Md, false);
        let (control, closed_bounds) = fixture.window.update(|window, _| {
            let id = exposed_node(window, AccessibilityRole::ComboBox, "Combobox");
            (id, window.accessibility_tree().get(id).unwrap().bounds)
        });
        native_action(&mut fixture.window, control, AccessibilityAction::Expand);
        let option = fixture.window.update(|window, cx| {
            assert_eq!(
                window.accessibility_tree().get(control).unwrap().bounds,
                closed_bounds
            );
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx)
        });
        native_action(&mut fixture.window, control, AccessibilityAction::Collapse);
        fixture.window.update(|window, cx| {
            assert!(!fixture.state.read(cx).is_open);
            assert!(!window.accessibility_tree().nodes.contains_key(&option));
            assert!(!window.has_accessibility_action_handler(option, AccessibilityAction::Click));
            assert_eq!(
                window
                    .accessibility_tree()
                    .get(control)
                    .unwrap()
                    .active_descendant,
                None
            );
        });
        native_action(&mut fixture.window, option, AccessibilityAction::Click);
        assert_eq!(fixture.counts.changes.get(), 0);
        assert!(fixture.counts.selected.borrow().is_empty());
        native_action(&mut fixture.window, control, AccessibilityAction::Click);
        fixture.window.update(|window, cx| {
            assert!(fixture.state.read(cx).is_open);
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
        });
    }

    #[::core::prelude::v1::test]
    fn filtered_native_option_preserves_original_identity_and_commits_once() {
        let mut fixture = indexed_combobox(InputSize::Md, false);
        fixture.window.simulate_keystrokes("tab enter up");
        let last_id = fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(fixture.state.read(cx).focused_index, Some(39));
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx)
        });
        fixture.window.simulate_keystrokes("3 9");
        fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(fixture.state.read(cx).search_text, "39");
            assert_eq!(fixture.combobox.read(cx).filtered_items(cx).len(), 1);
            assert_eq!(
                assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx),
                last_id
            );
            // Both requests are queued against the painted tree. Closing on the
            // first selection must also prevent a second callback before redraw.
            for _ in 0..2 {
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    last_id,
                    AccessibilityAction::Click,
                ));
            }
        });
        fixture.window.run_until_parked();
        fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(fixture.state.read(cx).selected, [39]);
            assert!(!fixture.state.read(cx).is_open);
            assert!(fixture.state.read(cx).search_text.is_empty());
            assert!(!window.has_accessibility_action_handler(last_id, AccessibilityAction::Click));
        });
        native_action(&mut fixture.window, last_id, AccessibilityAction::Click);
        assert_eq!(*fixture.counts.selected.borrow(), [39]);
        assert_eq!(fixture.counts.changes.get(), 1);
        assert_eq!(fixture.counts.searches.get(), 2);
        assert_eq!(fixture.counts.search_callbacks.get(), 2);
    }

    #[::core::prelude::v1::test]
    fn measured_reveal_uses_actual_wrapped_rows_for_all_sizes() {
        for size in [InputSize::Sm, InputSize::Md, InputSize::Lg] {
            let mut fixture = indexed_combobox(size, false);
            fixture.window.simulate_keystrokes("tab down up");
            fixture.window.update(|window, cx| {
                window.draw(cx).clear();
                assert_eq!(fixture.state.read(cx).focused_index, Some(39));
                assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
            });
            for index in 0..16 {
                fixture.window.simulate_keystrokes("down");
                fixture.window.update(|window, cx| {
                    window.draw(cx).clear();
                    assert_eq!(fixture.state.read(cx).focused_index, Some(index));
                    assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
                });
            }
            fixture.window.simulate_keystrokes("enter");
            fixture.window.simulate_event(KeyUpEvent {
                keystroke: Keystroke::parse("enter").unwrap(),
            });
            fixture.window.update(|_, cx| {
                assert_eq!(fixture.state.read(cx).selected, [15]);
                assert!(!fixture.state.read(cx).is_open);
            });
            assert_eq!(*fixture.counts.selected.borrow(), [15]);
            assert_eq!(fixture.counts.changes.get(), 1);
        }
    }

    #[::core::prelude::v1::test]
    fn pointer_wheel_keeps_popup_open_without_replaying_highlight_reveal() {
        let mut fixture = indexed_combobox(InputSize::Md, false);
        open_with_pointer(&mut fixture.window);
        let center = fixture
            .window
            .update(|_, cx| fixture.state.read(cx).scroll_handle.bounds().center());
        fixture
            .window
            .simulate_mouse_move(center, None, Modifiers::default());
        fixture.window.simulate_event(ScrollWheelEvent {
            position: center,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-240.0))),
            modifiers: Modifiers::default(),
            ..Default::default()
        });
        let scrolled = fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            let state = fixture.state.read(cx);
            assert!(state.is_open);
            assert_eq!(state.focused_index, Some(0));
            assert!(state.scroll_handle.offset().y < px(-100.0));
            state.scroll_handle.offset()
        });
        fixture.window.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear();
            assert_eq!(fixture.state.read(cx).scroll_handle.offset(), scrolled);
        });
        assert_eq!(fixture.counts.changes.get(), 0);
        assert!(fixture.counts.selected.borrow().is_empty());
    }

    #[::core::prelude::v1::test]
    fn disabled_model_rejects_painted_focus_handlers_and_releases_owned_focus() {
        let mut fixture = indexed_combobox(InputSize::Md, false);
        fixture.window.update(|window, cx| {
            fixture.state.update(cx, |state, cx| {
                state.selected = vec![7];
                cx.notify();
            });
            window.focus(&fixture.next_focus);
            window.draw(cx).clear();
        });
        let (control, center) = fixture.window.update(|window, cx| {
            let control = exposed_node(window, AccessibilityRole::ComboBox, "Combobox");
            let bounds = window
                .accessibility_tree()
                .get(control)
                .unwrap()
                .bounds
                .unwrap();
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                control,
                AccessibilityAction::Focus,
            ));
            fixture.combobox.update(cx, |combobox, _| {
                combobox.disabled = true;
            });
            assert!(
                !window
                    .accessibility_tree()
                    .get(control)
                    .unwrap()
                    .states
                    .contains(AccessibilityState::DISABLED),
                "queue against the old enabled tree before notifying the model change"
            );
            (
                control,
                point(
                    px((bounds.x + bounds.width / 2.0) as f32),
                    px((bounds.y + bounds.height / 2.0) as f32),
                ),
            )
        });
        fixture.window.run_until_parked();
        fixture.window.update(|window, cx| {
            assert!(
                fixture.next_focus.is_focused(window),
                "queued native Focus must check the current disabled model"
            );
            fixture.combobox.update(cx, |_, cx| cx.notify());
        });
        fixture.window.update(|_, cx| {
            fixture.combobox.update(cx, |combobox, cx| {
                combobox.disabled = false;
                cx.notify();
            });
        });
        fixture.window.update(|window, cx| {
            fixture
                .combobox
                .update(cx, |combobox, _| combobox.disabled = true);
            assert!(
                !window
                    .accessibility_tree()
                    .get(control)
                    .unwrap()
                    .states
                    .contains(AccessibilityState::DISABLED),
                "pointer must use the old enabled tree before notifying the model change"
            );
        });
        fixture.window.simulate_click(center, Modifiers::default());
        fixture.window.update(|window, cx| {
            assert!(
                fixture.next_focus.is_focused(window),
                "old painted default pointer focus must be prevented after disabling"
            );
            fixture.combobox.update(cx, |_, cx| cx.notify());
        });
        fixture.window.simulate_platform_frame();
        fixture.window.update(|window, cx| {
            let node = window.accessibility_tree().get(control).unwrap();
            assert!(node.states.contains(AccessibilityState::DISABLED));
            assert!(!node.states.contains(AccessibilityState::FOCUSED));
            assert_eq!(fixture.state.read(cx).selected, [7]);
            fixture.combobox.update(cx, |combobox, cx| {
                combobox.disabled = false;
                cx.notify();
            });
        });
        fixture.window.simulate_platform_frame();
        native_action(&mut fixture.window, control, AccessibilityAction::Expand);
        fixture.window.simulate_keystrokes("3");
        fixture.window.simulate_platform_frame();
        fixture.window.cx.update(|cx| {
            fixture.combobox.update(cx, |combobox, cx| {
                combobox.disabled = true;
                cx.notify();
            });
        });
        fixture.window.simulate_platform_frame();
        fixture.window.update(|window, cx| {
            assert!(!fixture.combobox.read(cx).focus_handle.is_focused(window));
            assert!(!fixture.state.read(cx).is_open);
            assert!(fixture.state.read(cx).search_text.is_empty());
            assert_eq!(fixture.state.read(cx).selected, [7]);
            let node = window.accessibility_tree().get(control).unwrap();
            assert!(node.states.contains(AccessibilityState::DISABLED));
            assert!(!node.states.contains(AccessibilityState::FOCUSED));
            assert_eq!(node.active_descendant, None);
        });
        assert!(fixture.counts.selected.borrow().is_empty());
        assert_eq!(fixture.counts.changes.get(), 0);
        assert_eq!(fixture.counts.searches.get(), 1);
    }

    #[::core::prelude::v1::test]
    fn disabled_combobox_rejects_pointer_keyboard_and_queued_native_actions() {
        let mut fixture = indexed_combobox(InputSize::Md, false);
        fixture.window.update(|_, cx| {
            fixture.state.update(cx, |state, cx| {
                state.selected = vec![7];
                cx.notify();
            });
        });
        fixture.window.update(|window, cx| window.draw(cx).clear());
        open_with_pointer(&mut fixture.window);
        let control = fixture.window.update(|window, cx| {
            let control = exposed_node(window, AccessibilityRole::ComboBox, "Combobox");
            let option =
                assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
            let clear = exposed_node(window, AccessibilityRole::Button, "Clear selection");
            for id in [option, clear] {
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Click,
                ));
            }
            fixture.combobox.update(cx, |combobox, cx| {
                combobox.disabled = true;
                cx.notify();
            });
            window.blur();
            window.draw(cx).clear();
            let node = window.accessibility_tree().get(control).unwrap();
            assert!(node.states.contains(AccessibilityState::DISABLED));
            assert!(node.states.contains(AccessibilityState::COLLAPSED));
            assert!(node.actions.is_empty());
            assert_eq!(node.active_descendant, None);
            assert!(!window.accessibility_tree().nodes.contains_key(&option));
            assert!(!window.accessibility_tree().nodes.contains_key(&clear));
            assert!(!window.has_accessibility_action_handler(option, AccessibilityAction::Click));
            assert!(!window.has_accessibility_action_handler(clear, AccessibilityAction::Click));
            control
        });
        fixture.window.run_until_parked();
        for action in [
            AccessibilityAction::Focus,
            AccessibilityAction::Click,
            AccessibilityAction::Expand,
        ] {
            native_action(&mut fixture.window, control, action);
        }
        fixture.window.simulate_keystrokes("tab enter down a");
        open_with_pointer(&mut fixture.window);
        fixture.window.update(|window, cx| {
            assert!(
                !fixture.combobox.read(cx).focus_handle.is_focused(window),
                "disabled control must not enter the tab sequence"
            );
            assert_eq!(fixture.state.read(cx).selected, [7]);
            assert!(fixture.state.read(cx).search_text.is_empty());
            fixture.combobox.update(cx, |combobox, cx| {
                combobox.disabled = false;
                cx.notify();
            });
            window.draw(cx).clear();
        });
        assert_eq!(fixture.counts.changes.get(), 0);
        assert_eq!(fixture.counts.searches.get(), 0);
        assert!(fixture.counts.selected.borrow().is_empty());
        native_action(&mut fixture.window, control, AccessibilityAction::Expand);
        let option = fixture.window.update(|window, _| {
            exposed_node(window, AccessibilityRole::ListItem, &choice_label(0))
        });
        native_action(&mut fixture.window, option, AccessibilityAction::Click);
        assert_eq!(*fixture.counts.selected.borrow(), [0]);
        assert_eq!(fixture.counts.changes.get(), 1);
    }

    #[::core::prelude::v1::test]
    fn multi_select_pointer_native_and_clear_emit_once_per_action() {
        let mut fixture = indexed_combobox(InputSize::Md, true);
        open_with_pointer(&mut fixture.window);
        let (option, center) = fixture.window.update(|window, cx| {
            let id = assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
            let bounds = window.accessibility_tree().get(id).unwrap().bounds.unwrap();
            (
                id,
                point(
                    px((bounds.x + bounds.width / 2.0) as f32),
                    px((bounds.y + bounds.height / 2.0) as f32),
                ),
            )
        });
        fixture.window.simulate_click(center, Modifiers::default());
        fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(fixture.state.read(cx).selected, [0]);
            assert!(fixture.state.read(cx).is_open);
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
        });
        native_action(&mut fixture.window, option, AccessibilityAction::Click);
        fixture
            .window
            .update(|_, cx| assert!(fixture.state.read(cx).selected.is_empty()));
        assert_eq!(*fixture.counts.selected.borrow(), [0, 0]);
        assert_eq!(fixture.counts.changes.get(), 2);
        fixture.window.update(|window, cx| {
            fixture.state.update(cx, |state, cx| {
                state.selected = vec![0, 1];
                cx.notify();
            });
            window.draw(cx).clear();
            let clear = exposed_node(window, AccessibilityRole::Button, "Clear selection");
            for _ in 0..2 {
                window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    clear,
                    AccessibilityAction::Click,
                ));
            }
        });
        fixture.window.run_until_parked();
        fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(fixture.state.read(cx).selected.is_empty());
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
        });
        assert_eq!(*fixture.counts.selected.borrow(), [0, 0]);
        assert_eq!(fixture.counts.changes.get(), 3);
    }

    #[::core::prelude::v1::test]
    fn focus_loss_closes_search_and_retires_options_without_selection() {
        let mut fixture = indexed_combobox(InputSize::Md, false);
        fixture.window.update(|_, cx| {
            fixture.state.update(cx, |state, cx| {
                state.selected = vec![7];
                cx.notify();
            });
        });
        fixture.window.update(|window, cx| window.draw(cx).clear());
        fixture.window.simulate_keystrokes("tab enter 3");
        let (control, option) = fixture.window.update(|window, cx| {
            window.draw(cx).clear();
            let option =
                assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
            (
                exposed_node(window, AccessibilityRole::ComboBox, "Combobox"),
                option,
            )
        });
        // Options expose Click, not an independent keyboard focus target.
        native_action(&mut fixture.window, option, AccessibilityAction::Focus);
        fixture.window.update(|window, cx| {
            assert!(fixture.combobox.read(cx).focus_handle.is_focused(window));
            assert!(fixture.state.read(cx).is_open);
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
        });
        fixture.window.simulate_keystrokes("tab");
        fixture.window.simulate_platform_frame();
        fixture.window.simulate_platform_frame();
        fixture.window.update(|window, cx| {
            assert!(fixture.next_focus.is_focused(window));
            let state = fixture.state.read(cx);
            eprintln!(
                "combobox_blur_after_ordinary_frames: closed={} popup_mounted={} click_handler={}",
                !state.is_open,
                window.accessibility_tree().nodes.contains_key(&option),
                window.has_accessibility_action_handler(option, AccessibilityAction::Click)
            );
            assert!(!state.is_open);
            assert!(state.search_text.is_empty());
            assert_eq!(state.focused_index, None);
            assert_eq!(state.selected, [7]);
            assert_eq!(
                window
                    .accessibility_tree()
                    .get(control)
                    .unwrap()
                    .active_descendant,
                None
            );
            assert!(!window.accessibility_tree().nodes.contains_key(&option));
            assert!(!window.has_accessibility_action_handler(option, AccessibilityAction::Click));
        });
        native_action(&mut fixture.window, option, AccessibilityAction::Click);
        assert!(fixture.counts.selected.borrow().is_empty());
        assert_eq!(fixture.counts.changes.get(), 0);
        assert_eq!(fixture.counts.searches.get(), 1);
        assert_eq!(fixture.counts.search_callbacks.get(), 1);
        native_action(&mut fixture.window, control, AccessibilityAction::Expand);
        fixture.window.update(|window, cx| {
            assert_eq!(fixture.combobox.read(cx).filtered_items(cx).len(), 40);
            assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
        });
    }

    #[::core::prelude::v1::test]
    fn state_notifications_update_ordinary_and_cached_controls_on_platform_frames() {
        for cache_control in [false, true] {
            let mut fixture = indexed_combobox_with_cache(InputSize::Md, false, cache_control);
            fixture.window.simulate_platform_frame();
            assert_eq!(
                fixture.counts.filter_calls.get(),
                0,
                "closed initial render must not scan/filter items"
            );
            fixture.window.cx.update(|cx| {
                // Notify only the separate state entity. No owner notify,
                // refresh, or forced draw may make this update visible.
                fixture.state.update(cx, |state, cx| {
                    state.selected = vec![7];
                    cx.notify();
                });
            });
            fixture.window.simulate_platform_frame();
            fixture.window.update(|window, _| {
                let id = exposed_node(window, AccessibilityRole::ComboBox, "Combobox");
                assert_eq!(
                    window.accessibility_tree().get(id).unwrap().value,
                    Some(kael::AccessibilityValue::Text(choice_label(7))),
                    "state notify must redraw the control, cached={cache_control}"
                );
            });
            assert_eq!(
                fixture.counts.filter_calls.get(),
                0,
                "closed model redraw must not scan/filter items"
            );
            fixture.window.simulate_keystrokes("tab down");
            fixture.window.simulate_platform_frame();
            fixture.window.update(|window, cx| {
                assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
            });
            fixture.window.simulate_keystrokes("down");
            fixture.window.simulate_platform_frame();
            fixture.window.update(|window, cx| {
                assert_eq!(fixture.state.read(cx).focused_index, Some(1));
                assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
            });
            fixture.window.simulate_keystrokes("3");
            fixture.window.simulate_platform_frame();
            fixture.window.update(|window, cx| {
                let active =
                    assert_active_option_is_visible(&fixture.combobox, &fixture.state, window, cx);
                assert_eq!(
                    window
                        .accessibility_tree()
                        .get(active)
                        .unwrap()
                        .label
                        .as_deref(),
                    Some(choice_label(3).as_str())
                );
            });
            assert_eq!(fixture.counts.changes.get(), 0);
            assert_eq!(fixture.counts.searches.get(), 1);
            assert!(fixture.counts.selected.borrow().is_empty());
        }
    }

    #[test]
    fn multi_select_toggles_equal_items_without_duplicates() {
        let mut state = ComboboxState::new();
        state.select_item_eq("alpha", true);
        state.select_item_eq("alpha", true);
        assert!(state.selected.is_empty());

        state.select_item_eq("alpha", true);
        state.select_item_eq("beta", true);
        assert_eq!(state.selected, vec!["alpha", "beta"]);
    }

    #[test]
    fn single_select_commits_and_resets_transient_search_state() {
        let mut state = ComboboxState::new();
        state.is_open = true;
        state.search_text = "alp".to_owned();
        state.focused_index = Some(2);

        state.select_item_eq("alpha", false);

        assert_eq!(state.selected, vec!["alpha"]);
        assert!(!state.is_open);
        assert!(state.search_text.is_empty());
        assert_eq!(state.focused_index, None);
    }
}
