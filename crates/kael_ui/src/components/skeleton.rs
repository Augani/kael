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

#[derive(IntoElement)]
pub struct Skeleton {
    base: Div,
    variant: SkeletonVariant,
    secondary: bool,
}

impl Skeleton {
    pub fn new() -> Self {
        Self {
            base: div(),
            variant: SkeletonVariant::default(),
            secondary: false,
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
}

impl RenderOnce for Skeleton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let radius_md = tokens.radius_md;

        let base_color = if self.secondary {
            tokens.muted_foreground.opacity(0.10)
        } else {
            tokens.muted_foreground.opacity(0.16)
        };

        self.base
            .when(self.variant == SkeletonVariant::Text, |this| {
                this.w_full().h(px(16.0)).rounded(radius_md)
            })
            .when(self.variant == SkeletonVariant::Circle, |this| {
                this.rounded_full()
            })
            .when(self.variant == SkeletonVariant::Rect, |this| {
                this.rounded(radius_md)
            })
            .bg(base_color)
            .with_animation(
                "skeleton-pulse",
                Animation::new(Duration::from_secs(2))
                    .repeat_forever()
                    .with_easing(ease_in_out),
                move |this, delta| {
                    let opacity = 1.0 - (delta * 0.3);
                    this.opacity(opacity)
                },
            )
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
