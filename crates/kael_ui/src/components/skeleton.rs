//! Skeleton component - Loading placeholder with pulsing animation effect.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkeletonVariant {
    #[default]
    Text,
    Circle,
    Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkeletonRadius {
    None,
    R0,
    R1,
    R2,
    #[default]
    R3,
    R4,
    Rounded,
}

impl SkeletonRadius {
    fn pixels(self, tokens: &crate::theme::ThemeTokens) -> Pixels {
        match self {
            Self::None | Self::R0 => px(0.0),
            Self::R1 => tokens.radius_sm,
            Self::R2 => tokens.radius_md,
            Self::R3 => tokens.radius_lg,
            Self::R4 => tokens.radius_xl,
            Self::Rounded => px(9999.0),
        }
    }
}

#[derive(IntoElement)]
pub struct Skeleton {
    base: Div,
    variant: SkeletonVariant,
    secondary: bool,
    radius: Option<SkeletonRadius>,
    index: usize,
    animated: bool,
}

impl Skeleton {
    pub fn new() -> Self {
        Self {
            base: div(),
            variant: SkeletonVariant::default(),
            secondary: false,
            radius: None,
            index: 0,
            animated: true,
        }
    }

    pub fn variant(mut self, variant: SkeletonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn secondary(mut self, secondary: bool) -> Self {
        self.secondary = secondary;
        self
    }

    pub fn radius(mut self, radius: SkeletonRadius) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Stagger this skeleton's pulse timing by its index in a loading group.
    pub fn index(mut self, index: usize) -> Self {
        self.index = index;
        self
    }

    /// Control the pulse animation while keeping the loading placeholder visible.
    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }
}

impl RenderOnce for Skeleton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let radius_md = tokens.radius_md;
        let explicit_radius = self.radius.map(|radius| radius.pixels(tokens));

        let base_color = if self.secondary {
            tokens.input.opacity(0.75)
        } else {
            tokens.input
        };
        let index = self.index;

        let skeleton = self
            .base
            .when(self.variant == SkeletonVariant::Text, |this| {
                this.w_full().h(px(16.0)).rounded(radius_md)
            })
            .when(self.variant == SkeletonVariant::Circle, |this| {
                this.rounded_full()
            })
            .when(self.variant == SkeletonVariant::Rect, |this| {
                this.rounded(radius_md)
            })
            .when_some(explicit_radius, |this, radius| this.rounded(radius))
            .bg(base_color)
            .opacity(0.25);

        if self.animated && window.animations_enabled() {
            skeleton
                .with_animation(
                    "skeleton-pulse",
                    Animation::new(Duration::from_millis(1100))
                        .delay(Duration::from_millis(1000 + 100 * index as u64))
                        .repeat_forever()
                        .with_easing(crate::animations::easings::linear),
                    move |this, delta| {
                        let stepped = (delta * 10.0).floor() / 10.0;
                        let wave = if stepped <= 0.5 {
                            stepped * 2.0
                        } else {
                            (1.0 - stepped) * 2.0
                        };
                        let opacity = 0.25 + wave * 0.75;
                        this.opacity(opacity)
                    },
                )
                .into_any_element()
        } else {
            skeleton.into_any_element()
        }
    }
}

impl Default for Skeleton {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveElement for Skeleton {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Skeleton {}

impl ParentElement for Skeleton {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements)
    }
}

impl Styled for Skeleton {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn animation_can_be_paused_without_hiding_the_placeholder() {
        let skeleton = Skeleton::new().animated(false);
        assert!(!skeleton.animated);
    }
}
