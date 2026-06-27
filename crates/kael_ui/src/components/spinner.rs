use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};

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
    style: StyleRefinement,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: SpinnerSize::Md,
            variant: SpinnerVariant::Default,
            shade: SpinnerShade::Default,
            label: None,
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
        self.label = Some(label.into());
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let muted_foreground = tokens.muted_foreground;
        let font_family = tokens.font_family.clone();
        let user_style = self.style;
        let frame_size = self.size.frame_pixels();
        let stroke_width = self.size.stroke_width();
        let inner_size = frame_size - stroke_width * 2.0;

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
        let spinner_bg = conic_gradient(
            0.5,
            0.5,
            270.0,
            &[
                linear_color_stop(active_color, 0.0),
                linear_color_stop(active_color, 0.75),
                linear_color_stop(track_color, 0.75),
                linear_color_stop(track_color, 1.0),
            ],
        );

        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.0))
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(
                div()
                    .size(frame_size)
                    .relative()
                    .overflow_hidden()
                    .rounded_full()
                    .bg(spinner_bg)
                    .child(
                        div()
                            .absolute()
                            .left(stroke_width)
                            .top(stroke_width)
                            .size(inner_size)
                            .rounded_full()
                            .bg(match self.shade {
                                SpinnerShade::OnMedia => kael::transparent_black(),
                                _ => tokens.background,
                            }),
                    )
                    .with_animation(
                        "spinner-rotation",
                        Animation::new(std::time::Duration::from_millis(730))
                            .repeat_forever()
                            .with_easing(crate::animations::easings::linear),
                        |el, delta| el.rotate(delta * 360.0),
                    ),
            )
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
                        .child(label),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Spinner, SpinnerVariant};

    #[test]
    fn color_sets_custom_spinner_variant() {
        let spinner = Spinner::new().color(kael::black());
        assert!(matches!(spinner.variant, SpinnerVariant::Custom(_)));
    }
}
