//! DatePicker component - Date selection with calendar popup and keyboard navigation.

use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::calendar::{Calendar, CalendarLocale, DateRange, DateValue};
use crate::components::field::{Field, FieldStatusType};
use crate::components::field_status::FieldStatusVariant;
use crate::components::icon::Icon;
use crate::components::input::InputSize;
use crate::components::spinner::{Spinner, SpinnerSize};
use crate::overlays::popover::{Popover, PopoverContent};
use crate::theme::{use_theme, Theme};

/// Date format options
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DateFormat {
    /// YYYY-MM-DD (e.g., 2025-01-15)
    #[default]
    IsoDate,
    /// MM/DD/YYYY (e.g., 01/15/2025)
    UsDate,
    /// DD/MM/YYYY (e.g., 15/01/2025)
    EuDate,
    /// Month DD, YYYY (e.g., January 15, 2025)
    LongDate,
}

impl DateFormat {
    /// Format a DateValue according to the format
    pub fn format(&self, date: &DateValue, locale: &CalendarLocale) -> String {
        match self {
            DateFormat::IsoDate => {
                format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
            }
            DateFormat::UsDate => {
                format!("{:02}/{:02}/{:04}", date.month, date.day, date.year)
            }
            DateFormat::EuDate => {
                format!("{:02}/{:02}/{:04}", date.day, date.month, date.year)
            }
            DateFormat::LongDate => {
                let month_name = if date.month >= 1 && date.month <= 12 {
                    locale.months[(date.month - 1) as usize].clone()
                } else {
                    "Unknown".into()
                };
                format!("{} {:02}, {:04}", month_name, date.day, date.year)
            }
        }
    }

    /// Format a range for trigger display, following ASTRYX's compact range
    /// style for range inputs.
    pub fn format_range(&self, range: &DateRange, locale: &CalendarLocale) -> String {
        let range = DateRange::new(range.start, range.end);
        match self {
            DateFormat::LongDate => {
                let start_month = if range.start.month >= 1 && range.start.month <= 12 {
                    locale.months[(range.start.month - 1) as usize].clone()
                } else {
                    "Unknown".into()
                };
                let end_month = if range.end.month >= 1 && range.end.month <= 12 {
                    locale.months[(range.end.month - 1) as usize].clone()
                } else {
                    "Unknown".into()
                };

                if range.start.year == range.end.year && range.start.month == range.end.month {
                    format!(
                        "{} {}-{}, {}",
                        start_month, range.start.day, range.end.day, range.start.year
                    )
                } else {
                    format!(
                        "{} {}, {} - {} {}, {}",
                        start_month,
                        range.start.day,
                        range.start.year,
                        end_month,
                        range.end.day,
                        range.end.year
                    )
                }
            }
            _ => format!(
                "{} - {}",
                self.format(&range.start, locale),
                self.format(&range.end, locale)
            ),
        }
    }
}

/// Date selection mode
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateSelectionMode {
    /// Single date selection
    Single,
    /// Date range selection
    Range,
}

/// State for DatePicker component
pub struct DatePickerState {
    pub selected_date: Option<DateValue>,
    pub selected_range: Option<DateRange>,
    pub range_start_temp: Option<DateValue>, // Temporary storage for first click in range mode
    pub selection_mode: DateSelectionMode,
    pub is_open: bool,
    pub viewing_month: DateValue,
    focus_handle: FocusHandle,
}

impl DatePickerState {
    pub fn new(cx: &mut App) -> Self {
        let today = DateValue::today();
        Self {
            selected_date: None,
            selected_range: None,
            range_start_temp: None,
            selection_mode: DateSelectionMode::Single,
            is_open: false,
            viewing_month: today,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn new_with_date(date: DateValue, cx: &mut App) -> Self {
        Self {
            selected_date: Some(date),
            selected_range: None,
            range_start_temp: None,
            selection_mode: DateSelectionMode::Single,
            is_open: false,
            viewing_month: date,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn new_with_mode(mode: DateSelectionMode, cx: &mut App) -> Self {
        let today = DateValue::today();
        Self {
            selected_date: None,
            selected_range: None,
            range_start_temp: None,
            selection_mode: mode,
            is_open: false,
            viewing_month: today,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn select_date(&mut self, date: DateValue, _cx: &mut App) {
        match self.selection_mode {
            DateSelectionMode::Single => {
                self.selected_date = Some(date);
                self.viewing_month = date;
            }
            DateSelectionMode::Range => {
                if let Some(start) = self.range_start_temp {
                    // Second click - complete the range
                    let (range_start, range_end) = if date.year < start.year
                        || (date.year == start.year && date.month < start.month)
                        || (date.year == start.year
                            && date.month == start.month
                            && date.day < start.day)
                    {
                        (date, start)
                    } else {
                        (start, date)
                    };
                    self.selected_range = Some(DateRange {
                        start: range_start,
                        end: range_end,
                    });
                    self.range_start_temp = None;
                } else {
                    // First click - store the start date
                    self.range_start_temp = Some(date);
                    self.selected_range = None;
                }
                self.viewing_month = date;
            }
        }
    }

    pub fn set_selection_mode(&mut self, mode: DateSelectionMode, _cx: &mut App) {
        self.selection_mode = mode;
        // Clear selections when changing mode
        self.selected_date = None;
        self.selected_range = None;
        self.range_start_temp = None;
    }

    pub fn clear_date(&mut self, _cx: &mut App) {
        self.selected_date = None;
        self.selected_range = None;
        self.range_start_temp = None;
    }

    pub fn set_viewing_month(&mut self, date: DateValue, _cx: &mut App) {
        self.viewing_month = date;
    }

    pub fn toggle_open(&mut self, _cx: &mut App) {
        self.is_open = !self.is_open;
    }

    pub fn close(&mut self, _cx: &mut App) {
        self.is_open = false;
    }

    pub fn open(&mut self, _cx: &mut App) {
        self.is_open = true;
    }

    pub fn jump_to_today(&mut self, _cx: &mut App) {
        let today = DateValue::today();
        self.viewing_month = today;
    }
}

impl Focusable for DatePickerState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for DatePickerState {}

actions!(
    date_picker,
    [ClosePicker, SelectToday, NextMonth, PrevMonth]
);

/// Initialize DatePicker keybindings
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", ClosePicker, Some("DatePicker")),
        KeyBinding::new("cmd-t", SelectToday, Some("DatePicker")),
        KeyBinding::new("cmd-]", NextMonth, Some("DatePicker")),
        KeyBinding::new("cmd-[", PrevMonth, Some("DatePicker")),
    ]);
}

/// DatePicker component with calendar popup
#[derive(IntoElement)]
pub struct DatePicker {
    state: Entity<DatePickerState>,
    label: Option<SharedString>,
    label_hidden: bool,
    description: Option<SharedString>,
    placeholder: SharedString,
    format: DateFormat,
    min_date: Option<DateValue>,
    max_date: Option<DateValue>,
    disabled_dates: Vec<DateValue>,
    disabled: bool,
    optional: bool,
    required: bool,
    loading: bool,
    size: InputSize,
    clearable: bool,
    status: Option<(FieldStatusType, SharedString)>,
    show_today_button: bool,
    on_select: Option<Rc<dyn Fn(&DateValue, &mut Window, &mut App)>>,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    locale: CalendarLocale,
    style: StyleRefinement,
}

impl DatePicker {
    pub fn new(state: Entity<DatePickerState>) -> Self {
        Self {
            state,
            label: None,
            label_hidden: false,
            description: None,
            placeholder: "Select date...".into(),
            format: DateFormat::default(),
            min_date: None,
            max_date: None,
            disabled_dates: Vec::new(),
            disabled: false,
            optional: false,
            required: false,
            loading: false,
            size: InputSize::default(),
            clearable: true,
            status: None,
            show_today_button: true,
            on_select: None,
            on_clear: None,
            locale: CalendarLocale::default(),
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn is_label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set placeholder text
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Set date format
    pub fn format(mut self, format: DateFormat) -> Self {
        self.format = format;
        self
    }

    /// Set minimum selectable date
    pub fn min_date(mut self, date: DateValue) -> Self {
        self.min_date = Some(date);
        self
    }

    pub fn min(self, date: DateValue) -> Self {
        self.min_date(date)
    }

    /// Set maximum selectable date
    pub fn max_date(mut self, date: DateValue) -> Self {
        self.max_date = Some(date);
        self
    }

    pub fn max(self, date: DateValue) -> Self {
        self.max_date(date)
    }

    /// Add a disabled date
    pub fn disabled_date(mut self, date: DateValue) -> Self {
        self.disabled_dates.push(date);
        self
    }

    /// Set disabled dates
    pub fn disabled_dates(mut self, dates: Vec<DateValue>) -> Self {
        self.disabled_dates = dates;
        self
    }

    /// Disable weekends
    pub fn disable_weekends(self) -> Self {
        self
    }

    /// Set disabled state
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn is_loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.is_loading(loading)
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    /// Enable/disable clear button
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasClear(self, clearable: bool) -> Self {
        self.clearable(clearable)
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }

    /// Show/hide today button
    pub fn show_today_button(mut self, show: bool) -> Self {
        self.show_today_button = show;
        self
    }

    /// Set callback when a date is selected
    pub fn on_select<F>(mut self, handler: F) -> Self
    where
        F: Fn(&DateValue, &mut Window, &mut App) + 'static,
    {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Set callback when date is cleared
    pub fn on_clear<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_clear = Some(Rc::new(handler));
        self
    }

    /// Set locale for month and day names
    pub fn locale(mut self, locale: CalendarLocale) -> Self {
        self.locale = locale;
        self
    }
}

impl Styled for DatePicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DatePicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let state_entity = self.state.clone();
        let state = self.state.read(cx);
        let focus_handle = state.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);

        let selected_date = state.selected_date;
        let selected_range = state.selected_range;
        let _viewing_month = state.viewing_month;
        let locale = self.locale.clone();
        let format = self.format;
        let disabled = self.disabled || self.loading;
        let clearable = self.clearable;
        let show_today_button = self.show_today_button;
        let status = self.status.clone();

        let display_text = if state.selection_mode == DateSelectionMode::Range {
            if let Some(range) = selected_range {
                format.format_range(&range, &locale)
            } else {
                self.placeholder.to_string()
            }
        } else if let Some(date) = selected_date {
            format.format(&date, &locale)
        } else {
            self.placeholder.to_string()
        };

        let has_value = selected_date.is_some() || selected_range.is_some();
        let text_color = if has_value {
            theme.tokens.foreground
        } else {
            theme.tokens.muted_foreground
        };
        let hover_ring = crate::astryx::input_hover_ring(theme.tokens.input);
        let focus_ring = crate::astryx::focus_ring(theme.tokens.primary);
        let (height, padding_x, text_size, icon_size) = match self.size {
            InputSize::Sm => (px(28.0), px(12.0), px(14.0), px(16.0)),
            InputSize::Md => (px(32.0), px(12.0), px(14.0), px(16.0)),
            InputSize::Lg => (px(36.0), px(12.0), px(14.0), px(16.0)),
        };

        let state_for_clear = state_entity.clone();
        let state_for_calendar = state_entity.clone();
        let state_for_today = state_entity.clone();
        let on_select_handler = self.on_select.clone();
        let on_clear_handler = self.on_clear.clone();
        let min_date = self.min_date;
        let max_date = self.max_date;
        let disabled_dates = self.disabled_dates.clone();

        let user_style = self.style;

        let popover_id = ElementId::Name(
            format!("date-picker-popover-{}", state_entity.entity_id().as_u64()).into(),
        );

        let control = Popover::new(popover_id.clone())
            .trigger(
                div()
                    .track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    .flex()
                    .items_center()
                    .w_full()
                    .h(height)
                    .min_w(px(180.0))
                    .px(padding_x)
                    .gap(px(8.0))
                    .bg(theme.tokens.card)
                    .border_1()
                    .border_color(if let Some((status, _)) = status {
                        match status {
                            FieldStatusType::Warning => theme.tokens.warning,
                            FieldStatusType::Error => theme.tokens.destructive,
                            FieldStatusType::Success => theme.tokens.success,
                        }
                    } else if is_focused {
                        theme.tokens.primary
                    } else {
                        theme.tokens.input
                    })
                    .rounded(theme.tokens.radius_md)
                    .transition(theme.tokens.transition_fast)
                    .shadow(smallvec::smallvec![crate::astryx::focus_ring(
                        kael::transparent_black()
                    )])
                    .when(!disabled, |div| {
                        div.cursor(CursorStyle::PointingHand).hover(move |style| {
                            style
                                .border_color(theme.tokens.input)
                                .shadow(smallvec::smallvec![hover_ring])
                        })
                    })
                    .when(is_focused && !disabled, |div| {
                        div.shadow(smallvec::smallvec![focus_ring])
                    })
                    .when(disabled, |div| {
                        div.cursor(CursorStyle::OperationNotAllowed).opacity(0.5)
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .child(
                                Icon::new("calendar")
                                    .size(icon_size)
                                    .color(theme.tokens.muted_foreground),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(text_size)
                            .line_height(px(20.0))
                            .text_color(text_color)
                            .child(display_text),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .when(clearable && has_value && !disabled, |parent_div| {
                                let on_clear = on_clear_handler.clone();
                                let muted_bg = theme.tokens.muted;
                                let muted_fg = theme.tokens.muted_foreground;
                                parent_div.child(
                                    div()
                                        .p(px(2.0))
                                        .rounded(theme.tokens.radius_sm)
                                        .cursor_pointer()
                                        .hover(move |style| style.bg(muted_bg.opacity(0.8)))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            move |_event: &MouseDownEvent, window, cx| {
                                                cx.stop_propagation();
                                                state_for_clear.update(cx, |state, cx| {
                                                    state.clear_date(cx);
                                                });
                                                if let Some(handler) = on_clear.as_ref() {
                                                    handler(window, cx);
                                                }
                                            },
                                        )
                                        .child(Icon::new("x").size(icon_size).color(muted_fg)),
                                )
                            })
                            .when(self.loading, |parent_div| {
                                parent_div.child(Spinner::new().size(SpinnerSize::Sm))
                            })
                            .when_some(self.status.clone(), |parent_div, (status, _)| {
                                let (icon, color) = match status {
                                    FieldStatusType::Warning => {
                                        ("triangle-alert", theme.tokens.warning)
                                    }
                                    FieldStatusType::Error => {
                                        ("circle-alert", theme.tokens.destructive)
                                    }
                                    FieldStatusType::Success => {
                                        ("circle-check", theme.tokens.success)
                                    }
                                };
                                parent_div.child(Icon::new(icon).size(icon_size).color(color))
                            }),
                    ),
            )
            .content(move |window: &mut Window, app_cx: &mut App| {
                let state_ref = state_for_calendar.clone();
                let on_select_ref = on_select_handler.clone();
                let locale_ref = locale.clone();
                let disabled_dates_ref = disabled_dates.clone();
                let state_today_ref = state_for_today.clone();

                app_cx.new(move |cx| {
                    PopoverContent::new(
                        window,
                        cx,
                        move |_window, popover_cx: &mut Context<PopoverContent>| {
                            let theme = use_theme();
                            let state = state_ref.read(popover_cx);
                            let viewing_month = state.viewing_month;
                            let selected_date = state.selected_date;
                            let selected_range = state.selected_range;
                            let range_start_temp = state.range_start_temp;

                            let state_for_select = state_ref.clone();
                            let state_for_month = state_ref.clone();
                            let on_select = on_select_ref.clone();
                            let locale_clone = locale_ref.clone();
                            let min_date_clone = min_date;
                            let max_date_clone = max_date;
                            let disabled_dates_clone = disabled_dates_ref.clone();
                            let state_today = state_today_ref.clone();
                            let border_color = theme.tokens.border;

                            // Get entity reference for closing popover
                            let popover_entity = popover_cx.entity().clone();

                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.0))
                                .child({
                                    Calendar::new()
                                        .current_month(viewing_month)
                                        .when_some(selected_date, |cal, date| {
                                            cal.selected_date(date)
                                        })
                                        .selected_range(selected_range)
                                        .range_start_temp(range_start_temp)
                                        .locale(locale_clone.clone())
                                        .is_date_disabled({
                                            let min = min_date_clone;
                                            let max = max_date_clone;
                                            let disabled = disabled_dates_clone.clone();
                                            move |date: &DateValue| {
                                                let is_before_min = if let Some(min) = min {
                                                    date.year < min.year
                                                        || (date.year == min.year
                                                            && date.month < min.month)
                                                        || (date.year == min.year
                                                            && date.month == min.month
                                                            && date.day < min.day)
                                                } else {
                                                    false
                                                };

                                                let is_after_max = if let Some(max) = max {
                                                    date.year > max.year
                                                        || (date.year == max.year
                                                            && date.month > max.month)
                                                        || (date.year == max.year
                                                            && date.month == max.month
                                                            && date.day > max.day)
                                                } else {
                                                    false
                                                };

                                                let is_in_disabled_list =
                                                    disabled.iter().any(|d| d == date);
                                                is_before_min || is_after_max || is_in_disabled_list
                                            }
                                        })
                                        .on_date_select({
                                            let popover_for_dismiss = popover_entity.clone();
                                            let on_select_for_date = on_select.clone();
                                            move |date, window, app_cx| {
                                                let is_before_min =
                                                    if let Some(min) = min_date_clone {
                                                        date.year < min.year
                                                            || (date.year == min.year
                                                                && date.month < min.month)
                                                            || (date.year == min.year
                                                                && date.month == min.month
                                                                && date.day < min.day)
                                                    } else {
                                                        false
                                                    };

                                                let is_after_max = if let Some(max) = max_date_clone
                                                {
                                                    date.year > max.year
                                                        || (date.year == max.year
                                                            && date.month > max.month)
                                                        || (date.year == max.year
                                                            && date.month == max.month
                                                            && date.day > max.day)
                                                } else {
                                                    false
                                                };

                                                let is_in_disabled_list =
                                                    disabled_dates_clone.iter().any(|d| d == date);
                                                let is_disabled = is_before_min
                                                    || is_after_max
                                                    || is_in_disabled_list;

                                                if !is_disabled {
                                                    // Check if we should close (different logic for single vs range mode)
                                                    let should_close = state_for_select
                                                        .read(app_cx)
                                                        .selection_mode
                                                        == DateSelectionMode::Single
                                                        || state_for_select
                                                            .read(app_cx)
                                                            .range_start_temp
                                                            .is_some(); // In range mode, close on second click

                                                    // Update the date picker state
                                                    state_for_select.update(app_cx, |state, cx| {
                                                        state.select_date(*date, cx);
                                                        if should_close {
                                                            state.close(cx);
                                                        }
                                                        cx.notify(); // Notify to trigger re-render
                                                    });

                                                    // Call the on_select callback (only when selection is complete)
                                                    if should_close {
                                                        if let Some(handler) =
                                                            on_select_for_date.as_ref()
                                                        {
                                                            handler(date, window, app_cx);
                                                        }

                                                        // Close the popover by emitting DismissEvent
                                                        popover_for_dismiss.update(
                                                            app_cx,
                                                            |_, cx| {
                                                                cx.emit(DismissEvent);
                                                            },
                                                        );
                                                    }
                                                }
                                            }
                                        })
                                        .on_month_change(move |date, _window, app_cx| {
                                            state_for_month.update(app_cx, |state, cx| {
                                                state.set_viewing_month(*date, cx);
                                                cx.notify(); // Notify to trigger re-render
                                            });
                                        })
                                })
                                .when(show_today_button, |parent_div| {
                                    let popover_for_today = popover_entity.clone();
                                    let on_select_for_today = on_select.clone();
                                    parent_div.child(
                                        div()
                                            .flex()
                                            .justify_center()
                                            .pt(px(8.0))
                                            .border_t_1()
                                            .border_color(border_color)
                                            .child(
                                                Button::new("today-btn", "Today")
                                                    .variant(ButtonVariant::Outline)
                                                    .size(ButtonSize::Sm)
                                                    .on_click(move |_, window, app_cx| {
                                                        let today = DateValue::today();
                                                        state_today.update(app_cx, |state, cx| {
                                                            state.select_date(today, cx);
                                                            state.close(cx);
                                                            cx.notify(); // Notify to trigger re-render
                                                        });

                                                        // Call the on_select callback
                                                        if let Some(handler) =
                                                            on_select_for_today.as_ref()
                                                        {
                                                            handler(&today, window, app_cx);
                                                        }

                                                        // Close the popover
                                                        popover_for_today.update(
                                                            app_cx,
                                                            |_, cx| {
                                                                cx.emit(DismissEvent);
                                                            },
                                                        );
                                                    }),
                                            ),
                                    )
                                })
                                .into_any_element()
                        },
                    )
                })
            })
            .map(|this| {
                let mut popover = this;
                popover.style().refine(&user_style);
                popover
            });

        match self.label {
            Some(label) => {
                let mut field = Field::new(label, control)
                    .hidden_label(self.label_hidden)
                    .optional(self.optional)
                    .required(self.required)
                    .disabled(self.disabled)
                    .status_variant(FieldStatusVariant::Detached);

                if let Some(description) = self.description {
                    field = field.description(description);
                }

                if let Some((status, message)) = self.status {
                    field = field.status(status, message);
                }

                field.into_any_element()
            }
            None => control.into_any_element(),
        }
    }
}
