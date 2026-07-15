use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

fn finite_progress(value: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, max)
    } else {
        0.0
    }
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn finite_max(max: f32) -> f32 {
    if max.is_finite() && max > 0.0 {
        max
    } else {
        1.0
    }
}

fn finite_positive_pixels(value: Pixels, fallback: Pixels) -> Pixels {
    let value = f32::from(value);
    if value.is_finite() && value > 0.0 {
        px(value)
    } else {
        fallback
    }
}

fn non_empty_label(label: SharedString, fallback: &'static str) -> SharedString {
    if label.trim().is_empty() {
        fallback.into()
    } else {
        label
    }
}

/// Progress bar variants
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ProgressVariant {
    /// Default blue progress bar
    Default,
    Accent,
    /// Success/complete state (green)
    Success,
    /// Warning state (yellow/orange)
    Warning,
    Neutral,
    Error,
    /// Error/failure state (red)
    Destructive,
    /// An app-defined fill color.
    Custom(Hsla),
}

pub type ProgressBarVariant = ProgressVariant;

/// Progress bar sizes
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProgressSize {
    /// Thin progress bar (h-1)
    Sm,
    /// Default height (h-2)
    Md,
    /// Larger height (h-3)
    Lg,
}

/// Spinner types for circular progress indicators
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SpinnerType {
    /// Single orbiting dot on a track
    Dot,
    /// Arc segment (longer than dot) on a track
    Arc,
    /// Arc segment rotating without visible track
    ArcNoTrack,
    /// Growing circle that fills from dot to complete circle (for determinate progress)
    GrowingCircle,
}

/// Progress bar component with determinate and indeterminate modes
#[derive(IntoElement)]
pub struct ProgressBar {
    /// Progress value (0.0 to 1.0 for determinate, None for indeterminate)
    value: Option<f32>,
    max: f32,
    variant: ProgressVariant,
    size: ProgressSize,
    /// Optional label to show percentage or custom text
    label: Option<SharedString>,
    label_hidden: bool,
    /// Show percentage text overlay
    show_percentage: bool,
    disabled: bool,
    style: StyleRefinement,
}

impl ProgressBar {
    /// Create a new progress bar with a value (0.0 to 1.0)
    pub fn new(value: f32) -> Self {
        Self {
            value: Some(finite_nonnegative(value)),
            max: 1.0,
            variant: ProgressVariant::Default,
            size: ProgressSize::Md,
            label: None,
            label_hidden: false,
            show_percentage: false,
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    /// Create an indeterminate progress bar (loading animation)
    pub fn indeterminate() -> Self {
        Self {
            value: None,
            max: 1.0,
            variant: ProgressVariant::Default,
            size: ProgressSize::Md,
            label: None,
            label_hidden: false,
            show_percentage: false,
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = Some(finite_nonnegative(value));
        self
    }

    pub fn max(mut self, max: f32) -> Self {
        self.max = finite_max(max);
        self
    }

    fn normalized_value(&self) -> Option<f32> {
        self.value.map(|value| finite_progress(value, self.max))
    }

    /// Set the progress variant
    pub fn variant(mut self, variant: ProgressVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Use a fully custom fill color, setting the variant to [`ProgressVariant::Custom`].
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.variant = ProgressVariant::Custom(color.into());
        self
    }

    /// Set the progress size
    pub fn size(mut self, size: ProgressSize) -> Self {
        self.size = size;
        self
    }

    /// Set a custom label
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(non_empty_label(label.into(), "Progress"));
        self
    }

    pub fn is_label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    /// Show percentage text (only for determinate progress)
    pub fn show_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasValueLabel(self, show: bool) -> Self {
        self.show_percentage(show)
    }

    pub fn is_indeterminate(mut self, indeterminate: bool) -> Self {
        if indeterminate {
            self.value = None;
        } else if self.value.is_none() {
            self.value = Some(0.0);
        }
        self
    }

    #[allow(non_snake_case)]
    pub fn isIndeterminate(self, indeterminate: bool) -> Self {
        self.is_indeterminate(indeterminate)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }
}

impl Styled for ProgressBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let primary = tokens.primary;
        let destructive = tokens.destructive;
        let foreground = tokens.foreground;
        let muted_foreground = tokens.muted_foreground;
        let normalized_value = self.normalized_value();
        let user_style = self.style;

        let height = match self.size {
            ProgressSize::Sm => px(4.0),
            ProgressSize::Md => px(8.0),
            ProgressSize::Lg => px(12.0),
        };

        let mut bar_color = match self.variant {
            ProgressVariant::Default | ProgressVariant::Accent => primary,
            ProgressVariant::Success => tokens.success, // green-500
            ProgressVariant::Warning => tokens.warning, // amber-500
            ProgressVariant::Neutral => tokens.muted_foreground,
            ProgressVariant::Error | ProgressVariant::Destructive => destructive,
            ProgressVariant::Custom(color) => color,
        };
        if self.disabled {
            bar_color = tokens.muted_foreground;
        }

        let progress_width = if let Some(value) = normalized_value {
            relative((value / self.max).clamp(0.0, 1.0))
        } else {
            relative(0.4)
        };

        let percentage_text =
            normalized_value.map(|v| format!("{}%", ((v / self.max) * 100.0).round() as u32));
        let accessibility_label = self
            .label
            .clone()
            .unwrap_or_else(|| "Progress".into())
            .to_string();
        let mut accessibility = if let Some(value) = normalized_value {
            AccessibilityAttributes::progress_bar(
                accessibility_label,
                value as f64,
                0.0,
                self.max as f64,
            )
        } else {
            AccessibilityAttributes::new(AccessibilityRole::ProgressBar)
                .label(accessibility_label)
                .value(AccessibilityValue::Text("Loading".into()))
                .busy(true)
        };
        if self.disabled {
            accessibility = accessibility.states(AccessibilityState::DISABLED);
        }

        div()
            .accessibility(accessibility)
            .flex()
            .flex_col()
            .gap(px(8.0))
            .w_full()
            .when(
                (self.label.is_some() && !self.label_hidden)
                    || (self.show_percentage && percentage_text.is_some()),
                |this| {
                    this.child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .when_some(self.label.filter(|_| !self.label_hidden), |this, label| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if self.disabled {
                                            muted_foreground
                                        } else {
                                            foreground
                                        })
                                        .child(StyledText::new(label).accessibility_hidden(true)),
                                )
                            })
                            .when(self.show_percentage && percentage_text.is_some(), |this| {
                                this.child(
                                    div().text_sm().text_color(muted_foreground).child(
                                        StyledText::new(SharedString::from(
                                            percentage_text.unwrap_or_default(),
                                        ))
                                        .accessibility_hidden(true),
                                    ),
                                )
                            }),
                    )
                },
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(height)
                    .rounded_full()
                    .bg(tokens.muted)
                    .overflow_hidden()
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .w(progress_width)
                            .bg(bar_color)
                            .rounded_full()
                            .map(|this| {
                                if normalized_value.is_none() && window.animations_enabled() {
                                    this.with_animation(
                                        "indeterminate-progress",
                                        Animation::new(std::time::Duration::from_millis(1500))
                                            .repeat_forever()
                                            .with_easing(
                                                crate::animations::easings::ease_in_out_quad,
                                            ),
                                        |div, delta| {
                                            let offset = -1.0 + delta * 3.5;
                                            div.left(relative(offset))
                                        },
                                    )
                                    .into_any_element()
                                } else {
                                    this.into_any_element()
                                }
                            }),
                    ),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

/// Circular progress/spinner component
#[derive(IntoElement)]
pub struct CircularProgress {
    /// Progress value (0.0 to 1.0 for determinate, None for indeterminate)
    value: Option<f32>,
    size: Pixels,
    stroke_width: Pixels,
    variant: ProgressVariant,
    spinner_type: SpinnerType,
    label: SharedString,
    animation_repeat: Repeat,
    style: StyleRefinement,
}

impl CircularProgress {
    /// Create a new circular progress with a value
    pub fn new(value: f32) -> Self {
        Self {
            value: Some(finite_progress(value, 1.0)),
            size: px(40.0),
            stroke_width: px(4.0),
            variant: ProgressVariant::Default,
            spinner_type: SpinnerType::Dot,
            label: "Progress".into(),
            animation_repeat: Repeat::Forever,
            style: StyleRefinement::default(),
        }
    }

    /// Create an indeterminate circular progress (spinner)
    pub fn indeterminate() -> Self {
        Self {
            value: None,
            size: px(40.0),
            stroke_width: px(4.0),
            variant: ProgressVariant::Default,
            spinner_type: SpinnerType::Dot,
            label: "Loading".into(),
            animation_repeat: Repeat::Forever,
            style: StyleRefinement::default(),
        }
    }

    /// Set the size
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = finite_positive_pixels(size, px(40.0));
        self.stroke_width = self.size * 0.1; // Stroke is 10% of size
        self
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = Some(finite_progress(value, 1.0));
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = non_empty_label(
            label.into(),
            if self.value.is_some() {
                "Progress"
            } else {
                "Loading"
            },
        );
        self
    }

    /// Set the variant
    pub fn variant(mut self, variant: ProgressVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Use a fully custom fill color, setting the variant to [`ProgressVariant::Custom`].
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.variant = ProgressVariant::Custom(color.into());
        self
    }

    /// Set the spinner type
    pub fn spinner_type(mut self, spinner_type: SpinnerType) -> Self {
        self.spinner_type = spinner_type;
        self
    }

    /// Limit an indeterminate spinner to a finite number of cycles.
    pub fn animation_cycles(mut self, cycles: u32) -> Self {
        self.animation_repeat = Repeat::Count(cycles.max(1));
        self
    }
}

impl Styled for CircularProgress {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CircularProgress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let primary = tokens.primary;
        let destructive = tokens.destructive;
        let muted = tokens.muted;
        let user_style = self.style;
        let animations_enabled = window.animations_enabled();
        let animation_repeat = if animations_enabled {
            self.animation_repeat
        } else {
            Repeat::Once
        };
        let animation_duration = move |millis| {
            if animations_enabled {
                std::time::Duration::from_millis(millis)
            } else {
                std::time::Duration::ZERO
            }
        };
        let accessibility = if let Some(value) = self.value {
            AccessibilityAttributes::progress_bar(self.label.to_string(), value as f64, 0.0, 1.0)
        } else {
            AccessibilityAttributes::new(AccessibilityRole::ProgressBar)
                .label(self.label.to_string())
                .value(AccessibilityValue::Text("Loading".into()))
                .busy(true)
        };

        let stroke_color = match self.variant {
            ProgressVariant::Default | ProgressVariant::Accent => primary,
            ProgressVariant::Success => tokens.success,
            ProgressVariant::Warning => tokens.warning,
            ProgressVariant::Neutral => tokens.muted_foreground,
            ProgressVariant::Error | ProgressVariant::Destructive => destructive,
            ProgressVariant::Custom(color) => color,
        };

        div()
            .accessibility(accessibility)
            .flex()
            .items_center()
            .justify_center()
            .size(self.size)
            .child(
                div()
                    .size(self.size)
                    .rounded(px(9999.0))
                    .relative()
                    .map(|container| {
                        let track_color = muted;
                        let stroke_w = self.stroke_width;
                        let container = container.child(
                            div()
                                .absolute()
                                .inset_0()
                                .border(stroke_w)
                                .border_color(track_color)
                                .rounded(px(9999.0)),
                        );

                        match (self.value, self.spinner_type) {
                            (Some(value), SpinnerType::GrowingCircle) => {
                                let size_px = self.size;
                                let center = size_px * 0.5;
                                let path_radius = (size_px - stroke_w) * 0.5;
                                let num_segments = 32; // More segments for smoother circle

                                container
                                    .children((0..num_segments).map(move |i| {
                                        let segment_angle = (i as f32 / num_segments as f32)
                                            * std::f32::consts::TAU;
                                        let progress_threshold = i as f32 / num_segments as f32;

                                        div()
                                            .absolute()
                                            .size(stroke_w * 1.2)
                                            .rounded(px(9999.0))
                                            .bg(stroke_color)
                                            .left(
                                                center + path_radius * segment_angle.cos()
                                                    - stroke_w * 0.6,
                                            )
                                            .top(
                                                center + path_radius * segment_angle.sin()
                                                    - stroke_w * 0.6,
                                            )
                                            .opacity(if value >= progress_threshold {
                                                1.0
                                            } else {
                                                0.0
                                            })
                                    }))
                                    .into_any_element()
                            }
                            (Some(value), _) => {
                                let progress_bg = conic_gradient(
                                    0.5,
                                    0.5,
                                    270.0,
                                    &[
                                        linear_color_stop(stroke_color, 0.0),
                                        linear_color_stop(stroke_color, value),
                                        linear_color_stop(muted, value),
                                        linear_color_stop(muted, 1.0),
                                    ],
                                );
                                container
                                    .child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .rounded_full()
                                            .bg(progress_bg)
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(stroke_w)
                                                    .top(stroke_w)
                                                    .size(self.size - stroke_w * 2.0)
                                                    .rounded_full()
                                                    .bg(tokens.background),
                                            ),
                                    )
                                    .into_any_element()
                            }
                            (None, SpinnerType::Dot) => {
                                let size_px = self.size;
                                let dot_diameter = stroke_w * 1.5;
                                let dot_radius = dot_diameter * 0.5;
                                let center = size_px * 0.5;
                                let path_radius = (size_px - stroke_w) * 0.5;

                                container
                                    .child(
                                        div()
                                            .absolute()
                                            .size(dot_diameter)
                                            .rounded(px(9999.0))
                                            .bg(stroke_color)
                                            .with_animation(
                                                "spinner-orbit",
                                                Animation::new(animation_duration(800))
                                                    .repeat(animation_repeat)
                                                    .with_easing(
                                                        crate::animations::easings::linear,
                                                    ),
                                                move |dot, delta| {
                                                    let angle = delta * std::f32::consts::TAU;
                                                    let x = center + path_radius * angle.cos()
                                                        - dot_radius;
                                                    let y = center + path_radius * angle.sin()
                                                        - dot_radius;
                                                    dot.left(x).top(y)
                                                },
                                            ),
                                    )
                                    .into_any_element()
                            }
                            (None, SpinnerType::Arc) => {
                                let size_px = self.size;
                                let center = size_px * 0.5;
                                let path_radius = (size_px - stroke_w) * 0.5;
                                let num_dots = 8; // Number of dots in the arc

                                container
                                    .children((0..num_dots).map(move |i| {
                                        let dot_angle = (i as f32 / num_dots as f32)
                                            * std::f32::consts::PI
                                            * 0.75; // Arc of about 135 degrees
                                        div()
                                            .absolute()
                                            .size(stroke_w * 1.2)
                                            .rounded(px(9999.0))
                                            .bg(stroke_color)
                                            .left(
                                                center + path_radius * dot_angle.cos()
                                                    - stroke_w * 0.6,
                                            )
                                            .top(
                                                center + path_radius * dot_angle.sin()
                                                    - stroke_w * 0.6,
                                            )
                                            .with_animation(
                                                ("spinner-arc", i as u32),
                                                Animation::new(animation_duration(1000))
                                                    .repeat(animation_repeat)
                                                    .with_easing(
                                                        crate::animations::easings::linear,
                                                    ),
                                                move |dot, delta| {
                                                    let visibility = ((delta
                                                        + i as f32 / num_dots as f32)
                                                        % 1.0)
                                                        < 0.6;
                                                    dot.opacity(if visibility { 1.0 } else { 0.0 })
                                                },
                                            )
                                    }))
                                    .into_any_element()
                            }
                            (None, SpinnerType::ArcNoTrack) => {
                                let size_px = self.size;
                                let center = size_px * 0.5;
                                let path_radius = (size_px - stroke_w) * 0.5;
                                let num_dots = 8;

                                div()
                                    .size(size_px)
                                    .relative()
                                    .children((0..num_dots).map(move |i| {
                                        div()
                                            .absolute()
                                            .size(stroke_w * 1.2)
                                            .rounded(px(9999.0))
                                            .bg(stroke_color)
                                            .with_animation(
                                                ("spinner-arc-no-track", i as u32),
                                                Animation::new(animation_duration(1000))
                                                    .repeat(animation_repeat)
                                                    .with_easing(
                                                        crate::animations::easings::linear,
                                                    ),
                                                move |dot, delta| {
                                                    let angle = delta * std::f32::consts::TAU
                                                        + (i as f32 / num_dots as f32)
                                                            * std::f32::consts::PI
                                                            * 0.75;
                                                    let x = center + path_radius * angle.cos()
                                                        - stroke_w * 0.6;
                                                    let y = center + path_radius * angle.sin()
                                                        - stroke_w * 0.6;
                                                    dot.left(x).top(y)
                                                },
                                            )
                                    }))
                                    .into_any_element()
                            }
                            (None, SpinnerType::GrowingCircle) => {
                                let size_px = self.size;
                                let center = size_px * 0.5;
                                let path_radius = (size_px - stroke_w) * 0.5;
                                let num_segments = 32;

                                container
                                    .children((0..num_segments).map(move |i| {
                                        div()
                                            .absolute()
                                            .size(stroke_w * 1.2)
                                            .rounded(px(9999.0))
                                            .bg(stroke_color)
                                            .with_animation(
                                                ("growing-circle", i as u32),
                                                Animation::new(animation_duration(2000))
                                                    .repeat(animation_repeat)
                                                    .with_easing(
                                                        crate::animations::easings::linear,
                                                    ),
                                                move |dot, delta| {
                                                    let segment_angle = (i as f32
                                                        / num_segments as f32)
                                                        * std::f32::consts::TAU;
                                                    let x = center
                                                        + path_radius * segment_angle.cos()
                                                        - stroke_w * 0.6;
                                                    let y = center
                                                        + path_radius * segment_angle.sin()
                                                        - stroke_w * 0.6;
                                                    let segment_progress =
                                                        i as f32 / num_segments as f32;
                                                    let visibility =
                                                        (delta - segment_progress + 1.0) % 1.0
                                                            < 0.3;
                                                    dot.left(x).top(y).opacity(if visibility {
                                                        1.0
                                                    } else {
                                                        0.0
                                                    })
                                                },
                                            )
                                    }))
                                    .into_any_element()
                            }
                        }
                    }),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_ranges_and_circular_sizes_use_safe_defaults() {
        assert_eq!(finite_max(f32::NAN), 1.0);
        assert_eq!(finite_max(f32::INFINITY), 1.0);
        assert_eq!(finite_max(-1.0), 1.0);

        let bar = ProgressBar::new(0.5).max(f32::INFINITY).label("  ");
        assert_eq!(bar.max, 1.0);
        assert_eq!(
            bar.label.as_ref().map(SharedString::as_ref),
            Some("Progress")
        );

        let circular = CircularProgress::new(0.5).size(px(f32::NAN)).label("");
        assert_eq!(circular.size, px(40.0));
        assert_eq!(circular.stroke_width, px(4.0));
        assert_eq!(circular.label.as_ref(), "Progress");
    }
}

#[cfg(test)]
mod tests {
    use super::{CircularProgress, ProgressBar, ProgressVariant};
    use kael::Repeat;

    #[test]
    fn color_sets_custom_progress_variant() {
        let bar = ProgressBar::new(0.5).color(kael::black());
        assert!(matches!(bar.variant, ProgressVariant::Custom(_)));
    }

    #[test]
    fn non_finite_progress_values_are_sanitized() {
        assert_eq!(ProgressBar::new(f32::NAN).value, Some(0.0));
        assert_eq!(CircularProgress::new(f32::INFINITY).value, Some(0.0));
    }

    #[test]
    fn progress_value_and_max_are_builder_order_independent() {
        let value_then_max = ProgressBar::new(0.0).value(62.0).max(100.0);
        let max_then_value = ProgressBar::new(0.0).max(100.0).value(62.0);

        assert_eq!(value_then_max.value, Some(62.0));
        assert_eq!(max_then_value.value, Some(62.0));
        assert_eq!(value_then_max.normalized_value(), Some(62.0));
        assert_eq!(max_then_value.normalized_value(), Some(62.0));
    }

    #[test]
    fn circular_spinner_can_use_finite_preview_cycles() {
        let spinner = CircularProgress::indeterminate().animation_cycles(0);
        assert_eq!(spinner.animation_repeat, Repeat::Count(1));
    }
}
