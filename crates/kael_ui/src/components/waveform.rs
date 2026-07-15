//! Audio waveform visualization using vertical bars.

use kael::{prelude::FluentBuilder as _, *};

const MAX_WAVEFORM_BARS: usize = 4096;

struct WaveformPaintData {
    data: Vec<f32>,
    bar_width: f32,
    gap: f32,
    color: Hsla,
    active_color: Hsla,
    playback_position: f32,
}

#[derive(IntoElement)]
pub struct Waveform {
    data: Vec<f32>,
    bar_width: Pixels,
    gap: Pixels,
    color: Option<Hsla>,
    active_color: Option<Hsla>,
    playback_position: f32,
    style: StyleRefinement,
}

impl Waveform {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            bar_width: px(3.0),
            gap: px(2.0),
            color: None,
            active_color: None,
            playback_position: 0.0,
            style: StyleRefinement::default(),
        }
    }

    pub fn data(mut self, data: &[f32]) -> Self {
        self.data = data
            .iter()
            .map(|value| {
                if value.is_finite() {
                    value.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect();
        self
    }

    pub fn bar_width(mut self, width: Pixels) -> Self {
        let width = width / px(1.0);
        self.bar_width = px(if width.is_finite() {
            width.max(0.5)
        } else {
            3.0
        });
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        let gap = gap / px(1.0);
        self.gap = px(if gap.is_finite() { gap.max(0.0) } else { 2.0 });
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn active_color(mut self, color: Hsla) -> Self {
        self.active_color = Some(color);
        self
    }

    pub fn playback_position(mut self, position: f32) -> Self {
        self.playback_position = if position.is_finite() {
            position.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }

    pub fn sample_count(&self) -> usize {
        self.data.len()
    }

    pub fn has_samples(&self) -> bool {
        !self.data.is_empty()
    }

    pub fn playback_position_class(&self) -> &'static str {
        waveform_fraction_class(self.playback_position)
    }

    pub fn bar_width_class(&self) -> &'static str {
        waveform_dimension_class(self.bar_width / px(1.0))
    }

    pub fn gap_class(&self) -> &'static str {
        waveform_dimension_class(self.gap / px(1.0))
    }

    pub fn has_custom_color(&self) -> bool {
        self.color.is_some()
    }

    pub fn has_custom_active_color(&self) -> bool {
        self.active_color.is_some()
    }

    pub fn to_text(&self) -> String {
        format!(
            "waveform: samples {}, has samples {}, playback {}, bar {}, gap {}, color {}, active color {}",
            self.sample_count(),
            self.has_samples(),
            self.playback_position_class(),
            self.bar_width_class(),
            self.gap_class(),
            self.has_custom_color(),
            self.has_custom_active_color()
        )
    }
}

impl Default for Waveform {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Waveform {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Waveform {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = crate::theme::use_theme();
        let user_style = self.style;
        let sample_count = self.data.len();
        let playback_percent = (self.playback_position * 100.0).round() as u32;

        let default_color = theme.tokens.muted_foreground.opacity(0.4);
        let default_active = theme.tokens.primary;

        let paint_data = WaveformPaintData {
            data: self.data,
            bar_width: self.bar_width / px(1.0),
            gap: self.gap / px(1.0),
            color: self.color.unwrap_or(default_color),
            active_color: self.active_color.unwrap_or(default_active),
            playback_position: self.playback_position,
        };

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Image)
                    .label("Audio waveform")
                    .description(format!(
                        "{sample_count} samples, playback at {playback_percent}%"
                    )),
            )
            .relative()
            .when(user_style.size.width.is_none(), |this| this.w_full())
            .when(user_style.size.height.is_none(), |this| this.h(px(48.0)))
            .child(
                canvas_with_prepaint(
                    move |_bounds, _window, _cx| paint_data,
                    move |bounds, data, window, _cx| {
                        paint_waveform(bounds, &data, window);
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

fn paint_waveform(bounds: Bounds<Pixels>, data: &WaveformPaintData, window: &mut Window) {
    if data.data.is_empty() || bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
        return;
    }

    let bar_w = data.bar_width;
    let gap_w = data.gap;
    let step = bar_w + gap_w;

    if step <= 0.0 {
        return;
    }

    let available_width = bounds.size.width / px(1.0);
    let max_bars = ((available_width / step).floor() as usize).min(MAX_WAVEFORM_BARS);

    if max_bars == 0 {
        return;
    }

    // Resample to the available width in both directions. Short sample arrays
    // expand smoothly instead of leaving most of a responsive surface blank;
    // dense arrays downsample to a bounded amount of paint work.
    let bar_count = max_bars;
    let active_bar_boundary = (data.playback_position * bar_count as f32).floor() as usize;
    let height_f = bounds.size.height / px(1.0);

    for i in 0..bar_count {
        let amplitude = resampled_amplitude(&data.data, i, bar_count);
        let bar_height = (amplitude * height_f).max(2.0);

        let x = bounds.left() + px(i as f32 * step);
        let y = bounds.top() + px((height_f - bar_height) * 0.5);

        let bar_color = if i < active_bar_boundary {
            data.active_color
        } else {
            data.color
        };

        window.paint_quad(PaintQuad {
            bounds: Bounds {
                origin: point(x, y),
                size: kael::size(px(bar_w), px(bar_height)),
            },
            corner_radii: Corners::all(px(bar_w * 0.5)),
            background: bar_color.into(),
            border_widths: Edges::default(),
            border_color: (transparent_black()).into(),
            border_style: BorderStyle::default(),
            continuous_corners: false,
            transform: Default::default(),
            blend_mode: Default::default(),
        });
    }
}

fn resampled_amplitude(samples: &[f32], index: usize, output_count: usize) -> f32 {
    match samples.len() {
        0 => 0.0,
        1 => samples[0].clamp(0.0, 1.0),
        sample_count => {
            let denominator = output_count.saturating_sub(1).max(1) as f32;
            let position = (index.min(output_count.saturating_sub(1)) as f32 / denominator)
                * (sample_count - 1) as f32;
            let lower = (position.floor() as usize).min(sample_count - 1);
            let upper = (lower + 1).min(sample_count - 1);
            let fraction = position - lower as f32;
            (samples[lower] + (samples[upper] - samples[lower]) * fraction).clamp(0.0, 1.0)
        }
    }
}

fn waveform_fraction_class(value: f32) -> &'static str {
    if value <= 0.0 {
        "start"
    } else if value < 0.25 {
        "early"
    } else if value < 0.75 {
        "middle"
    } else if value < 1.0 {
        "late"
    } else {
        "end"
    }
}

fn waveform_dimension_class(value: f32) -> &'static str {
    if value <= 0.0 {
        "invalid"
    } else if value < 2.0 {
        "fine"
    } else if value < 8.0 {
        "standard"
    } else {
        "wide"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn waveform_summary_is_content_safe() {
        let waveform = Waveform::new()
            .data(&[0.13, 0.87, 0.42, 1.0])
            .bar_width(px(4.0))
            .gap(px(2.0))
            .playback_position(0.63)
            .color(red())
            .active_color(green());

        let summary = waveform.to_text();
        assert!(summary.contains("samples 4"));
        assert!(summary.contains("has samples true"));
        assert!(summary.contains("playback middle"));
        assert!(summary.contains("bar standard"));
        assert!(summary.contains("gap standard"));
        assert!(summary.contains("color true"));
        assert!(summary.contains("active color true"));
        assert!(!summary.contains("0.13"));
        assert!(!summary.contains("0.87"));
        assert!(!summary.contains("0.63"));
    }

    #[::core::prelude::v1::test]
    fn invalid_samples_and_dimensions_are_sanitized() {
        let waveform = Waveform::new()
            .data(&[f32::NAN, -1.0, 0.5, 2.0])
            .bar_width(px(f32::NAN))
            .gap(px(-4.0))
            .playback_position(f32::INFINITY);

        assert_eq!(waveform.data, vec![0.0, 0.0, 0.5, 1.0]);
        assert_eq!(waveform.bar_width, px(3.0));
        assert_eq!(waveform.gap, px(0.0));
        assert_eq!(waveform.playback_position, 0.0);
    }

    #[::core::prelude::v1::test]
    fn responsive_resampling_fills_width_and_preserves_endpoints() {
        let samples = [0.0, 1.0, 0.0];
        assert_eq!(resampled_amplitude(&samples, 0, 5), 0.0);
        assert_eq!(resampled_amplitude(&samples, 2, 5), 1.0);
        assert_eq!(resampled_amplitude(&samples, 4, 5), 0.0);
        assert_eq!(resampled_amplitude(&[0.4], 100, 200), 0.4);
    }
}
