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

type DateSelectHandler = Rc<dyn Fn(&DateValue, &mut Window, &mut App)>;
type DateRangeSelectHandler = Rc<dyn Fn(&DateRange, &mut Window, &mut App)>;

fn date_is_disabled(
    date: &DateValue,
    min_date: Option<DateValue>,
    max_date: Option<DateValue>,
    disabled_dates: &[DateValue],
    disable_weekends: bool,
) -> bool {
    min_date.is_some_and(|min| *date < min)
        || max_date.is_some_and(|max| *date > max)
        || disabled_dates.contains(date)
        || (disable_weekends
            && matches!(
                date.day_of_week(),
                crate::components::calendar::DayOfWeek::SUNDAY
                    | crate::components::calendar::DayOfWeek::SATURDAY
            ))
}

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

    pub fn select_date(&mut self, date: DateValue, cx: &mut Context<Self>) {
        let previous = (
            self.selected_date,
            self.selected_range,
            self.range_start_temp,
            self.viewing_month,
        );
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
        if previous
            != (
                self.selected_date,
                self.selected_range,
                self.range_start_temp,
                self.viewing_month,
            )
        {
            cx.notify();
        }
    }

    pub fn set_selection_mode(&mut self, mode: DateSelectionMode, cx: &mut Context<Self>) {
        if self.selection_mode == mode {
            return;
        }
        self.selection_mode = mode;
        self.selected_date = None;
        self.selected_range = None;
        self.range_start_temp = None;
        cx.notify();
    }

    /// Replace the single-date value without simulating pointer selection.
    pub fn set_date(&mut self, date: Option<DateValue>, cx: &mut Context<Self>) {
        let changed = self.selected_date != date
            || self.selected_range.is_some()
            || self.range_start_temp.is_some();
        self.selected_date = date;
        self.selected_range = None;
        self.range_start_temp = None;
        if let Some(date) = date {
            self.viewing_month = date;
        }
        if changed {
            cx.notify();
        }
    }

    /// Replace the completed range and normalize reversed endpoints.
    pub fn set_range(&mut self, range: Option<DateRange>, cx: &mut Context<Self>) {
        let range = range.map(|range| DateRange::new(range.start, range.end));
        let changed = self.selected_range != range
            || self.selected_date.is_some()
            || self.range_start_temp.is_some();
        self.selected_date = None;
        self.selected_range = range;
        self.range_start_temp = None;
        if let Some(range) = range {
            self.viewing_month = range.end;
        }
        if changed {
            cx.notify();
        }
    }

    pub fn clear_date(&mut self, cx: &mut Context<Self>) {
        if self.selected_date.is_none()
            && self.selected_range.is_none()
            && self.range_start_temp.is_none()
        {
            return;
        }
        self.selected_date = None;
        self.selected_range = None;
        self.range_start_temp = None;
        cx.notify();
    }

    pub fn set_viewing_month(&mut self, date: DateValue, cx: &mut Context<Self>) {
        let month = DateValue::new(date.year, date.month.clamp(1, 12), 1);
        if self.viewing_month.year != month.year || self.viewing_month.month != month.month {
            self.viewing_month = month;
            cx.notify();
        }
    }

    pub fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.is_open = !self.is_open;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.is_open {
            self.is_open = false;
            cx.notify();
        }
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if !self.is_open {
            self.is_open = true;
            cx.notify();
        }
    }

    pub fn jump_to_today(&mut self, cx: &mut Context<Self>) {
        self.set_viewing_month(DateValue::today(), cx);
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
    disable_weekends: bool,
    disabled: bool,
    read_only: bool,
    optional: bool,
    required: bool,
    loading: bool,
    size: InputSize,
    clearable: bool,
    status: Option<(FieldStatusType, SharedString)>,
    show_today_button: bool,
    on_select: Option<DateSelectHandler>,
    on_range_select: Option<DateRangeSelectHandler>,
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
            disable_weekends: false,
            disabled: false,
            read_only: false,
            optional: false,
            required: false,
            loading: false,
            size: InputSize::default(),
            clearable: true,
            status: None,
            show_today_button: true,
            on_select: None,
            on_range_select: None,
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
    pub fn disable_weekends(mut self) -> Self {
        self.disable_weekends = true;
        self
    }

    pub fn weekends_disabled(mut self, disabled: bool) -> Self {
        self.disable_weekends = disabled;
        self
    }

    /// Set disabled state
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Keep the selected date focusable while preventing changes.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    #[allow(non_snake_case)]
    pub fn isReadOnly(self, read_only: bool) -> Self {
        self.read_only(read_only)
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

    /// Set a callback invoked when both ends of a range are selected.
    pub fn on_range_select<F>(mut self, handler: F) -> Self
    where
        F: Fn(&DateRange, &mut Window, &mut App) + 'static,
    {
        self.on_range_select = Some(Rc::new(handler));
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
        let theme = Theme::of(cx).clone();
        let state_entity = self.state.clone();
        let state = self.state.read(cx);
        let focus_handle = state.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let is_open = state.is_open;

        let selected_date = state.selected_date;
        let selected_range = state.selected_range;
        let _viewing_month = state.viewing_month;
        let locale = self.locale.clone();
        let format = self.format;
        let disabled = self.disabled || self.loading;
        let read_only = self.read_only;
        let interaction_disabled = disabled || read_only;
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
        let on_range_select_handler = self.on_range_select.clone();
        let on_clear_handler = self.on_clear.clone();
        let min_date = self.min_date;
        let max_date = self.max_date;
        let disabled_dates = self.disabled_dates.clone();
        let disable_weekends = self.disable_weekends;
        let today = DateValue::today();
        let today_disabled = date_is_disabled(
            &today,
            min_date,
            max_date,
            &disabled_dates,
            disable_weekends,
        );

        let user_style = self.style;

        let popover_id = ElementId::Name(
            format!("date-picker-popover-{}", state_entity.entity_id().as_u64()).into(),
        );
        let trigger_id = ElementId::NamedChild(Box::new(popover_id.clone()), "trigger".into());
        let clear_id = ElementId::NamedChild(Box::new(popover_id.clone()), "clear".into());
        let clear_focus_handle = window
            .use_keyed_state(clear_id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let clear_focus_on_mouse = clear_focus_handle.clone();
        let clear_label = self
            .label
            .clone()
            .unwrap_or_else(|| SharedString::from("Date"));
        let mut trigger_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if disabled {
            trigger_state |= AccessibilityState::DISABLED;
        }
        if read_only {
            trigger_state |= AccessibilityState::READ_ONLY;
        }
        if self.required {
            trigger_state |= AccessibilityState::REQUIRED;
        }
        if self.loading {
            trigger_state |= AccessibilityState::BUSY;
        }
        if status
            .as_ref()
            .is_some_and(|(status, _)| *status == FieldStatusType::Error)
        {
            trigger_state |= AccessibilityState::INVALID;
        }
        let mut trigger_accessibility = AccessibilityAttributes::new(AccessibilityRole::ComboBox)
            .label(clear_label.to_string())
            .placeholder(self.placeholder.to_string())
            .states(trigger_state);
        if has_value {
            trigger_accessibility =
                trigger_accessibility.value(AccessibilityValue::Text(display_text.clone()));
        }
        if let Some(description) = self.description.as_ref() {
            trigger_accessibility = trigger_accessibility.description(description.to_string());
        }
        if !interaction_disabled {
            trigger_accessibility = trigger_accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                if is_open {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ]);
        } else if read_only && !disabled {
            trigger_accessibility = trigger_accessibility.actions(vec![AccessibilityAction::Focus]);
        }
        let state_for_open = state_entity.clone();
        let state_for_accessibility = state_entity.clone();
        let accessibility_action = if is_open {
            AccessibilityAction::Collapse
        } else {
            AccessibilityAction::Expand
        };
        let state_for_close_action = state_entity.clone();
        let state_for_previous_action = state_entity.clone();
        let state_for_next_action = state_entity.clone();
        let state_for_today_action = state_entity.clone();
        let on_select_for_today_action = self.on_select.clone();
        let on_range_select_for_today_action = self.on_range_select.clone();

        let control = Popover::new(popover_id.clone())
            .disabled(interaction_disabled)
            .on_open_change(move |open, _, cx| {
                state_for_open.update(cx, |state, cx| {
                    state.is_open = open;
                    cx.notify();
                });
            })
            .trigger(
                div()
                    .id(trigger_id)
                    .accessibility(trigger_accessibility)
                    .when(!interaction_disabled, move |this| {
                        let state_for_accessibility = state_for_accessibility.clone();
                        this.on_accessibility_action(accessibility_action, move |_, _, cx| {
                            state_for_accessibility.update(cx, |state, cx| {
                                if accessibility_action == AccessibilityAction::Expand {
                                    state.open(cx);
                                } else {
                                    state.close(cx);
                                }
                            });
                        })
                    })
                    .when(!interaction_disabled, move |this| {
                        let previous = state_for_previous_action.clone();
                        let next = state_for_next_action.clone();
                        let today_state = state_for_today_action.clone();
                        let on_select = on_select_for_today_action.clone();
                        let on_range_select = on_range_select_for_today_action.clone();
                        this.key_context("DatePicker")
                            .on_action(move |_: &ClosePicker, _, cx| {
                                state_for_close_action.update(cx, |state, cx| state.close(cx));
                            })
                            .on_action(move |_: &PrevMonth, _, cx| {
                                previous.update(cx, |state, cx| {
                                    let target = state.viewing_month.add_months(-1);
                                    let allowed = min_date.is_none_or(|min| {
                                        (target.year, target.month) >= (min.year, min.month)
                                    });
                                    if allowed {
                                        state.set_viewing_month(target, cx);
                                    }
                                });
                            })
                            .on_action(move |_: &NextMonth, _, cx| {
                                next.update(cx, |state, cx| {
                                    let target = state.viewing_month.add_months(1);
                                    let allowed = max_date.is_none_or(|max| {
                                        (target.year, target.month) <= (max.year, max.month)
                                    });
                                    if allowed {
                                        state.set_viewing_month(target, cx);
                                    }
                                });
                            })
                            .on_action(move |_: &SelectToday, window, cx| {
                                if today_disabled {
                                    return;
                                }
                                let should_close = today_state.read(cx).selection_mode
                                    == DateSelectionMode::Single
                                    || today_state.read(cx).range_start_temp.is_some();
                                today_state.update(cx, |state, cx| {
                                    state.select_date(today, cx);
                                    if should_close {
                                        state.close(cx);
                                    }
                                });
                                if should_close {
                                    if let Some(handler) = on_select.as_ref() {
                                        handler(&today, window, cx);
                                    }
                                    if let Some(range) = today_state.read(cx).selected_range {
                                        if let Some(handler) = on_range_select.as_ref() {
                                            handler(&range, window, cx);
                                        }
                                    }
                                }
                            })
                    })
                    .when(!disabled, |this| {
                        this.track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    })
                    .flex()
                    .items_center()
                    .w_full()
                    .h(height)
                    .min_w(px(180.0))
                    .px(padding_x)
                    .gap(px(8.0))
                    .bg(if read_only {
                        theme.tokens.muted.opacity(0.35)
                    } else {
                        theme.tokens.card
                    })
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
                    .when(!interaction_disabled, |div| {
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
                            .when(
                                clearable && has_value && !interaction_disabled,
                                |parent_div| {
                                    let on_clear = on_clear_handler.clone();
                                    let muted_bg = theme.tokens.muted;
                                    let muted_fg = theme.tokens.muted_foreground;
                                    let state_for_clear_key = state_for_clear.clone();
                                    let state_for_clear_accessibility = state_for_clear.clone();
                                    let on_clear_key = on_clear.clone();
                                    let on_clear_accessibility = on_clear.clone();
                                    parent_div.child(
                                        div()
                                            .id(clear_id)
                                            .accessibility(
                                                AccessibilityAttributes::new(
                                                    AccessibilityRole::Button,
                                                )
                                                .label(format!("Clear {}", clear_label))
                                                .actions(vec![
                                                    AccessibilityAction::Focus,
                                                    AccessibilityAction::Click,
                                                ]),
                                            )
                                            .track_focus(
                                                &clear_focus_handle.tab_index(0).tab_stop(true),
                                            )
                                            .on_accessibility_action(
                                                AccessibilityAction::Click,
                                                move |_, window, cx| {
                                                    state_for_clear_accessibility
                                                        .update(cx, |state, cx| {
                                                            state.clear_date(cx)
                                                        });
                                                    if let Some(handler) =
                                                        on_clear_accessibility.as_ref()
                                                    {
                                                        handler(window, cx);
                                                    }
                                                },
                                            )
                                            .p(px(2.0))
                                            .rounded(theme.tokens.radius_sm)
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(muted_bg.opacity(0.8)))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                move |_event: &MouseDownEvent, window, cx| {
                                                    cx.stop_propagation();
                                                    window.focus(&clear_focus_on_mouse);
                                                    state_for_clear.update(cx, |state, cx| {
                                                        state.clear_date(cx);
                                                    });
                                                    if let Some(handler) = on_clear.as_ref() {
                                                        handler(window, cx);
                                                    }
                                                },
                                            )
                                            .on_key_down(move |event, window, cx| {
                                                if !matches!(
                                                    event.keystroke.key.as_str(),
                                                    "enter" | "space"
                                                ) {
                                                    return;
                                                }
                                                state_for_clear_key.update(cx, |state, cx| {
                                                    state.clear_date(cx);
                                                });
                                                if let Some(handler) = on_clear_key.as_ref() {
                                                    handler(window, cx);
                                                }
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            })
                                            .child(Icon::new("x").size(icon_size).color(muted_fg)),
                                    )
                                },
                            )
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
                let on_range_select_ref = on_range_select_handler.clone();
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
                            let on_range_select = on_range_select_ref.clone();
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
                                                date_is_disabled(
                                                    date,
                                                    min,
                                                    max,
                                                    &disabled,
                                                    disable_weekends,
                                                )
                                            }
                                        })
                                        .on_date_select({
                                            let popover_for_dismiss = popover_entity.clone();
                                            let on_select_for_date = on_select.clone();
                                            let on_range_select_for_date = on_range_select.clone();
                                            move |date, window, app_cx| {
                                                let is_disabled = date_is_disabled(
                                                    date,
                                                    min_date_clone,
                                                    max_date_clone,
                                                    &disabled_dates_clone,
                                                    disable_weekends,
                                                );

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
                                                    });

                                                    // Call the on_select callback (only when selection is complete)
                                                    if should_close {
                                                        if let Some(handler) =
                                                            on_select_for_date.as_ref()
                                                        {
                                                            handler(date, window, app_cx);
                                                        }
                                                        if let Some(range) = state_for_select
                                                            .read(app_cx)
                                                            .selected_range
                                                        {
                                                            if let Some(handler) =
                                                                on_range_select_for_date.as_ref()
                                                            {
                                                                handler(&range, window, app_cx);
                                                            }
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
                                    let on_range_select_for_today = on_range_select.clone();
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
                                                    .disabled(today_disabled)
                                                    .on_click(move |_, window, app_cx| {
                                                        let should_close =
                                                            state_today.read(app_cx).selection_mode
                                                                == DateSelectionMode::Single
                                                                || state_today
                                                                    .read(app_cx)
                                                                    .range_start_temp
                                                                    .is_some();
                                                        state_today.update(app_cx, |state, cx| {
                                                            state.select_date(today, cx);
                                                            if should_close {
                                                                state.close(cx);
                                                            }
                                                        });

                                                        if should_close {
                                                            if let Some(handler) =
                                                                on_select_for_today.as_ref()
                                                            {
                                                                handler(&today, window, app_cx);
                                                            }
                                                            if let Some(range) = state_today
                                                                .read(app_cx)
                                                                .selected_range
                                                            {
                                                                if let Some(handler) =
                                                                    on_range_select_for_today
                                                                        .as_ref()
                                                                {
                                                                    handler(&range, window, app_cx);
                                                                }
                                                            }
                                                            popover_for_today.update(
                                                                app_cx,
                                                                |_, cx| {
                                                                    cx.emit(DismissEvent);
                                                                },
                                                            );
                                                        }
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

#[cfg(test)]
mod tests {
    use super::{date_is_disabled, DateFormat, DatePicker, DatePickerState, DateSelectionMode};
    use crate::components::calendar::{CalendarLocale, DateRange, DateValue};
    use kael::{
        AccessibilityAction, AccessibilityRole, AccessibilityState, AppContext, Context, Entity,
        IntoElement, Render, TestAppContext, Window,
    };

    struct DatePickerHost {
        state: Entity<DatePickerState>,
        read_only: bool,
    }

    impl Render for DatePickerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            DatePicker::new(self.state.clone())
                .label("Release date")
                .read_only(self.read_only)
        }
    }

    #[kael::test]
    fn reapplying_range_mode_preserves_the_completed_range(cx: &mut TestAppContext) {
        let state = cx.new(|cx| DatePickerState::new_with_mode(DateSelectionMode::Range, cx));
        let expected = DateRange::new(DateValue::new(2026, 7, 20), DateValue::new(2026, 7, 14));
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.select_date(expected.end, cx);
                state.select_date(expected.start, cx);
                state.set_selection_mode(DateSelectionMode::Range, cx);
            });
        });

        assert_eq!(cx.read(|cx| state.read(cx).selected_range), Some(expected));
    }

    #[kael::test]
    fn programmatic_ranges_are_normalized_and_clear_transient_selection(cx: &mut TestAppContext) {
        let state = cx.new(|cx| DatePickerState::new_with_mode(DateSelectionMode::Range, cx));
        let expected = DateRange::new(DateValue::new(2026, 8, 3), DateValue::new(2026, 7, 28));
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.select_date(DateValue::new(2026, 1, 1), cx);
                state.set_range(Some(expected), cx);
            });
        });

        cx.read(|cx| {
            let state = state.read(cx);
            assert_eq!(state.selected_range, Some(expected));
            assert_eq!(state.range_start_temp, None);
            assert_eq!(state.viewing_month, expected.end);
        });
    }

    #[test]
    fn date_constraints_share_one_consistent_predicate() {
        let saturday = DateValue::new(2026, 7, 18);
        let weekday = DateValue::new(2026, 7, 20);
        assert!(date_is_disabled(
            &saturday,
            Some(DateValue::new(2026, 7, 1)),
            Some(DateValue::new(2026, 7, 31)),
            &[],
            true,
        ));
        assert!(date_is_disabled(&weekday, None, None, &[weekday], false,));
        assert!(!date_is_disabled(
            &weekday,
            Some(DateValue::new(2026, 7, 1)),
            Some(DateValue::new(2026, 7, 31)),
            &[],
            false,
        ));
    }

    #[test]
    fn long_range_format_is_compact_within_one_month() {
        let range = DateRange::new(DateValue::new(2026, 7, 14), DateValue::new(2026, 7, 20));
        assert_eq!(
            DateFormat::LongDate.format_range(&range, &CalendarLocale::english()),
            "July 14-20, 2026"
        );
    }

    #[kael::test]
    fn read_only_picker_is_focusable_without_edit_actions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(|cx| DatePickerState::new(cx));
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| DatePickerHost {
                state,
                read_only: true,
            }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::ComboBox)
                .expect("date picker should expose combobox semantics");
            assert!(node.states.contains(AccessibilityState::READ_ONLY));
            assert!(node.actions.contains(&AccessibilityAction::Focus));
            assert!(!node.actions.contains(&AccessibilityAction::Expand));
            assert!(!node.actions.contains(&AccessibilityAction::Click));
        });
    }
}
