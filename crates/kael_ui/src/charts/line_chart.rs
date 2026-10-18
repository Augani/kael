use crate::{
    charts::{
        expand_chart_range, finite_or_zero, format_axis_value, interpolate_chart_value,
        normalize_chart_value,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

const CHART_COLORS: [u32; 8] = crate::astryx::CHART_PALETTE;

fn get_chart_color(index: usize) -> Hsla {
    rgb(CHART_COLORS[index % CHART_COLORS.len()]).into()
}

#[derive(Clone, Debug)]
pub struct LineChartPoint {
    pub x: f64,
    pub y: f64,
    pub label: Option<SharedString>,
}

impl LineChartPoint {
    pub fn new(x: f64, y: f64) -> Self {
        Self {
            x: finite_or_zero(x),
            y: finite_or_zero(y),
            label: None,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
}

#[derive(Clone, Debug)]
pub struct LineChartSeries {
    pub name: SharedString,
    pub points: Vec<LineChartPoint>,
    pub color: Option<Hsla>,
    pub show_points: bool,
    pub fill_area: bool,
}

impl LineChartSeries {
    pub fn new(name: impl Into<SharedString>, points: Vec<LineChartPoint>) -> Self {
        Self {
            name: name.into(),
            points,
            color: None,
            show_points: false,
            fill_area: false,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn show_points(mut self, show: bool) -> Self {
        self.show_points = show;
        self
    }

    pub fn fill_area(mut self, fill: bool) -> Self {
        self.fill_area = fill;
        self
    }
}

struct DataRange {
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
}

impl DataRange {
    fn from_series(
        series: &[LineChartSeries],
        y_min_override: Option<f64>,
        y_max_override: Option<f64>,
    ) -> Self {
        let mut x_min = f64::MAX;
        let mut x_max = f64::MIN;
        let mut y_min = f64::MAX;
        let mut y_max = f64::MIN;
        let mut has_data = false;

        for s in series {
            for point in &s.points {
                has_data = true;
                x_min = x_min.min(point.x);
                x_max = x_max.max(point.x);
                y_min = y_min.min(point.y);
                y_max = y_max.max(point.y);
            }
        }

        if !has_data {
            x_min = 0.0;
            x_max = 1.0;
            y_min = 0.0;
            y_max = 1.0;
        }

        let (x_min, x_max) = expand_chart_range(x_min, x_max);
        let (y_min, y_max) = expand_chart_range(y_min, y_max);

        Self {
            x_min,
            x_max,
            y_min: y_min_override.unwrap_or(y_min),
            y_max: y_max_override.unwrap_or(y_max),
        }
    }

    fn normalize_x(&self, x: f64) -> f32 {
        normalize_chart_value(x, self.x_min, self.x_max)
    }

    fn normalize_y(&self, y: f64) -> f32 {
        normalize_chart_value(y, self.y_min, self.y_max)
    }

    fn y_value_at(&self, normalized: f64) -> f64 {
        interpolate_chart_value(self.y_min, self.y_max, 1.0 - normalized)
    }
}

fn line_chart_summary(series: &[LineChartSeries]) -> String {
    const MAX_POINTS: usize = 12;
    let point_count = series
        .iter()
        .map(|series| series.points.len())
        .sum::<usize>();
    if point_count == 0 {
        return "Line chart. No data".to_string();
    }

    let mut summary = format!(
        "Line chart with {} series and {point_count} points. ",
        series.len()
    );
    let mut shown = 0;
    for series in series {
        for point in &series.points {
            if shown == MAX_POINTS {
                break;
            }
            if shown > 0 {
                summary.push_str(", ");
            }
            summary.push_str(series.name.as_ref());
            summary.push(' ');
            if let Some(label) = &point.label {
                summary.push_str(label.as_ref());
            } else {
                summary.push_str(&format!("x {:.2}", point.x));
            }
            summary.push_str(&format!(": {:.2}", point.y));
            shown += 1;
        }
        if shown == MAX_POINTS {
            break;
        }
    }
    if point_count > shown {
        summary.push_str(&format!(", and {} more", point_count - shown));
    }
    summary
}

#[derive(IntoElement)]
pub struct LineChart {
    series: Vec<LineChartSeries>,
    show_grid: bool,
    show_x_axis: bool,
    show_y_axis: bool,
    x_axis_labels: Vec<SharedString>,
    y_min: Option<f64>,
    y_max: Option<f64>,
    smooth: bool,
    show_legend: bool,
    chart_height: Pixels,
    style: StyleRefinement,
}

impl LineChart {
    pub fn new(series: Vec<LineChartSeries>) -> Self {
        Self {
            series,
            show_grid: true,
            show_x_axis: true,
            show_y_axis: true,
            x_axis_labels: Vec::new(),
            y_min: None,
            y_max: None,
            smooth: false,
            show_legend: true,
            chart_height: px(240.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn single(series: LineChartSeries) -> Self {
        Self::new(vec![series])
    }

    pub fn show_grid(mut self, show: bool) -> Self {
        self.show_grid = show;
        self
    }

    pub fn show_x_axis(mut self, show: bool) -> Self {
        self.show_x_axis = show;
        self
    }

    pub fn show_y_axis(mut self, show: bool) -> Self {
        self.show_y_axis = show;
        self
    }

    pub fn smooth(mut self, smooth: bool) -> Self {
        self.smooth = smooth;
        self
    }

    pub fn y_range(mut self, min: f64, max: f64) -> Self {
        let min = finite_or_zero(min);
        let max = finite_or_zero(max);
        let (min, max) = (min.min(max), max.max(min));
        let (min, max) = if min == max {
            expand_chart_range(min, max)
        } else {
            // Retain the old builder's addition of positive zero (including -0).
            (min, max + 0.0)
        };
        self.y_min = Some(min);
        self.y_max = Some(max);
        self
    }

    pub fn x_labels(mut self, labels: Vec<impl Into<SharedString>>) -> Self {
        self.x_axis_labels = labels.into_iter().map(|l| l.into()).collect();
        self
    }

    pub fn show_legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    pub fn chart_height(mut self, height: impl Into<Pixels>) -> Self {
        let height = height.into();
        let value = f32::from(height);
        self.chart_height = if value.is_finite() && value > 0.0 {
            height
        } else {
            px(240.0)
        };
        self
    }
}

impl Styled for LineChart {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

struct PaintData {
    series: Vec<LineChartSeries>,
    show_grid: bool,
    smooth: bool,
    y_min: Option<f64>,
    y_max: Option<f64>,
    grid_color: Hsla,
    padding_left: f32,
    padding_right: f32,
    padding_top: f32,
    padding_bottom: f32,
}

impl RenderOnce for LineChart {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        let series = self.series.clone();
        let show_grid = self.show_grid;
        let show_x_axis = self.show_x_axis;
        let show_y_axis = self.show_y_axis;
        let smooth = self.smooth;
        let y_min = self.y_min;
        let y_max = self.y_max;
        let x_axis_labels = self.x_axis_labels.clone();
        let chart_height = self.chart_height;

        let grid_color = theme.tokens.border;
        let text_color = theme.tokens.muted_foreground;

        let padding_left: f32 = if show_y_axis { 50.0 } else { 10.0 };
        let padding_right: f32 = 20.0;
        let padding_top: f32 = 20.0;
        let padding_bottom: f32 = if show_x_axis { 40.0 } else { 10.0 };

        let series_for_legend = series.clone();
        let point_count = series
            .iter()
            .map(|series| series.points.len())
            .sum::<usize>();
        let description = line_chart_summary(&series);

        let data_range = DataRange::from_series(&series, y_min, y_max);

        let y_labels: Vec<String> = if show_y_axis {
            (0..=5)
                .map(|i| {
                    let normalized = i as f64 / 5.0;
                    let value = data_range.y_value_at(normalized);
                    format_axis_value(value)
                })
                .collect()
        } else {
            Vec::new()
        };

        let paint_data = PaintData {
            series,
            show_grid,
            smooth,
            y_min,
            y_max,
            grid_color,
            padding_left,
            padding_right,
            padding_top,
            padding_bottom,
        };

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label("Line chart")
                    .description(description),
            )
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.0))
            .min_h(chart_height)
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .h(chart_height)
                    .min_w(px(0.0))
                    .relative()
                    .child(
                        canvas_with_prepaint(
                            move |_bounds, _window, _cx| paint_data,
                            move |bounds, paint_data, window, _cx| {
                                if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
                                    return;
                                }

                                let data_range = DataRange::from_series(
                                    &paint_data.series,
                                    paint_data.y_min,
                                    paint_data.y_max,
                                );

                                let chart_left = bounds.left() + px(paint_data.padding_left);
                                let chart_right = bounds.right() - px(paint_data.padding_right);
                                let chart_top = bounds.top() + px(paint_data.padding_top);
                                let chart_bottom = bounds.bottom() - px(paint_data.padding_bottom);
                                let chart_width = chart_right - chart_left;
                                let chart_height = chart_bottom - chart_top;

                                if chart_width <= px(0.0) || chart_height <= px(0.0) {
                                    return;
                                }

                                if paint_data.show_grid {
                                    let grid_lines = 5;
                                    for i in 0..=grid_lines {
                                        let y = chart_top
                                            + chart_height * (i as f32 / grid_lines as f32);
                                        let mut builder = PathBuilder::stroke(px(1.0));
                                        builder.move_to(point(chart_left, y));
                                        builder.line_to(point(chart_right, y));
                                        if let Ok(path) = builder.build() {
                                            window.paint_path(
                                                path,
                                                paint_data.grid_color.opacity(0.3),
                                            );
                                        }
                                    }

                                    for i in 0..=grid_lines {
                                        let x = chart_left
                                            + chart_width * (i as f32 / grid_lines as f32);
                                        let mut builder = PathBuilder::stroke(px(1.0));
                                        builder.move_to(point(x, chart_top));
                                        builder.line_to(point(x, chart_bottom));
                                        if let Ok(path) = builder.build() {
                                            window.paint_path(
                                                path,
                                                paint_data.grid_color.opacity(0.3),
                                            );
                                        }
                                    }
                                }

                                for (series_index, s) in paint_data.series.iter().enumerate() {
                                    if s.points.is_empty() {
                                        continue;
                                    }

                                    let color =
                                        s.color.unwrap_or_else(|| get_chart_color(series_index));

                                    let screen_points: Vec<Point<Pixels>> = s
                                        .points
                                        .iter()
                                        .map(|p| {
                                            let norm_x = data_range.normalize_x(p.x);
                                            let norm_y = data_range.normalize_y(p.y);
                                            let screen_x = chart_left + chart_width * norm_x;
                                            let screen_y = chart_bottom - chart_height * norm_y;
                                            point(screen_x, screen_y)
                                        })
                                        .collect();

                                    if s.fill_area && screen_points.len() >= 2 {
                                        let mut builder = PathBuilder::fill();
                                        builder.move_to(point(screen_points[0].x, chart_bottom));
                                        builder.line_to(screen_points[0]);

                                        for pt in screen_points.iter().skip(1) {
                                            builder.line_to(*pt);
                                        }

                                        builder.line_to(point(
                                            screen_points.last().unwrap().x,
                                            chart_bottom,
                                        ));
                                        builder.close();

                                        if let Ok(path) = builder.build() {
                                            window.paint_path(path, color.opacity(0.15));
                                        }
                                    }

                                    if screen_points.len() >= 2 {
                                        let mut builder = PathBuilder::stroke(px(2.0));
                                        builder.move_to(screen_points[0]);

                                        if paint_data.smooth && screen_points.len() >= 3 {
                                            for i in 0..screen_points.len() - 1 {
                                                let p0 = screen_points[i];
                                                let p1 = screen_points[i + 1];
                                                let ctrl_x = (p0.x + p1.x) * 0.5;
                                                builder.curve_to(p1, point(ctrl_x, p0.y));
                                            }
                                        } else {
                                            for pt in screen_points.iter().skip(1) {
                                                builder.line_to(*pt);
                                            }
                                        }

                                        if let Ok(path) = builder.build() {
                                            window.paint_path(path, color);
                                        }
                                    }

                                    if s.show_points {
                                        let point_radius = px(4.0);
                                        for pt in &screen_points {
                                            window.paint_quad(fill(
                                                Bounds::centered_at(
                                                    *pt,
                                                    size(point_radius * 2.0, point_radius * 2.0),
                                                ),
                                                color,
                                            ));
                                        }
                                    }
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .when(point_count == 0, |this| {
                        this.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_sm()
                                .text_color(text_color)
                                .child("No data"),
                        )
                    })
                    .when(show_y_axis, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(px(4.0))
                                .top(px(padding_top - 6.0))
                                .bottom(px(padding_bottom - 6.0))
                                .flex()
                                .flex_col()
                                .justify_between()
                                .children(y_labels.iter().map(|label| {
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(text_color)
                                        .child(label.clone())
                                })),
                        )
                    })
                    .when(show_x_axis && !x_axis_labels.is_empty(), |this| {
                        let num_labels = x_axis_labels.len();
                        this.child(
                            div()
                                .absolute()
                                .left(px(padding_left))
                                .right(px(padding_right))
                                .bottom(px(8.0))
                                .flex()
                                .items_center()
                                .when(num_labels == 1, |this| this.justify_center())
                                .when(num_labels > 1, |this| this.justify_between())
                                .children(x_axis_labels.iter().map(|label| {
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(text_color)
                                        .child(label.clone())
                                })),
                        )
                    }),
            )
            .when(self.show_legend && series_for_legend.len() > 1, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(16.0))
                        .px(px(padding_left))
                        .py(px(8.0))
                        .children(series_for_legend.iter().enumerate().map(|(i, s)| {
                            let color = s.color.unwrap_or_else(|| get_chart_color(i));
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(div().size(px(12.0)).rounded(px(2.0)).bg(color))
                                .child(div().text_sm().text_color(text_color).child(s.name.clone()))
                        })),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn chart_height_is_definite_and_rejects_invalid_values() {
        let series = LineChartSeries::new(
            "Series",
            vec![LineChartPoint::new(0.0, 1.0), LineChartPoint::new(1.0, 2.0)],
        );
        assert_eq!(LineChart::single(series.clone()).chart_height, px(240.0));
        assert_eq!(
            LineChart::single(series.clone())
                .chart_height(px(320.0))
                .chart_height,
            px(320.0)
        );
        assert_eq!(
            LineChart::single(series)
                .chart_height(px(f32::NAN))
                .chart_height,
            px(240.0)
        );
    }

    #[::core::prelude::v1::test]
    fn accessible_summary_includes_labels_and_bounds_detail() {
        let points = (0..15)
            .map(|index| LineChartPoint::new(index as f64, index as f64).label(format!("P{index}")))
            .collect();
        let summary = line_chart_summary(&[LineChartSeries::new("Revenue", points)]);
        assert!(summary.contains("Revenue P0: 0.00"));
        assert!(summary.contains("and 3 more"));
        assert!(!summary.contains("Revenue P12"));
        assert_eq!(line_chart_summary(&[]), "Line chart. No data");
    }
}

#[cfg(test)]
mod numeric_range_regressions {
    use super::*;

    #[::core::prelude::v1::test]
    fn wide_finite_series_normalize_and_interpolate_ticks() {
        let series = [LineChartSeries::new(
            "Wide",
            vec![
                LineChartPoint::new(-f64::MAX, -f64::MAX),
                LineChartPoint::new(f64::MAX, f64::MAX),
            ],
        )];
        let range = DataRange::from_series(&series, None, None);
        for (value, fraction) in [(-f64::MAX, 0.0), (0.0, 0.5), (f64::MAX, 1.0)] {
            assert_eq!(range.normalize_x(value), fraction, "wide line x fraction");
            assert_eq!(range.normalize_y(value), fraction, "wide line y fraction");
        }
        assert_eq!(range.y_value_at(0.0), f64::MAX, "wide line top tick");
        assert_eq!(range.y_value_at(0.5), 0.0, "wide line middle tick");
        assert_eq!(range.y_value_at(1.0), -f64::MAX, "wide line bottom tick");
    }

    #[::core::prelude::v1::test]
    fn maximum_finite_samples_are_not_empty() {
        let series = [LineChartSeries::new(
            "MAX",
            vec![LineChartPoint::new(f64::MAX, f64::MAX)],
        )];
        let range = DataRange::from_series(&series, None, None);
        assert_eq!(
            (
                range.x_min.to_bits(),
                range.x_max.to_bits(),
                range.y_min.to_bits(),
                range.y_max.to_bits()
            ),
            (
                0x7fef_ffff_ffff_fffe,
                0x7fef_ffff_ffff_ffff,
                0x7fef_ffff_ffff_fffe,
                0x7fef_ffff_ffff_ffff
            ),
            "MAX line data is not an empty unit range"
        );
        assert_eq!(
            (range.normalize_x(f64::MAX), range.normalize_y(f64::MAX)),
            (1.0, 1.0)
        );
    }

    #[::core::prelude::v1::test]
    fn large_constant_series_and_y_override_use_finite_neighbors() {
        for (value, lower, upper) in [
            (1e20, 0x4415_af1d_78b5_8c40, 0x4415_af1d_78b5_8c41),
            (-1e20, 0xc415_af1d_78b5_8c40, 0xc415_af1d_78b5_8c3f),
            (f64::MAX, 0x7fef_ffff_ffff_fffe, 0x7fef_ffff_ffff_ffff),
        ] {
            let series = [LineChartSeries::new(
                "Constant",
                vec![LineChartPoint::new(value, value)],
            )];
            let range = DataRange::from_series(&series, None, None);
            assert_eq!(
                (range.x_min.to_bits(), range.x_max.to_bits()),
                (lower, upper),
                "line constant x neighbors"
            );
            let chart = LineChart::new(series.to_vec()).y_range(value, value);
            assert_eq!(
                (
                    chart.y_min.unwrap().to_bits(),
                    chart.y_max.unwrap().to_bits()
                ),
                (lower, upper),
                "line explicit y neighbors"
            );
            let overridden = DataRange::from_series(&chart.series, chart.y_min, chart.y_max);
            assert!((0.0..=1.0).contains(&overridden.normalize_y(value)));
        }
    }

    #[::core::prelude::v1::test]
    fn empty_and_ordinary_line_ranges_preserve_controls() {
        let empty = DataRange::from_series(&[], None, None);
        assert_eq!(
            (empty.x_min, empty.x_max, empty.y_min, empty.y_max),
            (0.0, 1.0, 0.0, 1.0)
        );
        let series = [LineChartSeries::new(
            "Constant",
            vec![LineChartPoint::new(8.0, 9.0)],
        )];
        let range = DataRange::from_series(&series, None, None);
        assert_eq!(
            (range.x_min, range.x_max, range.y_min, range.y_max),
            (8.0, 9.0, 9.0, 10.0)
        );
        let overridden = DataRange::from_series(&series, Some(3.0), Some(7.0));
        assert_eq!(overridden.y_value_at(0.5), 5.0);
        let tiny_override = LineChart::new(Vec::new()).y_range(0.0, f64::EPSILON / 2.0);
        assert_eq!(
            (tiny_override.y_min, tiny_override.y_max),
            (Some(0.0), Some(f64::EPSILON / 2.0))
        );
        assert_eq!(
            LineChart::new(Vec::new())
                .y_range(-1.0, -0.0)
                .y_max
                .unwrap()
                .to_bits(),
            0
        );
    }
}
