use crate::theme::Theme;
use kael::*;

#[derive(IntoElement)]
pub struct GradientBorder {
    base: Div,
    inner: Div,
    start_color: Option<Hsla>,
    end_color: Option<Hsla>,
    border_width: Pixels,
    angle: f32,
    corner_radius: Option<Pixels>,
}

impl GradientBorder {
    pub fn new() -> Self {
        Self {
            base: div(),
            inner: div(),
            start_color: None,
            end_color: None,
            border_width: px(2.0),
            angle: 135.0,
            corner_radius: None,
        }
    }

    pub fn colors(mut self, start: Hsla, end: Hsla) -> Self {
        self.start_color = Some(start);
        self.end_color = Some(end);
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        let width = width / px(1.0);
        self.border_width = px(if width.is_finite() {
            width.max(0.0)
        } else {
            2.0
        });
        self
    }

    pub fn angle(mut self, angle: f32) -> Self {
        self.angle = if angle.is_finite() { angle } else { 135.0 };
        self
    }

    pub fn rounded(mut self, radius: Pixels) -> Self {
        let radius = radius / px(1.0);
        self.corner_radius = Some(px(if radius.is_finite() {
            radius.max(0.0)
        } else {
            0.0
        }));
        self
    }
}

impl Default for GradientBorder {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for GradientBorder {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let start = self.start_color.unwrap_or(theme.tokens.primary);
        let end = self.end_color.unwrap_or(theme.tokens.accent);
        let radius = self.corner_radius.unwrap_or(theme.tokens.radius_lg);
        let border_w = self.border_width;

        self.base
            .bg(kael::linear_gradient(
                self.angle,
                kael::linear_color_stop(start, 0.0),
                kael::linear_color_stop(end, 1.0),
            ))
            .rounded(radius)
            .p(border_w)
            .child(
                self.inner
                    .flex_grow()
                    .bg(theme.tokens.card)
                    .rounded((radius - border_w).max(px(0.0))),
            )
    }
}

impl Styled for GradientBorder {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for GradientBorder {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for GradientBorder {}

impl ParentElement for GradientBorder {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.inner.extend(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::GradientBorder;
    use kael::px;

    #[test]
    fn invalid_geometry_uses_safe_values() {
        let border = GradientBorder::new()
            .width(px(f32::NAN))
            .angle(f32::INFINITY)
            .rounded(px(-8.0));
        assert_eq!(border.border_width, px(2.0));
        assert_eq!(border.angle, 135.0);
        assert_eq!(border.corner_radius, Some(px(0.0)));
    }
}
