//! GitHub-style contribution calendar for daily activity data.

use crate::{
    components::{calendar::DateValue, scrollable::scrollable_horizontal},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::collections::BTreeMap;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContributionCalendarSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl ContributionCalendarSize {
    fn metrics(self) -> (Pixels, Pixels) {
        match self {
            Self::Sm => (px(9.0), px(2.0)),
            Self::Md => (px(12.0), px(3.0)),
            Self::Lg => (px(16.0), px(4.0)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContributionCellShape {
    Square,
    #[default]
    Rounded,
    Circle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ContributionPalette {
    #[default]
    GitHub,
    Astryx,
    Blue,
    Purple,
    Custom([Hsla; 5]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContributionDay {
    pub date: DateValue,
    pub count: u32,
}

impl ContributionDay {
    pub fn new(date: DateValue, count: u32) -> Self {
        Self { date, count }
    }
}

#[derive(IntoElement)]
pub struct ContributionCalendar {
    contributions: Vec<ContributionDay>,
    start_date: Option<DateValue>,
    end_date: Option<DateValue>,
    size: ContributionCalendarSize,
    shape: ContributionCellShape,
    palette: ContributionPalette,
    thresholds: Option<[u32; 4]>,
    show_month_labels: bool,
    show_weekday_labels: bool,
    show_legend: bool,
    accessible_label: SharedString,
    style: StyleRefinement,
}

impl ContributionCalendar {
    pub fn new() -> Self {
        Self {
            contributions: Vec::new(),
            start_date: None,
            end_date: None,
            size: ContributionCalendarSize::default(),
            shape: ContributionCellShape::default(),
            palette: ContributionPalette::default(),
            thresholds: None,
            show_month_labels: true,
            show_weekday_labels: true,
            show_legend: true,
            accessible_label: "Contribution activity".into(),
            style: StyleRefinement::default(),
        }
    }

    pub fn contributions(mut self, contributions: Vec<ContributionDay>) -> Self {
        self.contributions = contributions;
        self
    }

    pub fn contribution(mut self, contribution: ContributionDay) -> Self {
        self.contributions.push(contribution);
        self
    }

    pub fn date_range(mut self, start: DateValue, end: DateValue) -> Self {
        self.start_date = Some(start.min(end));
        self.end_date = Some(start.max(end));
        self
    }

    pub fn size(mut self, size: ContributionCalendarSize) -> Self {
        self.size = size;
        self
    }

    pub fn shape(mut self, shape: ContributionCellShape) -> Self {
        self.shape = shape;
        self
    }

    pub fn palette(mut self, palette: ContributionPalette) -> Self {
        self.palette = palette;
        self
    }

    /// Sets inclusive thresholds for levels 1 through 4.
    pub fn thresholds(mut self, thresholds: [u32; 4]) -> Self {
        let mut normalized = thresholds;
        normalized.sort_unstable();
        self.thresholds = Some(normalized);
        self
    }

    pub fn show_month_labels(mut self, show: bool) -> Self {
        self.show_month_labels = show;
        self
    }

    pub fn show_weekday_labels(mut self, show: bool) -> Self {
        self.show_weekday_labels = show;
        self
    }

    pub fn show_legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    pub fn accessible_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessible_label = label.into();
        self
    }
}

impl Default for ContributionCalendar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for ContributionCalendar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ContributionCalendar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let values: BTreeMap<_, _> = self
            .contributions
            .into_iter()
            .map(|day| (day.date, day.count))
            .collect();

        let data_start = values.keys().next().copied();
        let data_end = values.keys().next_back().copied();
        let range_start = self.start_date.or(data_start);
        let range_end = self.end_date.or(data_end);
        let (cell_size, gap) = self.size.metrics();
        let colors = resolve_palette(self.palette, theme);
        let total: u64 = values.values().map(|&count| u64::from(count)).sum();
        let active_days = values.values().filter(|&&count| count > 0).count();
        let maximum = values.values().copied().max().unwrap_or(0);

        let description = match (range_start, range_end) {
            (Some(start), Some(end)) => format!(
                "Contribution activity from {} to {}. {total} contributions across {active_days} active days; busiest day has {maximum} contributions.",
                start.to_iso(),
                end.to_iso()
            ),
            _ => "Contribution activity. No data".to_string(),
        };

        let calendar = match (range_start, range_end) {
            (Some(start), Some(end)) => render_calendar_grid(CalendarGridData {
                start,
                end,
                values,
                maximum,
                thresholds: self.thresholds,
                colors,
                cell_size,
                gap,
                shape: self.shape,
                show_month_labels: self.show_month_labels,
                show_weekday_labels: self.show_weekday_labels,
                label_color: theme.tokens.muted_foreground,
            }),
            _ => div()
                .min_h(px(112.0))
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.tokens.muted_foreground)
                .child("No contribution data")
                .into_any_element(),
        };

        let legend = (self.show_legend && range_start.is_some())
            .then(|| render_legend(colors, cell_size, theme.tokens.muted_foreground));

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label(self.accessible_label.to_string())
                    .description(description),
            )
            .max_w_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(calendar)
            .when_some(legend, |this, legend| this.child(legend))
            .map(|this| {
                let mut element = this;
                element.style().refine(&user_style);
                element
            })
            .into_any_element()
    }
}

struct CalendarGridData {
    start: DateValue,
    end: DateValue,
    values: BTreeMap<DateValue, u32>,
    maximum: u32,
    thresholds: Option<[u32; 4]>,
    colors: [Hsla; 5],
    cell_size: Pixels,
    gap: Pixels,
    shape: ContributionCellShape,
    show_month_labels: bool,
    show_weekday_labels: bool,
    label_color: Hsla,
}

fn render_calendar_grid(data: CalendarGridData) -> AnyElement {
    let calendar_start = data
        .start
        .add_days(-i64::from(data.start.day_of_week().index()));
    let calendar_end = data
        .end
        .add_days(i64::from(6 - data.end.day_of_week().index()));
    let weeks = build_weeks(calendar_start, calendar_end);
    let radius = match data.shape {
        ContributionCellShape::Square => px(0.0),
        ContributionCellShape::Rounded => (data.cell_size * 0.22).max(px(2.0)),
        ContributionCellShape::Circle => px(9999.0),
    };

    let weekday_labels = if data.show_weekday_labels {
        let mut labels = Vec::with_capacity(7);
        for day in 0..7 {
            let label = match day {
                1 => "Mon",
                3 => "Wed",
                5 => "Fri",
                _ => "",
            };
            let label_element = div()
                .h(data.cell_size + data.gap)
                .flex()
                .items_center()
                .justify_end()
                .text_size(px(10.0))
                .text_color(data.label_color)
                .when(!label.is_empty(), |this| this.child(label));
            labels.push(label_element.into_any_element());
        }
        Some(
            div()
                .flex()
                .flex_col()
                .mr(px(6.0))
                .when(data.show_month_labels, |this| this.child(div().h(px(18.0))))
                .children(labels)
                .into_any_element(),
        )
    } else {
        None
    };

    let mut week_elements = Vec::with_capacity(weeks.len());
    for (week_index, week) in weeks.into_iter().enumerate() {
        let month_label = if data.show_month_labels {
            week.iter()
                .find(|date| date.day == 1)
                .or_else(|| (week_index == 0).then_some(&data.start))
                .map(|date| MONTHS[(date.month - 1) as usize])
                .unwrap_or("")
        } else {
            ""
        };

        let mut cells = Vec::with_capacity(7);
        for date in week {
            let in_range = date >= data.start && date <= data.end;
            let count = data.values.get(&date).copied().unwrap_or(0);
            let level = contribution_level(count, data.maximum, data.thresholds);
            let color = if in_range {
                data.colors[level]
            } else {
                data.colors[0].opacity(0.25)
            };
            cells.push(
                div()
                    .size(data.cell_size)
                    .rounded(radius)
                    .bg(color)
                    .into_any_element(),
            );
        }

        week_elements.push(
            div()
                .flex()
                .flex_col()
                .gap(data.gap)
                .when(data.show_month_labels, |this| {
                    this.child(
                        div()
                            .h(px(15.0))
                            .text_size(px(10.0))
                            .text_color(data.label_color)
                            .when(!month_label.is_empty(), |this| this.child(month_label)),
                    )
                })
                .children(cells)
                .into_any_element(),
        );
    }

    scrollable_horizontal(
        div().pb(px(4.0)).child(
            div()
                .flex()
                .items_start()
                .when_some(weekday_labels, |this, labels| this.child(labels))
                .child(div().flex().gap(data.gap).children(week_elements)),
        ),
    )
    .max_w_full()
    .into_any_element()
}

fn render_legend(colors: [Hsla; 5], cell_size: Pixels, label_color: Hsla) -> AnyElement {
    let swatches = colors.map(|color| {
        div()
            .size(cell_size.min(px(12.0)))
            .rounded(px(2.0))
            .bg(color)
            .into_any_element()
    });
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(5.0))
        .text_size(px(10.0))
        .text_color(label_color)
        .child("Less")
        .children(swatches)
        .child("More")
        .into_any_element()
}

fn build_weeks(start: DateValue, end: DateValue) -> Vec<Vec<DateValue>> {
    let mut weeks = Vec::new();
    let mut cursor = start;
    for _ in 0..=600 {
        if cursor > end {
            break;
        }
        let mut week = Vec::with_capacity(7);
        for _ in 0..7 {
            week.push(cursor);
            cursor = cursor.add_days(1);
        }
        weeks.push(week);
    }
    weeks
}

fn contribution_level(count: u32, maximum: u32, thresholds: Option<[u32; 4]>) -> usize {
    if count == 0 {
        return 0;
    }
    if let Some(thresholds) = thresholds {
        return thresholds
            .iter()
            .position(|&threshold| count <= threshold)
            .map_or(4, |index| index + 1);
    }
    if maximum == 0 {
        return 0;
    }
    (((count as u64 * 4).div_ceil(u64::from(maximum))) as usize).clamp(1, 4)
}

fn resolve_palette(palette: ContributionPalette, theme: &Theme) -> [Hsla; 5] {
    match palette {
        ContributionPalette::GitHub => [
            theme.tokens.muted,
            rgb(0x9be9a8).into(),
            rgb(0x40c463).into(),
            rgb(0x30a14e).into(),
            rgb(0x216e39).into(),
        ],
        ContributionPalette::Astryx => scale_from(theme.tokens.muted, theme.tokens.primary),
        ContributionPalette::Blue => scale_from(theme.tokens.muted, rgb(0x0969da).into()),
        ContributionPalette::Purple => scale_from(theme.tokens.muted, rgb(0x8250df).into()),
        ContributionPalette::Custom(colors) => colors,
    }
}

fn scale_from(low: Hsla, high: Hsla) -> [Hsla; 5] {
    [
        low,
        mix(low, high, 0.25),
        mix(low, high, 0.5),
        mix(low, high, 0.75),
        high,
    ]
}

fn mix(from: Hsla, to: Hsla, amount: f32) -> Hsla {
    hsla(
        to.h,
        from.s + (to.s - from.s) * amount,
        from.l + (to.l - from.l) * amount,
        from.a + (to.a - from.a) * amount,
    )
}

#[cfg(test)]
mod tests {
    use super::{build_weeks, contribution_level};
    use crate::components::calendar::DateValue;

    #[test]
    fn contribution_levels_are_bounded_and_preserve_zero() {
        assert_eq!(contribution_level(0, 12, None), 0);
        assert_eq!(contribution_level(1, 12, None), 1);
        assert_eq!(contribution_level(12, 12, None), 4);
        assert_eq!(contribution_level(u32::MAX, u32::MAX, None), 4);
    }

    #[test]
    fn explicit_thresholds_define_levels() {
        let thresholds = Some([1, 3, 6, 10]);
        assert_eq!(contribution_level(1, 10, thresholds), 1);
        assert_eq!(contribution_level(4, 10, thresholds), 3);
        assert_eq!(contribution_level(11, 11, thresholds), 4);
    }

    #[test]
    fn calendar_weeks_are_complete() {
        let weeks = build_weeks(DateValue::new(2026, 7, 5), DateValue::new(2026, 7, 18));
        assert_eq!(weeks.len(), 2);
        assert!(weeks.iter().all(|week| week.len() == 7));
    }
}
