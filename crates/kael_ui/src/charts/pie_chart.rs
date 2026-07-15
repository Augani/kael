use crate::{
    charts::{data_summary, finite_or_zero, paint_radial_segments},
    theme::use_theme,
};
use kael::{prelude::FluentBuilder as _, *};

const CHART_COLORS: [u32; 8] = crate::astryx::CHART_PALETTE;

fn default_color(index: usize) -> Hsla {
    rgb(CHART_COLORS[index % CHART_COLORS.len()]).into()
}

fn pixels_to_f32(p: Pixels) -> f32 {
    p / px(1.0)
}

#[derive(Clone)]
pub struct PieChartSegment {
    pub label: SharedString,
    pub value: f64,
    pub color: Option<Hsla>,
}

impl PieChartSegment {
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

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum PieChartVariant {
    #[default]
    Pie,
    Donut,
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum PieChartLabelPosition {
    #[default]
    None,
    Legend,
}

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum PieChartSize {
    Sm,
    #[default]
    Md,
    Lg,
    Custom(u32),
}

impl PieChartSize {
    fn to_pixels(self) -> Pixels {
        match self {
            PieChartSize::Sm => px(120.0),
            PieChartSize::Md => px(200.0),
            PieChartSize::Lg => px(280.0),
            PieChartSize::Custom(size) => px(size as f32),
        }
    }
}

#[derive(IntoElement)]
pub struct PieChart {
    segments: Vec<PieChartSegment>,
    variant: PieChartVariant,
    label_position: PieChartLabelPosition,
    show_percentages: bool,
    center_label: Option<SharedString>,
    size: PieChartSize,
    donut_thickness: f32,
    start_angle_degrees: f32,
    segment_gap_degrees: f32,
    style: StyleRefinement,
}

impl PieChart {
    pub fn new(segments: Vec<PieChartSegment>) -> Self {
        Self {
            segments,
            variant: PieChartVariant::Pie,
            label_position: PieChartLabelPosition::None,
            show_percentages: false,
            center_label: None,
            size: PieChartSize::Md,
            donut_thickness: 0.35,
            start_angle_degrees: -90.0,
            segment_gap_degrees: 0.0,
            style: StyleRefinement::default(),
        }
    }

    pub fn pie(segments: Vec<PieChartSegment>) -> Self {
        Self::new(segments).variant(PieChartVariant::Pie)
    }

    pub fn donut(segments: Vec<PieChartSegment>) -> Self {
        Self::new(segments).variant(PieChartVariant::Donut)
    }

    pub fn variant(mut self, variant: PieChartVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: PieChartSize) -> Self {
        self.size = size;
        self
    }

    pub fn size_px(mut self, size_val: u32) -> Self {
        self.size = PieChartSize::Custom(size_val);
        self
    }

    pub fn show_percentages(mut self, show: bool) -> Self {
        self.show_percentages = show;
        self
    }

    pub fn center_label(mut self, label: impl Into<SharedString>) -> Self {
        self.center_label = Some(label.into());
        self
    }

    pub fn donut_thickness(mut self, thickness: f32) -> Self {
        self.donut_thickness = if thickness.is_finite() {
            thickness.clamp(0.1, 0.9)
        } else {
            0.4
        };
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

    pub fn label_position(mut self, position: PieChartLabelPosition) -> Self {
        self.label_position = position;
        self
    }
}

impl Styled for PieChart {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PieChart {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let chart_size = self.size.to_pixels();
        let show_legend = self.label_position == PieChartLabelPosition::Legend;
        let show_percentages = self.show_percentages;
        let user_style = self.style;
        let description = data_summary(
            if self.variant == PieChartVariant::Donut {
                "Donut chart"
            } else {
                "Pie chart"
            },
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
            render_empty_chart(chart_size)
        } else {
            render_pie_chart(
                chart_size,
                &self.segments,
                total,
                self.variant,
                self.donut_thickness,
                self.center_label.clone(),
                self.start_angle_degrees,
                self.segment_gap_degrees,
            )
        };

        let legend = if show_legend {
            Some(render_legend(&self.segments, total, show_percentages))
        } else {
            None
        };

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label(if self.variant == PieChartVariant::Donut {
                        "Donut chart"
                    } else {
                        "Pie chart"
                    })
                    .description(description),
            )
            .flex()
            .flex_wrap()
            .gap(px(24.0))
            .items_center()
            .child(chart)
            .when_some(legend, |this, legend| this.child(legend))
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
    }
}

fn render_pie_chart(
    chart_size: Pixels,
    segments: &[PieChartSegment],
    total: f64,
    variant: PieChartVariant,
    donut_thickness: f32,
    center_label: Option<SharedString>,
    start_angle_degrees: f32,
    segment_gap_degrees: f32,
) -> Div {
    let theme = use_theme();
    let size_f32 = pixels_to_f32(chart_size);
    let center = size_f32 * 0.5;
    let outer_radius = size_f32 * 0.5;
    let inner_radius = if variant == PieChartVariant::Donut {
        outer_radius * (1.0 - donut_thickness)
    } else {
        0.0
    };

    let mut segment_data: Vec<(f32, f32, Hsla)> = Vec::new();
    let mut current_angle = start_angle_degrees.to_radians();
    let gap = segment_gap_degrees.to_radians();

    for (idx, segment) in segments.iter().enumerate() {
        if !segment.value.is_finite() || segment.value <= 0.0 {
            continue;
        }
        let fraction = (segment.value / total) as f32;
        let sweep_angle = fraction * std::f32::consts::TAU;
        let color = segment.color.unwrap_or_else(|| default_color(idx));
        let visible_gap = gap.min(sweep_angle * 0.5);
        segment_data.push((
            current_angle + visible_gap * 0.5,
            (sweep_angle - visible_gap).max(0.0),
            color,
        ));
        current_angle += sweep_angle;
    }

    if segment_data.len() == 1 {
        return render_single_segment(
            chart_size,
            segment_data[0].2,
            inner_radius,
            variant,
            center_label,
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

    if variant == PieChartVariant::Donut {
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
                .items_center()
                .justify_center()
                .when_some(center_label, |this, label| {
                    this.child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .child(label),
                    )
                }),
        );
    }

    container
}

fn render_single_segment(
    chart_size: Pixels,
    color: Hsla,
    inner_radius: f32,
    variant: PieChartVariant,
    center_label: Option<SharedString>,
) -> Div {
    let theme = use_theme();
    let size_f32 = pixels_to_f32(chart_size);
    let center = size_f32 * 0.5;

    let mut container = div()
        .size(chart_size)
        .rounded(px(9999.0))
        .relative()
        .bg(color);

    if variant == PieChartVariant::Donut {
        let inner_size = inner_radius * 2.0;
        let inner_offset = center - inner_radius;

        container = container.child(
            div()
                .absolute()
                .size(px(inner_size))
                .rounded(px(9999.0))
                .bg(theme.tokens.background)
                .left(px(inner_offset))
                .top(px(inner_offset))
                .flex()
                .items_center()
                .justify_center()
                .when_some(center_label, |this, label| {
                    this.child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .child(label),
                    )
                }),
        );
    }

    container
}

fn render_empty_chart(chart_size: Pixels) -> Div {
    let theme = use_theme();

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

fn render_legend(segments: &[PieChartSegment], total: f64, show_percentages: bool) -> Div {
    let theme = use_theme();

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
