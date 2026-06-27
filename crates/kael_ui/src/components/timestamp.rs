//! Timestamp component - formatted absolute and relative times.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::time::{SystemTime, UNIX_EPOCH};

const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const MONTH: i64 = 30 * DAY;
const YEAR: i64 = 365 * DAY;
const DEFAULT_AUTO_THRESHOLD: i64 = 7 * DAY;

#[derive(Clone, Debug)]
pub enum TimestampValue {
    UnixSeconds(i64),
    UnixMillis(i64),
    SystemTime(SystemTime),
    Text(SharedString),
}

impl From<i64> for TimestampValue {
    fn from(value: i64) -> Self {
        Self::UnixSeconds(value)
    }
}

impl From<u64> for TimestampValue {
    fn from(value: u64) -> Self {
        Self::UnixSeconds(value as i64)
    }
}

impl From<SystemTime> for TimestampValue {
    fn from(value: SystemTime) -> Self {
        Self::SystemTime(value)
    }
}

impl From<SharedString> for TimestampValue {
    fn from(value: SharedString) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for TimestampValue {
    fn from(value: &str) -> Self {
        Self::Text(SharedString::from(value.to_string()))
    }
}

impl From<String> for TimestampValue {
    fn from(value: String) -> Self {
        Self::Text(value.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TimestampFormat {
    Relative,
    #[default]
    Auto,
    Date,
    DateTime,
    Time,
    SystemDate,
    SystemDateTime,
    SystemTime,
}

#[derive(IntoElement)]
pub struct Timestamp {
    value: TimestampValue,
    format: TimestampFormat,
    auto_threshold_seconds: i64,
    show_timezone: bool,
    color: Option<Hsla>,
    size: Option<Pixels>,
    weight: Option<FontWeight>,
    style: StyleRefinement,
}

impl Timestamp {
    pub fn new(value: impl Into<TimestampValue>) -> Self {
        Self {
            value: value.into(),
            format: TimestampFormat::Auto,
            auto_threshold_seconds: DEFAULT_AUTO_THRESHOLD,
            show_timezone: false,
            color: None,
            size: None,
            weight: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn format(mut self, format: TimestampFormat) -> Self {
        self.format = format;
        self
    }

    pub fn auto_threshold_seconds(mut self, seconds: i64) -> Self {
        self.auto_threshold_seconds = seconds.max(0);
        self
    }

    pub fn show_timezone(mut self, show: bool) -> Self {
        self.show_timezone = show;
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = Some(size);
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.weight = Some(weight);
        self
    }
}

impl Styled for Timestamp {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Timestamp {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let now = unix_seconds(SystemTime::now());
        let display = format_timestamp(
            &self.value,
            self.format,
            self.auto_threshold_seconds,
            self.show_timezone,
            now,
        );

        div()
            .text_size(self.size.unwrap_or(px(12.0)))
            .line_height(px(16.0))
            .font_family(theme.tokens.font_family.clone())
            .font_weight(self.weight.unwrap_or(FontWeight::NORMAL))
            .text_color(self.color.unwrap_or(theme.tokens.muted_foreground))
            .child(display)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn format_timestamp(
    value: &TimestampValue,
    format: TimestampFormat,
    auto_threshold_seconds: i64,
    show_timezone: bool,
    now: i64,
) -> String {
    match value {
        TimestampValue::Text(value) => value.to_string(),
        _ => {
            let seconds = timestamp_seconds(value);
            let diff = now - seconds;
            let effective_format =
                if format == TimestampFormat::Auto && diff.abs() <= auto_threshold_seconds {
                    TimestampFormat::Relative
                } else if format == TimestampFormat::Auto {
                    TimestampFormat::DateTime
                } else {
                    format
                };

            match effective_format {
                TimestampFormat::Relative => relative_time(diff),
                TimestampFormat::Date
                | TimestampFormat::DateTime
                | TimestampFormat::Time
                | TimestampFormat::SystemDate
                | TimestampFormat::SystemDateTime
                | TimestampFormat::SystemTime => {
                    absolute_time(seconds, effective_format, show_timezone)
                }
                TimestampFormat::Auto => unreachable!(),
            }
        }
    }
}

fn timestamp_seconds(value: &TimestampValue) -> i64 {
    match value {
        TimestampValue::UnixSeconds(value) => *value,
        TimestampValue::UnixMillis(value) => *value / 1000,
        TimestampValue::SystemTime(value) => unix_seconds(*value),
        TimestampValue::Text(_) => 0,
    }
}

fn unix_seconds(value: SystemTime) -> i64 {
    match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(err) => -(err.duration().as_secs() as i64),
    }
}

fn relative_time(diff_seconds: i64) -> String {
    if diff_seconds < 0 {
        let abs = diff_seconds.abs();
        if abs < MINUTE {
            return "in a few seconds".to_string();
        }
        return format!("in {}", relative_unit(abs));
    }

    if diff_seconds < 10 {
        "just now".to_string()
    } else {
        format!("{} ago", relative_unit(diff_seconds))
    }
}

fn relative_unit(seconds: i64) -> String {
    if seconds < MINUTE {
        plural(seconds, "second")
    } else if seconds < HOUR {
        plural((seconds + MINUTE / 2) / MINUTE, "minute")
    } else if seconds < DAY {
        plural((seconds + HOUR / 2) / HOUR, "hour")
    } else if seconds < 2 * DAY {
        "yesterday".to_string()
    } else if seconds < MONTH {
        plural((seconds + DAY / 2) / DAY, "day")
    } else if seconds < YEAR {
        plural((seconds + MONTH / 2) / MONTH, "month")
    } else {
        plural((seconds + YEAR / 2) / YEAR, "year")
    }
}

fn plural(value: i64, unit: &str) -> String {
    if value == 1 {
        format!("1 {unit}")
    } else {
        format!("{value} {unit}s")
    }
}

fn absolute_time(seconds: i64, format: TimestampFormat, show_timezone: bool) -> String {
    let days = div_floor(seconds, DAY);
    let seconds_of_day = seconds - days * DAY;
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / HOUR;
    let minute = (seconds_of_day % HOUR) / MINUTE;
    let second = seconds_of_day % MINUTE;
    let tz = if show_timezone { " UTC" } else { "" };

    match format {
        TimestampFormat::Date => {
            format!("{month_name} {day}, {year}", month_name = month_name(month))
        }
        TimestampFormat::DateTime => {
            format!(
                "{} {}, {}, {}:{}{}",
                month_name(month),
                day,
                year,
                hour12(hour),
                pad2(minute),
                am_pm(hour)
            ) + tz
        }
        TimestampFormat::Time => format!("{}:{}{}{}", hour12(hour), pad2(minute), am_pm(hour), tz),
        TimestampFormat::SystemDate => {
            format!("{year}-{}-{}", pad2(month as i64), pad2(day as i64))
        }
        TimestampFormat::SystemDateTime => format!(
            "{year}-{}-{} {}:{}:{}{}",
            pad2(month as i64),
            pad2(day as i64),
            pad2(hour),
            pad2(minute),
            pad2(second),
            tz
        ),
        TimestampFormat::SystemTime => {
            format!("{}:{}:{}{}", pad2(hour), pad2(minute), pad2(second), tz)
        }
        TimestampFormat::Relative | TimestampFormat::Auto => unreachable!(),
    }
}

fn pad2(value: i64) -> String {
    format!("{value:02}")
}

fn hour12(hour: i64) -> i64 {
    let hour = hour % 24;
    let h = hour % 12;
    if h == 0 {
        12
    } else {
        h
    }
}

fn am_pm(hour: i64) -> &'static str {
    if hour < 12 {
        " AM"
    } else {
        " PM"
    }
}

fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    }
}

fn div_floor(a: i64, b: i64) -> i64 {
    let mut q = a / b;
    let r = a % b;
    if r != 0 && ((r > 0) != (b > 0)) {
        q -= 1;
    }
    q
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}
