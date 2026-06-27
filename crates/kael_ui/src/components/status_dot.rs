//! StatusDot - a small colored presence/status indicator.

use crate::astryx::Hue;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum StatusTone {
    Neutral,
    Success,
    Warning,
    Error,
    Info,
    Hue(Hue),
    Custom(Hsla),
}

pub type StatusDotVariant = StatusTone;
pub type StatusDotVariantMap = StatusTone;
pub type StatusDotProps = StatusDot;

pub struct StatusDot {
    tone: StatusTone,
    size: Pixels,
    label: Option<SharedString>,
    tooltip: Option<SharedString>,
    pulse: bool,
    style: StyleRefinement,
}

impl StatusDot {
    pub fn default() -> Self {
        Self::new(StatusTone::Neutral)
    }

    pub fn new(tone: StatusTone) -> Self {
        Self {
            tone,
            size: px(8.0),
            label: None,
            tooltip: None,
            pulse: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn success() -> Self {
        Self::new(StatusTone::Success)
    }

    pub fn warning() -> Self {
        Self::new(StatusTone::Warning)
    }

    pub fn error() -> Self {
        Self::new(StatusTone::Error)
    }

    pub fn info() -> Self {
        Self::new(StatusTone::Info)
    }

    pub fn accent() -> Self {
        Self::new(StatusTone::Info)
    }

    pub fn neutral() -> Self {
        Self::new(StatusTone::Neutral)
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn variant(mut self, variant: StatusTone) -> Self {
        self.tone = variant;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Draw a soft halo behind the dot to signal an active/live status.
    pub fn pulse(mut self, pulse: bool) -> Self {
        self.pulse = pulse;
        self
    }

    pub fn is_pulsing(self, pulse: bool) -> Self {
        self.pulse(pulse)
    }

    #[allow(non_snake_case)]
    pub fn isPulsing(self, pulse: bool) -> Self {
        self.is_pulsing(pulse)
    }
}

impl Styled for StatusDot {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for StatusDot {
    type Element = Div;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let dark = theme.tokens.background.l < 0.5;
        let user_style = self.style;
        let accessible_label = self
            .tooltip
            .clone()
            .or_else(|| self.label.clone())
            .unwrap_or_else(|| "Status".into());

        let color = match self.tone {
            StatusTone::Neutral => theme.tokens.muted_foreground,
            StatusTone::Success => theme.tokens.success,
            StatusTone::Warning => theme.tokens.warning,
            StatusTone::Error => theme.tokens.destructive,
            StatusTone::Info => theme.tokens.accent,
            StatusTone::Hue(hue) => hue.colors(dark).border,
            StatusTone::Custom(c) => c,
        };

        let size = self.size;
        let dot = div()
            .relative()
            .size(size)
            .flex_shrink_0()
            .when(self.pulse, |this| {
                this.child(
                    div()
                        .absolute()
                        .left(-(size * 0.5))
                        .top(-(size * 0.5))
                        .size(size * 2.0)
                        .rounded_full()
                        .bg(color.opacity(0.22)),
                )
            })
            .child(div().size(size).rounded_full().bg(color));

        div()
            .relative()
            .flex()
            .flex_shrink_0()
            .size(size)
            .child(dot)
            .child(
                div()
                    .size(px(1.0))
                    .overflow_hidden()
                    .opacity(0.0)
                    .child(accessible_label),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
