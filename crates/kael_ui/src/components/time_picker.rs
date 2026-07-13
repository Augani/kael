use crate::components::{
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
            Some(TimePeriod::AM) => {
                if self.hour == 12 {
                    0
                } else {
                    self.hour
                }
            }
            Some(TimePeriod::PM) => {
                if self.hour == 12 {
                    12
                } else {
                    self.hour + 12
                }
            }
            None => self.hour,
        };
        (hour, self.minute, self.second)
    }

    pub fn from_24h(hour: u8, minute: u8, second: Option<u8>, format: TimeFormat) -> Self {
        match format {
            TimeFormat::Hour24 => Self {
                hour: hour.min(23),
                minute: minute.min(59),
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
                    minute: minute.min(59),
                    second,
                    period: Some(period),
                }
            }
        }
    }

    pub fn format_string(&self, format: TimeFormat) -> String {
        let hour_str = format!("{:02}", self.hour);
        let minute_str = format!("{:02}", self.minute);

        let base = if let Some(sec) = self.second {
            format!("{}:{}:{:02}", hour_str, minute_str, sec)
        } else {
            format!("{}:{}", hour_str, minute_str)
        };

        match (format, self.period) {
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

pub struct TimePickerState {
    value: TimeValue,
    format: TimeFormat,
    show_seconds: bool,
    open: bool,
    pending_input: Option<String>,
    focus_handle: FocusHandle,
    trigger_bounds: Bounds<Pixels>,
}

impl TimePickerState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            value: TimeValue::default(),
            format: TimeFormat::Hour24,
            show_seconds: false,
            open: false,
            pending_input: None,
            focus_handle: cx.focus_handle(),
            trigger_bounds: Bounds::default(),
        }
    }

    pub fn value(&self) -> TimeValue {
        self.value
    }

    pub fn set_value(&mut self, value: TimeValue, cx: &mut Context<Self>) {
        self.value = value;
        cx.notify();
    }

    pub fn set_hour(&mut self, hour: u8, cx: &mut Context<Self>) {
        let max_hour = match self.format {
            TimeFormat::Hour12 => 12,
            TimeFormat::Hour24 => 23,
        };
        self.value.hour = hour.min(max_hour);
        if self.format == TimeFormat::Hour12 && self.value.hour == 0 {
            self.value.hour = 12;
        }
        cx.notify();
    }

    pub fn set_minute(&mut self, minute: u8, cx: &mut Context<Self>) {
        self.value.minute = minute.min(59);
        cx.notify();
    }

    pub fn set_second(&mut self, second: u8, cx: &mut Context<Self>) {
        self.value.second = Some(second.min(59));
        cx.notify();
    }

    pub fn set_period(&mut self, period: TimePeriod, cx: &mut Context<Self>) {
        self.value.period = Some(period);
        cx.notify();
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
        self.value.hour = if self.value.hour >= max {
            min
        } else {
            self.value.hour + 1
        };
        cx.notify();
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
        self.value.hour = if self.value.hour <= min {
            max
        } else {
            self.value.hour - 1
        };
        cx.notify();
    }

    pub fn increment_minute(&mut self, cx: &mut Context<Self>) {
        self.value.minute = if self.value.minute >= 59 {
            0
        } else {
            self.value.minute + 1
        };
        cx.notify();
    }

    pub fn decrement_minute(&mut self, cx: &mut Context<Self>) {
        self.value.minute = if self.value.minute == 0 {
            59
        } else {
            self.value.minute - 1
        };
        cx.notify();
    }

    pub fn increment_second(&mut self, cx: &mut Context<Self>) {
        if let Some(sec) = self.value.second {
            self.value.second = Some(if sec >= 59 { 0 } else { sec + 1 });
            cx.notify();
        }
    }

    pub fn decrement_second(&mut self, cx: &mut Context<Self>) {
        if let Some(sec) = self.value.second {
            self.value.second = Some(if sec == 0 { 59 } else { sec - 1 });
            cx.notify();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.notify();
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.pending_input = None;
        cx.notify();
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
            cx.notify();
        }
    }

    pub fn show_seconds(&self) -> bool {
        self.show_seconds
    }

    pub fn set_show_seconds(&mut self, show: bool, cx: &mut Context<Self>) {
        self.show_seconds = show;
        if show && self.value.second.is_none() {
            self.value.second = Some(0);
        } else if !show {
            self.value.second = None;
        }
        cx.notify();
    }

    fn display_text(&self) -> String {
        self.pending_input
            .clone()
            .unwrap_or_else(|| self.value.format_string(self.format))
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
        cx.notify();
        true
    }

    fn cancel_pending(&mut self, cx: &mut Context<Self>) {
        self.pending_input = None;
        cx.notify();
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
        cx.notify();
    }
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
    optional: bool,
    required: bool,
    loading: bool,
    clearable: bool,
    status: Option<(FieldStatusType, SharedString)>,
    format: Option<TimeFormat>,
    show_seconds: Option<bool>,
    size: InputSize,
    on_change: Option<Rc<dyn Fn(&TimeValue, &mut Window, &mut App)>>,
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
            optional: false,
            required: false,
            loading: false,
            clearable: false,
            status: None,
            format: None,
            show_seconds: None,
            size: InputSize::default(),
            on_change: None,
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
        let pending_input = state_data.pending_input.clone();
        let focus_handle = state_data.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let state = self.state.clone();
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
        let has_value = !display_text.is_empty();
        let status = self.status.clone();

        let control = div()
            .relative()
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(
                div()
                    .id("time-picker-trigger")
                    .relative()
                    .track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .h(height)
                    .px(padding_x)
                    .min_w(px(120.0))
                    .bg(theme.tokens.card)
                    .border_1()
                    .border_color(if status.is_some() {
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
                    .when(!effectively_disabled && !is_focused, |d| {
                        d.cursor_pointer().hover(move |style| {
                            style
                                .border_color(theme.tokens.input)
                                .shadow(smallvec::smallvec![hover_ring])
                        })
                    })
                    .when(effectively_disabled, |d| d.opacity(0.5))
                    .when(!effectively_disabled, {
                        let state = state.clone();
                        let on_change = self.on_change.clone();
                        move |d| {
                            d.on_mouse_down(MouseButton::Left, {
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
                                state.update(cx, |s, cx| {
                                    let mut changed = false;

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
                                            s.cancel_pending(cx);
                                            s.close(cx);
                                        }
                                        "enter" => changed = s.commit_pending(cx),
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
                                            }
                                        }
                                    }

                                    if changed {
                                        if let Some(handler) = on_change.as_ref() {
                                            handler(&s.value, window, cx);
                                        }
                                    }
                                });
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
                                theme.tokens.foreground
                            } else {
                                theme.tokens.muted_foreground
                            })
                            .child(display_text),
                    )
                    .when(self.loading, |this| {
                        this.child(Spinner::new().size(SpinnerSize::Sm))
                    })
                    .when(self.clearable && has_value && !effectively_disabled, {
                        let state = state.clone();
                        let on_change = self.on_change.clone();
                        move |this| {
                            this.child(
                                div()
                                    .size(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .line_height(px(0.0))
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(theme.tokens.accent))
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        window.prevent_default();
                                        state.update(cx, |state, cx| {
                                            state.set_value(TimeValue::default(), cx);
                                            if let Some(handler) = on_change.as_ref() {
                                                handler(&state.value, window, cx);
                                            }
                                        });
                                    })
                                    .child(
                                        Icon::new("x")
                                            .size(px(14.0))
                                            .color(theme.tokens.muted_foreground),
                                    ),
                            )
                        }
                    })
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
                move |_, _, cx| {
                    state.update(cx, |s, cx| {
                        if s.open {
                            s.close(cx);
                        }
                    });
                }
            })
            .when(open && !effectively_disabled, |control| {
                control.child(time_picker_popup(
                    theme,
                    &state,
                    value,
                    format,
                    show_seconds,
                    self.on_change.clone(),
                    trigger_bounds,
                    window.viewport_size(),
                ))
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

#[derive(Clone, Copy)]
enum TimeCol {
    Hour,
    Minute,
    Second,
}

#[allow(clippy::too_many_arguments)]
fn time_picker_popup(
    theme: &Theme,
    state: &Entity<TimePickerState>,
    value: TimeValue,
    format: TimeFormat,
    show_seconds: bool,
    on_change: Option<Rc<dyn Fn(&TimeValue, &mut Window, &mut App)>>,
    trigger_bounds: Bounds<Pixels>,
    viewport: Size<Pixels>,
) -> AnyElement {
    const GAP: Pixels = px(4.0);
    let est_height = px(280.0);
    let measured = trigger_bounds.size.width > px(0.0);
    let space_below = viewport.height - trigger_bounds.bottom();
    let space_above = trigger_bounds.top();
    let open_up = space_below < est_height + GAP && space_above > space_below;
    let (anchor_corner, anchor_pos) = if open_up {
        (Corner::BottomLeft, trigger_bounds.corner(Corner::TopLeft))
    } else {
        (Corner::TopLeft, trigger_bounds.corner(Corner::BottomLeft))
    };
    let use_mb = measured && open_up;

    let make_col = |prefix: &'static str, values: Vec<u8>, selected: u8, kind: TimeCol| {
        div().flex().flex_col().child(div().max_h(px(220.0)).child(
            scrollable_vertical(div().flex().flex_col().gap(px(2.0)).px(px(2.0)).children(
                values.into_iter().map(|n| {
                    let is_sel = n == selected;
                    let state = state.clone();
                    let on_change = on_change.clone();
                    div()
                        .id(SharedString::from(format!("{prefix}-{n}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(46.0))
                        .h(px(30.0))
                        .flex_shrink_0()
                        .rounded(theme.tokens.radius_md)
                        .text_size(px(13.0))
                        .cursor_pointer()
                        .font_family(theme.tokens.font_family.clone())
                        .when(is_sel, |d| {
                            d.bg(theme.tokens.primary)
                                .text_color(theme.tokens.primary_foreground)
                        })
                        .when(!is_sel, |d| {
                            d.text_color(theme.tokens.foreground)
                                .hover(|s| s.bg(theme.tokens.accent))
                        })
                        .child(format!("{:02}", n))
                        .on_click(move |_, window, cx| {
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
                        })
                        .into_any_element()
                }),
            )),
        ))
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
                .child(make_col("tp-hour", hours, value.hour, TimeCol::Hour))
                .child(make_col("tp-min", minutes, value.minute, TimeCol::Minute))
                .when(show_seconds, |d| {
                    d.child(make_col(
                        "tp-sec",
                        seconds,
                        value.second.unwrap_or(0),
                        TimeCol::Second,
                    ))
                })
                .when(format == TimeFormat::Hour12, |d| {
                    d.child(period_col(theme, state, value, on_change.clone()))
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
) -> AnyElement {
    let btn = |label: &'static str, period: TimePeriod, selected: bool| {
        let state = state.clone();
        let on_change = on_change.clone();
        div()
            .id(SharedString::from(format!("tp-period-{label}")))
            .flex()
            .items_center()
            .justify_center()
            .w(px(46.0))
            .h(px(30.0))
            .rounded(theme.tokens.radius_md)
            .text_size(px(13.0))
            .cursor_pointer()
            .font_family(theme.tokens.font_family.clone())
            .when(selected, |d| {
                d.bg(theme.tokens.primary)
                    .text_color(theme.tokens.primary_foreground)
            })
            .when(!selected, |d| {
                d.text_color(theme.tokens.foreground)
                    .hover(|s| s.bg(theme.tokens.accent))
            })
            .child(label)
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
        .gap(px(2.0))
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
    let second = if show_seconds && parts.len() == 3 {
        Some(parts[2].parse::<u8>().ok()?)
    } else {
        None
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
            if hour > 23 {
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
