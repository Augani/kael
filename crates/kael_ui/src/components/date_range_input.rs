//! DateRangeInput component - ASTRYX-named range date input facade.

use crate::components::{
    calendar::{CalendarLocale, DateRange, DateValue},
    date_picker::{DateFormat, DatePicker, DatePickerState, DateSelectionMode},
    field::FieldStatusType,
    input::InputSize,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

type RangeSelectHandler = Rc<dyn Fn(&DateRange, &mut Window, &mut App)>;
type ClearHandler = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct DateRangeInput {
    state: Entity<DatePickerState>,
    label: SharedString,
    hidden_label: bool,
    description: Option<SharedString>,
    placeholder: SharedString,
    disabled: bool,
    read_only: bool,
    optional: bool,
    required: bool,
    clearable: bool,
    size: InputSize,
    format: DateFormat,
    min_date: Option<DateValue>,
    max_date: Option<DateValue>,
    disabled_dates: Vec<DateValue>,
    disable_weekends: bool,
    locale: CalendarLocale,
    show_today_button: bool,
    status: Option<(FieldStatusType, SharedString)>,
    on_select: Option<RangeSelectHandler>,
    on_clear: Option<ClearHandler>,
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
            read_only: false,
            optional: false,
            required: false,
            clearable: true,
            size: InputSize::default(),
            format: DateFormat::LongDate,
            min_date: None,
            max_date: None,
            disabled_dates: Vec::new(),
            disable_weekends: false,
            locale: CalendarLocale::default(),
            show_today_button: true,
            status: None,
            on_select: None,
            on_clear: None,
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

    pub fn min(mut self, date: DateValue) -> Self {
        self.min_date = Some(date);
        self
    }

    pub fn max(mut self, date: DateValue) -> Self {
        self.max_date = Some(date);
        self
    }

    pub fn disabled_dates(mut self, dates: Vec<DateValue>) -> Self {
        self.disabled_dates = dates;
        self
    }

    pub fn weekends_disabled(mut self, disabled: bool) -> Self {
        self.disable_weekends = disabled;
        self
    }

    pub fn locale(mut self, locale: CalendarLocale) -> Self {
        self.locale = locale;
        self
    }

    pub fn show_today_button(mut self, show: bool) -> Self {
        self.show_today_button = show;
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }

    pub fn on_select(
        mut self,
        handler: impl Fn(&DateRange, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
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

        let mut picker = DatePicker::new(self.state)
            .label(self.label)
            .is_label_hidden(self.hidden_label)
            .placeholder(self.placeholder)
            .format(self.format)
            .size(self.size)
            .disabled(self.disabled)
            .read_only(self.read_only)
            .clearable(self.clearable)
            .optional(self.optional)
            .required(self.required)
            .disabled_dates(self.disabled_dates)
            .weekends_disabled(self.disable_weekends)
            .locale(self.locale)
            .show_today_button(self.show_today_button);

        if let Some(description) = self.description {
            picker = picker.description(description);
        }
        if let Some(min) = self.min_date {
            picker = picker.min(min);
        }
        if let Some(max) = self.max_date {
            picker = picker.max(max);
        }
        if let Some((tone, message)) = self.status {
            picker = picker.status(tone, message);
        }
        if let Some(on_select) = self.on_select {
            picker = picker.on_range_select(move |range, window, cx| {
                on_select(range, window, cx);
            });
        }
        if let Some(on_clear) = self.on_clear {
            picker = picker.on_clear(move |window, cx| on_clear(window, cx));
        }

        picker.map(|this| {
            let mut picker = this;
            picker.style().refine(&self.style);
            picker
        })
    }
}

pub fn date_range(start: DateValue, end: DateValue) -> DateRange {
    DateRange::new(start, end)
}
