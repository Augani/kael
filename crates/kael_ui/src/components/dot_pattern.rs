//! Dot pattern background - renders a repeating grid of dots.

use kael::{prelude::FluentBuilder as _, *};

use crate::theme::Theme;

#[derive(IntoElement)]
pub struct DotPattern {
    spacing: Pixels,
    dot_size: Pixels,
    color: Option<Hsla>,
    dot_opacity: f32,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

impl DotPattern {
    pub fn new() -> Self {
        Self {
            spacing: px(20.0),
            dot_size: px(2.0),
            color: None,
            dot_opacity: 0.3,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    pub fn spacing(mut self, spacing: impl Into<Pixels>) -> Self {
        let spacing = spacing.into() / px(1.0);
        self.spacing = px(if spacing.is_finite() {
            spacing.max(1.0)
        } else {
            20.0
        });
        self
    }

    pub fn dot_size(mut self, size: impl Into<Pixels>) -> Self {
        let size = size.into() / px(1.0);
        self.dot_size = px(if size.is_finite() { size.max(0.5) } else { 2.0 });
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn opacity(mut self, opacity: f32) -> Self {
        self.dot_opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            0.3
        };
        self
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::DotPattern;
    use kael::px;

    #[test]
    fn invalid_geometry_and_opacity_use_safe_defaults() {
        let pattern = DotPattern::new()
            .spacing(px(0.0))
            .dot_size(px(f32::NAN))
            .opacity(f32::NAN);
        assert_eq!(pattern.spacing, px(1.0));
        assert_eq!(pattern.dot_size, px(2.0));
        assert_eq!(pattern.dot_opacity, 0.3);
    }
}

impl Default for DotPattern {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for DotPattern {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for DotPattern {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for DotPattern {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let dot_color = self
            .color
            .unwrap_or(theme.tokens.border)
            .opacity(self.dot_opacity);
        let spacing = self.spacing;
        let dot_size = self.dot_size;

        div()
            .relative()
            .w_full()
            .h_full()
            .overflow_hidden()
            .child(
                canvas_with_prepaint(
                    move |_bounds, _window, _cx| {},
                    move |bounds, _, window, _cx| {
                        let origin = bounds.origin;
                        let size = bounds.size;
                        let spacing_f32 = spacing / px(1.0);
                        let cols = ((size.width / px(1.0)) / spacing_f32).ceil() as usize + 1;
                        let rows = ((size.height / px(1.0)) / spacing_f32).ceil() as usize + 1;
                        let half_dot = dot_size / px(1.0) / 2.0;
                        let origin_x = origin.x / px(1.0);
                        let origin_y = origin.y / px(1.0);

                        for row in 0..rows {
                            for col in 0..cols {
                                let cx_pos = origin_x + col as f32 * spacing_f32;
                                let cy_pos = origin_y + row as f32 * spacing_f32;

                                let dot_bounds = Bounds {
                                    origin: point(px(cx_pos - half_dot), px(cy_pos - half_dot)),
                                    size: kael::size(dot_size, dot_size),
                                };

                                window.paint_quad(PaintQuad {
                                    bounds: dot_bounds,
                                    corner_radii: Corners::all(dot_size),
                                    background: dot_color.into(),
                                    border_widths: Edges::default(),
                                    border_color: (transparent_black()).into(),
                                    border_style: BorderStyle::default(),
                                    continuous_corners: false,
                                    transform: Default::default(),
                                    blend_mode: Default::default(),
                                });
                            }
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .child(div().relative().size_full().children(self.children))
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}
