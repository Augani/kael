use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

const SPINNER_START_ANGLE_DEGREES: f32 = 270.0;

fn spinner_gradient(active_color: Hsla, track_color: Hsla, rotation_degrees: f32) -> Background {
    conic_gradient(
        0.5,
        0.5,
        SPINNER_START_ANGLE_DEGREES + rotation_degrees.rem_euclid(360.0),
        &[
            linear_color_stop(active_color, 0.0),
            linear_color_stop(active_color, 0.75),
            linear_color_stop(track_color, 0.75),
            linear_color_stop(track_color, 1.0),
        ],
    )
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SpinnerSize {
    Xs,
    Sm,
    Md,
    Lg,
    Xl,
}

impl SpinnerSize {
    fn frame_pixels(self) -> Pixels {
        match self {
            SpinnerSize::Xs | SpinnerSize::Sm => px(14.0),
            SpinnerSize::Md => px(20.0),
            SpinnerSize::Lg => px(24.0),
            SpinnerSize::Xl => px(36.0),
        }
    }

    fn stroke_width(self) -> Pixels {
        match self {
            SpinnerSize::Xs | SpinnerSize::Sm => px(2.0),
            SpinnerSize::Md | SpinnerSize::Lg => px(3.0),
            SpinnerSize::Xl => px(4.0),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SpinnerVariant {
    Default,
    Primary,
    Secondary,
    Muted,
    /// An app-defined spinner color.
    Custom(Hsla),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SpinnerShade {
    #[default]
    Default,
    OnMedia,
    Subtle,
    Inherit,
}

#[derive(IntoElement)]
pub struct Spinner {
    size: SpinnerSize,
    variant: SpinnerVariant,
    shade: SpinnerShade,
    label: Option<SharedString>,
    accessibility_label: SharedString,
    decorative: bool,
    animation_repeat: Repeat,
    style: StyleRefinement,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: SpinnerSize::Md,
            variant: SpinnerVariant::Default,
            shade: SpinnerShade::Default,
            label: None,
            accessibility_label: "Loading".into(),
            decorative: false,
            animation_repeat: Repeat::Forever,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: SpinnerSize) -> Self {
        self.size = size;
        self
    }

    pub fn variant(mut self, variant: SpinnerVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn shade(mut self, shade: SpinnerShade) -> Self {
        self.shade = shade;
        self
    }

    /// Use a fully custom spinner color, setting the variant to [`SpinnerVariant::Custom`].
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.variant = SpinnerVariant::Custom(color.into());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        if label.trim().is_empty() {
            self.accessibility_label = "Loading".into();
            self.label = None;
        } else {
            self.accessibility_label = label.clone();
            self.label = Some(label);
        }
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = if label.trim().is_empty() {
            "Loading".into()
        } else {
            label
        };
        self
    }

    /// Hide the spinner from assistive technology when a semantic parent
    /// already communicates the loading state.
    pub fn decorative(mut self, decorative: bool) -> Self {
        self.decorative = decorative;
        self
    }

    /// Limit the spinner to a finite number of rotations.
    ///
    /// This is useful for previews and low-attention surfaces. Loading indicators
    /// remain continuous by default.
    pub fn animation_cycles(mut self, cycles: u32) -> Self {
        self.animation_repeat = Repeat::Count(cycles.max(1));
        self
    }
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Spinner {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Spinner {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let muted_foreground = tokens.muted_foreground;
        let font_family = tokens.font_family.clone();
        let user_style = self.style;
        let frame_size = self.size.frame_pixels();
        let stroke_width = self.size.stroke_width();
        let animation_repeat = self.animation_repeat;

        let active_color = match self.shade {
            SpinnerShade::OnMedia => white(),
            SpinnerShade::Subtle => tokens.muted_foreground,
            SpinnerShade::Inherit => tokens.foreground,
            SpinnerShade::Default => match self.variant {
                SpinnerVariant::Default | SpinnerVariant::Primary => tokens.primary,
                // Kept for API compatibility; ASTRYX exposes shade rather than
                // secondary/muted color variants for Spinner.
                SpinnerVariant::Secondary => tokens.muted_foreground,
                SpinnerVariant::Muted => tokens.muted_foreground,
                SpinnerVariant::Custom(color) => color,
            },
        };
        let track_color = match self.shade {
            SpinnerShade::OnMedia => white().opacity(0.3),
            SpinnerShade::Inherit => active_color.opacity(0.3),
            SpinnerShade::Default | SpinnerShade::Subtle => match self.variant {
                SpinnerVariant::Custom(_) => active_color.opacity(0.3),
                SpinnerVariant::Default
                | SpinnerVariant::Primary
                | SpinnerVariant::Secondary
                | SpinnerVariant::Muted => tokens.input,
            },
        };
        let accessibility = if self.decorative {
            AccessibilityAttributes::new(AccessibilityRole::Group)
                .states(AccessibilityState::HIDDEN)
        } else {
            AccessibilityAttributes::new(AccessibilityRole::ProgressBar)
                .label(self.accessibility_label.to_string())
                .value(AccessibilityValue::Text("Loading".into()))
                .busy(true)
        };

        div()
            .accessibility(accessibility)
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0))
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child({
                let spinner = div()
                    .size(frame_size)
                    .flex_shrink_0()
                    .rounded_full()
                    .border(stroke_width)
                    .border_gradient(spinner_gradient(active_color, track_color, 0.0));

                if window.animations_enabled() {
                    spinner
                        .with_animation(
                            "spinner-rotation",
                            Animation::new(std::time::Duration::from_millis(730))
                                .repeat(animation_repeat)
                                .with_easing(crate::animations::easings::linear),
                            move |el, delta| {
                                el.border_gradient(spinner_gradient(
                                    active_color,
                                    track_color,
                                    delta * 360.0,
                                ))
                            },
                        )
                        .into_any_element()
                } else {
                    spinner.into_any_element()
                }
            })
            .when_some(self.label, |d, label| {
                d.child(
                    div()
                        .text_size(px(14.0))
                        .line_height(px(20.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(if self.shade == SpinnerShade::Subtle {
                            muted_foreground
                        } else {
                            tokens.foreground
                        })
                        .font_family(font_family.clone())
                        .child(StyledText::new(label).accessibility_hidden(true)),
                )
            })
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn empty_labels_keep_a_meaningful_accessible_default() {
        let spinner = Spinner::new().label(" ").accessibility_label("");
        assert!(spinner.label.is_none());
        assert_eq!(spinner.accessibility_label.as_ref(), "Loading");
    }
}

#[cfg(test)]
mod tests {
    use super::{Spinner, SpinnerVariant, spinner_gradient};
    use kael::Repeat;

    #[test]
    fn color_sets_custom_spinner_variant() {
        let spinner = Spinner::new().color(kael::black());
        assert!(matches!(spinner.variant, SpinnerVariant::Custom(_)));
    }

    #[test]
    fn preview_cycles_are_finite_and_nonzero() {
        let spinner = Spinner::new().animation_cycles(0);
        assert_eq!(spinner.animation_repeat, Repeat::Count(1));
    }

    #[test]
    fn gradient_rotation_is_seamless_at_a_full_turn() {
        let active = kael::white();
        let track = active.opacity(0.3);

        assert_eq!(
            spinner_gradient(active, track, 0.0),
            spinner_gradient(active, track, 360.0)
        );
        assert_ne!(
            spinner_gradient(active, track, 0.0),
            spinner_gradient(active, track, 90.0)
        );
    }
}
