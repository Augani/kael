//! Calendar component - Date selection with month/year navigation.

use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::components::button::ButtonVariant;
use crate::components::icon_button::IconButton;
use crate::styled_ext::StyledExt;
use crate::theme::Theme;

/// Default English weekday abbreviations
pub const DEFAULT_WEEKDAYS: [&str; 7] = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];

/// Default English month names
pub const DEFAULT_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Localization configuration for the Calendar component
#[derive(Clone)]
pub struct CalendarLocale {
    /// Weekday abbreviations (Sunday to Saturday)
    pub weekdays: [SharedString; 7],
    /// Full month names (January to December)
    pub months: [SharedString; 12],
}

impl CalendarLocale {
    /// Create a new locale with custom weekdays and months
    pub fn new(weekdays: [SharedString; 7], months: [SharedString; 12]) -> Self {
        Self { weekdays, months }
    }

    /// English locale (default)
    pub fn english() -> Self {
        Self {
            weekdays: DEFAULT_WEEKDAYS.map(|s| s.into()),
            months: DEFAULT_MONTHS.map(|s| s.into()),
        }
    }

    /// French locale
    pub fn french() -> Self {
        Self {
            weekdays: ["Di", "Lu", "Ma", "Me", "Je", "Ve", "Sa"].map(|s| s.into()),
            months: [
                "Janvier",
                "Février",
                "Mars",
                "Avril",
                "Mai",
                "Juin",
                "Juillet",
                "Août",
                "Septembre",
                "Octobre",
                "Novembre",
                "Décembre",
            ]
            .map(|s| s.into()),
        }
    }

    /// Spanish locale
    pub fn spanish() -> Self {
        Self {
            weekdays: ["Do", "Lu", "Ma", "Mi", "Ju", "Vi", "Sá"].map(|s| s.into()),
            months: [
                "Enero",
                "Febrero",
                "Marzo",
                "Abril",
                "Mayo",
                "Junio",
                "Julio",
                "Agosto",
                "Septiembre",
                "Octubre",
                "Noviembre",
                "Diciembre",
            ]
            .map(|s| s.into()),
        }
    }

    /// German locale
    pub fn german() -> Self {
        Self {
            weekdays: ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"].map(|s| s.into()),
            months: [
                "Januar",
                "Februar",
                "März",
                "April",
                "Mai",
                "Juni",
                "Juli",
                "August",
                "September",
                "Oktober",
                "November",
                "Dezember",
            ]
            .map(|s| s.into()),
        }
    }

    /// Portuguese locale
    pub fn portuguese() -> Self {
        Self {
            weekdays: ["Do", "Se", "Te", "Qa", "Qi", "Sx", "Sá"].map(|s| s.into()),
            months: [
                "Janeiro",
                "Fevereiro",
                "Março",
                "Abril",
                "Maio",
                "Junho",
                "Julho",
                "Agosto",
                "Setembro",
                "Outubro",
                "Novembro",
                "Dezembro",
            ]
            .map(|s| s.into()),
        }
    }

    /// Italian locale
    pub fn italian() -> Self {
        Self {
            weekdays: ["Do", "Lu", "Ma", "Me", "Gi", "Ve", "Sa"].map(|s| s.into()),
            months: [
                "Gennaio",
                "Febbraio",
                "Marzo",
                "Aprile",
                "Maggio",
                "Giugno",
                "Luglio",
                "Agosto",
                "Settembre",
                "Ottobre",
                "Novembre",
                "Dicembre",
            ]
            .map(|s| s.into()),
        }
    }
}

impl Default for CalendarLocale {
    fn default() -> Self {
        Self::english()
    }
}

pub type ISODateString = SharedString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayOfWeek(u8);

impl DayOfWeek {
    pub const SUNDAY: Self = Self(0);
    pub const MONDAY: Self = Self(1);
    pub const TUESDAY: Self = Self(2);
    pub const WEDNESDAY: Self = Self(3);
    pub const THURSDAY: Self = Self(4);
    pub const FRIDAY: Self = Self(5);
    pub const SATURDAY: Self = Self(6);

    pub fn new(day: u8) -> Self {
        Self(day % 7)
    }

    pub fn index(self) -> u8 {
        self.0
    }
}

impl Default for DayOfWeek {
    fn default() -> Self {
        Self::SUNDAY
    }
}

impl From<u8> for DayOfWeek {
    fn from(value: u8) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DateValue {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl DateValue {
    pub fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }

    pub fn from_iso(date: impl AsRef<str>) -> Option<Self> {
        let date = date.as_ref();
        let mut parts = date.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&month) {
            return None;
        }

        let value = Self::new(year, month, day);
        if day == 0 || day > value.days_in_month() {
            return None;
        }
        Some(value)
    }

    pub fn to_iso(self) -> ISODateString {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day).into()
    }

    pub fn today() -> Self {
        let days = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64 / 86_400)
            .unwrap_or(0);
        Self::from_unix_days(days)
    }

    fn from_unix_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = mp + if mp < 10 { 3 } else { -9 };
        let year = year + if month <= 2 { 1 } else { 0 };

        Self::new(year as i32, month as u32, day as u32)
    }

    pub fn days_in_month(&self) -> u32 {
        match self.month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (self.year % 4 == 0 && self.year % 100 != 0) || (self.year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            _ => 30,
        }
    }

    pub fn day_of_week(&self) -> DayOfWeek {
        let days = self.to_unix_days();
        DayOfWeek::new((days + 4).rem_euclid(7) as u8)
    }

    pub fn add_days(&self, days: i64) -> Self {
        Self::from_unix_days(self.to_unix_days() + days)
    }

    pub fn add_months(&self, months: i32) -> Self {
        let month_index = self.year * 12 + self.month as i32 - 1 + months;
        let year = month_index.div_euclid(12);
        let month = month_index.rem_euclid(12) as u32 + 1;
        let candidate = Self::new(year, month, 1);
        Self::new(year, month, self.day.min(candidate.days_in_month()))
    }

    fn first_day_of_week(&self) -> u32 {
        let q = self.day as i32;
        let m = if self.month < 3 {
            (self.month + 12) as i32
        } else {
            self.month as i32
        };
        let y = if self.month < 3 {
            self.year - 1
        } else {
            self.year
        };

        let h = (q + (13 * (m + 1)) / 5 + y + y / 4 - y / 100 + y / 400) % 7;
        ((h + 6) % 7) as u32
    }

    fn to_unix_days(self) -> i64 {
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let month = self.month as i64;
        let mp = month + if month > 2 { -3 } else { 9 };
        let doy = (153 * mp + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateRange {
    pub start: DateValue,
    pub end: DateValue,
}

impl DateRange {
    pub fn new(start: DateValue, end: DateValue) -> Self {
        if end < start {
            Self {
                start: end,
                end: start,
            }
        } else {
            Self { start, end }
        }
    }

    pub fn from_iso(start: impl AsRef<str>, end: impl AsRef<str>) -> Option<Self> {
        Some(Self::new(
            DateValue::from_iso(start)?,
            DateValue::from_iso(end)?,
        ))
    }

    pub fn start_iso(self) -> ISODateString {
        self.start.to_iso()
    }

    pub fn end_iso(self) -> ISODateString {
        self.end.to_iso()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarDay {
    pub date: DateValue,
    pub iso: ISODateString,
    pub is_outside: bool,
    pub day_number: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseCalendarDaysOptions {
    pub year: i32,
    pub month: u32,
    pub week_starts_on: DayOfWeek,
    pub has_variable_row_count: bool,
}

impl UseCalendarDaysOptions {
    pub fn new(year: i32, month: u32) -> Self {
        Self {
            year,
            month,
            week_starts_on: DayOfWeek::SUNDAY,
            has_variable_row_count: false,
        }
    }

    pub fn week_starts_on(mut self, week_starts_on: impl Into<DayOfWeek>) -> Self {
        self.week_starts_on = week_starts_on.into();
        self
    }

    pub fn has_variable_row_count(mut self, has_variable_row_count: bool) -> Self {
        self.has_variable_row_count = has_variable_row_count;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UseCalendarDaysReturn {
    pub days: Vec<CalendarDay>,
    pub weeks: Vec<Vec<CalendarDay>>,
    pub day_names: Vec<SharedString>,
    pub total_cells: usize,
}

#[derive(Clone)]
pub struct UseCalendarConstraintsOptions {
    pub min: Option<DateValue>,
    pub max: Option<DateValue>,
    pub date_constraints: Vec<Rc<dyn Fn(&DateValue) -> bool>>,
}

#[derive(Clone)]
pub struct UseCalendarConstraintsReturn {
    pub min: Option<DateValue>,
    pub max: Option<DateValue>,
    pub date_constraints: Vec<Rc<dyn Fn(&DateValue) -> bool>>,
}

impl UseCalendarConstraintsReturn {
    pub fn is_date_disabled(&self, date: &DateValue) -> bool {
        self.min.is_some_and(|min| *date < min)
            || self.max.is_some_and(|max| *date > max)
            || self
                .date_constraints
                .iter()
                .any(|constraint| !constraint(date))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CalendarHandle {
    focus_date: Option<DateValue>,
}

impl CalendarHandle {
    pub fn navigate_to(&mut self, date: DateValue) {
        self.focus_date = Some(DateValue::new(date.year, date.month, 1));
    }

    #[allow(non_snake_case)]
    pub fn navigateTo(&mut self, date: ISODateString) {
        if let Some(date) = DateValue::from_iso(date.as_ref()) {
            self.navigate_to(date);
        }
    }

    pub fn focus_date(&self) -> Option<DateValue> {
        self.focus_date
    }
}

pub type CalendarProps = Calendar;

pub fn use_calendar_days(options: UseCalendarDaysOptions) -> UseCalendarDaysReturn {
    let month = DateValue::new(options.year, options.month.clamp(1, 12), 1);
    let total_days_in_month = month.days_in_month();
    let mut starting_day = month.first_day_of_week() as i32 - options.week_starts_on.index() as i32;
    if starting_day < 0 {
        starting_day += 7;
    }

    let total_days = total_days_in_month as i32 + starting_day;
    let total_rows = if options.has_variable_row_count {
        ((total_days + 6) / 7).max(1) as usize
    } else {
        6
    };
    let total_cells = total_rows * 7;

    let days = (0..total_cells)
        .map(|index| {
            let day_offset = index as i32 - starting_day + 1;
            let is_outside = day_offset < 1 || day_offset > total_days_in_month as i32;
            let date = if is_outside {
                month.add_days(day_offset as i64 - 1)
            } else {
                DateValue::new(options.year, month.month, day_offset as u32)
            };
            CalendarDay {
                date,
                iso: date.to_iso(),
                is_outside,
                day_number: date.day,
            }
        })
        .collect::<Vec<_>>();

    let weeks = days.chunks(7).map(|week| week.to_vec()).collect::<Vec<_>>();
    let names = DEFAULT_WEEKDAYS
        .iter()
        .map(|day| SharedString::from(*day))
        .collect::<Vec<_>>();
    let day_names = (0..7)
        .map(|index| names[(index + options.week_starts_on.index() as usize) % 7].clone())
        .collect();

    UseCalendarDaysReturn {
        days,
        weeks,
        day_names,
        total_cells,
    }
}

#[allow(non_snake_case)]
pub fn useCalendarDays(options: UseCalendarDaysOptions) -> UseCalendarDaysReturn {
    use_calendar_days(options)
}

pub fn use_calendar_constraints(
    options: UseCalendarConstraintsOptions,
) -> UseCalendarConstraintsReturn {
    UseCalendarConstraintsReturn {
        min: options.min,
        max: options.max,
        date_constraints: options.date_constraints,
    }
}

#[allow(non_snake_case)]
pub fn useCalendarConstraints(
    options: UseCalendarConstraintsOptions,
) -> UseCalendarConstraintsReturn {
    use_calendar_constraints(options)
}

pub fn is_same_day(a: &DateValue, b: &DateValue) -> bool {
    a == b
}

#[allow(non_snake_case)]
pub fn isSameDay(a: &DateValue, b: &DateValue) -> bool {
    is_same_day(a, b)
}

pub fn is_date_in_range(date: &DateValue, range: &DateRange) -> bool {
    *date >= range.start && *date <= range.end
}

#[allow(non_snake_case)]
pub fn isDateInRange(date: &DateValue, range: &DateRange) -> bool {
    is_date_in_range(date, range)
}

pub fn get_week_number(date: &DateValue) -> u32 {
    let thursday = date.add_days(3 - ((date.day_of_week().index() as i32 + 6) % 7) as i64);
    let first_thursday = DateValue::new(thursday.year, 1, 4);
    ((thursday.to_unix_days() - first_thursday.to_unix_days()) / 7 + 1).max(1) as u32
}

#[allow(non_snake_case)]
pub fn getWeekNumber(date: &DateValue) -> u32 {
    get_week_number(date)
}

pub fn format_accessible_date(date: &DateValue, locale: &CalendarLocale) -> SharedString {
    let month = locale
        .months
        .get(date.month.saturating_sub(1) as usize)
        .cloned()
        .unwrap_or_else(|| "Unknown".into());
    let weekday = locale
        .weekdays
        .get(date.day_of_week().index() as usize)
        .cloned()
        .unwrap_or_else(|| "".into());
    format!("{weekday}, {month} {} {}", date.day, date.year).into()
}

#[allow(non_snake_case)]
pub fn formatAccessibleDate(date: &DateValue, locale: &CalendarLocale) -> SharedString {
    format_accessible_date(date, locale)
}

#[derive(IntoElement)]
pub struct Calendar {
    current_month: DateValue,
    selected_date: Option<DateValue>,
    selected_range: Option<DateRange>,
    range_start_temp: Option<DateValue>,
    min: Option<DateValue>,
    max: Option<DateValue>,
    date_constraints: Vec<Rc<dyn Fn(&DateValue) -> bool>>,
    on_date_select: Option<Rc<dyn Fn(&DateValue, &mut Window, &mut App)>>,
    on_month_change: Option<Rc<dyn Fn(&DateValue, &mut Window, &mut App)>>,
    on_focus_date_change: Option<Rc<dyn Fn(&ISODateString, &mut Window, &mut App)>>,
    disabled_dates: Vec<DateValue>,
    is_date_disabled: Option<Rc<dyn Fn(&DateValue) -> bool>>,
    number_of_months: usize,
    has_outside_days: bool,
    has_week_numbers: bool,
    has_variable_row_count: bool,
    week_starts_on: DayOfWeek,
    locale: CalendarLocale,
    style: StyleRefinement,
}

impl Calendar {
    pub fn new() -> Self {
        let today = DateValue::today();
        let current_month = DateValue::new(today.year, today.month, 1);

        Self {
            current_month,
            selected_date: None,
            selected_range: None,
            range_start_temp: None,
            min: None,
            max: None,
            date_constraints: Vec::new(),
            on_date_select: None,
            on_month_change: None,
            on_focus_date_change: None,
            disabled_dates: Vec::new(),
            is_date_disabled: None,
            number_of_months: 1,
            has_outside_days: true,
            has_week_numbers: false,
            has_variable_row_count: false,
            week_starts_on: DayOfWeek::SUNDAY,
            locale: CalendarLocale::default(),
            style: StyleRefinement::default(),
        }
    }

    pub fn current_month(mut self, date: DateValue) -> Self {
        self.current_month = date;
        self
    }

    pub fn focus_date(mut self, date: impl AsRef<str>) -> Self {
        if let Some(date) = DateValue::from_iso(date) {
            self.current_month = DateValue::new(date.year, date.month, 1);
        }
        self
    }

    pub fn selected_date(mut self, date: DateValue) -> Self {
        self.selected_date = Some(date);
        self
    }

    pub fn value(mut self, date: impl AsRef<str>) -> Self {
        self.selected_date = DateValue::from_iso(date);
        self
    }

    pub fn default_value(self, date: impl AsRef<str>) -> Self {
        self.value(date)
    }

    #[allow(non_snake_case)]
    pub fn defaultValue(self, date: impl AsRef<str>) -> Self {
        self.default_value(date)
    }

    pub fn locale(mut self, locale: CalendarLocale) -> Self {
        self.locale = locale;
        self
    }

    pub fn number_of_months(mut self, count: usize) -> Self {
        self.number_of_months = count.clamp(1, 2);
        self
    }

    #[allow(non_snake_case)]
    pub fn numberOfMonths(self, count: usize) -> Self {
        self.number_of_months(count)
    }

    pub fn min(mut self, date: impl AsRef<str>) -> Self {
        self.min = DateValue::from_iso(date);
        self
    }

    pub fn min_date(mut self, date: DateValue) -> Self {
        self.min = Some(date);
        self
    }

    pub fn max(mut self, date: impl AsRef<str>) -> Self {
        self.max = DateValue::from_iso(date);
        self
    }

    pub fn max_date(mut self, date: DateValue) -> Self {
        self.max = Some(date);
        self
    }

    pub fn date_constraint<F>(mut self, checker: F) -> Self
    where
        F: Fn(&DateValue) -> bool + 'static,
    {
        self.date_constraints.push(Rc::new(checker));
        self
    }

    pub fn has_outside_days(mut self, has_outside_days: bool) -> Self {
        self.has_outside_days = has_outside_days;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasOutsideDays(self, has_outside_days: bool) -> Self {
        self.has_outside_days(has_outside_days)
    }

    pub fn has_week_numbers(mut self, has_week_numbers: bool) -> Self {
        self.has_week_numbers = has_week_numbers;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasWeekNumbers(self, has_week_numbers: bool) -> Self {
        self.has_week_numbers(has_week_numbers)
    }

    pub fn has_variable_row_count(mut self, has_variable_row_count: bool) -> Self {
        self.has_variable_row_count = has_variable_row_count;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasVariableRowCount(self, has_variable_row_count: bool) -> Self {
        self.has_variable_row_count(has_variable_row_count)
    }

    pub fn week_starts_on(mut self, day: impl Into<DayOfWeek>) -> Self {
        self.week_starts_on = day.into();
        self
    }

    #[allow(non_snake_case)]
    pub fn weekStartsOn(self, day: impl Into<DayOfWeek>) -> Self {
        self.week_starts_on(day)
    }

    pub fn on_date_select<F>(mut self, handler: F) -> Self
    where
        F: Fn(&DateValue, &mut Window, &mut App) + 'static,
    {
        self.on_date_select = Some(Rc::new(handler));
        self
    }

    pub fn on_month_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(&DateValue, &mut Window, &mut App) + 'static,
    {
        self.on_month_change = Some(Rc::new(handler));
        self
    }

    pub fn on_focus_date_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(&ISODateString, &mut Window, &mut App) + 'static,
    {
        self.on_focus_date_change = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onFocusDateChange<F>(self, handler: F) -> Self
    where
        F: Fn(&ISODateString, &mut Window, &mut App) + 'static,
    {
        self.on_focus_date_change(handler)
    }

    pub fn is_date_disabled<F>(mut self, checker: F) -> Self
    where
        F: Fn(&DateValue) -> bool + 'static,
    {
        self.is_date_disabled = Some(Rc::new(checker));
        self
    }

    pub fn disabled_dates(mut self, dates: Vec<DateValue>) -> Self {
        self.disabled_dates = dates;
        self
    }

    pub fn selected_range(mut self, range: Option<DateRange>) -> Self {
        self.selected_range = range;
        self
    }

    pub fn range_start_temp(mut self, date: Option<DateValue>) -> Self {
        self.range_start_temp = date;
        self
    }

    fn is_date_in_range(date: &DateValue, range: &DateRange) -> bool {
        is_date_in_range(date, range)
    }

    fn prev_month(&self) -> DateValue {
        self.current_month.add_months(-1)
    }

    fn next_month(&self) -> DateValue {
        self.current_month.add_months(1)
    }
}

impl Default for Calendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Calendar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Calendar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let current_month = self.current_month;
        let selected_date = self.selected_date;
        let locale = self.locale.clone();

        let prev_month_date = self.prev_month();
        let next_month_date = self.next_month();

        let on_month_change_handler = self.on_month_change.clone();
        let on_focus_date_change_handler = self.on_focus_date_change.clone();
        let on_date_select_handler = self.on_date_select;
        let is_date_disabled_fn = self.is_date_disabled;
        let disabled_dates = self.disabled_dates;
        let selected_range = self.selected_range;
        let range_start_temp = self.range_start_temp;
        let number_of_months = self.number_of_months;
        let has_outside_days = self.has_outside_days;
        let has_week_numbers = self.has_week_numbers;
        let has_variable_row_count = self.has_variable_row_count;
        let week_starts_on = self.week_starts_on;
        let constraints = use_calendar_constraints(UseCalendarConstraintsOptions {
            min: self.min,
            max: self.max,
            date_constraints: self.date_constraints,
        });

        let user_style = self.style;

        let today = DateValue::today();
        let dark = theme.tokens.background.l < 0.5;
        let hover_overlay = crate::astryx::overlay_hover(dark);
        let visible_months = (0..number_of_months)
            .map(|offset| current_month.add_months(offset as i32))
            .collect::<Vec<_>>();
        let last_visible_month = visible_months.last().copied().unwrap_or(current_month);
        let can_navigate_previous = constraints.min.is_none_or(|min| {
            min.year < current_month.year
                || (min.year == current_month.year && min.month < current_month.month)
        });
        let can_navigate_next = constraints.max.is_none_or(|max| {
            max.year > last_visible_month.year
                || (max.year == last_visible_month.year && max.month > last_visible_month.month)
        });
        let month_label = visible_months
            .iter()
            .map(|month| {
                let month_name = locale
                    .months
                    .get(month.month.saturating_sub(1) as usize)
                    .cloned()
                    .unwrap_or_else(|| "Unknown".into());
                format!("{} {}", month_name, month.year)
            })
            .collect::<Vec<_>>()
            .join(" - ");

        div()
            .flex()
            .flex_col()
            .w(px(if number_of_months == 1 { 220.0 } else { 456.0 }))
            .p(px(12.0))
            .bg(theme.tokens.card)
            .rounded(theme.tokens.radius_md)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .mb(px(8.0))
                    .child({
                        let handler = on_month_change_handler.clone();
                        IconButton::new("chevron-left")
                            .variant(ButtonVariant::Ghost)
                            .size(px(32.0))
                            .icon_size(px(16.0))
                            .disabled(!can_navigate_previous)
                            .when(can_navigate_previous && handler.is_some(), |btn| {
                                let month_handler = handler.unwrap();
                                let focus_handler = on_focus_date_change_handler.clone();
                                btn.on_click(move |_, window, cx| {
                                    month_handler(&prev_month_date, window, cx);
                                    if let Some(focus_handler) = &focus_handler {
                                        let iso = prev_month_date.to_iso();
                                        focus_handler(&iso, window, cx);
                                    }
                                })
                            })
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_center()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .child(month_label),
                    )
                    .child({
                        let handler = on_month_change_handler;
                        IconButton::new("chevron-right")
                            .variant(ButtonVariant::Ghost)
                            .size(px(32.0))
                            .icon_size(px(16.0))
                            .disabled(!can_navigate_next)
                            .when(can_navigate_next && handler.is_some(), |btn| {
                                let month_handler = handler.unwrap();
                                let focus_handler = on_focus_date_change_handler.clone();
                                btn.on_click(move |_, window, cx| {
                                    month_handler(&next_month_date, window, cx);
                                    if let Some(focus_handler) = &focus_handler {
                                        let iso = next_month_date.to_iso();
                                        focus_handler(&iso, window, cx);
                                    }
                                })
                            })
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(16.0))
                    .children(visible_months.into_iter().map(move |month| {
                        let calendar_days = use_calendar_days(
                            UseCalendarDaysOptions::new(month.year, month.month)
                                .week_starts_on(week_starts_on)
                                .has_variable_row_count(has_variable_row_count),
                        );
                        let weekday_names = (0..7)
                            .map(|index| {
                                locale.weekdays[(index + week_starts_on.index() as usize) % 7]
                                    .clone()
                            })
                            .collect::<Vec<_>>();
                        let on_date_select_for_weeks = on_date_select_handler.clone();
                        let is_date_disabled_for_weeks = is_date_disabled_fn.clone();
                        let disabled_dates_for_weeks = disabled_dates.clone();
                        let constraints_for_month = constraints.clone();

                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .mb(px(4.0))
                                    .when(has_week_numbers, |this| {
                                        this.child(div().w(px(32.0)).h(px(32.0)))
                                    })
                                    .children(weekday_names.into_iter().map(|day| {
                                        div()
                                            .w(px(28.0))
                                            .h(px(32.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(12.0))
                                            .font_weight(FontWeight::NORMAL)
                                            .text_color(theme.tokens.muted_foreground)
                                            .child(day)
                                    })),
                            )
                            .child(div().flex().flex_col().children(
                                calendar_days.weeks.into_iter().map(move |week| {
                                    let on_date_select_for_days = on_date_select_for_weeks.clone();
                                    let is_date_disabled_for_days =
                                        is_date_disabled_for_weeks.clone();
                                    let disabled_dates_for_days = disabled_dates_for_weeks.clone();
                                    let range_for_week = selected_range;
                                    let range_start_for_week = range_start_temp;
                                    let constraints_for_week = constraints_for_month.clone();
                                    let week_number =
                                        week.first().map(|day| get_week_number(&day.date));
                                    div()
                                        .flex()
                                        .when(has_week_numbers, |this| {
                                            this.child(
                                                div()
                                                    .w(px(32.0))
                                                    .h(px(32.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .text_size(px(11.0))
                                                    .text_color(
                                                        theme.tokens.muted_foreground.opacity(0.72),
                                                    )
                                                    .child(
                                                        week_number.unwrap_or_default().to_string(),
                                                    ),
                                            )
                                        })
                                        .children(week.into_iter().map(move |calendar_day| {
                                            let date = calendar_day.date;
                                            let is_selected = selected_date == Some(date);

                                            // Check if date is disabled
                                            let is_outside_hidden =
                                                calendar_day.is_outside && !has_outside_days;
                                            let is_disabled = is_outside_hidden
                                                || constraints_for_week.is_date_disabled(&date)
                                                || is_date_disabled_for_days
                                                    .as_ref()
                                                    .map(|f| f(&date))
                                                    .unwrap_or_else(|| {
                                                        disabled_dates_for_days
                                                            .iter()
                                                            .any(|d| d == &date)
                                                    });

                                            // Check if date is in selected range
                                            let is_in_range = range_for_week
                                                .map(|r| Calendar::is_date_in_range(&date, &r))
                                                .unwrap_or(false);

                                            // Check if date is the range start (first click in range mode)
                                            let is_range_start = range_start_for_week == Some(date);

                                            // Check if date is a range endpoint
                                            let is_range_endpoint = range_for_week
                                                .map(|r| date == r.start || date == r.end)
                                                .unwrap_or(false);
                                            let is_today = date == today;

                                            let handler = if is_disabled {
                                                None
                                            } else {
                                                on_date_select_for_days.clone()
                                            };

                                            let day_button = div()
                                                .size(px(28.0))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded_full()
                                                .text_size(px(14.0))
                                                .transition(theme.tokens.transition_fast)
                                                .when(
                                                    is_today && !is_selected && !is_in_range,
                                                    |this| {
                                                        this.inset_ring(
                                                            theme.tokens.border,
                                                            px(1.0),
                                                        )
                                                    },
                                                )
                                                .when(
                                                    is_today && is_in_range && !is_range_endpoint,
                                                    |this| {
                                                        this.inset_ring(
                                                            theme.tokens.foreground,
                                                            px(1.0),
                                                        )
                                                    },
                                                )
                                                .when(
                                                    !is_disabled
                                                        && (is_range_endpoint
                                                            || is_selected
                                                            || is_range_start),
                                                    |this| {
                                                        this.bg(theme.tokens.primary)
                                                            .text_color(
                                                                theme.tokens.primary_foreground,
                                                            )
                                                            .font_weight(FontWeight::MEDIUM)
                                                    },
                                                )
                                                .when(
                                                    !is_disabled
                                                        && !(is_range_endpoint
                                                            || is_selected
                                                            || is_range_start),
                                                    |this| this.text_color(theme.tokens.foreground),
                                                )
                                                .when(calendar_day.is_outside, |this| {
                                                    this.text_color(theme.tokens.muted_foreground)
                                                        .opacity(0.64)
                                                })
                                                .when(is_outside_hidden, |this| this.opacity(0.0))
                                                .child(calendar_day.day_number.to_string());

                                            div()
                                                .relative()
                                                .w(px(28.0))
                                                .h(px(32.0))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .transition(theme.tokens.transition_fast)
                                                .when(!is_disabled && is_in_range, |this| {
                                                    this.child(
                                                        div()
                                                            .absolute()
                                                            .top(px(2.0))
                                                            .bottom(px(2.0))
                                                            .left_0()
                                                            .right_0()
                                                            .bg(theme.tokens.primary.opacity(0.16))
                                                            .when(
                                                                is_range_endpoint || is_range_start,
                                                                |bg| {
                                                                    bg.left(px(2.0))
                                                                        .right(px(2.0))
                                                                        .rounded_full()
                                                                },
                                                            ),
                                                    )
                                                })
                                                // Disabled state styling
                                                .when(is_disabled, |this: Div| {
                                                    this.text_color(theme.tokens.muted_foreground)
                                                        .opacity(if is_outside_hidden {
                                                            0.0
                                                        } else {
                                                            0.3
                                                        })
                                                        .cursor(CursorStyle::OperationNotAllowed)
                                                })
                                                .when(!is_disabled, |this: Div| {
                                                    this.cursor(CursorStyle::PointingHand)
                                                        .hover(move |style| style.bg(hover_overlay))
                                                })
                                                // Click handler only for non-disabled dates
                                                .when(handler.is_some(), |this: Div| {
                                                    let handler = handler.unwrap();
                                                    this.on_mouse_down(
                                                        MouseButton::Left,
                                                        move |_, window, cx| {
                                                            handler(&date, window, cx);
                                                        },
                                                    )
                                                })
                                                .child(day_button)
                                                .into_any_element()
                                        }))
                                }),
                            ))
                    })),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
