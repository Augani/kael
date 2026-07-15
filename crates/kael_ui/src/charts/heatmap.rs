use crate::{
    charts::{finite_or_zero, readable_text_color},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

fn lerp_color(low: Hsla, high: Hsla, t: f32) -> Hsla {
    let t = t.clamp(0.0, 1.0);
    hsla(
        low.h + (high.h - low.h) * t,
        low.s + (high.s - low.s) * t,
        low.l + (high.l - low.l) * t,
        low.a + (high.a - low.a) * t,
    )
}

fn value_domain(data: &[Vec<f64>]) -> Option<(f64, f64)> {
    let mut values = data.iter().flatten().copied();
    let first = values.next()?;
    Some(values.fold((first, first), |(min, max), value| {
        (min.min(value), max.max(value))
    }))
}

fn heatmap_text_sizes(cell_size: Pixels) -> (Pixels, Pixels) {
    let cell_size = f32::from(cell_size);
    (
        px((cell_size * 0.25).clamp(10.0, 14.0)),
        px((cell_size * 0.22).clamp(11.0, 13.0)),
    )
}

#[derive(IntoElement)]
pub struct Heatmap {
    data: Vec<Vec<f64>>,
    x_labels: Vec<SharedString>,
    y_labels: Vec<SharedString>,
    low_color: Option<Hsla>,
    high_color: Option<Hsla>,
    cell_size: Pixels,
    gap: Pixels,
    show_values: bool,
    show_labels: bool,
    style: StyleRefinement,
}

impl Default for Heatmap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heatmap {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            x_labels: Vec::new(),
            y_labels: Vec::new(),
            low_color: None,
            high_color: None,
            cell_size: px(40.0),
            gap: px(2.0),
            show_values: false,
            show_labels: true,
            style: StyleRefinement::default(),
        }
    }

    pub fn data(mut self, data: Vec<Vec<f64>>) -> Self {
        self.data = data
            .into_iter()
            .map(|row| row.into_iter().map(finite_or_zero).collect())
            .collect();
        self
    }

    pub fn x_labels(mut self, labels: Vec<impl Into<SharedString>>) -> Self {
        self.x_labels = labels.into_iter().map(|l| l.into()).collect();
        self
    }

    pub fn y_labels(mut self, labels: Vec<impl Into<SharedString>>) -> Self {
        self.y_labels = labels.into_iter().map(|l| l.into()).collect();
        self
    }

    pub fn color_scale(mut self, low: Hsla, high: Hsla) -> Self {
        self.low_color = Some(low);
        self.high_color = Some(high);
        self
    }

    pub fn cell_size(mut self, size: Pixels) -> Self {
        let value = f32::from(size);
        self.cell_size = if value.is_finite() && value > 0.0 {
            size
        } else {
            px(40.0)
        };
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        let value = f32::from(gap);
        self.gap = if value.is_finite() && value >= 0.0 {
            gap
        } else {
            px(2.0)
        };
        self
    }

    pub fn show_values(mut self, show: bool) -> Self {
        self.show_values = show;
        self
    }

    pub fn show_labels(mut self, show: bool) -> Self {
        self.show_labels = show;
        self
    }
}

impl Styled for Heatmap {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Heatmap {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        let low_color = self.low_color.unwrap_or_else(|| hsla(0.58, 0.7, 0.92, 1.0));
        let high_color = self
            .high_color
            .unwrap_or_else(|| hsla(0.58, 0.9, 0.35, 1.0));

        let domain = value_domain(&self.data);
        let (global_min, global_max) = domain.unwrap_or((0.0, 1.0));
        let value_range = (global_max - global_min).abs().max(1.0);
        let cell_size = self.cell_size;
        let (value_text_size, label_text_size) = heatmap_text_sizes(cell_size);
        let gap = self.gap;
        let show_values = self.show_values;
        let show_labels = self.show_labels;
        let _text_color = theme.tokens.foreground;
        let label_color = theme.tokens.muted_foreground;

        let has_y_labels = show_labels && !self.y_labels.is_empty();
        let has_x_labels = show_labels && !self.x_labels.is_empty();
        let cell_count = self.data.iter().map(Vec::len).sum::<usize>();
        let description = if cell_count == 0 {
            "Heatmap. No data".to_string()
        } else {
            format!(
                "Heatmap with {} rows and {cell_count} cells. Values range from {global_min:.2} to {global_max:.2}",
                self.data.len()
            )
        };

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label("Heatmap")
                    .description(description),
            )
            .max_w_full()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .map(|this| {
                let mut d = this;
                d.style().refine(&user_style);
                d
            })
            .when(cell_count == 0, |this| {
                this.min_h(px(120.0))
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(label_color)
                    .child("No data")
            })
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .when(has_y_labels, |this| {
                        this.child(
                            div()
                                .w(px(56.0))
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .gap(gap)
                                .children(self.y_labels.iter().map(|label| {
                                    div()
                                        .h(cell_size)
                                        .flex()
                                        .items_center()
                                        .justify_end()
                                        .pr(px(4.0))
                                        .text_size(label_text_size)
                                        .text_color(label_color)
                                        .child(label.clone())
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(gap)
                            .children(self.data.iter().map(|row| {
                                div().flex().gap(gap).children(row.iter().map(|&val| {
                                    let t = ((val - global_min) / value_range) as f32;
                                    let bg = lerp_color(low_color, high_color, t);

                                    let contrast = readable_text_color(bg);

                                    div()
                                        .size(cell_size)
                                        .rounded(px(4.0))
                                        .bg(bg)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .when(show_values, |this| {
                                            this.child(
                                                div()
                                                    .text_size(value_text_size)
                                                    .text_color(contrast)
                                                    .child(format!("{:.0}", val)),
                                            )
                                        })
                                }))
                            })),
                    ),
            )
            .when(has_x_labels, |this| {
                this.child(
                    div()
                        .flex()
                        .gap(gap)
                        .when(has_y_labels, |this| this.pl(px(60.0)))
                        .children(self.x_labels.iter().map(|label| {
                            div()
                                .w(cell_size)
                                .text_size(label_text_size)
                                .text_color(label_color)
                                .text_center()
                                .child(label.clone())
                        })),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn value_domain_preserves_constant_data() {
        assert_eq!(value_domain(&[]), None);
        assert_eq!(value_domain(&[vec![5.0, 5.0], vec![5.0]]), Some((5.0, 5.0)));
        assert_eq!(
            value_domain(&[vec![-2.0, 4.0], vec![1.0]]),
            Some((-2.0, 4.0))
        );
    }

    #[::core::prelude::v1::test]
    fn invalid_geometry_uses_safe_defaults() {
        let heatmap = Heatmap::new().cell_size(px(f32::NAN)).gap(px(-1.0));
        assert_eq!(f32::from(heatmap.cell_size), 40.0);
        assert_eq!(f32::from(heatmap.gap), 2.0);
    }

    #[::core::prelude::v1::test]
    fn text_scales_with_cells_inside_readable_bounds() {
        assert_eq!(heatmap_text_sizes(px(20.0)), (px(10.0), px(11.0)));
        assert_eq!(heatmap_text_sizes(px(64.0)), (px(14.0), px(13.0)));
        assert_eq!(heatmap_text_sizes(px(200.0)), (px(14.0), px(13.0)));
    }
}
