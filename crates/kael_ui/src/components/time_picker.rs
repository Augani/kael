use crate::components::{
    button::{Button, ButtonColors, ButtonSize, ButtonVariant},
    field::{Field, FieldStatusType},
    field_status::FieldStatusVariant,
    icon::Icon,
    input::InputSize,
    scrollable::scrollable_vertical,
    spinner::{Spinner, SpinnerSize},
};
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeFormat {
    Hour12,
    Hour24,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimePeriod {
    AM,
    PM,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeValue {
    pub hour: u8,
    pub minute: u8,
    pub second: Option<u8>,
    pub period: Option<TimePeriod>,
}

impl TimeValue {
    pub fn new(hour: u8, minute: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
            second: None,
            period: None,
        }
    }

    pub fn with_seconds(mut self, second: u8) -> Self {
        self.second = Some(second.min(59));
        self
    }

    pub fn with_period(mut self, period: TimePeriod) -> Self {
        self.period = Some(period);
        self
    }

    pub fn to_24h(&self) -> (u8, u8, Option<u8>) {
        let hour = match self.period {
            Some(TimePeriod::AM) => self.hour % 12,
            Some(TimePeriod::PM) => (self.hour % 12) + 12,
            None => self.hour.min(23),
        };
        (
            hour,
            self.minute.min(59),
            self.second.map(|second| second.min(59)),
        )
    }

    pub fn from_24h(hour: u8, minute: u8, second: Option<u8>, format: TimeFormat) -> Self {
        let hour = hour.min(23);
        let minute = minute.min(59);
        let second = second.map(|second| second.min(59));
        match format {
            TimeFormat::Hour24 => Self {
                hour,
                minute,
                second,
                period: None,
            },
            TimeFormat::Hour12 => {
                let (h12, period) = if hour == 0 {
                    (12, TimePeriod::AM)
                } else if hour < 12 {
                    (hour, TimePeriod::AM)
                } else if hour == 12 {
                    (12, TimePeriod::PM)
                } else {
                    (hour - 12, TimePeriod::PM)
                };
                Self {
                    hour: h12,
                    minute,
                    second,
                    period: Some(period),
                }
            }
        }
    }

    pub fn format_string(&self, format: TimeFormat) -> String {
        let (hour, minute, second) = self.to_24h();
        let display = Self::from_24h(hour, minute, second, format);
        let hour_str = format!("{:02}", display.hour);
        let minute_str = format!("{:02}", display.minute);

        let base = if let Some(sec) = display.second {
            format!("{}:{}:{:02}", hour_str, minute_str, sec)
        } else {
            format!("{}:{}", hour_str, minute_str)
        };

        match (format, display.period) {
            (TimeFormat::Hour12, Some(TimePeriod::AM)) => format!("{} AM", base),
            (TimeFormat::Hour12, Some(TimePeriod::PM)) => format!("{} PM", base),
            _ => base,
        }
    }
}

impl Default for TimeValue {
    fn default() -> Self {
        Self::new(12, 0)
    }
}

const TIME_COLUMN_VISIBLE_ROWS: usize = 6;
const TIME_COLUMN_MIN_VISIBLE_ROWS: usize = 3;
const TIME_ROW_STRIDE: f32 = 34.0;

#[derive(Clone, Debug, Default)]
struct TimePickerScrollHandles {
    hour: ScrollHandle,
    minute: ScrollHandle,
    second: ScrollHandle,
}

impl TimePickerScrollHandles {
    fn sync(&self, value: TimeValue, format: TimeFormat, show_seconds: bool) {
        let (hour_index, hour_count) = match format {
            TimeFormat::Hour12 => (value.hour.saturating_sub(1) as usize, 12),
            TimeFormat::Hour24 => (value.hour as usize, 24),
        };
        self.hour
            .set_offset(scroll_offset_for_selection(hour_index, hour_count));
        self.minute
            .set_offset(scroll_offset_for_selection(value.minute as usize, 60));
        if show_seconds {
            self.second.set_offset(scroll_offset_for_selection(
                value.second.unwrap_or(0) as usize,
                60,
            ));
        }
    }
}

fn scroll_offset_for_selection(index: usize, item_count: usize) -> Point<Pixels> {
    // Center for the normal six-row popup, but clamp against the three rows
    // guaranteed by the minimum popup height so 59 remains reachable in a
    // vertically constrained window.
    let max_first = item_count.saturating_sub(TIME_COLUMN_MIN_VISIBLE_ROWS);
    let centered_first = index.saturating_sub(TIME_COLUMN_VISIBLE_ROWS / 2);
    let first_that_keeps_selection_visible = index
        .saturating_add(1)
        .saturating_sub(TIME_COLUMN_MIN_VISIBLE_ROWS);
    let first = centered_first
        .max(first_that_keeps_selection_visible)
        .min(max_first);
    point(px(0.0), px(-(first as f32 * TIME_ROW_STRIDE)))
}

pub struct TimePickerState {
    value: TimeValue,
    has_value: bool,
    format: TimeFormat,
    show_seconds: bool,
    open: bool,
    pending_input: Option<String>,
    focus_handle: FocusHandle,
    trigger_bounds: Bounds<Pixels>,
    scroll_handles: TimePickerScrollHandles,
}

impl TimePickerState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            value: TimeValue::default(),
            has_value: true,
            format: TimeFormat::Hour24,
            show_seconds: false,
            open: false,
            pending_input: None,
            focus_handle: cx.focus_handle(),
            trigger_bounds: Bounds::default(),
            scroll_handles: TimePickerScrollHandles::default(),
        }
    }

    pub fn value(&self) -> TimeValue {
        self.value
    }

    /// Returns the selected value, or `None` after a clear action.
    pub fn value_option(&self) -> Option<TimeValue> {
        self.has_value.then_some(self.value)
    }

    pub fn has_value(&self) -> bool {
        self.has_value
    }

    pub fn set_value(&mut self, value: TimeValue, cx: &mut Context<Self>) {
        let value = normalize_time_value(value, self.format, self.show_seconds);
        if self.value != value || !self.has_value {
            self.value = value;
            self.has_value = true;
            self.sync_scrollers();
            cx.notify();
        }
    }

    pub fn set_hour(&mut self, hour: u8, cx: &mut Context<Self>) {
        let max_hour = match self.format {
            TimeFormat::Hour12 => 12,
            TimeFormat::Hour24 => 23,
        };
        let hour = match self.format {
            TimeFormat::Hour12 => hour.clamp(1, max_hour),
            TimeFormat::Hour24 => hour.min(max_hour),
        };
        if self.value.hour != hour || !self.has_value {
            self.value.hour = hour;
            self.has_value = true;
            self.scroll_handles
                .hour
                .set_offset(scroll_offset_for_selection(
                    match self.format {
                        TimeFormat::Hour12 => hour.saturating_sub(1) as usize,
                        TimeFormat::Hour24 => hour as usize,
                    },
                    match self.format {
                        TimeFormat::Hour12 => 12,
                        TimeFormat::Hour24 => 24,
                    },
                ));
            cx.notify();
        }
    }

    pub fn set_minute(&mut self, minute: u8, cx: &mut Context<Self>) {
        let minute = minute.min(59);
        if self.value.minute != minute || !self.has_value {
            self.value.minute = minute;
            self.has_value = true;
            self.scroll_handles
                .minute
                .set_offset(scroll_offset_for_selection(minute as usize, 60));
            cx.notify();
        }
    }

    pub fn set_second(&mut self, second: u8, cx: &mut Context<Self>) {
        let second = second.min(59);
        if self.value.second != Some(second) || !self.has_value {
            self.value.second = Some(second);
            self.has_value = true;
            self.scroll_handles
                .second
                .set_offset(scroll_offset_for_selection(second as usize, 60));
            cx.notify();
        }
    }

    pub fn set_period(&mut self, period: TimePeriod, cx: &mut Context<Self>) {
        if self.format == TimeFormat::Hour12
            && (self.value.period != Some(period) || !self.has_value)
        {
            self.value.period = Some(period);
            self.has_value = true;
            cx.notify();
        }
    }

    pub fn increment_hour(&mut self, cx: &mut Context<Self>) {
        let max = match self.format {
            TimeFormat::Hour12 => 12,
            TimeFormat::Hour24 => 23,
        };
        let min = match self.format {
            TimeFormat::Hour12 => 1,
            TimeFormat::Hour24 => 0,
        };
        let hour = if self.value.hour >= max {
            min
        } else {
            self.value.hour + 1
        };
        self.set_hour(hour, cx);
    }

    pub fn decrement_hour(&mut self, cx: &mut Context<Self>) {
        let max = match self.format {
            TimeFormat::Hour12 => 12,
            TimeFormat::Hour24 => 23,
        };
        let min = match self.format {
            TimeFormat::Hour12 => 1,
            TimeFormat::Hour24 => 0,
        };
        let hour = if self.value.hour <= min {
            max
        } else {
            self.value.hour - 1
        };
        self.set_hour(hour, cx);
    }

    pub fn increment_minute(&mut self, cx: &mut Context<Self>) {
        let minute = if self.value.minute >= 59 {
            0
        } else {
            self.value.minute + 1
        };
        self.set_minute(minute, cx);
    }

    pub fn decrement_minute(&mut self, cx: &mut Context<Self>) {
        let minute = if self.value.minute == 0 {
            59
        } else {
            self.value.minute - 1
        };
        self.set_minute(minute, cx);
    }

    pub fn increment_second(&mut self, cx: &mut Context<Self>) {
        if let Some(sec) = self.value.second {
            self.set_second(if sec >= 59 { 0 } else { sec + 1 }, cx);
        }
    }

    pub fn decrement_second(&mut self, cx: &mut Context<Self>) {
        if let Some(sec) = self.value.second {
            self.set_second(if sec == 0 { 59 } else { sec - 1 }, cx);
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.close(cx);
        } else {
            self.open(cx);
        }
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            self.open = true;
            self.sync_scrollers();
            cx.notify();
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open || self.pending_input.is_some() {
            self.open = false;
            self.pending_input = None;
            cx.notify();
        }
    }

    pub fn format(&self) -> TimeFormat {
        self.format
    }

    pub fn set_format(&mut self, format: TimeFormat, cx: &mut Context<Self>) {
        if self.format != format {
            let (h24, m, s) = self.value.to_24h();
            self.value = TimeValue::from_24h(h24, m, s, format);
            self.format = format;
            self.pending_input = None;
            self.sync_scrollers();
            cx.notify();
        }
    }

    pub fn show_seconds(&self) -> bool {
        self.show_seconds
    }

    pub fn set_show_seconds(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_seconds == show {
            return;
        }
        self.show_seconds = show;
        if show && self.value.second.is_none() {
            self.value.second = Some(0);
        } else if !show {
            self.value.second = None;
        }
        self.sync_scrollers();
        cx.notify();
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.has_value || self.pending_input.is_some() || self.open {
            self.has_value = false;
            self.pending_input = None;
            self.open = false;
            cx.notify();
        }
    }

    fn display_text(&self) -> String {
        self.pending_input.clone().unwrap_or_else(|| {
            if self.has_value {
                self.value.format_string(self.format)
            } else {
                String::new()
            }
        })
    }

    fn update_pending_input(&mut self, f: impl FnOnce(&mut String), cx: &mut Context<Self>) {
        let mut input = self.pending_input.take().unwrap_or_default();
        f(&mut input);
        self.pending_input = Some(input);
        cx.notify();
    }

    fn commit_pending(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(input) = self.pending_input.take() else {
            return false;
        };
        let Some(value) = parse_time_input(&input, self.format, self.show_seconds) else {
            self.pending_input = Some(input);
            cx.notify();
            return false;
        };
        self.value = value;
        self.has_value = true;
        self.sync_scrollers();
        cx.notify();
        true
    }

    fn adjust_minutes(&mut self, delta: i16, cx: &mut Context<Self>) {
        if let Some(input) = self.pending_input.as_ref() {
            if let Some(value) = parse_time_input(input, self.format, self.show_seconds) {
                self.value = value;
                self.pending_input = None;
            }
        }

        let (hour, minute, second) = self.value.to_24h();
        let total = ((hour as i16) * 60 + minute as i16 + delta).rem_euclid(24 * 60);
        self.value =
            TimeValue::from_24h((total / 60) as u8, (total % 60) as u8, second, self.format);
        self.has_value = true;
        self.sync_scrollers();
        cx.notify();
    }

    fn sync_scrollers(&self) {
        self.scroll_handles
            .sync(self.value, self.format, self.show_seconds);
    }
}

fn normalize_time_value(value: TimeValue, format: TimeFormat, show_seconds: bool) -> TimeValue {
    let (hour, minute, second) = value.to_24h();
    TimeValue::from_24h(
        hour,
        minute,
        if show_seconds {
            Some(second.unwrap_or(0))
        } else {
            None
        },
        format,
    )
}

impl Focusable for TimePickerState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TimePickerState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct TimePicker {
    state: Entity<TimePickerState>,
    label: Option<SharedString>,
    label_hidden: bool,
    description: Option<SharedString>,
    placeholder: SharedString,
    disabled: bool,
    read_only: bool,
    optional: bool,
    required: bool,
    loading: bool,
    clearable: bool,
    status: Option<(FieldStatusType, SharedString)>,
    format: Option<TimeFormat>,
    show_seconds: Option<bool>,
    size: InputSize,
    on_change: Option<Rc<dyn Fn(&TimeValue, &mut Window, &mut App)>>,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl TimePicker {
    pub fn new(state: Entity<TimePickerState>) -> Self {
        Self {
            state,
            label: None,
            label_hidden: false,
            description: None,
            placeholder: "Select time".into(),
            disabled: false,
            read_only: false,
            optional: false,
            required: false,
            loading: false,
            clearable: false,
            status: None,
            format: None,
            show_seconds: None,
            size: InputSize::default(),
            on_change: None,
            on_clear: None,
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

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Keep the selected time focusable and exposed to assistive technology,
    /// while preventing edits, clearing, and popup interaction.
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

    pub fn hour_format(mut self, format: TimeFormat) -> Self {
        self.format = Some(format);
        self
    }

    #[allow(non_snake_case)]
    pub fn hourFormat(self, format: TimeFormat) -> Self {
        self.hour_format(format)
    }

    pub fn has_seconds(mut self, show_seconds: bool) -> Self {
        self.show_seconds = Some(show_seconds);
        self
    }

    #[allow(non_snake_case)]
    pub fn hasSeconds(self, show_seconds: bool) -> Self {
        self.has_seconds(show_seconds)
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(&TimeValue, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }
}

impl Styled for TimePicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TimePicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if let Some(format) = self.format {
            self.state
                .update(cx, |state, cx| state.set_format(format, cx));
        }
        if let Some(show_seconds) = self.show_seconds {
            self.state
                .update(cx, |state, cx| state.set_show_seconds(show_seconds, cx));
        }
        let theme = Theme::of(cx);
        let user_style = self.style;
        let state_data = self.state.read(cx);
        let format = state_data.format;
        let show_seconds = state_data.show_seconds;
        let open = state_data.open;
        let trigger_bounds = state_data.trigger_bounds;
        let value = state_data.value;
        let has_value = state_data.has_value;
        let scroll_handles = state_data.scroll_handles.clone();
        let pending_input = state_data.pending_input.clone();
        let focus_handle = state_data.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let state = self.state.clone();
        let state_entity_id = state.entity_id();
        let effectively_disabled = self.disabled || self.loading;

        let display_text = state_data.display_text();
        let is_valid = pending_input.as_ref().is_none_or(|input| {
            input.is_empty() || parse_time_input(input, format, show_seconds).is_some()
        });
        let hover_ring = crate::astryx::input_hover_ring(theme.tokens.input);
        let focus_ring = crate::astryx::focus_ring(theme.tokens.primary);
        let (height, padding_x, text_size, icon_size) = match self.size {
            InputSize::Sm => (px(28.0), px(12.0), px(14.0), px(16.0)),
            InputSize::Md => (px(32.0), px(12.0), px(14.0), px(16.0)),
            InputSize::Lg => (px(36.0), px(12.0), px(14.0), px(16.0)),
        };
        let display_is_empty = display_text.is_empty();
        let rendered_text = if display_is_empty {
            self.placeholder.clone()
        } else {
            SharedString::from(display_text.clone())
        };
        let status = self.status.clone();
        let accessible_label = self
            .label
            .clone()
            .unwrap_or_else(|| SharedString::from("Time"));
        let mut accessibility_state = if open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if effectively_disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if self.read_only {
            accessibility_state |= AccessibilityState::READ_ONLY;
        }
        if self.loading {
            accessibility_state |= AccessibilityState::BUSY;
        }
        if self.required {
            accessibility_state |= AccessibilityState::REQUIRED;
        }
        if !is_valid || matches!(status.as_ref(), Some((FieldStatusType::Error, _))) {
            accessibility_state |= AccessibilityState::INVALID;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::ComboBox)
            .label(accessible_label.to_string())
            .description(
                self.description
                    .as_deref()
                    .map(|description| description.to_string())
                    .unwrap_or_else(|| "Choose or type a time".to_owned()),
            )
            .placeholder(self.placeholder.to_string())
            .states(accessibility_state);
        if has_value || pending_input.is_some() {
            accessibility = accessibility.value(AccessibilityValue::Text(display_text.clone()));
        }
        if !effectively_disabled {
            let mut actions = vec![AccessibilityAction::Focus];
            if !self.read_only {
                actions.extend([
                    AccessibilityAction::Click,
                    if open {
                        AccessibilityAction::Collapse
                    } else {
                        AccessibilityAction::Expand
                    },
                ]);
            }
            accessibility = accessibility.actions(actions);
        }

        let control = div()
            .relative()
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(
                div()
                    .id(("time-picker-trigger", state_entity_id))
                    .relative()
                    .track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    .accessibility(accessibility)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .h(height)
                    .px(padding_x)
                    .min_w(px(120.0))
                    .bg(theme.tokens.card)
                    .border_1()
                    .border_color(if !is_valid {
                        theme.tokens.destructive
                    } else if status.is_some() {
                        match status.as_ref().map(|(status, _)| status) {
                            Some(FieldStatusType::Warning) => theme.tokens.warning,
                            Some(FieldStatusType::Error) => theme.tokens.destructive,
                            Some(FieldStatusType::Success) => theme.tokens.success,
                            None => theme.tokens.input,
                        }
                    } else if is_focused {
                        theme.tokens.ring
                    } else {
                        theme.tokens.input
                    })
                    .rounded(theme.tokens.radius_md)
                    .transition(theme.tokens.transition_fast)
                    .text_size(text_size)
                    .line_height(px(20.0))
                    .text_color(theme.tokens.foreground)
                    .font_family(theme.tokens.font_family.clone())
                    .shadow(smallvec::smallvec![crate::astryx::focus_ring(
                        kael::transparent_black()
                    )])
                    .when(is_focused && !effectively_disabled, |d| {
                        d.border_color(theme.tokens.primary)
                            .shadow(smallvec::smallvec![focus_ring])
                    })
                    .when(
                        !effectively_disabled && !self.read_only && !is_focused,
                        |d| {
                            d.cursor_pointer().hover(move |style| {
                                style
                                    .border_color(theme.tokens.input)
                                    .shadow(smallvec::smallvec![hover_ring])
                            })
                        },
                    )
                    .when(effectively_disabled, |d| d.opacity(0.5))
                    .when(self.read_only, |d| d.bg(theme.tokens.muted.opacity(0.25)))
                    .when(!effectively_disabled && !self.read_only, {
                        let state = state.clone();
                        let on_change = self.on_change.clone();
                        move |d| {
                            d.on_click({
                                let focus_handle = focus_handle.clone();
                                let state = state.clone();
                                move |_, window, cx| {
                                    window.focus(&focus_handle);
                                    state.update(cx, |s, cx| s.toggle(cx));
                                }
                            })
                            .on_key_down(move |event, window, cx| {
                                let key = event.keystroke.key.as_str();
                                let key_char = event.keystroke.key_char.clone();
                                let (_, handled) = state.update(cx, |s, cx| {
                                    let mut changed = false;
                                    let mut handled = true;

                                    match key {
                                        "backspace" => {
                                            s.update_pending_input(
                                                |input| {
                                                    input.pop();
                                                },
                                                cx,
                                            );
                                        }
                                        "escape" => {
                                            s.close(cx);
                                        }
                                        "enter" => {
                                            if s.pending_input.is_some() {
                                                changed = s.commit_pending(cx);
                                            } else {
                                                s.toggle(cx);
                                            }
                                        }
                                        "space" => {
                                            if s.pending_input.is_some() {
                                                s.update_pending_input(|input| input.push(' '), cx);
                                            } else {
                                                s.toggle(cx);
                                            }
                                        }
                                        "up" => {
                                            s.adjust_minutes(1, cx);
                                            changed = true;
                                        }
                                        "down" => {
                                            s.adjust_minutes(-1, cx);
                                            changed = true;
                                        }
                                        _ => {
                                            if let Some(text) = key_char
                                                .as_deref()
                                                .filter(|text| is_time_input_text(text))
                                            {
                                                s.update_pending_input(
                                                    |input| input.push_str(text),
                                                    cx,
                                                );
                                            } else {
                                                handled = false;
                                            }
                                        }
                                    }

                                    if changed {
                                        if let Some(handler) = on_change.as_ref() {
                                            handler(&s.value, window, cx);
                                        }
                                    }
                                    (changed, handled)
                                });
                                if handled {
                                    cx.stop_propagation();
                                    window.prevent_default();
                                }
                            })
                        }
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .child(
                                Icon::new("clock")
                                    .size(icon_size)
                                    .color(theme.tokens.muted_foreground),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_color(if is_valid {
                                if display_is_empty {
                                    theme.tokens.muted_foreground
                                } else {
                                    theme.tokens.foreground
                                }
                            } else {
                                theme.tokens.destructive
                            })
                            .child(rendered_text),
                    )
                    .when(self.loading, |this| {
                        this.child(Spinner::new().size(SpinnerSize::Sm))
                    })
                    .when(
                        self.clearable && has_value && !effectively_disabled && !self.read_only,
                        {
                            let state = state.clone();
                            let on_clear = self.on_clear.clone();
                            move |this| {
                                this.child(
                                    Button::new(
                                        ElementId::Name(
                                            format!(
                                                "time-picker-clear-{}",
                                                state.entity_id().as_u64()
                                            )
                                            .into(),
                                        ),
                                        "",
                                    )
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Icon)
                                    .icon("x")
                                    .tooltip("Clear time")
                                    .w(px(24.0))
                                    .h(px(24.0))
                                    .on_click(
                                        move |_, window, cx| {
                                            state.update(cx, |state, cx| state.clear(cx));
                                            if let Some(handler) = on_clear.as_ref() {
                                                handler(window, cx);
                                            }
                                        },
                                    ),
                                )
                            }
                        },
                    )
                    .when_some(status.clone(), |this, (status, _)| {
                        let (icon, color) = match status {
                            FieldStatusType::Warning => ("triangle-alert", theme.tokens.warning),
                            FieldStatusType::Error => ("circle-alert", theme.tokens.destructive),
                            FieldStatusType::Success => ("circle-check", theme.tokens.success),
                        };
                        this.child(Icon::new(icon).size(px(16.0)).color(color))
                    })
                    .child({
                        let state = state.clone();
                        canvas_with_prepaint(
                            move |bounds, _, cx| {
                                state.update(cx, |s, _| {
                                    s.trigger_bounds = bounds;
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    }),
            )
            .on_mouse_down_out({
                let state = state.clone();
                let on_change = self.on_change.clone();
                move |_, window, cx| {
                    state.update(cx, |s, cx| {
                        let changed = s.pending_input.is_some() && s.commit_pending(cx);
                        s.close(cx);
                        if changed {
                            if let Some(handler) = on_change.as_ref() {
                                handler(&s.value, window, cx);
                            }
                        }
                    });
                }
            })
            .when(
                open && !effectively_disabled && !self.read_only,
                |control| {
                    control.child(time_picker_popup(
                        theme,
                        &state,
                        value,
                        format,
                        show_seconds,
                        scroll_handles,
                        self.on_change.clone(),
                        trigger_bounds,
                        window.viewport_size(),
                    ))
                },
            );

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

#[derive(Clone, Copy)]
enum TimeCol {
    Hour,
    Minute,
    Second,
}

impl TimeCol {
    const fn debug_selector(self) -> &'static str {
        match self {
            Self::Hour => "time-picker-hour-column",
            Self::Minute => "time-picker-minute-column",
            Self::Second => "time-picker-second-column",
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn time_picker_popup(
    theme: &Theme,
    state: &Entity<TimePickerState>,
    value: TimeValue,
    format: TimeFormat,
    show_seconds: bool,
    scroll_handles: TimePickerScrollHandles,
    on_change: Option<Rc<dyn Fn(&TimeValue, &mut Window, &mut App)>>,
    trigger_bounds: Bounds<Pixels>,
    viewport: Size<Pixels>,
) -> AnyElement {
    const GAP: Pixels = px(4.0);
    const MIN_POPUP_HEIGHT: Pixels = px(176.0);
    const MAX_COLUMN_HEIGHT: Pixels = px(204.0);
    let measured = trigger_bounds.size.width > px(0.0);
    let space_below = viewport.height - trigger_bounds.bottom();
    let space_above = trigger_bounds.top();
    let open_up = space_below < MIN_POPUP_HEIGHT + GAP && space_above > space_below;
    let available_space = if open_up { space_above } else { space_below };
    let available_column_height = available_space - px(60.0);
    let column_height = if available_column_height < px(112.0) {
        px(112.0)
    } else if available_column_height > MAX_COLUMN_HEIGHT {
        MAX_COLUMN_HEIGHT
    } else {
        available_column_height
    };
    let (anchor_corner, anchor_pos) = if open_up {
        (Corner::BottomLeft, trigger_bounds.corner(Corner::TopLeft))
    } else {
        (Corner::TopLeft, trigger_bounds.corner(Corner::BottomLeft))
    };
    let use_mb = measured && open_up;
    let entity_id = state.entity_id().as_u64();

    let make_col = |label: &'static str,
                    prefix: &'static str,
                    values: Vec<u8>,
                    selected: u8,
                    kind: TimeCol,
                    scroll_handle: ScrollHandle| {
        let scroll_id = ElementId::Name(format!("{prefix}-scroll-{entity_id}").into());
        let option_colors = ButtonColors {
            background: kael::transparent_black(),
            foreground: theme.tokens.foreground,
            border: kael::transparent_black(),
            hover_background: theme.tokens.accent,
            hover_foreground: theme.tokens.accent_foreground,
            has_shadow: false,
            has_border: false,
        };

        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .w(px(60.0))
            .child(
                div()
                    .px(px(4.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.tokens.muted_foreground)
                    .child(label),
            )
            .child(
                div()
                    .debug_selector(move || kind.debug_selector().to_string())
                    .h(column_height)
                    .w_full()
                    .rounded(theme.tokens.radius_md)
                    .bg(theme.tokens.muted.opacity(0.35))
                    .overflow_hidden()
                    .child(
                        scrollable_vertical(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .p(px(2.0))
                                .pr(px(10.0))
                                .children(values.into_iter().map(|n| {
                                    let is_selected = n == selected;
                                    let state = state.clone();
                                    let on_change = on_change.clone();
                                    Button::new(
                                        ElementId::Name(format!("{prefix}-{n}-{entity_id}").into()),
                                        format!("{:02}", n),
                                    )
                                    .colors(option_colors)
                                    .selected(is_selected)
                                    .size(ButtonSize::Sm)
                                    .w_full()
                                    .h(px(32.0))
                                    .flex_shrink_0()
                                    .rounded(theme.tokens.radius_sm)
                                    .when(is_selected, |button| {
                                        button
                                            .bg(theme.tokens.primary)
                                            .text_color(theme.tokens.primary_foreground)
                                    })
                                    .on_click(
                                        move |_, window, cx| {
                                            state.update(cx, |s, cx| {
                                                match kind {
                                                    TimeCol::Hour => s.set_hour(n, cx),
                                                    TimeCol::Minute => s.set_minute(n, cx),
                                                    TimeCol::Second => s.set_second(n, cx),
                                                }
                                                if let Some(handler) = on_change.as_ref() {
                                                    handler(&s.value, window, cx);
                                                }
                                            });
                                        },
                                    )
                                })),
                        )
                        .id(scroll_id)
                        .with_scroll_handle(scroll_handle)
                        .always_show_scrollbars(),
                    ),
            )
    };

    let hours: Vec<u8> = match format {
        TimeFormat::Hour12 => (1..=12).collect(),
        TimeFormat::Hour24 => (0..=23).collect(),
    };
    let minutes: Vec<u8> = (0..=59).collect();
    let seconds: Vec<u8> = (0..=59).collect();

    let mut anchor = anchored().snap_to_window_with_margin(Edges::all(GAP));
    if measured {
        anchor = anchor.anchor(anchor_corner).position(anchor_pos);
    }

    deferred(
        anchor.child(
            div()
                .id(("time-picker-popup", entity_id))
                .occlude()
                .when(use_mb, |d| d.mb(GAP))
                .when(!use_mb, |d| d.mt(GAP))
                .flex()
                .flex_row()
                .gap(px(6.0))
                .bg(theme.tokens.popover)
                .border_1()
                .border_color(theme.tokens.border)
                .rounded(theme.tokens.radius_lg)
                .shadow(theme.tokens.shadow_lg.to_vec())
                .p(px(8.0))
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Dialog).label("Choose time"),
                )
                .child(make_col(
                    "Hour",
                    "tp-hour",
                    hours,
                    value.hour,
                    TimeCol::Hour,
                    scroll_handles.hour,
                ))
                .child(make_col(
                    "Minute",
                    "tp-min",
                    minutes,
                    value.minute,
                    TimeCol::Minute,
                    scroll_handles.minute,
                ))
                .when(show_seconds, |d| {
                    d.child(make_col(
                        "Second",
                        "tp-sec",
                        seconds,
                        value.second.unwrap_or(0),
                        TimeCol::Second,
                        scroll_handles.second,
                    ))
                })
                .when(format == TimeFormat::Hour12, |d| {
                    d.child(period_col(
                        theme,
                        state,
                        value,
                        on_change.clone(),
                        entity_id,
                    ))
                }),
        ),
    )
    .with_priority(1)
    .into_any_element()
}

fn period_col(
    theme: &Theme,
    state: &Entity<TimePickerState>,
    value: TimeValue,
    on_change: Option<Rc<dyn Fn(&TimeValue, &mut Window, &mut App)>>,
    entity_id: u64,
) -> AnyElement {
    let btn = |label: &'static str, period: TimePeriod, selected: bool| {
        let state = state.clone();
        let on_change = on_change.clone();
        Button::new(
            ElementId::Name(format!("tp-period-{label}-{entity_id}").into()),
            label,
        )
        .variant(ButtonVariant::Ghost)
        .selected(selected)
        .size(ButtonSize::Sm)
        .w_full()
        .when(selected, |button| {
            button
                .bg(theme.tokens.primary)
                .text_color(theme.tokens.primary_foreground)
        })
        .on_click(move |_, window, cx| {
            state.update(cx, |s, cx| {
                s.set_period(period, cx);
                if let Some(handler) = on_change.as_ref() {
                    handler(&s.value, window, cx);
                }
            });
        })
        .into_any_element()
    };

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .w(px(60.0))
        .child(
            div()
                .px(px(4.0))
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.tokens.muted_foreground)
                .child("Period"),
        )
        .child(btn(
            "AM",
            TimePeriod::AM,
            value.period == Some(TimePeriod::AM),
        ))
        .child(btn(
            "PM",
            TimePeriod::PM,
            value.period == Some(TimePeriod::PM),
        ))
        .into_any_element()
}

fn is_time_input_text(text: &str) -> bool {
    text.chars().all(|ch| {
        ch.is_ascii_digit() || matches!(ch, ':' | ' ' | 'a' | 'A' | 'p' | 'P' | 'm' | 'M')
    })
}

fn parse_time_input(input: &str, format: TimeFormat, show_seconds: bool) -> Option<TimeValue> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let period = if lower.ends_with("am") {
        Some(TimePeriod::AM)
    } else if lower.ends_with("pm") {
        Some(TimePeriod::PM)
    } else {
        None
    };

    let time_part = lower.trim_end_matches("am").trim_end_matches("pm").trim();
    let parts = time_part.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }

    let hour = parts[0].parse::<u8>().ok()?;
    let minute = parts[1].parse::<u8>().ok()?;
    let second = match (show_seconds, parts.len()) {
        (true, 2) => Some(0),
        (true, 3) => Some(parts[2].parse::<u8>().ok()?),
        (false, 2) => None,
        (false, 3) => return None,
        _ => return None,
    };

    if minute > 59 || second.is_some_and(|second| second > 59) {
        return None;
    }

    match format {
        TimeFormat::Hour12 => {
            let period = period.or(Some(TimePeriod::AM));
            if !(1..=12).contains(&hour) {
                return None;
            }
            Some(TimeValue {
                hour,
                minute,
                second,
                period,
            })
        }
        TimeFormat::Hour24 => {
            if hour > 23 || period.is_some() {
                return None;
            }
            Some(TimeValue {
                hour,
                minute,
                second,
                period: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_time_value, parse_time_input, scroll_offset_for_selection, TimeFormat,
        TimePeriod, TimePicker, TimePickerScrollHandles, TimePickerState, TimeValue,
    };
    use kael::{
        point, px, AccessibilityAction, AccessibilityRole, AccessibilityState, AppContext, Context,
        Entity, IntoElement, Modifiers, Render, ScrollDelta, ScrollWheelEvent, TestAppContext,
        Window,
    };

    struct TimePickerHost {
        state: Entity<TimePickerState>,
        read_only: bool,
    }

    impl Render for TimePickerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            TimePicker::new(self.state.clone()).read_only(self.read_only)
        }
    }

    #[test]
    fn time_value_conversion_clamps_untrusted_public_fields() {
        let value = TimeValue {
            hour: u8::MAX,
            minute: u8::MAX,
            second: Some(u8::MAX),
            period: Some(TimePeriod::PM),
        };

        assert_eq!(value.to_24h(), (15, 59, Some(59)));
        assert_eq!(
            TimeValue::from_24h(99, 99, Some(99), TimeFormat::Hour24),
            TimeValue::new(23, 59).with_seconds(59)
        );
    }

    #[test]
    fn normalization_preserves_picker_invariants() {
        let normalized = normalize_time_value(
            TimeValue::new(23, 75).with_seconds(75),
            TimeFormat::Hour12,
            true,
        );

        assert_eq!(normalized.hour, 11);
        assert_eq!(normalized.minute, 59);
        assert_eq!(normalized.second, Some(59));
        assert_eq!(normalized.period, Some(TimePeriod::PM));
    }

    #[test]
    fn formatting_converts_values_into_the_requested_clock() {
        assert_eq!(
            TimeValue::new(23, 5).format_string(TimeFormat::Hour12),
            "11:05 PM"
        );
        assert_eq!(
            TimeValue::new(11, 5)
                .with_period(TimePeriod::PM)
                .format_string(TimeFormat::Hour24),
            "23:05"
        );
    }

    #[test]
    fn parser_respects_format_and_seconds_configuration() {
        assert_eq!(
            parse_time_input("11:42 pm", TimeFormat::Hour12, false),
            Some(TimeValue {
                hour: 11,
                minute: 42,
                second: None,
                period: Some(TimePeriod::PM),
            })
        );
        assert_eq!(
            parse_time_input("23:59", TimeFormat::Hour24, true),
            Some(TimeValue::new(23, 59).with_seconds(0))
        );
        assert!(parse_time_input("23:59 pm", TimeFormat::Hour24, false).is_none());
        assert!(parse_time_input("12:10:30", TimeFormat::Hour24, false).is_none());
        assert!(parse_time_input("24:00", TimeFormat::Hour24, false).is_none());
    }

    #[test]
    fn time_columns_keep_independent_offsets() {
        let handles = TimePickerScrollHandles::default();
        handles.hour.set_offset(point(px(0.0), px(-34.0)));
        handles.minute.set_offset(point(px(0.0), px(-340.0)));

        assert_eq!(handles.hour.offset(), point(px(0.0), px(-34.0)));
        assert_eq!(handles.minute.offset(), point(px(0.0), px(-340.0)));
        assert_eq!(handles.second.offset(), point(px(0.0), px(0.0)));
    }

    #[test]
    fn selected_item_offset_centers_when_possible_and_clamps_at_ends() {
        assert_eq!(scroll_offset_for_selection(0, 60), point(px(0.0), px(0.0)));
        assert_eq!(
            scroll_offset_for_selection(30, 60),
            point(px(0.0), px(-952.0))
        );
        assert_eq!(
            scroll_offset_for_selection(59, 60),
            point(px(0.0), px(-1938.0))
        );
    }

    #[kael::test]
    fn wheel_events_scroll_minute_column_without_moving_hours(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(TimePickerState::new);
        cx.update(|cx| state.update(cx, |state, cx| state.open(cx)));
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| TimePickerHost {
                state,
                read_only: false,
            }
        });
        window.simulate_resize(kael::size(px(500.0), px(500.0)));
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
        });

        let minute_bounds = window
            .debug_bounds("time-picker-minute-column")
            .expect("open popup should lay out a minute column");
        assert!(minute_bounds.top() >= px(0.0));
        assert!(minute_bounds.bottom() <= px(500.0));

        let (hour_before, minute_before) = window.update(|_, cx| {
            let state = state.read(cx);
            (
                state.scroll_handles.hour.offset(),
                state.scroll_handles.minute.offset(),
            )
        });
        window.simulate_event(ScrollWheelEvent {
            position: minute_bounds.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-10_000.0))),
            modifiers: Modifiers::default(),
            ..Default::default()
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.update(|_, cx| {
            let state = state.read(cx);
            assert_eq!(state.scroll_handles.hour.offset(), hour_before);
            assert!(state.scroll_handles.minute.offset().y < minute_before.y);
            assert!(state.scroll_handles.minute.offset().y <= px(-1_800.0));
        });

        let minute_59 = window.update(|window, cx| {
            window.draw(cx).clear();
            window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("59")
                        && node.bounds.is_some()
                })
                .map(|node| {
                    assert!(node.actions.contains(&AccessibilityAction::Click));
                    node.id
                })
                .expect("minute 59 should be visible after scrolling to the end")
        });
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(kael::AccessibilityActionRequest::new(
                minute_59,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
        window.update(|_, cx| assert_eq!(state.read(cx).value().minute, 59));
    }

    #[kael::test]
    fn popup_accessibility_identity_is_stable_across_frames(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(TimePickerState::new);
        cx.update(|cx| state.update(cx, |state, cx| state.open(cx)));
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| TimePickerHost {
                state,
                read_only: false,
            }
        });

        let dialog_id = |window: &mut Window, cx: &mut kael::App| {
            window.draw(cx).clear();
            window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::Dialog)
                .expect("open picker should expose a dialog")
                .id
        };
        let first_id = window.update(dialog_id);
        let second_id = window.update(dialog_id);
        assert_eq!(first_id, second_id);
    }

    #[kael::test]
    fn read_only_picker_is_focusable_but_not_editable(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(TimePickerState::new);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| TimePickerHost {
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
                .expect("picker should expose combobox semantics");
            assert!(node.states.contains(AccessibilityState::READ_ONLY));
            assert!(node.actions.contains(&AccessibilityAction::Focus));
            assert!(!node.actions.contains(&AccessibilityAction::Click));
            assert!(!node.actions.contains(&AccessibilityAction::Expand));
        });
    }
}
