//! DateTimeInput component - paired date and time input.

use crate::components::{
    date_picker::{DateFormat, DatePicker, DatePickerState},
    field::{Field, FieldStatusType},
    field_status::FieldStatusVariant,
    input::InputSize,
    time_picker::{TimeFormat, TimePicker, TimePickerState},
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct DateTimeInput {
    date_state: Entity<DatePickerState>,
    time_state: Entity<TimePickerState>,
    label: SharedString,
    hidden_label: bool,
    description: Option<SharedString>,
    disabled: bool,
    optional: bool,
    required: bool,
    loading: bool,
    clearable: bool,
    has_seconds: bool,
    hour_format: TimeFormat,
    size: InputSize,
    status: Option<(FieldStatusType, SharedString)>,
    style: StyleRefinement,
}

impl DateTimeInput {
    pub fn new(
        label: impl Into<SharedString>,
        date_state: Entity<DatePickerState>,
        time_state: Entity<TimePickerState>,
    ) -> Self {
        Self {
            date_state,
            time_state,
            label: label.into(),
            hidden_label: false,
            description: None,
            disabled: false,
            optional: false,
            required: false,
            loading: false,
            clearable: false,
            has_seconds: false,
            hour_format: TimeFormat::Hour12,
            size: InputSize::default(),
            status: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
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

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.hidden_label(hidden)
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

    pub fn has_seconds(mut self, has_seconds: bool) -> Self {
        self.has_seconds = has_seconds;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasSeconds(self, has_seconds: bool) -> Self {
        self.has_seconds(has_seconds)
    }

    pub fn hour_format(mut self, hour_format: TimeFormat) -> Self {
        self.hour_format = hour_format;
        self
    }

    #[allow(non_snake_case)]
    pub fn hourFormat(self, hour_format: TimeFormat) -> Self {
        self.hour_format(hour_format)
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }
}

impl Styled for DateTimeInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DateTimeInput {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut field = Field::new(
            self.label,
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    div().flex_1().child(
                        DatePicker::new(self.date_state)
                            .placeholder("Select a date")
                            .format(DateFormat::LongDate)
                            .size(self.size)
                            .disabled(self.disabled)
                            .isLoading(self.loading)
                            .hasClear(self.clearable),
                    ),
                )
                .child(
                    div().w(px(168.0)).flex_shrink_0().child(
                        TimePicker::new(self.time_state)
                            .placeholder("Select time")
                            .size(self.size)
                            .disabled(self.disabled)
                            .isLoading(self.loading)
                            .hasClear(self.clearable)
                            .hasSeconds(self.has_seconds)
                            .hourFormat(self.hour_format),
                    ),
                ),
        )
        .disabled(self.disabled)
        .optional(self.optional)
        .required(self.required)
        .hidden_label(self.hidden_label)
        .status_variant(FieldStatusVariant::Detached);

        if let Some(description) = self.description {
            field = field.description(description);
        }
        if let Some((tone, message)) = self.status {
            field = field.status(tone, message);
        }

        field.map(|this| {
            let mut field = this;
            field.style().refine(&self.style);
            field
        })
    }
}
