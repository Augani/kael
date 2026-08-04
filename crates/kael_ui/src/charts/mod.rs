pub mod area_chart;
pub mod bar_chart;
pub mod chart;
pub mod contribution_calendar;
pub mod donut_chart;
pub mod gauge;
pub mod heatmap;
pub mod line_chart;
pub mod pie_chart;
pub mod radar_chart;
pub mod treemap;

pub use bar_chart::{BarChart, BarChartData, BarChartMode, BarChartOrientation, BarChartSeries};
pub use chart::{
    Axis, AxisPosition, Chart, ChartArea, ChartPadding, DataPoint, DataRange, Legend,
    LegendPosition, Series, SeriesType, TooltipConfig,
};
pub use contribution_calendar::{
    ContributionCalendar, ContributionCalendarSize, ContributionCellShape, ContributionDay,
    ContributionPalette,
};
pub use line_chart::{LineChart, LineChartPoint, LineChartSeries};
pub use pie_chart::{
    PieChart, PieChartLabelPosition, PieChartSegment, PieChartSize, PieChartVariant,
};

use kael::{Bounds, Hsla, PathBuilder, Pixels, Window, point};

pub(crate) type RadialSegment = (f32, f32, Hsla);

/// Paints solid radial sectors into `bounds`. Donut charts add their center cutout as a
/// regular element above this canvas, which keeps both chart variants on the same renderer.
pub(crate) fn paint_radial_segments(
    bounds: Bounds<Pixels>,
    segments: &[RadialSegment],
    window: &mut Window,
) {
    if bounds.size.width <= kael::px(0.0) || bounds.size.height <= kael::px(0.0) {
        return;
    }

    let center = point(
        bounds.left() + bounds.size.width * 0.5,
        bounds.top() + bounds.size.height * 0.5,
    );
    let radius = bounds.size.width.min(bounds.size.height) * 0.5;
    let radius_f32 = radius / kael::px(1.0);

    for &(start_angle, sweep_angle, color) in segments {
        if !start_angle.is_finite() || !sweep_angle.is_finite() || sweep_angle <= 0.0 {
            continue;
        }

        let steps = ((sweep_angle * radius_f32 / 2.0).ceil() as usize).clamp(4, 512);
        let mut builder = PathBuilder::fill();
        builder.move_to(center);
        for step in 0..=steps {
            let angle = start_angle + sweep_angle * (step as f32 / steps as f32);
            builder.line_to(point(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            ));
        }
        builder.close();
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

pub(crate) fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

pub(crate) fn format_axis_value(value: f64) -> String {
    let value = finite_or_zero(value);
    if value == 0.0 {
        "0".to_string()
    } else if value.abs() >= 1000.0 {
        format!("{:.0}k", value / 1000.0)
    } else if value.abs() >= 1.0 {
        format!("{:.0}", value)
    } else {
        format!("{:.2}", value)
    }
}

pub(crate) fn readable_text_color(background: Hsla) -> Hsla {
    let rgb = background.to_rgb();
    let linear = |component: f32| {
        if component <= 0.04045 {
            component / 12.92
        } else {
            ((component + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b);
    let black_contrast = (luminance + 0.05) / 0.05;
    let white_contrast = 1.05 / (luminance + 0.05);
    if black_contrast >= white_contrast {
        kael::hsla(0.0, 0.0, 0.08, 1.0)
    } else {
        kael::hsla(0.0, 0.0, 0.98, 1.0)
    }
}

pub(crate) fn data_summary<'a>(
    kind: &str,
    values: impl IntoIterator<Item = (&'a str, f64)>,
) -> String {
    const MAX_ITEMS: usize = 12;
    let values: Vec<_> = values.into_iter().collect();
    if values.is_empty() {
        return format!("{kind}. No data");
    }

    let mut summary = format!("{kind}. ");
    for (index, (label, value)) in values.iter().take(MAX_ITEMS).enumerate() {
        if index > 0 {
            summary.push_str(", ");
        }
        summary.push_str(label);
        summary.push_str(": ");
        summary.push_str(&format!("{:.2}", finite_or_zero(*value)));
    }
    if values.len() > MAX_ITEMS {
        summary.push_str(&format!(", and {} more", values.len() - MAX_ITEMS));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_chart_values_are_normalized() {
        assert_eq!(finite_or_zero(f64::NAN), 0.0);
        assert_eq!(finite_or_zero(f64::INFINITY), 0.0);
        assert_eq!(finite_or_zero(4.5), 4.5);
    }

    #[test]
    fn axis_values_keep_fractional_precision_without_decorating_zero() {
        assert_eq!(format_axis_value(0.0), "0");
        assert_eq!(format_axis_value(-0.0), "0");
        assert_eq!(format_axis_value(0.25), "0.25");
        assert_eq!(format_axis_value(42.0), "42");
        assert_eq!(format_axis_value(2_000.0), "2k");
        assert_eq!(format_axis_value(f64::NAN), "0");
    }

    #[test]
    fn chart_summaries_are_bounded() {
        let values: Vec<_> = (0..20).map(|index| ("item", index as f64)).collect();
        let summary = data_summary("Chart", values);
        assert!(summary.contains("and 8 more"));
    }

    #[test]
    fn chart_text_color_uses_wcag_relative_luminance() {
        assert_eq!(
            readable_text_color(kael::white()),
            kael::hsla(0.0, 0.0, 0.08, 1.0)
        );
        assert_eq!(
            readable_text_color(kael::black()),
            kael::hsla(0.0, 0.0, 0.98, 1.0)
        );
    }
}
