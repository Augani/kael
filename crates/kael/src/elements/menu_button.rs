use crate::prelude::FluentBuilder as _;
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, AppContext, Bounds, Context, DismissEvent, Element, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId, InteractiveElement,
    IntoElement, KeyDownEvent, LayerAnchor, LayerOptions, LayerStack, LayoutId, ParentElement,
    Pixels, Point, Render, SharedString, StatefulInteractiveElement, Styled, Window, div, menu,
    point, px,
};
use std::rc::Rc;

type SelectListener<T> = Rc<dyn Fn(&T, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of menu button trigger state passed to a custom renderer.
pub struct MenuButtonTriggerRenderState {
    /// Whether the popup menu is currently open.
    pub open: bool,
    /// The configured trigger label, if any.
    pub label: Option<SharedString>,
    /// Whether the trigger currently owns keyboard focus.
    pub focused: bool,
}

impl MenuButtonTriggerRenderState {
    /// Returns true when the trigger has a configured label.
    pub fn has_label(&self) -> bool {
        self.label.is_some()
    }

    /// Length of the configured trigger label in bytes, without exposing label text.
    pub fn label_len_bytes(&self) -> usize {
        self.label.as_ref().map_or(0, |label| label.len())
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "menu_button_trigger_render_state(open={}, focused={}, has_label={}, label_len_bytes={})",
            self.open,
            self.focused,
            self.has_label(),
            self.label_len_bytes()
        )
    }
}

type MenuButtonTriggerRenderer =
    Rc<dyn Fn(MenuButtonTriggerRenderState, &Window, &App) -> AnyElement>;

#[non_exhaustive]
/// Snapshot of a popup menu item row passed to a custom renderer.
pub struct MenuButtonItemRenderState<T> {
    /// The value emitted when this item is activated.
    pub value: T,
    /// The visible label for this item.
    pub label: SharedString,
    /// Zero-based index of the item in the original menu item list.
    pub index: usize,
    /// Whether this item is currently highlighted for keyboard navigation.
    pub highlighted: bool,
    /// Whether this item is disabled.
    pub disabled: bool,
    /// Whether this row is a non-interactive visual separator.
    pub separator: bool,
}

impl<T> MenuButtonItemRenderState<T> {
    /// Length of the menu item label in bytes, without exposing label text.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "menu_button_item_render_state(index={}, highlighted={}, disabled={}, separator={}, label_len_bytes={})",
            self.index,
            self.highlighted,
            self.disabled,
            self.separator,
            self.label_len_bytes()
        )
    }
}

type MenuButtonItemRenderer<T> =
    Rc<dyn Fn(MenuButtonItemRenderState<T>, &Window, &App) -> AnyElement>;

/// Construct a menu button primitive backed by an anchored popup menu.
#[track_caller]
pub fn menu_button<T, I, O>(id: impl Into<ElementId>, items: I) -> MenuButton<T>
where
    T: Clone + PartialEq + 'static,
    I: IntoIterator<Item = O>,
    O: Into<MenuButtonItem<T>>,
{
    MenuButton::new(id.into(), items.into_iter().map(Into::into).collect())
}

#[derive(Clone, Debug, PartialEq)]
/// A single popup menu item.
pub struct MenuButtonItem<T> {
    /// The value emitted when this item is activated.
    pub value: T,
    /// The visible label for this item.
    pub label: SharedString,
    /// Whether the item is disabled.
    pub disabled: bool,
    /// Whether the row is a non-interactive visual separator.
    pub separator: bool,
}

impl<T> MenuButtonItem<T> {
    /// Create a new popup menu item.
    pub fn new(value: T, label: impl Into<SharedString>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: false,
            separator: false,
        }
    }

    /// Disable this popup menu item.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Render this item as a non-interactive separator.
    pub fn separator(mut self) -> Self {
        self.disabled = true;
        self.separator = true;
        self
    }
}

impl<T, L> From<(T, L)> for MenuButtonItem<T>
where
    L: Into<SharedString>,
{
    fn from((value, label): (T, L)) -> Self {
        Self::new(value, label)
    }
}

/// A popup-backed menu button with caller-owned trigger and item visuals.
pub struct MenuButton<T> {
    element_id: ElementId,
    items: Vec<MenuButtonItem<T>>,
    label: Option<SharedString>,
    on_select: Option<SelectListener<T>>,
    custom_trigger_renderer: Option<MenuButtonTriggerRenderer>,
    custom_item_renderer: Option<MenuButtonItemRenderer<T>>,
    source_location: &'static core::panic::Location<'static>,
}

#[doc(hidden)]
pub struct MenuButtonElementState<T> {
    root: AnyElement,
    state: Entity<MenuButtonState<T>>,
}

struct MenuButtonState<T> {
    focus_handle: FocusHandle,
    layer_stack: Entity<LayerStack>,
    items: Vec<MenuButtonItem<T>>,
    label: Option<SharedString>,
    highlighted_index: Option<usize>,
    trigger_bounds: Option<Bounds<Pixels>>,
    on_select: Option<SelectListener<T>>,
    item_renderer: Option<MenuButtonItemRenderer<T>>,
}

struct MenuButtonPopup<T> {
    selector_id: String,
    state: Entity<MenuButtonState<T>>,
    root_focus: FocusHandle,
    item_renderer: Option<MenuButtonItemRenderer<T>>,
}

impl<T> MenuButton<T>
where
    T: Clone + PartialEq + 'static,
{
    #[track_caller]
    fn new(element_id: ElementId, items: Vec<MenuButtonItem<T>>) -> Self {
        Self {
            element_id,
            items,
            label: None,
            on_select: None,
            custom_trigger_renderer: None,
            custom_item_renderer: None,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the visible trigger label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Register a callback invoked when a menu item is activated.
    pub fn on_select(mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(listener));
        self
    }

    /// Render the trigger with caller-owned visuals.
    pub fn render_trigger_with(
        mut self,
        renderer: impl Fn(MenuButtonTriggerRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_trigger_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render each popup menu item row with caller-owned visuals.
    pub fn render_items_with(
        mut self,
        renderer: impl Fn(MenuButtonItemRenderState<T>, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_item_renderer = Some(Rc::new(renderer));
        self
    }

    fn build_trigger(
        &self,
        selector_id: &str,
        state: Entity<MenuButtonState<T>>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let snapshot = state.read(cx).snapshot(cx);
        let focus_handle = snapshot.focus_handle.clone();
        let toggle_state = state.clone();
        let close_state = state.clone();
        let down_state = state.clone();
        let up_state = state.clone();
        let selector_id = selector_id.to_string();
        let click_selector_id = selector_id.clone();
        #[cfg(any(test, feature = "test-support"))]
        let trigger_selector_id = selector_id.clone();
        let key_selector_id = selector_id;
        let has_custom_trigger = self.custom_trigger_renderer.is_some();
        let accessible_label = snapshot
            .label
            .clone()
            .unwrap_or_else(|| SharedString::from("Menu"));

        let mut accessibility_state = if snapshot.is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if focus_handle.is_focused(window) {
            accessibility_state |= AccessibilityState::FOCUSED;
        }

        let mut trigger = div()
            .id(self.element_id.clone())
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(true)
            .cursor_pointer()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Button)
                    .label(accessible_label.to_string())
                    .states(accessibility_state)
                    .actions(vec![
                        AccessibilityAction::Focus,
                        AccessibilityAction::Click,
                        AccessibilityAction::ShowMenu,
                    ]),
            )
            .when(!has_custom_trigger, |this| {
                this.min_w(px(160.0))
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
                    .focus_visible(|style: crate::StyleRefinement| {
                        style.bg(crate::rgba(0x1d4ed810))
                    })
                    .hover(|style| style.bg(crate::rgb(0xf8fafc)))
            })
            .on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }

                let popup_state = toggle_state.clone();
                popup_state.update(cx, |state, cx| {
                    if state.is_open(cx) {
                        if matches!(event, crate::ClickEvent::Keyboard(_)) {
                            state.commit_highlighted(window, cx);
                        } else {
                            state.close_popup(cx);
                        }
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
                    "down" => {
                        let popup_state = down_state.clone();
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
                        let popup_state = up_state.clone();
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
                    "escape" => {
                        let popup_state = close_state.clone();
                        popup_state.update(cx, |state, cx| {
                            if state.is_open(cx) {
                                state.close_popup(cx);
                            }
                        });
                        window.prevent_default();
                    }
                    _ => {}
                }
            });

        let body = if let Some(renderer) = &self.custom_trigger_renderer {
            renderer(
                MenuButtonTriggerRenderState {
                    open: snapshot.is_open,
                    label: snapshot.label.clone(),
                    focused: focus_handle.is_focused(window),
                },
                window,
                cx,
            )
        } else {
            default_menu_button_trigger(snapshot.label.clone(), snapshot.is_open)
        };
        trigger = trigger.child(body);

        #[cfg(any(test, feature = "test-support"))]
        {
            let trigger_selector = format!("menu-button-{}", trigger_selector_id);
            trigger = trigger.debug_selector(move || trigger_selector);
        }

        trigger.into_any_element()
    }
}

impl<T> IntoElement for MenuButton<T>
where
    T: Clone + PartialEq + 'static,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T> Element for MenuButton<T>
where
    T: Clone + PartialEq + 'static,
{
    type RequestLayoutState = MenuButtonElementState<T>;
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
        let global_id = global_id.expect("menu button requires a global id");
        let current_view = window.current_view();
        let selector_id = self.element_id.to_string();
        let items = self.items.clone();
        let label = self.label.clone();
        let on_select = self.on_select.clone();
        let custom_item_renderer = self.custom_item_renderer.clone();

        let state =
            window.with_element_state(global_id, |state: Option<Entity<MenuButtonState<T>>>, _| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let layer_stack = cx.new(|_| LayerStack::new());
                    let state = cx.new(|cx| {
                        MenuButtonState::new(
                            cx.focus_handle(),
                            layer_stack.clone(),
                            items.clone(),
                            label.clone(),
                            on_select.clone(),
                            custom_item_renderer.clone(),
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
            state.sync_from_props(items, label, on_select, custom_item_renderer, cx);
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

        (layout_id, MenuButtonElementState { root, state })
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

impl<T> MenuButtonState<T>
where
    T: Clone + PartialEq + 'static,
{
    fn new(
        focus_handle: FocusHandle,
        layer_stack: Entity<LayerStack>,
        items: Vec<MenuButtonItem<T>>,
        label: Option<SharedString>,
        on_select: Option<SelectListener<T>>,
        item_renderer: Option<MenuButtonItemRenderer<T>>,
    ) -> Self {
        let mut state = Self {
            focus_handle,
            layer_stack,
            items,
            label,
            highlighted_index: None,
            trigger_bounds: None,
            on_select,
            item_renderer,
        };
        state.reset_highlight();
        state
    }

    fn sync_from_props(
        &mut self,
        items: Vec<MenuButtonItem<T>>,
        label: Option<SharedString>,
        on_select: Option<SelectListener<T>>,
        item_renderer: Option<MenuButtonItemRenderer<T>>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        let mut reset_highlight = false;

        if self.items != items {
            self.items = items;
            changed = true;
            reset_highlight = true;
        }
        if self.label != label {
            self.label = label;
            changed = true;
        }
        if self.on_select.as_ref().map(Rc::as_ptr) != on_select.as_ref().map(Rc::as_ptr) {
            self.on_select = on_select;
            changed = true;
        }
        if self.item_renderer.as_ref().map(Rc::as_ptr) != item_renderer.as_ref().map(Rc::as_ptr) {
            self.item_renderer = item_renderer;
            changed = true;
        }

        if changed {
            if reset_highlight {
                self.reset_highlight();
            }
            cx.notify();
        }
    }

    fn snapshot(&self, cx: &App) -> MenuButtonSnapshot {
        MenuButtonSnapshot {
            focus_handle: self.focus_handle.clone(),
            label: self.label.clone(),
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

        self.reset_highlight();

        let popup = cx.new({
            let selector_id = selector_id.to_string();
            let item_renderer = self.item_renderer.clone();
            move |cx| MenuButtonPopup::new(selector_id.clone(), state, item_renderer.clone(), cx)
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
        window.focus(&self.focus_handle);
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

        let enabled = self.enabled_indices();
        self.highlighted_index = next_enabled_index(&enabled, self.highlighted_index, delta);
        cx.notify();
    }

    fn highlight_first(&mut self, cx: &mut Context<Self>) {
        self.highlighted_index = self.enabled_indices().first().copied();
        cx.notify();
    }

    fn highlight_last(&mut self, cx: &mut Context<Self>) {
        self.highlighted_index = self.enabled_indices().last().copied();
        cx.notify();
    }

    fn commit_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.highlighted_index else {
            return;
        };

        self.commit_index(index, window, cx);
    }

    fn commit_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(index).cloned() else {
            return;
        };
        if item.disabled || item.separator {
            return;
        }

        self.close_popup(cx);
        window.focus(&self.focus_handle);
        if let Some(listener) = self.on_select.clone() {
            listener(&item.value, window, cx);
        }
    }

    fn enabled_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!item.disabled && !item.separator).then_some(index))
            .collect()
    }

    fn reset_highlight(&mut self) {
        self.highlighted_index = self.enabled_indices().first().copied();
    }

    fn anchor_position(&self) -> Option<Point<Pixels>> {
        self.trigger_bounds
            .map(|bounds| point(bounds.left(), bounds.bottom()))
    }

    fn popup_width(&self) -> Pixels {
        self.trigger_bounds
            .map(|bounds| bounds.size.width.max(px(176.0)))
            .unwrap_or(px(176.0))
    }
}

impl<T> MenuButtonPopup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn new(
        selector_id: String,
        state: Entity<MenuButtonState<T>>,
        item_renderer: Option<MenuButtonItemRenderer<T>>,
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
            item_renderer,
        }
    }
}

impl<T> EventEmitter<DismissEvent> for MenuButtonPopup<T> where T: Clone + PartialEq + 'static {}

impl<T> Focusable for MenuButtonPopup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.root_focus.clone()
    }
}

impl<T> Render for MenuButtonPopup<T>
where
    T: Clone + PartialEq + 'static,
{
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = {
            let state = self.state.read(cx);
            MenuButtonPopupSnapshot {
                width: state.popup_width(),
                highlighted_index: state.highlighted_index,
                items: state.items.clone(),
            }
        };

        let navigation_state = self.state.clone();
        let commit_state = self.state.clone();
        let close_state = self.state.clone();
        let selector_id = self.selector_id.clone();

        let mut panel = menu(ElementId::named_usize(
            format!("{}-popup", self.selector_id),
            0,
        ))
        .track_focus(&self.root_focus)
        .focusable()
        .tab_stop(true)
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
                "home" => {
                    navigation_state.update(cx, |state, cx| {
                        state.highlight_first(cx);
                    });
                    window.prevent_default();
                }
                "end" => {
                    navigation_state.update(cx, |state, cx| {
                        state.highlight_last(cx);
                    });
                    window.prevent_default();
                }
                "enter" | "space" => {
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
        });

        #[cfg(any(test, feature = "test-support"))]
        {
            let popup_selector = format!("menu-button-popup-{}", self.selector_id);
            panel = panel.debug_selector(move || popup_selector);
        }

        for (index, item) in snapshot.items.iter().cloned().enumerate() {
            let is_highlighted = snapshot.highlighted_index == Some(index);
            let commit_state = self.state.clone();

            let mut row = div()
                .id(ElementId::named_usize(
                    format!("{}-item", self.selector_id),
                    index,
                ))
                .accessibility(if item.separator {
                    AccessibilityAttributes::new(AccessibilityRole::Separator)
                } else {
                    AccessibilityAttributes::new(AccessibilityRole::MenuItem)
                        .label(item.label.to_string())
                        .states(if item.disabled {
                            AccessibilityState::DISABLED
                        } else {
                            AccessibilityState::NONE
                        })
                        .actions(if item.disabled {
                            Vec::new()
                        } else {
                            vec![AccessibilityAction::Click]
                        })
                });

            if !item.disabled && !item.separator {
                row = row.cursor_pointer().on_click(move |event, window, cx| {
                    if !event.standard_click() {
                        return;
                    }

                    commit_state.update(cx, |state, cx| {
                        state.commit_index(index, window, cx);
                    });
                });
            }

            let row_content = if let Some(renderer) = &self.item_renderer {
                renderer(
                    MenuButtonItemRenderState {
                        value: item.value.clone(),
                        label: item.label.clone(),
                        index,
                        highlighted: is_highlighted,
                        disabled: item.disabled,
                        separator: item.separator,
                    },
                    window,
                    cx,
                )
            } else {
                default_menu_button_item(
                    item.label.clone(),
                    is_highlighted,
                    item.disabled,
                    item.separator,
                )
            };
            row = row.child(row_content);

            #[cfg(any(test, feature = "test-support"))]
            {
                let option_selector = format!("menu-button-item-{}-{}", self.selector_id, index);
                row = row.debug_selector(move || option_selector);
            }

            panel = panel.child(row);
        }

        panel.into_any_element()
    }
}

fn default_menu_button_trigger(label: Option<SharedString>, open: bool) -> AnyElement {
    let label = label.unwrap_or_else(|| SharedString::from("Menu"));
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .size_full()
        .child(div().child(label))
        .child(
            div()
                .text_color(crate::rgb(0x64748b))
                .child(if open { "^" } else { "v" }),
        )
        .into_any_element()
}

fn default_menu_button_item(
    label: SharedString,
    highlighted: bool,
    disabled: bool,
    separator: bool,
) -> AnyElement {
    if separator {
        return div()
            .h(px(1.0))
            .my_1()
            .bg(crate::rgb(0xe2e8f0))
            .into_any_element();
    }

    div()
        .flex()
        .items_center()
        .gap_3()
        .px(px(10.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .bg(if highlighted {
            crate::rgb(0xe0f2fe)
        } else {
            crate::rgba(0x00000000)
        })
        .hover(|style| style.bg(crate::rgb(0xf1f5f9)))
        .text_color(if disabled {
            crate::rgb(0x94a3b8)
        } else {
            crate::rgb(0x0f172a)
        })
        .child(label)
        .into_any_element()
}

struct MenuButtonSnapshot {
    focus_handle: FocusHandle,
    label: Option<SharedString>,
    is_open: bool,
}

struct MenuButtonPopupSnapshot<T> {
    width: Pixels,
    highlighted_index: Option<usize>,
    items: Vec<MenuButtonItem<T>>,
}

fn next_enabled_index(enabled: &[usize], current: Option<usize>, delta: isize) -> Option<usize> {
    if enabled.is_empty() {
        return None;
    }

    let Some(current_position) =
        current.and_then(|current| enabled.iter().position(|index| *index == current))
    else {
        return if delta < 0 {
            enabled.last().copied()
        } else {
            enabled.first().copied()
        };
    };

    let next_position = (current_position as isize + delta).rem_euclid(enabled.len() as isize);
    enabled.get(next_position as usize).copied()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessibilityRole, AccessibilityState, Context, Modifiers, Render, TestAppContext, div,
    };

    struct MenuButtonView {
        last_action: &'static str,
    }

    struct CustomMenuButtonView {
        last_action: &'static str,
    }

    impl Render for MenuButtonView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            menu_button(
                "file_menu_button",
                [
                    MenuButtonItem::new("copy", "Copy"),
                    MenuButtonItem::new("paste", "Paste").disabled(),
                    MenuButtonItem::new("delete", "Delete"),
                ],
            )
            .label("File")
            .on_select(cx.listener(|this, value, _, cx| {
                this.last_action = *value;
                cx.notify();
            }))
        }
    }

    impl Render for CustomMenuButtonView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            menu_button(
                "custom_menu_button",
                [
                    MenuButtonItem::new("copy", "Copy"),
                    MenuButtonItem::new("rename", "Rename"),
                ],
            )
            .label("Actions")
            .render_trigger_with(|state, _, _| {
                let selector = format!(
                    "menu-trigger-{}-{}",
                    state
                        .label
                        .clone()
                        .unwrap_or_else(|| SharedString::from("Menu")),
                    if state.open { "open" } else { "closed" },
                );
                div()
                    .debug_selector(move || selector)
                    .child("trigger")
                    .into_any_element()
            })
            .render_items_with(|state, _, _| {
                let selector = format!(
                    "menu-item-{}-{}-{}",
                    state.index,
                    if state.highlighted {
                        "highlighted"
                    } else {
                        "idle"
                    },
                    if state.disabled {
                        "disabled"
                    } else {
                        "enabled"
                    },
                );
                div()
                    .debug_selector(move || selector)
                    .child(state.label)
                    .into_any_element()
            })
            .on_select(cx.listener(|this, value, _, cx| {
                this.last_action = *value;
                cx.notify();
            }))
        }
    }

    #[test]
    fn next_enabled_index_wraps_and_skips_disabled_items() {
        let enabled = vec![0, 2, 4];

        assert_eq!(next_enabled_index(&enabled, None, 1), Some(0));
        assert_eq!(next_enabled_index(&enabled, None, -1), Some(4));
        assert_eq!(next_enabled_index(&enabled, Some(2), 1), Some(4));
        assert_eq!(next_enabled_index(&enabled, Some(4), 1), Some(0));
        assert_eq!(next_enabled_index(&enabled, Some(0), -1), Some(4));
    }

    #[test]
    fn separator_items_are_disabled_and_identifiable() {
        let item = MenuButtonItem::new("separator", "").separator();
        assert!(item.disabled);
        assert!(item.separator);
    }

    #[crate::test]
    fn menu_button_click_opens_and_commits_item(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| MenuButtonView {
            last_action: "copy",
        });

        window.update(|window, cx| {
            window.draw(cx).clear();

            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::COLLAPSED));
        });

        let trigger_bounds = window.debug_bounds("menu-button-file_menu_button").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::EXPANDED));
        });

        assert!(
            window
                .debug_bounds("menu-button-popup-file_menu_button")
                .is_some()
        );

        let item_bounds = window
            .debug_bounds("menu-button-item-file_menu_button-2")
            .unwrap();
        window.simulate_click(item_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).last_action, "delete");

            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::COLLAPSED));
        });
    }

    #[crate::test]
    fn menu_button_keyboard_opens_and_escape_closes(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| MenuButtonView {
            last_action: "copy",
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            window.draw(cx).clear();

            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::FOCUSED));
        });

        window.simulate_keystrokes("down");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::EXPANDED));
        });

        window.simulate_keystrokes("escape");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::COLLAPSED));
        });
    }

    #[crate::test]
    fn menu_button_escape_dismisses_popup(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| MenuButtonView {
            last_action: "copy",
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let trigger_bounds = window.debug_bounds("menu-button-file_menu_button").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("menu-button-popup-file_menu_button")
                .is_some()
        );
        window.simulate_keystrokes("escape");
        window.update(|window, cx| {
            window.draw(cx).clear();

            let button = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Button)
                .unwrap();
            assert!(button.states.contains(AccessibilityState::COLLAPSED));
        });
    }

    #[crate::test]
    fn menu_button_render_hooks_receive_trigger_and_item_state(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| CustomMenuButtonView {
            last_action: "copy",
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("menu-trigger-Actions-closed").is_some());

        let trigger_bounds = window
            .debug_bounds("menu-button-custom_menu_button")
            .unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("menu-item-0-highlighted-enabled")
                .is_some()
        );
    }

    #[test]
    fn menu_button_render_state_summary_is_content_safe() {
        let trigger = MenuButtonTriggerRenderState {
            open: true,
            label: Some(SharedString::from("Secret Actions")),
            focused: false,
        };

        assert!(trigger.has_label());
        assert_eq!(trigger.label_len_bytes(), "Secret Actions".len());

        let trigger_summary = trigger.to_text();
        assert!(trigger_summary.contains("open=true"));
        assert!(trigger_summary.contains("focused=false"));
        assert!(trigger_summary.contains("has_label=true"));
        assert!(trigger_summary.contains("label_len_bytes=14"));
        assert!(!trigger_summary.contains("Secret"));
        assert!(!trigger_summary.contains("Actions"));

        let item = MenuButtonItemRenderState {
            value: "secret-action",
            label: SharedString::from("Private Delete"),
            index: 2,
            highlighted: true,
            disabled: false,
            separator: false,
        };

        assert_eq!(item.label_len_bytes(), "Private Delete".len());

        let item_summary = item.to_text();
        assert!(item_summary.contains("index=2"));
        assert!(item_summary.contains("highlighted=true"));
        assert!(item_summary.contains("disabled=false"));
        assert!(item_summary.contains("separator=false"));
        assert!(item_summary.contains("label_len_bytes=14"));
        assert!(!item_summary.contains("secret-action"));
        assert!(!item_summary.contains("Private"));
        assert!(!item_summary.contains("Delete"));
    }
}
