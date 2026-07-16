use crate::charts::{
    data_summary, finite_or_zero, paint_radial_segments, pie_chart::PieChartSegment,
};
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

const CHART_COLORS: [u32; 8] = crate::astryx::CHART_PALETTE;

fn default_color(index: usize) -> Hsla {
    rgb(CHART_COLORS[index % CHART_COLORS.len()]).into()
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum DonutChartSize {
    Sm,
    #[default]
    Md,
    Lg,
    Custom(u32),
}

impl DonutChartSize {
    fn to_pixels(self) -> Pixels {
        match self {
            DonutChartSize::Sm => px(120.0),
            DonutChartSize::Md => px(200.0),
            DonutChartSize::Lg => px(280.0),
            DonutChartSize::Custom(size) => px(size as f32),
        }
    }
}

#[derive(IntoElement)]
pub struct DonutChart {
    segments: Vec<PieChartSegment>,
    inner_radius: f32,
    center_label: Option<SharedString>,
    center_value: Option<SharedString>,
    size: DonutChartSize,
    show_legend: bool,
    show_percentages: bool,
    start_angle_degrees: f32,
    segment_gap_degrees: f32,
    style: StyleRefinement,
}

impl Default for DonutChart {
    fn default() -> Self {
        Self::new()
    }
}

impl DonutChart {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            inner_radius: 0.6,
            center_label: None,
            center_value: None,
            size: DonutChartSize::default(),
            show_legend: false,
            show_percentages: false,
            start_angle_degrees: -90.0,
            segment_gap_degrees: 0.0,
            style: StyleRefinement::default(),
        }
    }

    pub fn segments(mut self, segments: Vec<PieChartSegment>) -> Self {
        self.segments = segments;
        self
    }

    pub fn segment(mut self, segment: PieChartSegment) -> Self {
        self.segments.push(segment);
        self
    }

    pub fn inner_radius(mut self, ratio: f32) -> Self {
        self.inner_radius = if ratio.is_finite() {
            ratio.clamp(0.0, 0.9)
        } else {
            0.6
        };
        self
    }

    pub fn center_label(mut self, label: impl Into<SharedString>) -> Self {
        self.center_label = Some(label.into());
        self
    }

    pub fn center_value(mut self, value: impl Into<SharedString>) -> Self {
        self.center_value = Some(value.into());
        self
    }

    pub fn size(mut self, size: DonutChartSize) -> Self {
        self.size = size;
        self
    }

    pub fn size_px(mut self, size_val: u32) -> Self {
        self.size = DonutChartSize::Custom(size_val);
        self
    }

    pub fn show_legend(mut self, show: bool) -> Self {
        self.show_legend = show;
        self
    }

    pub fn show_percentages(mut self, show: bool) -> Self {
        self.show_percentages = show;
        self
    }

    /// Rotates the first segment; `-90` starts at twelve o'clock.
    pub fn start_angle_degrees(mut self, angle: f32) -> Self {
        self.start_angle_degrees = if angle.is_finite() { angle } else { -90.0 };
        self
    }

    /// Adds angular spacing between segments, clamped to a restrained 0–12 degrees.
    pub fn segment_gap_degrees(mut self, gap: f32) -> Self {
        self.segment_gap_degrees = if gap.is_finite() {
            gap.clamp(0.0, 12.0)
        } else {
            0.0
        };
        self
    }
}

impl Styled for DonutChart {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DonutChart {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let chart_size = self.size.to_pixels();
        let show_legend = self.show_legend;
        let show_percentages = self.show_percentages;
        let description = data_summary(
            "Donut chart",
            self.segments
                .iter()
                .map(|segment| (segment.label.as_ref(), segment.value)),
        );

        let total: f64 = self
            .segments
            .iter()
            .map(|segment| finite_or_zero(segment.value).max(0.0))
            .sum();

        let chart = if total == 0.0 || self.segments.is_empty() {
            render_empty(chart_size, theme)
        } else {
            render_donut(
                chart_size,
                &self.segments,
                total,
                self.inner_radius,
                self.center_label.clone(),
                self.center_value.clone(),
                self.start_angle_degrees,
                self.segment_gap_degrees,
                theme,
            )
        };

        let legend = if show_legend {
            Some(render_legend(
                &self.segments,
                total,
                show_percentages,
                theme,
            ))
        } else {
            None
        };

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label("Donut chart")
                    .description(description),
            )
            .flex()
            .flex_wrap()
            .gap(px(24.0))
            .items_center()
            .child(chart)
            .when_some(legend, |this, l| this.child(l))
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
    }
}

fn render_empty(chart_size: Pixels, theme: &crate::theme::Theme) -> Div {
    div()
        .size(chart_size)
        .rounded(px(9999.0))
        .bg(theme.tokens.muted)
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_sm()
                .text_color(theme.tokens.muted_foreground)
                .child("No data"),
        )
}

fn render_donut(
    chart_size: Pixels,
    segments: &[PieChartSegment],
    total: f64,
    inner_ratio: f32,
    center_label: Option<SharedString>,
    center_value: Option<SharedString>,
    start_angle_degrees: f32,
    segment_gap_degrees: f32,
    theme: &crate::theme::Theme,
) -> Div {
    let size_f32 = chart_size / px(1.0);
    let center = size_f32 * 0.5;
    let outer_radius = size_f32 * 0.5;
    let inner_radius = outer_radius * inner_ratio;

    let mut segment_data: Vec<(f32, f32, Hsla)> = Vec::new();
    let mut current_angle = start_angle_degrees.to_radians();
    let gap = segment_gap_degrees.to_radians();

    for (idx, segment) in segments.iter().enumerate() {
        if !segment.value.is_finite() || segment.value <= 0.0 {
            continue;
        }
        let fraction = (segment.value / total) as f32;
        let sweep = fraction * std::f32::consts::TAU;
        let color = segment.color.unwrap_or_else(|| default_color(idx));
        let visible_gap = gap.min(sweep * 0.5);
        segment_data.push((
            current_angle + visible_gap * 0.5,
            (sweep - visible_gap).max(0.0),
            color,
        ));
        current_angle += sweep;
    }

    if segment_data.len() == 1 {
        return render_single_segment(
            chart_size,
            segment_data[0].2,
            inner_radius,
            center_label,
            center_value,
            theme,
        );
    }

    let mut container = div()
        .size(chart_size)
        .rounded(px(9999.0))
        .relative()
        .overflow_hidden()
        .child(
            canvas_with_prepaint(
                move |_bounds, _window, _cx| segment_data,
                move |bounds, segments, window, _cx| {
                    paint_radial_segments(bounds, &segments, window);
                },
            )
            .absolute()
            .inset_0(),
        );

    let inner_size = inner_radius * 2.0 - 4.0;
    let inner_offset = center - inner_radius + 2.0;

    container = container.child(
        div()
            .absolute()
            .size(px(inner_size))
            .rounded(px(9999.0))
            .bg(theme.tokens.background)
            .left(px(inner_offset))
            .top(px(inner_offset))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.0))
            .when_some(center_value, |this, val| {
                this.child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.tokens.foreground)
                        .child(val),
                )
            })
            .when_some(center_label, |this, lbl| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(theme.tokens.muted_foreground)
                        .child(lbl),
                )
            }),
    );

    container
}

fn render_single_segment(
    chart_size: Pixels,
    color: Hsla,
    inner_radius: f32,
    center_label: Option<SharedString>,
    center_value: Option<SharedString>,
    theme: &crate::theme::Theme,
) -> Div {
    let size_f32 = chart_size / px(1.0);
    let center = size_f32 * 0.5;
    let inner_size = inner_radius * 2.0;
    let inner_offset = center - inner_radius;

    div()
        .size(chart_size)
        .rounded(px(9999.0))
        .relative()
        .bg(color)
        .child(
            div()
                .absolute()
                .size(px(inner_size))
                .rounded(px(9999.0))
                .bg(theme.tokens.background)
                .left(px(inner_offset))
                .top(px(inner_offset))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .when_some(center_value, |this, val| {
                    this.child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.tokens.foreground)
                            .child(val),
                    )
                })
                .when_some(center_label, |this, lbl| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(theme.tokens.muted_foreground)
                            .child(lbl),
                    )
                }),
        )
}

fn render_legend(
    segments: &[PieChartSegment],
    total: f64,
    show_percentages: bool,
    theme: &crate::theme::Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .children(segments.iter().enumerate().filter_map(|(idx, segment)| {
            if !segment.value.is_finite() || segment.value <= 0.0 {
                return None;
            }

            let color = segment.color.unwrap_or_else(|| default_color(idx));
            let percentage = if total > 0.0 {
                (segment.value / total * 100.0) as u32
            } else {
                0
            };

            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().size(px(12.0)).rounded(px(2.0)).bg(color))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.tokens.foreground)
                                    .child(segment.label.clone()),
                            )
                            .when(show_percentages, |this| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(format!("{}%", percentage)),
                                )
                            }),
                    ),
            )
        }))
}
