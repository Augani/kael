use crate::animations::lerp_color;
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct GradientText {
    text: SharedString,
    start_color: Option<Hsla>,
    end_color: Option<Hsla>,
    stops: Vec<Hsla>,
    text_size: Option<Pixels>,
    font_weight: Option<FontWeight>,
    font_family: Option<SharedString>,
    style: StyleRefinement,
}

impl GradientText {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            start_color: None,
            end_color: None,
            stops: Vec::new(),
            text_size: None,
            font_weight: None,
            font_family: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn start_color(mut self, color: Hsla) -> Self {
        self.start_color = Some(color);
        self
    }

    pub fn end_color(mut self, color: Hsla) -> Self {
        self.end_color = Some(color);
        self
    }

    /// Set three or more gradient stops, sampled evenly across the text. Takes
    /// precedence over `start_color`/`end_color` when it holds at least two colors.
    pub fn colors(mut self, stops: impl Into<Vec<Hsla>>) -> Self {
        self.stops = stops.into();
        self
    }

    pub fn text_size(mut self, size: Pixels) -> Self {
        let size = size / px(1.0);
        self.text_size = Some(px(if size.is_finite() {
            size.max(1.0)
        } else {
            16.0
        }));
        self
    }

    pub fn font_weight(mut self, weight: FontWeight) -> Self {
        self.font_weight = Some(weight);
        self
    }

    pub fn font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.font_family = Some(family.into());
        self
    }
}

impl Styled for GradientText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn sample_stops(stops: &[Hsla], t: f32) -> Hsla {
    match stops.len() {
        0 => hsla(0.0, 0.0, 0.0, 1.0),
        1 => stops[0],
        n => {
            let scaled = t.clamp(0.0, 1.0) * (n - 1) as f32;
            let index = (scaled.floor() as usize).min(n - 2);
            lerp_color(stops[index], stops[index + 1], scaled - index as f32)
        }
    }
}

impl RenderOnce for GradientText {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let accessible_text = if self.text.trim().is_empty() {
            "Gradient text".to_string()
        } else {
            self.text.to_string()
        };
        let start = self.start_color.unwrap_or(hsla(0.6, 0.8, 0.6, 1.0));
        let end = self.end_color.unwrap_or(hsla(0.8, 0.8, 0.6, 1.0));
        let stops = self.stops;

        let chars: Vec<char> = self.text.chars().collect();
        let count = chars.len();
        let max_index = if count > 1 { count - 1 } else { 1 };

        let text_size = self.text_size;
        let font_weight = self.font_weight;
        let font_family = self.font_family;
        let user_style = self.style;

        let mut row = div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::StaticText).label(accessible_text),
            )
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .map(|mut el| {
                el.style().refine(&user_style);
                el
            });

        for (i, ch) in chars.into_iter().enumerate() {
            let t = i as f32 / max_index as f32;
            let color = if stops.len() >= 2 {
                sample_stops(&stops, t)
            } else {
                lerp_color(start, end, t)
            };
            let s: SharedString = ch.to_string().into();

            let mut span = div()
                .text_color(color)
                .child(StyledText::new(s).accessibility_hidden(true));

            if let Some(size) = text_size {
                span = span.text_size(size);
            }
            if let Some(weight) = font_weight {
                span = span.font_weight(weight);
            }
            if let Some(ref family) = font_family {
                span = span.font_family(family.clone());
            }

            row = row.child(span);
        }

        row
    }
}

#[cfg(test)]
mod tests {
    use super::sample_stops;
    use kael::{hsla, Hsla};

    fn close(a: Hsla, b: Hsla) -> bool {
        (a.h - b.h).abs() < 1e-4
            && (a.s - b.s).abs() < 1e-4
            && (a.l - b.l).abs() < 1e-4
            && (a.a - b.a).abs() < 1e-4
    }

    #[test]
    fn sample_stops_hits_endpoints_and_interior_stop() {
        let red = hsla(0.0, 1.0, 0.5, 1.0);
        let green = hsla(0.33, 1.0, 0.5, 1.0);
        let blue = hsla(0.66, 1.0, 0.5, 1.0);
        let stops = [red, green, blue];

        assert!(close(sample_stops(&stops, 0.0), red));
        assert!(close(sample_stops(&stops, 1.0), blue));
        assert!(close(sample_stops(&stops, 0.5), green));
        assert!(close(sample_stops(&[red], 0.7), red));
        assert!(close(sample_stops(&stops, 1.5), blue));
    }
}
