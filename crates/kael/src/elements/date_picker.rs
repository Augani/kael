use super::local_history::{
    WindowValueHistory, ensure_local_undo_redo_bindings, local_undo_redo_key_context,
};
use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AccessibilityValue, AnyElement, App, AppContext, Bounds, Context, DismissEvent, Element,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId,
    InteractiveElement, IntoElement, KeyDownEvent, LayerAnchor, LayerOptions, LayerStack, LayoutId,
    ParentElement, Pixels, Point, Redo, Render, SharedString, StatefulInteractiveElement, Styled,
    Undo, Window, div, point, px,
};
use std::rc::Rc;
use time::{Date, Duration, Month};

type ChangeListener = Rc<dyn Fn(&Date, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of date picker trigger state passed to a custom renderer.
pub struct DatePickerRenderState {
    /// Whether the popup is currently open.
    pub open: bool,
    /// The currently selected date.
    pub date: Date,
    /// The formatted label currently shown in the trigger.
    pub display_label: SharedString,
    /// Whether the trigger currently owns keyboard focus.
    pub focused: bool,
}

impl DatePickerRenderState {
    /// Length of the trigger display label in bytes.
    pub fn display_label_len_bytes(&self) -> usize {
        self.display_label.len()
    }

    /// Content-safe summary for custom trigger renderers.
    pub fn to_text(&self) -> String {
        format!(
            "date picker render state: open {}, focused {}, display-label-bytes {}",
            self.open,
            self.focused,
            self.display_label_len_bytes()
        )
    }
}

#[non_exhaustive]
/// Snapshot of a calendar day cell passed to a custom renderer.
pub struct DatePickerDayRenderState {
    /// The date represented by the day cell.
    pub date: Date,
    /// The day-of-month number shown in the cell.
    pub day: u8,
    /// Whether the day is disabled by the current min or max bounds.
    pub disabled: bool,
    /// Whether this day is currently selected.
    pub selected: bool,
    /// Whether this day is currently highlighted for keyboard navigation.
    pub highlighted: bool,
}

impl DatePickerDayRenderState {
    /// Whether this day can be selected.
    pub fn is_selectable(&self) -> bool {
        !self.disabled
    }

    /// Content-safe summary for custom day-cell renderers.
    pub fn to_text(&self) -> String {
        format!(
            "date picker day render state: selectable {}, selected {}, highlighted {}",
            self.is_selectable(),
            self.selected,
            self.highlighted
        )
    }
}

#[non_exhaustive]
/// Snapshot of date picker popup state passed to a custom renderer.
pub struct DatePickerPopupRenderState {
    /// The resolved popup width.
    pub width: Pixels,
    /// The formatted month title currently shown in the popup header.
    pub month_label: SharedString,
    /// The currently selected date.
    pub selected_date: Date,
    /// The currently highlighted date.
    pub highlighted_date: Date,
    /// Whether navigating to the previous month is currently allowed.
    pub can_navigate_previous: bool,
    /// Whether navigating to the next month is currently allowed.
    pub can_navigate_next: bool,
}

impl DatePickerPopupRenderState {
    /// Length of the formatted month label in bytes.
    pub fn month_label_len_bytes(&self) -> usize {
        self.month_label.len()
    }

    /// Whether the selected date is also highlighted.
    pub fn selected_is_highlighted(&self) -> bool {
        self.selected_date == self.highlighted_date
    }

    /// Whether either month navigation direction is available.
    pub fn can_navigate(&self) -> bool {
        self.can_navigate_previous || self.can_navigate_next
    }

    /// Content-safe summary for custom popup renderers.
    pub fn to_text(&self) -> String {
        format!(
            "date picker popup render state: month-label-bytes {}, selected-highlighted {}, can-prev {}, can-next {}, can-navigate {}",
            self.month_label_len_bytes(),
            self.selected_is_highlighted(),
            self.can_navigate_previous,
            self.can_navigate_next,
            self.can_navigate()
        )
    }
}

type DatePickerPopupCustomRenderer =
    Rc<dyn Fn(DatePickerPopupRenderState, Vec<AnyElement>, &Window, &App) -> AnyElement>;

#[non_exhaustive]
/// Snapshot of the calendar header passed to a custom renderer.
pub struct DatePickerHeaderRenderState {
    /// The formatted month title currently shown in the popup header.
    pub month_label: SharedString,
    /// Whether navigating to the previous month is currently allowed.
    pub can_navigate_previous: bool,
    /// Whether navigating to the next month is currently allowed.
    pub can_navigate_next: bool,
}

impl DatePickerHeaderRenderState {
    /// Length of the formatted month label in bytes.
    pub fn month_label_len_bytes(&self) -> usize {
        self.month_label.len()
    }

    /// Whether either month navigation direction is available.
    pub fn can_navigate(&self) -> bool {
        self.can_navigate_previous || self.can_navigate_next
    }

    /// Content-safe summary for custom header renderers.
    pub fn to_text(&self) -> String {
        format!(
            "date picker header render state: month-label-bytes {}, can-prev {}, can-next {}, can-navigate {}",
            self.month_label_len_bytes(),
            self.can_navigate_previous,
            self.can_navigate_next,
            self.can_navigate()
        )
    }
}

type DatePickerHeaderCustomRenderer =
    Rc<dyn Fn(DatePickerHeaderRenderState, AnyElement, AnyElement, &Window, &App) -> AnyElement>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Direction of a date picker navigation button.
pub enum DatePickerNavigationDirection {
    /// Navigate to the previous month.
    PreviousMonth,
    /// Navigate to the next month.
    NextMonth,
}

impl DatePickerNavigationDirection {
    /// Stable label for diagnostics and custom navigation button renderers.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::PreviousMonth => "previous-month",
            Self::NextMonth => "next-month",
        }
    }
}

#[non_exhaustive]
/// Snapshot of a month navigation button passed to a custom renderer.
pub struct DatePickerNavButtonRenderState {
    /// Which direction this button navigates.
    pub direction: DatePickerNavigationDirection,
    /// The visible label currently assigned to the button.
    pub label: SharedString,
    /// Whether this button is enabled.
    pub enabled: bool,
}

impl DatePickerNavButtonRenderState {
    /// Length of the visible navigation label in bytes.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Content-safe summary for custom navigation button renderers.
    pub fn to_text(&self) -> String {
        format!(
            "date picker nav button render state: direction {}, enabled {}, label-bytes {}",
            self.direction.to_text(),
            self.enabled,
            self.label_len_bytes()
        )
    }
}

type DatePickerNavButtonCustomRenderer =
    Rc<dyn Fn(DatePickerNavButtonRenderState, AnyElement, &Window, &App) -> AnyElement>;

#[non_exhaustive]
/// Snapshot of a weekday label passed to a custom renderer.
pub struct DatePickerWeekdayRenderState {
    /// Zero-based weekday index in the rendered row.
    pub index: usize,
    /// The visible weekday label.
    pub label: SharedString,
}

impl DatePickerWeekdayRenderState {
    /// Length of the visible weekday label in bytes.
    pub fn label_len_bytes(&self) -> usize {
        self.label.len()
    }

    /// Content-safe summary for custom weekday renderers.
    pub fn to_text(&self) -> String {
        format!(
            "date picker weekday render state: index {}, label-bytes {}",
            self.index,
            self.label_len_bytes()
        )
    }
}

type DatePickerWeekdayCustomRenderer =
    Rc<dyn Fn(DatePickerWeekdayRenderState, &Window, &App) -> AnyElement>;

type DatePickerCustomRenderer = Rc<dyn Fn(DatePickerRenderState, &Window, &App) -> AnyElement>;
type DatePickerDayCustomRenderer =
    Rc<dyn Fn(DatePickerDayRenderState, &Window, &App) -> AnyElement>;

/// Construct a controlled popup-backed date picker.
#[track_caller]
pub fn date_picker(id: impl Into<ElementId>, date: Date) -> DatePicker {
    DatePicker::new(id.into(), date)
}

/// A controlled date picker with a popup month grid.
pub struct DatePicker {
    element_id: ElementId,
    date: Date,
    min: Option<Date>,
    max: Option<Date>,
    on_change: Option<ChangeListener>,
    custom_renderer: Option<DatePickerCustomRenderer>,
    custom_day_renderer: Option<DatePickerDayCustomRenderer>,
    custom_popup_renderer: Option<DatePickerPopupCustomRenderer>,
    custom_header_renderer: Option<DatePickerHeaderCustomRenderer>,
    custom_nav_button_renderer: Option<DatePickerNavButtonCustomRenderer>,
    custom_weekday_renderer: Option<DatePickerWeekdayCustomRenderer>,
    source_location: &'static core::panic::Location<'static>,
}

#[doc(hidden)]
pub struct DatePickerElementState {
    root: AnyElement,
    state: Entity<DatePickerState>,
}

struct DatePickerState {
    focus_handle: FocusHandle,
    layer_stack: Entity<LayerStack>,
    date: Date,
    history: WindowValueHistory<Date>,
    min: Option<Date>,
    max: Option<Date>,
    visible_month: CalendarMonth,
    highlighted_date: Option<Date>,
    trigger_bounds: Option<Bounds<Pixels>>,
    on_change: Option<ChangeListener>,
    day_renderer: Option<DatePickerDayCustomRenderer>,
    popup_renderer: Option<DatePickerPopupCustomRenderer>,
    header_renderer: Option<DatePickerHeaderCustomRenderer>,
    nav_button_renderer: Option<DatePickerNavButtonCustomRenderer>,
    weekday_renderer: Option<DatePickerWeekdayCustomRenderer>,
}

struct DatePickerPopup {
    selector_id: String,
    state: Entity<DatePickerState>,
    root_focus: FocusHandle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CalendarMonth {
    year: i32,
    month: Month,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CalendarDayCell {
    date: Date,
    day: u8,
    disabled: bool,
}

#[derive(Clone)]
struct DatePickerSnapshot {
    focus_handle: FocusHandle,
    display_date: Date,
    is_open: bool,
}

struct PopupSnapshot {
    width: Pixels,
    selected_date: Date,
    visible_month: CalendarMonth,
    highlighted_date: Date,
    min: Option<Date>,
    max: Option<Date>,
}

impl DatePicker {
    #[track_caller]
    fn new(element_id: ElementId, date: Date) -> Self {
        Self {
            element_id,
            date,
            min: None,
            max: None,
            on_change: None,
            custom_renderer: None,
            custom_day_renderer: None,
            custom_popup_renderer: None,
            custom_header_renderer: None,
            custom_nav_button_renderer: None,
            custom_weekday_renderer: None,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the minimum selectable date.
    pub fn min(mut self, min: Date) -> Self {
        self.min = Some(min);
        self
    }

    /// Set the maximum selectable date.
    pub fn max(mut self, max: Date) -> Self {
        self.max = Some(max);
        self
    }

    /// Register a callback invoked with the newly selected date.
    pub fn on_change(mut self, listener: impl Fn(&Date, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Render the date picker trigger with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(DatePickerRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render each calendar day cell with caller-owned visuals.
    pub fn render_days_with(
        mut self,
        renderer: impl Fn(DatePickerDayRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_day_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render the popup shell with caller-owned visuals around the generated calendar sections.
    pub fn render_popup_with(
        mut self,
        renderer: impl Fn(DatePickerPopupRenderState, Vec<AnyElement>, &Window, &App) -> AnyElement
        + 'static,
    ) -> Self {
        self.custom_popup_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render the popup header with caller-owned layout around the generated navigation buttons.
    pub fn render_header_with(
        mut self,
        renderer: impl Fn(
            DatePickerHeaderRenderState,
            AnyElement,
            AnyElement,
            &Window,
            &App,
        ) -> AnyElement
        + 'static,
    ) -> Self {
        self.custom_header_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render the month navigation buttons with caller-owned visuals around the generated button.
    pub fn render_nav_buttons_with(
        mut self,
        renderer: impl Fn(DatePickerNavButtonRenderState, AnyElement, &Window, &App) -> AnyElement
        + 'static,
    ) -> Self {
        self.custom_nav_button_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render the weekday labels with caller-owned visuals.
    pub fn render_weekdays_with(
        mut self,
        renderer: impl Fn(DatePickerWeekdayRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_weekday_renderer = Some(Rc::new(renderer));
        self
    }

    fn build_trigger(
        &self,
        selector_id: &str,
        state: Entity<DatePickerState>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let (snapshot, can_undo, can_redo) = {
            let date_picker_state = state.read(cx);
            (
                date_picker_state.snapshot(cx),
                date_picker_state.history.can_undo(),
                date_picker_state.history.can_redo(),
            )
        };
        let focus_handle = snapshot.focus_handle.clone();
        let undo_state = state.clone();
        let click_state = state.clone();
        let key_state = state.clone();
        let redo_state = state;
        let element_id = self.element_id.clone();
        let selector_id = selector_id.to_string();
        let click_selector_id = selector_id.clone();
        #[cfg(any(test, feature = "test-support"))]
        let debug_selector_id = selector_id.clone();
        let key_selector_id = selector_id;
        let accessibility_value = format_date(snapshot.display_date);

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
            .min_w(px(180.0))
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
                    .value(AccessibilityValue::Text(accessibility_value.to_string()))
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

                let popup_state = click_state.clone();
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

                if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                    let popup_state = key_state.clone();
                    popup_state.update(cx, |state, cx| {
                        if state.is_open(cx) {
                            state.close_popup(cx);
                        } else {
                            state.open_popup(popup_state.clone(), &key_selector_id, window, cx);
                        }
                    });
                    window.prevent_default();
                }
            });

        if let Some(renderer) = &self.custom_renderer {
            let render_state = DatePickerRenderState {
                open: snapshot.is_open,
                date: snapshot.display_date,
                display_label: accessibility_value.clone(),
                focused: focus_handle.is_focused(window),
            };
            trigger = trigger.child(renderer(render_state, window, cx));
        } else {
            trigger = trigger
                .child(
                    div()
                        .text_color(crate::rgb(0x0f172a))
                        .child(accessibility_value),
                )
                .child(
                    div()
                        .text_color(crate::rgb(0x64748b))
                        .child(if snapshot.is_open { "^" } else { "v" }),
                );
        }

        #[cfg(any(test, feature = "test-support"))]
        {
            let trigger_selector = format!("date-picker-{}", debug_selector_id);
            trigger = trigger.debug_selector(move || trigger_selector);
        }

        trigger.into_any_element()
    }
}

impl IntoElement for DatePicker {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DatePicker {
    type RequestLayoutState = DatePickerElementState;
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
        let global_id = global_id.expect("date_picker requires a global id");
        let current_view = window.current_view();
        let selector_id = self.element_id.to_string();
        let date = self.date;
        let min = self.min;
        let max = self.max;
        let on_change = self.on_change.clone();
        let custom_day_renderer = self.custom_day_renderer.clone();
        let custom_popup_renderer = self.custom_popup_renderer.clone();
        let custom_header_renderer = self.custom_header_renderer.clone();
        let custom_nav_button_renderer = self.custom_nav_button_renderer.clone();
        let custom_weekday_renderer = self.custom_weekday_renderer.clone();
        let undo_manager = window.undo_manager();

        ensure_local_undo_redo_bindings(cx);

        let state =
            window.with_element_state(global_id, |state: Option<Entity<DatePickerState>>, _| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let layer_stack = cx.new(|_| LayerStack::new());
                    let state = cx.new(|cx| {
                        let focus_handle = cx.focus_handle();
                        let history = WindowValueHistory::new(
                            undo_manager.clone(),
                            &focus_handle,
                            "Date selection",
                        );
                        DatePickerState::new(
                            focus_handle,
                            layer_stack.clone(),
                            history,
                            date,
                            min,
                            max,
                            on_change.clone(),
                            custom_day_renderer.clone(),
                            custom_popup_renderer.clone(),
                            custom_header_renderer.clone(),
                            custom_nav_button_renderer.clone(),
                            custom_weekday_renderer.clone(),
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
            state.sync_from_props(
                date,
                min,
                max,
                on_change,
                custom_day_renderer,
                custom_popup_renderer,
                custom_header_renderer,
                custom_nav_button_renderer,
                custom_weekday_renderer,
                cx,
            );
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

        (layout_id, DatePickerElementState { root, state })
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

impl DatePickerState {
    fn new(
        focus_handle: FocusHandle,
        layer_stack: Entity<LayerStack>,
        history: WindowValueHistory<Date>,
        date: Date,
        min: Option<Date>,
        max: Option<Date>,
        on_change: Option<ChangeListener>,
        day_renderer: Option<DatePickerDayCustomRenderer>,
        popup_renderer: Option<DatePickerPopupCustomRenderer>,
        header_renderer: Option<DatePickerHeaderCustomRenderer>,
        nav_button_renderer: Option<DatePickerNavButtonCustomRenderer>,
        weekday_renderer: Option<DatePickerWeekdayCustomRenderer>,
    ) -> Self {
        let date = clamp_date(date, min, max);
        let visible_month = month_for_date(date);
        Self {
            focus_handle,
            layer_stack,
            date,
            history,
            min,
            max,
            visible_month,
            highlighted_date: Some(date),
            trigger_bounds: None,
            on_change,
            day_renderer,
            popup_renderer,
            header_renderer,
            nav_button_renderer,
            weekday_renderer,
        }
    }

    fn sync_from_props(
        &mut self,
        date: Date,
        min: Option<Date>,
        max: Option<Date>,
        on_change: Option<ChangeListener>,
        day_renderer: Option<DatePickerDayCustomRenderer>,
        popup_renderer: Option<DatePickerPopupCustomRenderer>,
        header_renderer: Option<DatePickerHeaderCustomRenderer>,
        nav_button_renderer: Option<DatePickerNavButtonCustomRenderer>,
        weekday_renderer: Option<DatePickerWeekdayCustomRenderer>,
        cx: &mut Context<Self>,
    ) {
        let clamped_date = clamp_date(date, min, max);
        let mut changed = false;
        let mut reset_history = false;
        let mut reset_navigation = false;

        if self.date != clamped_date {
            self.date = clamped_date;
            changed = true;
            reset_history = true;
            reset_navigation = true;
        }
        if self.min != min {
            self.min = min;
            changed = true;
            reset_history = true;
            reset_navigation = true;
        }
        if self.max != max {
            self.max = max;
            changed = true;
            reset_history = true;
            reset_navigation = true;
        }
        if self.on_change.as_ref().map(Rc::as_ptr) != on_change.as_ref().map(Rc::as_ptr) {
            self.on_change = on_change;
            changed = true;
        }
        if self.day_renderer.as_ref().map(Rc::as_ptr) != day_renderer.as_ref().map(Rc::as_ptr) {
            self.day_renderer = day_renderer;
            changed = true;
        }
        if self.popup_renderer.as_ref().map(Rc::as_ptr) != popup_renderer.as_ref().map(Rc::as_ptr) {
            self.popup_renderer = popup_renderer;
            changed = true;
        }
        if self.header_renderer.as_ref().map(Rc::as_ptr) != header_renderer.as_ref().map(Rc::as_ptr)
        {
            self.header_renderer = header_renderer;
            changed = true;
        }
        if self.nav_button_renderer.as_ref().map(Rc::as_ptr)
            != nav_button_renderer.as_ref().map(Rc::as_ptr)
        {
            self.nav_button_renderer = nav_button_renderer;
            changed = true;
        }
        if self.weekday_renderer.as_ref().map(Rc::as_ptr)
            != weekday_renderer.as_ref().map(Rc::as_ptr)
        {
            self.weekday_renderer = weekday_renderer;
            changed = true;
        }

        if reset_history {
            self.history.clear();
        }

        if changed {
            if reset_navigation {
                self.visible_month = month_for_date(self.date);
                self.highlighted_date = Some(self.date);
            }
            cx.notify();
        }
    }

    fn snapshot(&self, cx: &App) -> DatePickerSnapshot {
        DatePickerSnapshot {
            focus_handle: self.focus_handle.clone(),
            display_date: self.date,
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

        self.date = clamp_date(self.date, self.min, self.max);
        self.visible_month = month_for_date(self.date);
        self.highlighted_date = Some(self.date);

        let popup = cx.new({
            let selector_id = selector_id.to_string();
            move |cx| DatePickerPopup::new(selector_id.clone(), state, cx)
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

    fn popup_width(&self) -> Pixels {
        self.trigger_bounds
            .map(|bounds| bounds.size.width.max(px(280.0)))
            .unwrap_or(px(280.0))
    }

    fn anchor_position(&self) -> Option<Point<Pixels>> {
        self.trigger_bounds
            .map(|bounds| point(bounds.left(), bounds.bottom()))
    }

    fn popup_snapshot(&self) -> PopupSnapshot {
        let highlighted_date = self.highlighted_date.unwrap_or(self.date);
        PopupSnapshot {
            width: self.popup_width(),
            selected_date: self.date,
            visible_month: self.visible_month,
            highlighted_date,
            min: self.min,
            max: self.max,
        }
    }

    fn navigate_month(&mut self, delta: i32, cx: &mut Context<Self>) {
        let target_month = shift_month(self.visible_month, delta);
        if !month_has_selectable_day(target_month, self.min, self.max) {
            return;
        }

        let base_date = self.highlighted_date.unwrap_or(self.date);
        let candidate = clamp_date(shift_date_by_months(base_date, delta), self.min, self.max);
        self.visible_month = month_for_date(candidate);
        self.highlighted_date = Some(candidate);
        cx.notify();
    }

    fn move_highlight_by_days(&mut self, delta: i64, cx: &mut Context<Self>) {
        let base_date = self.highlighted_date.unwrap_or(self.date);
        let candidate = base_date
            .checked_add(Duration::days(delta))
            .unwrap_or(base_date);
        let candidate = clamp_date(candidate, self.min, self.max);
        self.visible_month = month_for_date(candidate);
        self.highlighted_date = Some(candidate);
        cx.notify();
    }

    fn commit_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let date = self.highlighted_date.unwrap_or(self.date);
        self.commit_date(date, window, cx);
    }

    fn commit_date(&mut self, date: Date, window: &mut Window, cx: &mut Context<Self>) {
        let date = clamp_date(date, self.min, self.max);
        self.close_popup(cx);
        window.focus(&self.focus_handle);
        if self.date != date {
            let previous = self.date;
            self.date = date;
            self.history.record(previous, date);
            self.visible_month = month_for_date(date);
            self.highlighted_date = Some(date);
        }
        if let Some(listener) = self.on_change.clone() {
            listener(&date, window, cx);
        }
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.history.undo() else {
            return;
        };

        self.date = previous;
        self.visible_month = month_for_date(previous);
        self.highlighted_date = Some(previous);
        if let Some(listener) = self.on_change.clone() {
            listener(&previous, window, cx);
        }
    }

    fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(next) = self.history.redo() else {
            return;
        };

        self.date = next;
        self.visible_month = month_for_date(next);
        self.highlighted_date = Some(next);
        if let Some(listener) = self.on_change.clone() {
            listener(&next, window, cx);
        }
    }
}

impl DatePickerPopup {
    fn new(selector_id: String, state: Entity<DatePickerState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| {
            cx.notify();
        })
        .detach();

        Self {
            selector_id,
            state,
            root_focus: cx.focus_handle(),
        }
    }
}

impl EventEmitter<DismissEvent> for DatePickerPopup {}

impl Focusable for DatePickerPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.root_focus.clone()
    }
}

impl Render for DatePickerPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (
            snapshot,
            can_undo,
            can_redo,
            day_renderer,
            popup_renderer,
            header_renderer,
            nav_button_renderer,
            weekday_renderer,
        ) = {
            let state = self.state.read(cx);
            (
                state.popup_snapshot(),
                state.history.can_undo(),
                state.history.can_redo(),
                state.day_renderer.clone(),
                state.popup_renderer.clone(),
                state.header_renderer.clone(),
                state.nav_button_renderer.clone(),
                state.weekday_renderer.clone(),
            )
        };
        let move_state = self.state.clone();
        let month_state = self.state.clone();
        let commit_state = self.state.clone();
        let close_state = self.state.clone();
        let can_prev = month_has_selectable_day(
            shift_month(snapshot.visible_month, -1),
            snapshot.min,
            snapshot.max,
        );
        let can_next = month_has_selectable_day(
            shift_month(snapshot.visible_month, 1),
            snapshot.min,
            snapshot.max,
        );
        let month_label: SharedString = format!(
            "{} {}",
            month_name(snapshot.visible_month.month),
            snapshot.visible_month.year
        )
        .into();
        let popup_render_state = DatePickerPopupRenderState {
            width: snapshot.width,
            month_label: month_label.clone(),
            selected_date: snapshot.selected_date,
            highlighted_date: snapshot.highlighted_date,
            can_navigate_previous: can_prev,
            can_navigate_next: can_next,
        };

        let mut panel = div()
            .id(ElementId::named_usize(
                format!("{}-popup", self.selector_id),
                0,
            ))
            .track_focus(&self.root_focus)
            .focusable()
            .tab_stop(true)
            .key_context(local_undo_redo_key_context())
            .capture_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }

                match event.keystroke.key.as_str() {
                    "left" => {
                        move_state.update(cx, |state, cx| {
                            state.move_highlight_by_days(-1, cx);
                        });
                        window.prevent_default();
                    }
                    "right" => {
                        move_state.update(cx, |state, cx| {
                            state.move_highlight_by_days(1, cx);
                        });
                        window.prevent_default();
                    }
                    "up" => {
                        move_state.update(cx, |state, cx| {
                            state.move_highlight_by_days(-7, cx);
                        });
                        window.prevent_default();
                    }
                    "down" => {
                        move_state.update(cx, |state, cx| {
                            state.move_highlight_by_days(7, cx);
                        });
                        window.prevent_default();
                    }
                    "pageup" => {
                        month_state.update(cx, |state, cx| {
                            state.navigate_month(-1, cx);
                        });
                        window.prevent_default();
                    }
                    "pagedown" => {
                        month_state.update(cx, |state, cx| {
                            state.navigate_month(1, cx);
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
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group));

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

        #[cfg(any(test, feature = "test-support"))]
        {
            let popup_selector = format!("date-picker-popup-{}", self.selector_id);
            panel = panel.debug_selector(move || popup_selector);
        }

        let prev_state = self.state.clone();
        let next_state = self.state.clone();
        let prev_id: SharedString = format!("{}-prev", self.selector_id).into();
        let next_id: SharedString = format!("{}-next", self.selector_id).into();

        let mut prev_button = calendar_nav_button("<", can_prev)
            .id((prev_id, 0usize))
            .on_click(move |event, _, cx| {
                if !event.standard_click() || !can_prev {
                    return;
                }

                prev_state.update(cx, |state, cx| {
                    state.navigate_month(-1, cx);
                });
            });

        let mut next_button = calendar_nav_button(">", can_next)
            .id((next_id, 0usize))
            .on_click(move |event, _, cx| {
                if !event.standard_click() || !can_next {
                    return;
                }

                next_state.update(cx, |state, cx| {
                    state.navigate_month(1, cx);
                });
            });

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector_id = self.selector_id.clone();
            let prev_selector = format!("date-picker-prev-{}", selector_id);
            prev_button = prev_button.debug_selector(move || prev_selector);

            let next_selector = format!("date-picker-next-{}", self.selector_id);
            next_button = next_button.debug_selector(move || next_selector);
        }
        let prev_button = if let Some(renderer) = &nav_button_renderer {
            renderer(
                DatePickerNavButtonRenderState {
                    direction: DatePickerNavigationDirection::PreviousMonth,
                    label: SharedString::from("<"),
                    enabled: can_prev,
                },
                prev_button.into_any_element(),
                window,
                cx,
            )
        } else {
            prev_button.into_any_element()
        };

        let next_button = if let Some(renderer) = &nav_button_renderer {
            renderer(
                DatePickerNavButtonRenderState {
                    direction: DatePickerNavigationDirection::NextMonth,
                    label: SharedString::from(">"),
                    enabled: can_next,
                },
                next_button.into_any_element(),
                window,
                cx,
            )
        } else {
            next_button.into_any_element()
        };

        let header = if let Some(renderer) = &header_renderer {
            renderer(
                DatePickerHeaderRenderState {
                    month_label: month_label.clone(),
                    can_navigate_previous: can_prev,
                    can_navigate_next: can_next,
                },
                prev_button,
                next_button,
                window,
                cx,
            )
        } else {
            default_date_picker_header(
                DatePickerHeaderRenderState {
                    month_label: month_label.clone(),
                    can_navigate_previous: can_prev,
                    can_navigate_next: can_next,
                },
                prev_button,
                next_button,
            )
        };

        let weekday_labels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                if let Some(renderer) = &weekday_renderer {
                    renderer(
                        DatePickerWeekdayRenderState {
                            index,
                            label: SharedString::from(label),
                        },
                        window,
                        cx,
                    )
                } else {
                    weekday_label(label)
                }
            })
            .collect::<Vec<_>>();

        let mut popup_children = vec![header, default_date_picker_weekday_row(weekday_labels)];

        for week in build_month_grid(snapshot.visible_month, snapshot.min, snapshot.max).chunks(7) {
            let mut row = div().flex().gap_1();
            for cell in week {
                row = row.child(render_day_cell(
                    &self.selector_id,
                    *cell,
                    snapshot.selected_date,
                    snapshot.highlighted_date,
                    self.state.clone(),
                    day_renderer.as_ref(),
                    window,
                    cx,
                ));
            }
            popup_children.push(row.into_any_element());
        }

        let popup_body = if let Some(renderer) = &popup_renderer {
            renderer(popup_render_state, popup_children, window, cx)
        } else {
            default_date_picker_popup_body(popup_render_state, popup_children)
        };

        panel.child(popup_body).into_any_element()
    }
}

fn default_date_picker_popup_body(
    state: DatePickerPopupRenderState,
    children: Vec<AnyElement>,
) -> AnyElement {
    div()
        .min_w(state.width)
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .rounded(px(10.0))
        .border_1()
        .border_color(crate::rgb(0xcbd5e1))
        .bg(crate::rgb(0xffffff))
        .shadow_lg()
        .children(children)
        .into_any_element()
}

fn default_date_picker_header(
    state: DatePickerHeaderRenderState,
    prev_button: AnyElement,
    next_button: AnyElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(prev_button)
        .child(
            div()
                .font_weight(crate::FontWeight::SEMIBOLD)
                .child(state.month_label),
        )
        .child(next_button)
        .into_any_element()
}

fn default_date_picker_weekday_row(children: Vec<AnyElement>) -> AnyElement {
    div().flex().gap_1().children(children).into_any_element()
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

fn render_day_cell(
    selector_id: &str,
    cell: Option<CalendarDayCell>,
    selected_date: Date,
    highlighted_date: Date,
    state: Entity<DatePickerState>,
    day_renderer: Option<&DatePickerDayCustomRenderer>,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let Some(cell) = cell else {
        return div().w(px(36.0)).h(px(36.0)).into_any_element();
    };

    let is_selected = cell.date == selected_date;
    let is_highlighted = cell.date == highlighted_date;
    let day_id: SharedString = format!("{}-day-{}", selector_id, format_date(cell.date)).into();
    let mut day = div()
        .id(day_id)
        .child(if let Some(renderer) = day_renderer {
            renderer(
                DatePickerDayRenderState {
                    date: cell.date,
                    day: cell.day,
                    disabled: cell.disabled,
                    selected: is_selected,
                    highlighted: is_highlighted,
                },
                window,
                cx,
            )
        } else {
            default_day_cell_body(cell, is_selected, is_highlighted)
        });

    if !cell.disabled {
        day = day
            .cursor_pointer()
            .hover(|style| style.bg(crate::rgb(0xf1f5f9)))
            .on_click(move |event, window, cx| {
                if !event.standard_click() {
                    return;
                }

                state.update(cx, |state, cx| {
                    state.commit_date(cell.date, window, cx);
                });
            });
    }

    #[cfg(any(test, feature = "test-support"))]
    {
        let day_selector = format!("date-picker-day-{}-{}", selector_id, format_date(cell.date));
        day = day.debug_selector(move || day_selector);
    }

    day.into_any_element()
}

fn default_day_cell_body(cell: CalendarDayCell, selected: bool, highlighted: bool) -> AnyElement {
    div()
        .w(px(36.0))
        .h(px(36.0))
        .rounded(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .text_color(if cell.disabled {
            crate::rgb(0x94a3b8)
        } else if selected {
            crate::rgb(0xffffff)
        } else {
            crate::rgb(0x0f172a)
        })
        .bg(if cell.disabled {
            crate::rgba(0x00000000)
        } else if selected {
            crate::rgb(0x1d4ed8)
        } else if highlighted {
            crate::rgb(0xe0f2fe)
        } else {
            crate::rgba(0x00000000)
        })
        .border_1()
        .border_color(if highlighted && !selected {
            crate::rgb(0x1d4ed8)
        } else {
            crate::rgba(0x00000000)
        })
        .child(cell.day.to_string())
        .into_any_element()
}

fn calendar_nav_button(label: &'static str, enabled: bool) -> crate::elements::div::Div {
    div()
        .w(px(32.0))
        .h(px(32.0))
        .rounded(px(8.0))
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(crate::rgb(0xcbd5e1))
        .bg(crate::rgb(0xffffff))
        .text_color(if enabled {
            crate::rgb(0x0f172a)
        } else {
            crate::rgb(0x94a3b8)
        })
        .cursor_pointer()
        .child(label)
}

fn weekday_label(label: &'static str) -> AnyElement {
    div()
        .w(px(36.0))
        .h(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(crate::rgb(0x64748b))
        .child(label)
        .into_any_element()
}

fn clamp_date(date: Date, min: Option<Date>, max: Option<Date>) -> Date {
    let mut date = date;
    if let Some(min) = min
        && date < min
    {
        date = min;
    }
    if let Some(max) = max
        && date > max
    {
        date = max;
    }
    date
}

fn month_for_date(date: Date) -> CalendarMonth {
    CalendarMonth {
        year: date.year(),
        month: date.month(),
    }
}

fn shift_month(month: CalendarMonth, delta: i32) -> CalendarMonth {
    let total_months = month.year * 12 + i32::from(month_number(month.month) - 1) + delta;
    let year = total_months.div_euclid(12);
    let month_index = total_months.rem_euclid(12) + 1;
    CalendarMonth {
        year,
        month: month_from_number(month_index as u8),
    }
}

fn shift_date_by_months(date: Date, delta: i32) -> Date {
    let target_month = shift_month(month_for_date(date), delta);
    let target_day = date
        .day()
        .min(days_in_month(target_month.year, target_month.month));
    Date::from_calendar_date(target_month.year, target_month.month, target_day).unwrap()
}

fn month_has_selectable_day(month: CalendarMonth, min: Option<Date>, max: Option<Date>) -> bool {
    let first = Date::from_calendar_date(month.year, month.month, 1).unwrap();
    let last = Date::from_calendar_date(
        month.year,
        month.month,
        days_in_month(month.year, month.month),
    )
    .unwrap();

    if let Some(min) = min
        && last < min
    {
        return false;
    }
    if let Some(max) = max
        && first > max
    {
        return false;
    }

    true
}

fn build_month_grid(
    month: CalendarMonth,
    min: Option<Date>,
    max: Option<Date>,
) -> Vec<Option<CalendarDayCell>> {
    let first = Date::from_calendar_date(month.year, month.month, 1).unwrap();
    let offset = first.weekday().number_days_from_monday() as usize;
    let day_count = days_in_month(month.year, month.month) as usize;
    let cell_count = (offset + day_count).div_ceil(7).max(5) * 7;
    let mut cells = Vec::with_capacity(cell_count);

    for index in 0..cell_count {
        if index < offset || index >= offset + day_count {
            cells.push(None);
            continue;
        }

        let day = (index - offset + 1) as u8;
        let date = Date::from_calendar_date(month.year, month.month, day).unwrap();
        let disabled = min.is_some_and(|min| date < min) || max.is_some_and(|max| date > max);
        cells.push(Some(CalendarDayCell {
            date,
            day,
            disabled,
        }));
    }

    cells
}

fn days_in_month(year: i32, month: Month) -> u8 {
    match month {
        Month::January => 31,
        Month::February => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        Month::March => 31,
        Month::April => 30,
        Month::May => 31,
        Month::June => 30,
        Month::July => 31,
        Month::August => 31,
        Month::September => 30,
        Month::October => 31,
        Month::November => 30,
        Month::December => 31,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn month_number(month: Month) -> u8 {
    match month {
        Month::January => 1,
        Month::February => 2,
        Month::March => 3,
        Month::April => 4,
        Month::May => 5,
        Month::June => 6,
        Month::July => 7,
        Month::August => 8,
        Month::September => 9,
        Month::October => 10,
        Month::November => 11,
        Month::December => 12,
    }
}

fn month_from_number(number: u8) -> Month {
    match number {
        1 => Month::January,
        2 => Month::February,
        3 => Month::March,
        4 => Month::April,
        5 => Month::May,
        6 => Month::June,
        7 => Month::July,
        8 => Month::August,
        9 => Month::September,
        10 => Month::October,
        11 => Month::November,
        12 => Month::December,
        _ => panic!("invalid month number: {number}"),
    }
}

fn month_name(month: Month) -> &'static str {
    match month {
        Month::January => "January",
        Month::February => "February",
        Month::March => "March",
        Month::April => "April",
        Month::May => "May",
        Month::June => "June",
        Month::July => "July",
        Month::August => "August",
        Month::September => "September",
        Month::October => "October",
        Month::November => "November",
        Month::December => "December",
    }
}

fn format_date(date: Date) -> SharedString {
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        month_number(date.month()),
        date.day()
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessibilityRole, AccessibilityValue, Context, Modifiers, Render, TestAppContext, Undo,
        div,
    };

    struct DatePickerView {
        date: Date,
    }

    struct CustomDatePickerView {
        date: Date,
    }

    impl Render for DatePickerView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            date_picker("schedule", self.date)
                .min(Date::from_calendar_date(2026, Month::May, 10).unwrap())
                .max(Date::from_calendar_date(2026, Month::May, 25).unwrap())
                .on_change(cx.listener(|this, date, _, cx| {
                    this.date = *date;
                    cx.notify();
                }))
        }
    }

    impl Render for CustomDatePickerView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            date_picker("schedule_custom", self.date)
                .render_with(|state, _, _| {
                    let selector = format!(
                        "date-picker-trigger-{}-{}",
                        state.display_label,
                        if state.open { "open" } else { "closed" },
                    );
                    div()
                        .debug_selector(move || selector)
                        .child(state.display_label)
                        .into_any_element()
                })
                .render_popup_with(|state, children, _, _| {
                    let selector = format!(
                        "date-picker-popup-shell-{}",
                        state.month_label.to_string().replace(' ', "-"),
                    );
                    div()
                        .debug_selector(move || selector)
                        .children(children)
                        .into_any_element()
                })
                .render_header_with(|state, prev, next, _, _| {
                    let selector = format!(
                        "date-picker-header-{}",
                        state.month_label.to_string().replace(' ', "-"),
                    );
                    div()
                        .debug_selector(move || selector)
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(prev)
                        .child(state.month_label)
                        .child(next)
                        .into_any_element()
                })
                .render_nav_buttons_with(|state, button, _, _| {
                    let selector = format!(
                        "date-picker-nav-{}-{}",
                        match state.direction {
                            DatePickerNavigationDirection::PreviousMonth => "prev",
                            DatePickerNavigationDirection::NextMonth => "next",
                        },
                        if state.enabled { "enabled" } else { "disabled" },
                    );
                    div()
                        .debug_selector(move || selector)
                        .child(button)
                        .into_any_element()
                })
                .render_weekdays_with(|state, _, _| {
                    let selector = format!("date-picker-weekday-{}-{}", state.index, state.label);
                    div()
                        .debug_selector(move || selector)
                        .child(state.label)
                        .into_any_element()
                })
                .render_days_with(|state, _, _| {
                    let selector = format!(
                        "date-picker-custom-day-{}-{}-{}",
                        format_date(state.date),
                        if state.selected { "selected" } else { "idle" },
                        if state.highlighted {
                            "highlighted"
                        } else {
                            "plain"
                        },
                    );
                    div()
                        .debug_selector(move || selector)
                        .w(px(36.0))
                        .h(px(36.0))
                        .rounded(px(8.0))
                        .child(state.day.to_string())
                        .into_any_element()
                })
                .on_change(cx.listener(|this, date, _, cx| {
                    this.date = *date;
                    cx.notify();
                }))
        }
    }

    #[test]
    fn clamp_date_respects_min_and_max() {
        let min = Date::from_calendar_date(2026, Month::May, 10).unwrap();
        let max = Date::from_calendar_date(2026, Month::May, 20).unwrap();

        assert_eq!(
            clamp_date(
                Date::from_calendar_date(2026, Month::May, 1).unwrap(),
                Some(min),
                Some(max)
            ),
            min
        );
        assert_eq!(
            clamp_date(
                Date::from_calendar_date(2026, Month::May, 30).unwrap(),
                Some(min),
                Some(max)
            ),
            max
        );
        assert_eq!(
            clamp_date(
                Date::from_calendar_date(2026, Month::May, 15).unwrap(),
                Some(min),
                Some(max)
            ),
            Date::from_calendar_date(2026, Month::May, 15).unwrap()
        );
    }

    #[test]
    fn render_state_summaries_are_content_safe() {
        let selected = Date::from_calendar_date(2026, Month::May, 20).unwrap();
        let highlighted = Date::from_calendar_date(2026, Month::May, 21).unwrap();

        let trigger = DatePickerRenderState {
            open: true,
            date: selected,
            display_label: "2026-05-20".into(),
            focused: false,
        };
        assert_eq!(trigger.display_label_len_bytes(), 10);
        assert_eq!(
            trigger.to_text(),
            "date picker render state: open true, focused false, display-label-bytes 10"
        );
        assert!(!trigger.to_text().contains("2026"));
        assert!(!trigger.to_text().contains("05-20"));

        let day = DatePickerDayRenderState {
            date: selected,
            day: 20,
            disabled: false,
            selected: true,
            highlighted: false,
        };
        assert!(day.is_selectable());
        assert_eq!(
            day.to_text(),
            "date picker day render state: selectable true, selected true, highlighted false"
        );
        assert!(!day.to_text().contains("20"));
        assert!(!day.to_text().contains("2026"));

        let popup = DatePickerPopupRenderState {
            width: px(280.0),
            month_label: "May 2026".into(),
            selected_date: selected,
            highlighted_date: highlighted,
            can_navigate_previous: true,
            can_navigate_next: false,
        };
        assert_eq!(popup.month_label_len_bytes(), 8);
        assert!(!popup.selected_is_highlighted());
        assert!(popup.can_navigate());
        assert_eq!(
            popup.to_text(),
            "date picker popup render state: month-label-bytes 8, selected-highlighted false, can-prev true, can-next false, can-navigate true"
        );
        assert!(!popup.to_text().contains("May"));
        assert!(!popup.to_text().contains("2026"));
        assert!(!popup.to_text().contains("280"));

        let header = DatePickerHeaderRenderState {
            month_label: "May 2026".into(),
            can_navigate_previous: false,
            can_navigate_next: false,
        };
        assert_eq!(header.month_label_len_bytes(), 8);
        assert!(!header.can_navigate());
        assert_eq!(
            header.to_text(),
            "date picker header render state: month-label-bytes 8, can-prev false, can-next false, can-navigate false"
        );
        assert!(!header.to_text().contains("May"));

        let nav = DatePickerNavButtonRenderState {
            direction: DatePickerNavigationDirection::PreviousMonth,
            label: "< Previous".into(),
            enabled: true,
        };
        assert_eq!(
            DatePickerNavigationDirection::NextMonth.to_text(),
            "next-month"
        );
        assert_eq!(nav.label_len_bytes(), 10);
        assert_eq!(
            nav.to_text(),
            "date picker nav button render state: direction previous-month, enabled true, label-bytes 10"
        );
        assert!(!nav.to_text().contains("Previous"));

        let weekday = DatePickerWeekdayRenderState {
            index: 2,
            label: "Wednesday".into(),
        };
        assert_eq!(weekday.label_len_bytes(), 9);
        assert_eq!(
            weekday.to_text(),
            "date picker weekday render state: index 2, label-bytes 9"
        );
        assert!(!weekday.to_text().contains("Wednesday"));
    }

    #[test]
    fn build_month_grid_handles_leap_year_february() {
        let month = CalendarMonth {
            year: 2028,
            month: Month::February,
        };
        let cells = build_month_grid(month, None, None);
        let populated = cells.into_iter().flatten().count();

        assert_eq!(populated, 29);
    }

    #[crate::test]
    fn date_picker_click_selects_day(cx: &mut TestAppContext) {
        let initial = Date::from_calendar_date(2026, Month::May, 13).unwrap();
        let selected = Date::from_calendar_date(2026, Month::May, 20).unwrap();
        let (view, mut window) = cx.add_window_view(|_, _| DatePickerView { date: initial });

        window.update(|window, cx| {
            window.draw(cx).clear();

            let picker = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .unwrap();
            assert_eq!(
                picker.value,
                Some(AccessibilityValue::Text(format_date(initial).to_string()))
            );
        });

        let trigger_bounds = window.debug_bounds("date-picker-schedule").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let picker = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .unwrap();
            assert!(picker.states.contains(AccessibilityState::EXPANDED));
        });

        assert!(window.debug_bounds("date-picker-popup-schedule").is_some());

        let selected_bounds = window
            .debug_bounds("date-picker-day-schedule-2026-05-20")
            .unwrap();
        window.simulate_click(selected_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).date, selected);

            let picker = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .unwrap();
            assert_eq!(
                picker.value,
                Some(AccessibilityValue::Text(format_date(selected).to_string()))
            );
        });
    }

    #[crate::test]
    fn date_picker_undo_redo_tracks_shared_history(cx: &mut TestAppContext) {
        let initial = Date::from_calendar_date(2026, Month::May, 13).unwrap();
        let selected = Date::from_calendar_date(2026, Month::May, 20).unwrap();
        let (view, mut window) = cx.add_window_view(|_, _| DatePickerView { date: initial });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!window.is_action_available(&Undo, cx));
        });

        let trigger_bounds = window.debug_bounds("date-picker-schedule").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let selected_bounds = window
            .debug_bounds("date-picker-day-schedule-2026-05-20")
            .unwrap();
        window.simulate_click(selected_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).date, selected);
            assert!(window.is_action_available(&Undo, cx));
        });

        window.simulate_keystrokes("secondary-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).date, initial);
        });

        window.simulate_keystrokes("secondary-shift-z");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).date, selected);
        });
    }

    #[crate::test]
    fn date_picker_render_hooks_receive_trigger_and_day_state(cx: &mut TestAppContext) {
        let initial = Date::from_calendar_date(2026, Month::May, 13).unwrap();
        let selected = Date::from_calendar_date(2026, Month::May, 20).unwrap();
        let (_view, mut window) = cx.add_window_view(|_, _| CustomDatePickerView { date: initial });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("date-picker-trigger-2026-05-13-closed")
                .is_some()
        );

        let trigger_bounds = window.debug_bounds("date-picker-schedule_custom").unwrap();
        window.simulate_click(trigger_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("date-picker-popup-shell-May-2026")
                .is_some()
        );
        assert!(window.debug_bounds("date-picker-header-May-2026").is_some());
        assert!(
            window
                .debug_bounds("date-picker-nav-prev-enabled")
                .is_some()
        );
        assert!(window.debug_bounds("date-picker-weekday-0-Mon").is_some());
        assert!(
            window
                .debug_bounds("date-picker-custom-day-2026-05-13-selected-highlighted")
                .is_some()
        );

        let selected_bounds = window
            .debug_bounds("date-picker-day-schedule_custom-2026-05-20")
            .unwrap();
        window.simulate_click(selected_bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("date-picker-trigger-2026-05-20-closed")
                .is_some()
        );
        assert_eq!(
            selected,
            Date::from_calendar_date(2026, Month::May, 20).unwrap()
        );
    }
}
