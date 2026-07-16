use crate::animations::easings;
use kael::prelude::FluentBuilder as _;
use kael::*;
use std::time::Duration;

#[derive(IntoElement)]
pub struct PulseIndicator {
    base: Stateful<Div>,
    color: Option<Hsla>,
    dot_size: Pixels,
    speed: Duration,
    animation_repeat: Repeat,
    accessibility_label: Option<SharedString>,
}

impl PulseIndicator {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id.into()),
            color: None,
            dot_size: px(8.0),
            speed: Duration::from_secs(2),
            animation_repeat: Repeat::Forever,
            accessibility_label: None,
        }
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        let size = f32::from(size);
        self.dot_size = px(if size.is_finite() && size > 0.0 {
            size
        } else {
            8.0
        });
        self
    }

    pub fn speed(mut self, speed: Duration) -> Self {
        self.speed = if speed.is_zero() {
            Duration::from_secs(2)
        } else {
            speed
        };
        self
    }

    /// Give a meaningful status to assistive technology. By default the dot is decorative.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = (!label.trim().is_empty()).then_some(label);
        self
    }

    /// Limit the pulse to a finite number of cycles.
    pub fn animation_cycles(mut self, cycles: u32) -> Self {
        self.animation_repeat = Repeat::Count(cycles.max(1));
        self
    }
}

impl RenderOnce for PulseIndicator {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let color = self
            .color
            .unwrap_or_else(|| crate::theme::use_theme().tokens.success);
        let dot = self.dot_size;
        let ring_max = dot * 2.5;
        let speed = self.speed;
        let animation_repeat = self.animation_repeat;
        let ring = div()
            .absolute()
            .rounded_full()
            .bg(color.opacity(0.6))
            .size(dot);
        let ring = if window.animations_enabled() {
            ring.with_animation(
                "pulse-ring",
                Animation::new(speed)
                    .repeat(animation_repeat)
                    .with_easing(easings::ease_out_cubic),
                move |this, delta| {
                    let current_size = dot + (ring_max - dot) * delta;
                    let current_opacity = 0.6 * (1.0 - delta);
                    this.size(current_size).opacity(current_opacity)
                },
            )
            .into_any_element()
        } else {
            ring.into_any_element()
        };

        self.base
            .when_some(self.accessibility_label, |this, label| {
                this.accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Image).label(label),
                )
            })
            .flex()
            .items_center()
            .justify_center()
            .size(ring_max)
            .child(ring)
            .child(div().absolute().rounded_full().bg(color).size(dot))
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_animation_inputs_use_safe_defaults() {
        let indicator = PulseIndicator::new("status")
            .size(px(f32::NAN))
            .speed(Duration::ZERO)
            .accessibility_label("  ");

        assert_eq!(indicator.dot_size, px(8.0));
        assert_eq!(indicator.speed, Duration::from_secs(2));
        assert!(indicator.accessibility_label.is_none());
    }
}

impl Styled for PulseIndicator {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for PulseIndicator {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for PulseIndicator {}

impl ParentElement for PulseIndicator {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements)
    }
}
