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
    fn to_pixels(self) -> Pixels {
        match self {
            SpinnerSize::Xs => px(16.0),
            SpinnerSize::Sm => px(20.0),
            SpinnerSize::Md => px(24.0),
            SpinnerSize::Lg => px(32.0),
            SpinnerSize::Xl => px(48.0),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SpinnerVariant {
    Default,
    Primary,
    Secondary,
    Muted,
}

#[derive(IntoElement)]
pub struct Spinner {
    size: SpinnerSize,
    variant: SpinnerVariant,
    label: Option<SharedString>,
    style: StyleRefinement,
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: SpinnerSize::Md,
            variant: SpinnerVariant::Default,
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
        let muted = tokens.muted;
        let muted_foreground = tokens.muted_foreground;
        let font_family = tokens.font_family.clone();
        let user_style = self.style;
        let size_px = self.size.to_pixels();
        let stroke_width = size_px * 0.15;

        let color = match self.variant {
            SpinnerVariant::Default => tokens.foreground,
            SpinnerVariant::Primary => tokens.primary,
            SpinnerVariant::Secondary => tokens.secondary,
            SpinnerVariant::Muted => tokens.muted_foreground,
        };

        let center = size_px * 0.5;
        let path_radius = (size_px - stroke_width) * 0.5;
        let dot_size = stroke_width * 1.2;
        let dot_offset = dot_size * 0.5;

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
                    .size(size_px)
                    .relative()
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .border(stroke_width)
                            .border_color(muted)
                            .rounded(px(9999.0)),
                    )
                    .child(
                        div()
                            .absolute()
                            .size(dot_size)
                            .rounded(px(9999.0))
                            .bg(color)
                            .with_animation(
                                "spinner-orbit",
                                Animation::new(std::time::Duration::from_millis(700))
                                    .repeat_forever()
                                    .with_easing(crate::animations::easings::linear),
                                move |dot, delta| {
                                    let angle = delta * std::f32::consts::TAU;
                                    let x = center + path_radius * angle.cos() - dot_offset;
                                    let y = center + path_radius * angle.sin() - dot_offset;
                                    dot.left(x).top(y)
                                },
                            ),
                    ),
            )
            .when_some(self.label, |d, label| {
                d.child(
                    div()
                        .text_size(px(12.0))
                        .text_color(muted_foreground)
                        .font_family(font_family.clone())
                        .child(label),
                )
            })
    }
}
