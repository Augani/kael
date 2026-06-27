//! DateRangeInput component - ASTRYX-named range date input facade.

use crate::components::{
    calendar::{DateRange, DateValue},
    date_picker::{DateFormat, DatePicker, DatePickerState, DateSelectionMode},
    field::{Field, FieldStatusType},
    field_status::FieldStatusVariant,
    input::InputSize,
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct DateRangeInput {
    state: Entity<DatePickerState>,
    label: SharedString,
    hidden_label: bool,
    description: Option<SharedString>,
    placeholder: SharedString,
    disabled: bool,
    optional: bool,
    required: bool,
    clearable: bool,
    size: InputSize,
    format: DateFormat,
    status: Option<(FieldStatusType, SharedString)>,
    style: StyleRefinement,
}

impl DateRangeInput {
    pub fn new(label: impl Into<SharedString>, state: Entity<DatePickerState>) -> Self {
        Self {
            state,
            label: label.into(),
            hidden_label: false,
            description: None,
            placeholder: "Select date range".into(),
            disabled: false,
            optional: false,
            required: false,
            clearable: true,
            size: InputSize::default(),
            format: DateFormat::LongDate,
            status: None,
            style: StyleRefinement::default(),
        }
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

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.hidden_label(hidden)
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasClear(self, clearable: bool) -> Self {
        self.clearable(clearable)
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    pub fn format(mut self, format: DateFormat) -> Self {
        self.format = format;
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }
}

impl Styled for DateRangeInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DateRangeInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        self.state.update(cx, |state, cx| {
            state.set_selection_mode(DateSelectionMode::Range, cx);
        });

        let mut field = Field::new(
            self.label,
            DatePicker::new(self.state)
                .placeholder(self.placeholder)
                .format(self.format)
                .size(self.size)
                .disabled(self.disabled)
                .clearable(self.clearable),
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

pub fn date_range(start: DateValue, end: DateValue) -> DateRange {
    DateRange::new(start, end)
}
