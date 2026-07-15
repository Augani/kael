use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::animations::easings;
use crate::components::progress::{ProgressSize, ProgressVariant};
use crate::theme::Theme;

#[derive(IntoElement)]
pub struct AnimatedProgress {
    id: ElementId,
    base: Div,
    value: f32,
    variant: ProgressVariant,
    size: ProgressSize,
    shimmer: bool,
    shimmer_repeat: Repeat,
    color: Option<Hsla>,
    duration: Duration,
    accessibility_label: SharedString,
}

impl AnimatedProgress {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            base: div(),
            value: 0.0,
            variant: ProgressVariant::Default,
            size: ProgressSize::Md,
            shimmer: false,
            shimmer_repeat: Repeat::Forever,
            color: None,
            duration: Duration::from_millis(500),
            accessibility_label: "Progress".into(),
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

    /// Set the label announced with the progress value.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = if label.is_empty() {
            "Progress".into()
        } else {
            label
        };
        self
    }

    pub fn variant(mut self, variant: ProgressVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ProgressSize) -> Self {
        self.size = size;
        self
    }

    pub fn shimmer(mut self, shimmer: bool) -> Self {
        self.shimmer = shimmer;
        self
    }

    /// Limit the shimmer to a finite number of sweeps.
    pub fn shimmer_cycles(mut self, cycles: u32) -> Self {
        self.shimmer_repeat = Repeat::Count(cycles.max(1));
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

impl Styled for AnimatedProgress {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for AnimatedProgress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);

        let height = match self.size {
            ProgressSize::Sm => px(4.0),
            ProgressSize::Md => px(8.0),
            ProgressSize::Lg => px(12.0),
        };

        let bar_color = self.color.unwrap_or(match self.variant {
            ProgressVariant::Default | ProgressVariant::Accent => theme.tokens.primary,
            ProgressVariant::Success => theme.tokens.success,
            ProgressVariant::Warning => theme.tokens.warning,
            ProgressVariant::Neutral => theme.tokens.muted_foreground,
            ProgressVariant::Error | ProgressVariant::Destructive => theme.tokens.destructive,
            ProgressVariant::Custom(color) => color,
        });

        let target_value = self.value;
        let duration = if window.animations_enabled() {
            self.duration
        } else {
            Duration::ZERO
        };
        let shimmer_enabled = self.shimmer;
        let animations_enabled = window.animations_enabled();
        let shimmer_repeat = self.shimmer_repeat;
        let value_key = (target_value * 10000.0) as u32;

        self.base
            .accessibility(AccessibilityAttributes::progress_bar(
                self.accessibility_label.to_string(),
                f64::from(target_value * 100.0),
                0.0,
                100.0,
            ))
            .w_full()
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(height)
                    .rounded(theme.tokens.radius_lg)
                    .bg(theme.tokens.muted)
                    .overflow_hidden()
                    .child(
                        div()
                            .id(self.id.clone())
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .bg(bar_color)
                            .rounded(theme.tokens.radius_lg)
                            .overflow_hidden()
                            .when(shimmer_enabled && animations_enabled, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(120.0))
                                        .bg(kael::linear_gradient(
                                            90.0,
                                            kael::linear_color_stop(kael::transparent_black(), 0.0),
                                            kael::linear_color_stop(hsla(0.0, 0.0, 1.0, 0.2), 1.0),
                                        ))
                                        .with_animation(
                                            "shimmer-sweep",
                                            Animation::new(Duration::from_millis(1500))
                                                .repeat(shimmer_repeat)
                                                .with_easing(kael::linear),
                                            move |el, delta| {
                                                let start = px(-120.0);
                                                let end = px(600.0);
                                                let pos = start + (end - start) * delta;
                                                el.left(pos)
                                            },
                                        ),
                                )
                            })
                            .with_animation(
                                ("progress-fill", value_key),
                                Animation::new(duration).with_easing(easings::ease_out_cubic),
                                move |el, delta| {
                                    let width = target_value * delta;
                                    el.w(relative(width))
                                },
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_values_and_labels_fall_back_to_accessible_defaults() {
        let progress = AnimatedProgress::new("progress")
            .value(f32::NAN)
            .accessibility_label("");
        assert_eq!(progress.value, 0.0);
        assert_eq!(progress.accessibility_label.as_ref(), "Progress");
        assert_eq!(AnimatedProgress::new("progress").value(2.0).value, 1.0);
    }
}
