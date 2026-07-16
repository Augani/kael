use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

const GAUGE_START_ANGLE: f32 = std::f32::consts::PI;
const GAUGE_END_ANGLE: f32 = std::f32::consts::TAU;

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum GaugeSize {
    Sm,
    #[default]
    Md,
    Lg,
    Custom(u32),
}

impl GaugeSize {
    fn dimensions(self) -> (f32, f32) {
        match self {
            GaugeSize::Sm => (120.0, 80.0),
            GaugeSize::Md => (200.0, 130.0),
            GaugeSize::Lg => (300.0, 190.0),
            GaugeSize::Custom(w) => (w as f32, w as f32 * 0.65),
        }
    }

    fn stroke_width(self) -> f32 {
        match self {
            GaugeSize::Sm => 10.0,
            GaugeSize::Md => 16.0,
            GaugeSize::Lg => 22.0,
            GaugeSize::Custom(w) => (w as f32 * 0.08).max(6.0),
        }
    }
}

struct PaintData {
    value: f32,
    color: Hsla,
    track_color: Hsla,
    stroke_width: f32,
}

#[derive(IntoElement)]
pub struct Gauge {
    id: SharedString,
    value: f32,
    label: Option<SharedString>,
    format_fn: Option<Rc<dyn Fn(f32) -> String>>,
    size: GaugeSize,
    color: Option<Hsla>,
    track_color: Option<Hsla>,
    style: StyleRefinement,
}

impl Gauge {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            value: 0.0,
            label: None,
            format_fn: None,
            size: GaugeSize::default(),
            color: None,
            track_color: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn format(mut self, f: impl Fn(f32) -> String + 'static) -> Self {
        self.format_fn = Some(Rc::new(f));
        self
    }

    pub fn size(mut self, size: GaugeSize) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn track_color(mut self, color: Hsla) -> Self {
        self.track_color = Some(color);
        self
    }
}

impl Styled for Gauge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Gauge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let (width, height) = self.size.dimensions();
        let aspect_ratio = width / height;
        let stroke = self.size.stroke_width();

        let gauge_color = self.color.unwrap_or_else(|| rgb(0x3b82f6).into());
        let track = self.track_color.unwrap_or(theme.tokens.muted);

        let formatted_value = if let Some(ref fmt) = self.format_fn {
            fmt(self.value)
        } else {
            format!("{}%", (self.value * 100.0) as i32)
        };

        let text_color = theme.tokens.foreground;
        let label_color = theme.tokens.muted_foreground;
        let accessible_label = self.label.clone().unwrap_or_else(|| "Gauge".into());

        let paint_data = PaintData {
            value: self.value,
            color: gauge_color,
            track_color: track,
            stroke_width: stroke,
        };

        div()
            .id(self.id)
            .accessibility(AccessibilityAttributes::progress_bar(
                accessible_label.as_ref(),
                self.value as f64,
                0.0,
                1.0,
            ))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .w_full()
            .max_w(px(width))
            .min_w(px(0.0))
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .child(
                div()
                    .w_full()
                    .aspect_ratio(aspect_ratio)
                    .relative()
                    .overflow_hidden()
                    .child(
                        canvas_with_prepaint(
                            move |_bounds, _window, _cx| paint_data,
                            move |bounds, data, window, _cx| {
                                if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
                                    return;
                                }

                                let w = bounds.size.width;
                                let center_x = bounds.left() + w * 0.5;
                                let arc_radius = w * 0.5 - px(data.stroke_width * 0.5);

                                if arc_radius <= px(0.0) {
                                    return;
                                }

                                let center_y = bounds.bottom() - px(data.stroke_width * 0.5);

                                let segments = 60_usize;
                                // Screen-space Y grows downward, so the upper semicircle runs
                                // from PI to TAU. PI to zero would draw below the canvas and make
                                // larger gauges escape their layout bounds.
                                let start_angle = GAUGE_START_ANGLE;
                                let end_angle = GAUGE_END_ANGLE;

                                {
                                    let mut builder = PathBuilder::stroke(px(data.stroke_width));
                                    for i in 0..=segments {
                                        let t = i as f32 / segments as f32;
                                        let angle = start_angle + (end_angle - start_angle) * t;
                                        let pt = point(
                                            center_x + arc_radius * angle.cos(),
                                            center_y + arc_radius * angle.sin(),
                                        );
                                        if i == 0 {
                                            builder.move_to(pt);
                                        } else {
                                            builder.line_to(pt);
                                        }
                                    }
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, data.track_color);
                                    }
                                }

                                if data.value > 0.0 {
                                    let value_end =
                                        start_angle + (end_angle - start_angle) * data.value;
                                    let value_segments =
                                        ((segments as f32 * data.value) as usize).max(2);
                                    let mut builder = PathBuilder::stroke(px(data.stroke_width));
                                    for i in 0..=value_segments {
                                        let t = i as f32 / value_segments as f32;
                                        let angle = start_angle + (value_end - start_angle) * t;
                                        let pt = point(
                                            center_x + arc_radius * angle.cos(),
                                            center_y + arc_radius * angle.sin(),
                                        );
                                        if i == 0 {
                                            builder.move_to(pt);
                                        } else {
                                            builder.line_to(pt);
                                        }
                                    }
                                    if let Ok(path) = builder.build() {
                                        window.paint_path(path, data.color);
                                    }
                                }
                            },
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .child(
                        div()
                            .absolute()
                            .bottom(px(4.0))
                            .left_0()
                            .right_0()
                            .flex()
                            .justify_center()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(text_color)
                                    .child(formatted_value),
                            ),
                    ),
            )
            .when_some(self.label, |this, lbl| {
                this.child(div().text_sm().text_color(label_color).child(lbl))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn values_are_finite_and_bounded() {
        assert_eq!(Gauge::new("low").value(-1.0).value, 0.0);
        assert_eq!(Gauge::new("high").value(2.0).value, 1.0);
        assert_eq!(Gauge::new("nan").value(f32::NAN).value, 0.0);
    }

    #[::core::prelude::v1::test]
    fn semicircle_uses_the_upper_screen_space_arc() {
        assert_eq!(GAUGE_START_ANGLE, std::f32::consts::PI);
        assert_eq!(GAUGE_END_ANGLE, std::f32::consts::TAU);
        const { assert!(GAUGE_END_ANGLE > GAUGE_START_ANGLE) };
    }
}
