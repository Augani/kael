use crate::{
    charts::{data_summary, finite_or_zero},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

const CHART_COLORS: [u32; 8] = crate::astryx::CHART_PALETTE;

fn get_chart_color(index: usize) -> Hsla {
    rgb(CHART_COLORS[index % CHART_COLORS.len()]).into()
}

fn pixels_to_f32(p: Pixels) -> f32 {
    p / px(1.0)
}

fn multi_series_summary(labels: &[SharedString], series: &[BarChartSeries]) -> String {
    const MAX_VALUES: usize = 12;
    if labels.is_empty() || series.iter().all(|series| series.data.is_empty()) {
        return "Bar chart. No data".to_string();
    }

    let mut summary = format!(
        "Bar chart with {} series across {} categories. ",
        series.len(),
        labels.len()
    );
    let total = series
        .iter()
        .map(|series| series.data.len().min(labels.len()))
        .sum::<usize>();
    let mut shown = 0;
    for series in series {
        for (label, value) in labels.iter().zip(&series.data) {
            if shown == MAX_VALUES {
                break;
            }
            if shown > 0 {
                summary.push_str(", ");
            }
            summary.push_str(series.name.as_ref());
            summary.push(' ');
            summary.push_str(label.as_ref());
            summary.push_str(": ");
            summary.push_str(&format!("{value:.2}"));
            shown += 1;
        }
        if shown == MAX_VALUES {
            break;
        }
    }
    if total > shown {
        summary.push_str(&format!(", and {} more", total - shown));
    }
    summary
}

#[derive(Clone)]
pub struct BarChartData {
    pub label: SharedString,
    pub value: f64,
    pub color: Option<Hsla>,
}

impl BarChartData {
    pub fn new(label: impl Into<SharedString>, value: f64) -> Self {
        Self {
            label: label.into(),
            value: finite_or_zero(value).max(0.0),
            color: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(Clone)]
pub struct BarChartSeries {
    pub name: SharedString,
    pub data: Vec<f64>,
    pub color: Option<Hsla>,
}

impl BarChartSeries {
    pub fn new(name: impl Into<SharedString>, data: Vec<f64>) -> Self {
        Self {
            name: name.into(),
            data: data
                .into_iter()
                .map(|value| finite_or_zero(value).max(0.0))
                .collect(),
            color: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum BarChartOrientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum BarChartMode {
    #[default]
    Single,
    Grouped,
    Stacked,
}

#[derive(IntoElement)]
pub struct BarChart {
    data: Vec<BarChartData>,
    series: Vec<BarChartSeries>,
    labels: Vec<SharedString>,
    orientation: BarChartOrientation,
    mode: BarChartMode,
    show_values: bool,
    show_grid: bool,
    show_legend: bool,
    show_axis_labels: bool,
    bar_width: Option<Pixels>,
    gap: Pixels,
    height: Pixels,
    style: StyleRefinement,
}

impl BarChart {
    pub fn new(data: Vec<BarChartData>) -> Self {
        Self {
            data,
            series: Vec::new(),
            labels: Vec::new(),
            orientation: BarChartOrientation::default(),
            mode: BarChartMode::Single,
            show_values: false,
            show_grid: false,
            show_legend: false,
            show_axis_labels: true,
            bar_width: None,
            gap: px(8.0),
            height: px(300.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn multi_series(labels: Vec<impl Into<SharedString>>, series: Vec<BarChartSeries>) -> Self {
        Self {
            data: Vec::new(),
            series,
            labels: labels.into_iter().map(|l| l.into()).collect(),
            orientation: BarChartOrientation::default(),
            mode: BarChartMode::Grouped,
            show_values: false,
            show_grid: false,
            show_legend: true,
            show_axis_labels: true,
            bar_width: None,
            gap: px(8.0),
            height: px(300.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn vertical(mut self) -> Self {
        self.orientation = BarChartOrientation::Vertical;
        self
    }

    pub fn horizontal(mut self) -> Self {
        self.orientation = BarChartOrientation::Horizontal;
        self
    }

    pub fn orientation(mut self, orientation: BarChartOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn stacked(mut self) -> Self {
        self.mode = BarChartMode::Stacked;
        self
    }

    pub fn grouped(mut self) -> Self {
        self.mode = BarChartMode::Grouped;
        self
    }

    pub fn mode(mut self, mode: BarChartMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn show_values(mut self, show: bool) -> Self {
        self.show_values = show;
        self
    }

    pub fn show_grid(mut self, show: bool) -> Self {
        self.show_grid = show;
        self
    }

    pub fn show_legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    pub fn show_axis_labels(mut self, show: bool) -> Self {
        self.show_axis_labels = show;
        self
    }

    pub fn bar_width(mut self, width: Pixels) -> Self {
        let value = f32::from(width);
        self.bar_width = (value.is_finite() && value > 0.0).then_some(width);
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        let value = f32::from(gap);
        self.gap = if value.is_finite() && value >= 0.0 {
            gap
        } else {
            px(8.0)
        };
        self
    }

    pub fn chart_height(mut self, height: Pixels) -> Self {
        let value = f32::from(height);
        self.height = if value.is_finite() && value > 0.0 {
            height
        } else {
            px(300.0)
        };
        self
    }

    fn render_single_vertical(self, theme: &crate::theme::Theme) -> Div {
        let max_value = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, |a, b| a.max(b));

        let chart_height = self.height;
        let bar_width = self.bar_width.unwrap_or(px(40.0));
        let gap = self.gap;
        let show_values = self.show_values;
        let show_grid = self.show_grid;
        let show_axis_labels = self.show_axis_labels;
        let reserved_height =
            if show_values { 20.0 } else { 0.0 } + if show_axis_labels { 18.0 } else { 0.0 };
        let available_bar_height = chart_height - px(reserved_height);
        let item_count = self.data.len();
        let plot_width = px(32.0
            + pixels_to_f32(bar_width) * item_count as f32
            + pixels_to_f32(gap) * item_count.saturating_sub(1) as f32);

        let grid_lines = if show_grid {
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .children((0..5).map(|_| {
                        div()
                            .w_full()
                            .h(px(1.0))
                            .bg(theme.tokens.border.opacity(0.3))
                    })),
            )
        } else {
            None
        };

        div().flex().flex_col().gap(px(8.0)).child(
            div()
                .relative()
                .h(chart_height)
                .w_full()
                .when_some(grid_lines, |this, grid| this.child(grid))
                .child(
                    div()
                        .h_full()
                        .w(plot_width)
                        .flex()
                        .items_end()
                        .justify_between()
                        .gap(gap)
                        .px(px(16.0))
                        .children(self.data.iter().enumerate().map(|(i, item)| {
                            let height_percent = if max_value > 0.0 {
                                (item.value / max_value) as f32
                            } else {
                                0.0
                            };
                            let bar_color = item.color.unwrap_or_else(|| get_chart_color(i));
                            let value = item.value;
                            let bar_height = available_bar_height * height_percent;

                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_end()
                                .h_full()
                                .gap(px(4.0))
                                .when(show_values, |this| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.tokens.muted_foreground)
                                            .child(format!("{:.0}", value)),
                                    )
                                })
                                .child(
                                    div()
                                        .w(bar_width)
                                        .h(bar_height)
                                        .bg(bar_color)
                                        .rounded_t(theme.tokens.radius_sm),
                                )
                                .when(show_axis_labels, |this| {
                                    this.child(
                                        div()
                                            .w(bar_width)
                                            .text_xs()
                                            .text_color(theme.tokens.muted_foreground)
                                            .text_center()
                                            .child(item.label.clone()),
                                    )
                                })
                        })),
                ),
        )
    }

    fn render_single_horizontal(self, theme: &crate::theme::Theme) -> Div {
        let max_value = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, |a, b| a.max(b));

        let bar_height = self.bar_width.unwrap_or(px(24.0));
        let gap = self.gap;
        let show_values = self.show_values;
        let show_grid = self.show_grid;
        let show_axis_labels = self.show_axis_labels;
        let grid_color = theme.tokens.border.opacity(0.3);

        div()
            .flex()
            .flex_col()
            .items_stretch()
            .w_full()
            .gap(gap)
            .children(self.data.iter().enumerate().map(|(i, item)| {
                let width_percent = if max_value > 0.0 {
                    (item.value / max_value) as f32
                } else {
                    0.0
                };
                let bar_color = item.color.unwrap_or_else(|| get_chart_color(i));
                let value = item.value;
                let label = item.label.clone();

                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .min_w(px(0.0))
                    .gap(px(8.0))
                    .h(bar_height)
                    .when(show_axis_labels, |this| {
                        this.child(
                            div()
                                .w(px(80.0))
                                .flex_shrink_0()
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_end()
                                .text_xs()
                                .text_color(theme.tokens.muted_foreground)
                                .text_right()
                                .overflow_hidden()
                                .child(label),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .w_full()
                            .min_w(px(0.0))
                            .h(bar_height)
                            .relative()
                            .when(show_grid, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .justify_between()
                                        .children(
                                            (0..5)
                                                .map(|_| div().h_full().w(px(1.0)).bg(grid_color)),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(width_percent))
                                    .bg(bar_color)
                                    .rounded_r(theme.tokens.radius_sm),
                            ),
                    )
                    .when(show_values, |this| {
                        this.child(
                            div()
                                .w(px(50.0))
                                .flex_shrink_0()
                                .h_full()
                                .flex()
                                .items_center()
                                .text_xs()
                                .text_color(theme.tokens.muted_foreground)
                                .child(format!("{:.0}", value)),
                        )
                    })
            }))
    }

    fn render_multi_vertical_grouped(self, theme: &crate::theme::Theme) -> Div {
        let max_value = self
            .series
            .iter()
            .flat_map(|s| s.data.iter())
            .fold(0.0_f64, |a, &b| a.max(b));

        let chart_height = self.height;
        let series_count = self.series.len();
        let bar_width = self
            .bar_width
            .unwrap_or(px(24.0 / series_count.max(1) as f32 * series_count as f32));
        let single_bar_width = px(pixels_to_f32(bar_width) / series_count.max(1) as f32);
        let show_values = self.show_values;
        let show_grid = self.show_grid;
        let show_axis_labels = self.show_axis_labels;
        let show_legend = self.show_legend;

        let grid_lines = if show_grid {
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .children((0..5).map(|_| {
                        div()
                            .w_full()
                            .h(px(1.0))
                            .bg(theme.tokens.border.opacity(0.3))
                    })),
            )
        } else {
            None
        };

        let labels = self.labels.clone();
        let series_for_legend = self.series.clone();
        let label_count = labels.len();
        let label_tracks: Vec<_> = (0..label_count).map(|_| GridTrack::fr(1.0)).collect();

        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(
                div()
                    .relative()
                    .h(chart_height)
                    .w_full()
                    .when_some(grid_lines, |this, grid| this.child(grid))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_end()
                            .px(px(16.0))
                            .children(std::iter::once(div().flex_1().into_any_element()).chain(
                                (0..label_count).flat_map(|label_idx| {
                                    let group = div()
                                        .flex()
                                        .items_end()
                                        .justify_center()
                                        .h_full()
                                        .gap(px(2.0))
                                        .children(self.series.iter().enumerate().map(
                                            |(series_idx, series)| {
                                                let value = series
                                                    .data
                                                    .get(label_idx)
                                                    .copied()
                                                    .unwrap_or(0.0);
                                                let height_percent = if max_value > 0.0 {
                                                    (value / max_value) as f32
                                                } else {
                                                    0.0
                                                };
                                                let bar_color = series
                                                    .color
                                                    .unwrap_or_else(|| get_chart_color(series_idx));
                                                let bar_height = chart_height * height_percent;

                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .items_center()
                                                    .justify_end()
                                                    .h_full()
                                                    .gap(px(2.0))
                                                    .when(show_values, |this| {
                                                        this.child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(
                                                                    theme.tokens.muted_foreground,
                                                                )
                                                                .child(format!("{:.0}", value)),
                                                        )
                                                    })
                                                    .child(
                                                        div()
                                                            .w(single_bar_width)
                                                            .h(bar_height)
                                                            .bg(bar_color)
                                                            .rounded_t(theme.tokens.radius_sm),
                                                    )
                                            },
                                        ));
                                    [group.into_any_element(), div().flex_1().into_any_element()]
                                }),
                            )),
                    ),
            )
            .when(show_axis_labels, |this| {
                this.child(
                    div()
                        .grid()
                        .grid_template_columns(label_tracks)
                        .px(px(16.0))
                        .children(labels.iter().map(|label| {
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .text_xs()
                                .text_color(theme.tokens.muted_foreground)
                                .text_center()
                                .child(label.clone())
                        })),
                )
            })
            .when(show_legend, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(16.0))
                        .justify_center()
                        .children(series_for_legend.iter().enumerate().map(|(i, s)| {
                            let color = s.color.unwrap_or_else(|| get_chart_color(i));
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(div().size(px(12.0)).rounded(px(2.0)).bg(color))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(s.name.clone()),
                                )
                        })),
                )
            })
    }

    fn render_multi_vertical_stacked(self, theme: &crate::theme::Theme) -> Div {
        let label_count = self.labels.len();
        let stacked_totals: Vec<f64> = (0..label_count)
            .map(|i| {
                self.series
                    .iter()
                    .map(|s| s.data.get(i).copied().unwrap_or(0.0))
                    .sum()
            })
            .collect();

        let max_total = stacked_totals.iter().fold(0.0_f64, |a, &b| a.max(b));

        let chart_height = self.height;
        let bar_width = self.bar_width.unwrap_or(px(40.0));
        let show_values = self.show_values;
        let show_grid = self.show_grid;
        let show_axis_labels = self.show_axis_labels;
        let show_legend = self.show_legend;

        let grid_lines = if show_grid {
            Some(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .children((0..5).map(|_| {
                        div()
                            .w_full()
                            .h(px(1.0))
                            .bg(theme.tokens.border.opacity(0.3))
                    })),
            )
        } else {
            None
        };

        let labels = self.labels.clone();
        let series_for_legend = self.series.clone();
        let label_tracks: Vec<_> = (0..label_count).map(|_| GridTrack::fr(1.0)).collect();

        div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .child(
                div()
                    .relative()
                    .h(chart_height)
                    .w_full()
                    .when_some(grid_lines, |this, grid| this.child(grid))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_end()
                            .px(px(16.0))
                            .children(std::iter::once(div().flex_1().into_any_element()).chain(
                                (0..label_count).flat_map(|label_idx| {
                                    let total_height_percent = if max_total > 0.0 {
                                        (stacked_totals[label_idx] / max_total) as f32
                                    } else {
                                        0.0
                                    };
                                    let total_value = stacked_totals[label_idx];
                                    let bar_height = chart_height * total_height_percent;

                                    let group = div()
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .justify_end()
                                        .h_full()
                                        .gap(px(4.0))
                                        .when(show_values, |this| {
                                            this.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(theme.tokens.muted_foreground)
                                                    .child(format!("{:.0}", total_value)),
                                            )
                                        })
                                        .child(
                                            div()
                                                .w(bar_width)
                                                .h(bar_height)
                                                .flex()
                                                .flex_col_reverse()
                                                .overflow_hidden()
                                                .rounded_t(theme.tokens.radius_sm)
                                                .children(self.series.iter().enumerate().map(
                                                    |(series_idx, series)| {
                                                        let value = series
                                                            .data
                                                            .get(label_idx)
                                                            .copied()
                                                            .unwrap_or(0.0);
                                                        let segment_percent =
                                                            if stacked_totals[label_idx] > 0.0 {
                                                                (value / stacked_totals[label_idx])
                                                                    as f32
                                                            } else {
                                                                0.0
                                                            };
                                                        let bar_color =
                                                            series.color.unwrap_or_else(|| {
                                                                get_chart_color(series_idx)
                                                            });

                                                        div()
                                                            .w_full()
                                                            .h(relative(segment_percent))
                                                            .bg(bar_color)
                                                    },
                                                )),
                                        );
                                    [group.into_any_element(), div().flex_1().into_any_element()]
                                }),
                            )),
                    ),
            )
            .when(show_axis_labels, |this| {
                this.child(
                    div()
                        .grid()
                        .grid_template_columns(label_tracks)
                        .px(px(16.0))
                        .children(labels.iter().map(|label| {
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .text_xs()
                                .text_color(theme.tokens.muted_foreground)
                                .text_center()
                                .child(label.clone())
                        })),
                )
            })
            .when(show_legend, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(16.0))
                        .justify_center()
                        .children(series_for_legend.iter().enumerate().map(|(i, s)| {
                            let color = s.color.unwrap_or_else(|| get_chart_color(i));
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(div().size(px(12.0)).rounded(px(2.0)).bg(color))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(s.name.clone()),
                                )
                        })),
                )
            })
    }

    fn render_multi_horizontal_grouped(self, theme: &crate::theme::Theme) -> Div {
        let max_value = self
            .series
            .iter()
            .flat_map(|s| s.data.iter())
            .fold(0.0_f64, |a, &b| a.max(b));

        let series_count = self.series.len();
        let bar_height = self.bar_width.unwrap_or(px(16.0));
        let single_bar_height = px(pixels_to_f32(bar_height) / series_count.max(1) as f32);
        let gap = self.gap;
        let show_values = self.show_values;
        let show_grid = self.show_grid;
        let show_axis_labels = self.show_axis_labels;
        let show_legend = self.show_legend;
        let grid_color = theme.tokens.border.opacity(0.3);

        let labels = self.labels.clone();
        let series_for_legend = self.series.clone();

        div()
            .flex()
            .flex_col()
            .items_stretch()
            .w_full()
            .gap(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_stretch()
                    .w_full()
                    .gap(gap)
                    .children(labels.iter().enumerate().map(|(label_idx, label)| {
                        div()
                            .flex()
                            .w_full()
                            .items_center()
                            .gap(px(8.0))
                            .when(show_axis_labels, |this| {
                                this.child(
                                    div()
                                        .w(px(80.0))
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .text_right()
                                        .overflow_hidden()
                                        .child(label.clone()),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .w_full()
                                    .min_w(px(0.0))
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.0))
                                    .relative()
                                    .when(show_grid, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .flex()
                                                .justify_between()
                                                .children((0..5).map(|_| {
                                                    div().h_full().w(px(1.0)).bg(grid_color)
                                                })),
                                        )
                                    })
                                    .children(self.series.iter().enumerate().map(
                                        |(series_idx, series)| {
                                            let value =
                                                series.data.get(label_idx).copied().unwrap_or(0.0);
                                            let width_percent = if max_value > 0.0 {
                                                (value / max_value) as f32
                                            } else {
                                                0.0
                                            };
                                            let bar_color = series
                                                .color
                                                .unwrap_or_else(|| get_chart_color(series_idx));

                                            div()
                                                .flex()
                                                .items_center()
                                                .gap(px(4.0))
                                                .child(
                                                    div()
                                                        .h(single_bar_height)
                                                        .w(relative(width_percent))
                                                        .bg(bar_color)
                                                        .rounded_r(theme.tokens.radius_sm),
                                                )
                                                .when(show_values, |this| {
                                                    this.child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(
                                                                theme.tokens.muted_foreground,
                                                            )
                                                            .child(format!("{:.0}", value)),
                                                    )
                                                })
                                        },
                                    )),
                            )
                    })),
            )
            .when(show_legend, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(16.0))
                        .justify_center()
                        .children(series_for_legend.iter().enumerate().map(|(i, s)| {
                            let color = s.color.unwrap_or_else(|| get_chart_color(i));
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(div().size(px(12.0)).rounded(px(2.0)).bg(color))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(s.name.clone()),
                                )
                        })),
                )
            })
    }

    fn render_multi_horizontal_stacked(self, theme: &crate::theme::Theme) -> Div {
        let label_count = self.labels.len();
        let stacked_totals: Vec<f64> = (0..label_count)
            .map(|i| {
                self.series
                    .iter()
                    .map(|s| s.data.get(i).copied().unwrap_or(0.0))
                    .sum()
            })
            .collect();

        let max_total = stacked_totals.iter().fold(0.0_f64, |a, &b| a.max(b));

        let bar_height = self.bar_width.unwrap_or(px(24.0));
        let gap = self.gap;
        let show_values = self.show_values;
        let show_grid = self.show_grid;
        let show_axis_labels = self.show_axis_labels;
        let show_legend = self.show_legend;
        let grid_color = theme.tokens.border.opacity(0.3);

        let labels = self.labels.clone();
        let series_for_legend = self.series.clone();

        div()
            .flex()
            .flex_col()
            .items_stretch()
            .w_full()
            .gap(px(16.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_stretch()
                    .w_full()
                    .gap(gap)
                    .children(labels.iter().enumerate().map(|(label_idx, label)| {
                        let total_width_percent = if max_total > 0.0 {
                            (stacked_totals[label_idx] / max_total) as f32
                        } else {
                            0.0
                        };
                        let total_value = stacked_totals[label_idx];

                        div()
                            .flex()
                            .w_full()
                            .items_center()
                            .gap(px(8.0))
                            .when(show_axis_labels, |this| {
                                this.child(
                                    div()
                                        .w(px(80.0))
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .text_right()
                                        .overflow_hidden()
                                        .child(label.clone()),
                                )
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .w_full()
                                    .min_w(px(0.0))
                                    .relative()
                                    .h(bar_height)
                                    .when(show_grid, |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .flex()
                                                .justify_between()
                                                .children((0..5).map(|_| {
                                                    div().h_full().w(px(1.0)).bg(grid_color)
                                                })),
                                        )
                                    })
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(total_width_percent))
                                            .flex()
                                            .overflow_hidden()
                                            .rounded_r(theme.tokens.radius_sm)
                                            .children(self.series.iter().enumerate().map(
                                                |(series_idx, series)| {
                                                    let value = series
                                                        .data
                                                        .get(label_idx)
                                                        .copied()
                                                        .unwrap_or(0.0);
                                                    let segment_percent =
                                                        if stacked_totals[label_idx] > 0.0 {
                                                            (value / stacked_totals[label_idx])
                                                                as f32
                                                        } else {
                                                            0.0
                                                        };
                                                    let bar_color =
                                                        series.color.unwrap_or_else(|| {
                                                            get_chart_color(series_idx)
                                                        });

                                                    div()
                                                        .h_full()
                                                        .w(relative(segment_percent))
                                                        .bg(bar_color)
                                                },
                                            )),
                                    ),
                            )
                            .when(show_values, |this| {
                                this.child(
                                    div()
                                        .w(px(50.0))
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(format!("{:.0}", total_value)),
                                )
                            })
                    })),
            )
            .when(show_legend, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(16.0))
                        .justify_center()
                        .children(series_for_legend.iter().enumerate().map(|(i, s)| {
                            let color = s.color.unwrap_or_else(|| get_chart_color(i));
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .child(div().size(px(12.0)).rounded(px(2.0)).bg(color))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(s.name.clone()),
                                )
                        })),
                )
            })
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn data_and_series_sanitize_invalid_values() {
        assert_eq!(BarChartData::new("bad", f64::NAN).value, 0.0);
        assert_eq!(BarChartData::new("negative", -2.0).value, 0.0);
        assert_eq!(
            BarChartSeries::new("series", vec![1.0, f64::INFINITY, -1.0]).data,
            vec![1.0, 0.0, 0.0]
        );
    }

    #[::core::prelude::v1::test]
    fn invalid_geometry_uses_safe_defaults() {
        let chart = BarChart::new(Vec::new())
            .bar_width(px(f32::NAN))
            .gap(px(-1.0))
            .chart_height(px(0.0));
        assert!(chart.bar_width.is_none());
        assert_eq!(f32::from(chart.gap), 8.0);
        assert_eq!(f32::from(chart.height), 300.0);
    }

    #[::core::prelude::v1::test]
    fn multi_series_summary_includes_values_and_is_bounded() {
        let labels: Vec<SharedString> = (0..20).map(|index| format!("Q{index}").into()).collect();
        let summary =
            multi_series_summary(&labels, &[BarChartSeries::new("Revenue", vec![42.0; 20])]);
        assert!(summary.contains("Revenue Q0: 42.00"));
        assert!(summary.contains("and 8 more"));
        assert!(!summary.contains("Revenue Q12"));
    }
}

impl Styled for BarChart {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for BarChart {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style.clone();

        let is_single = self.series.is_empty();
        let is_vertical = self.orientation == BarChartOrientation::Vertical;
        let is_stacked = self.mode == BarChartMode::Stacked;
        let is_empty = if is_single {
            self.data.is_empty()
        } else {
            self.labels.is_empty() || self.series.iter().all(|series| series.data.is_empty())
        };
        let description = if is_single {
            data_summary(
                "Bar chart",
                self.data
                    .iter()
                    .map(|item| (item.label.as_ref(), item.value)),
            )
        } else {
            multi_series_summary(&self.labels, &self.series)
        };

        let content = match (is_single, is_vertical, is_stacked) {
            (true, true, _) => self.render_single_vertical(theme),
            (true, false, _) => self.render_single_horizontal(theme),
            (false, true, false) => self.render_multi_vertical_grouped(theme),
            (false, true, true) => self.render_multi_vertical_stacked(theme),
            (false, false, false) => self.render_multi_horizontal_grouped(theme),
            (false, false, true) => self.render_multi_horizontal_stacked(theme),
        };

        content
            .w_full()
            .min_w(px(0.0))
            .relative()
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .when(is_empty, |this| {
                this.min_h(px(160.0)).child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.tokens.background)
                        .text_sm()
                        .text_color(theme.tokens.muted_foreground)
                        .child("No data"),
                )
            })
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label("Bar chart")
                    .description(description),
            )
    }
}
