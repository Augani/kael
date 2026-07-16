use kael::*;
use std::time::Duration;

#[derive(IntoElement)]
pub struct Shimmer {
    base: Div,
    duration: Duration,
    animation_repeat: Repeat,
}

impl Shimmer {
    pub fn new() -> Self {
        Self {
            base: div(),
            duration: Duration::from_millis(1500),
            animation_repeat: Repeat::Forever,
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = if duration.is_zero() {
            Duration::from_millis(1500)
        } else {
            duration
        };
        self
    }

    /// Limit the shimmer to a finite number of sweeps.
    pub fn animation_cycles(mut self, cycles: u32) -> Self {
        self.animation_repeat = Repeat::Count(cycles.max(1));
        self
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn zero_duration_uses_the_safe_default() {
        assert_eq!(
            Shimmer::new().duration(Duration::ZERO).duration,
            Duration::from_millis(1500)
        );
    }
}

impl Default for Shimmer {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for Shimmer {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let duration = self.duration;
        let animation_repeat = self.animation_repeat;

        let sweep = div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(200.0))
            .bg(kael::linear_gradient(
                90.0,
                kael::linear_color_stop(kael::transparent_black(), 0.0),
                kael::linear_color_stop(hsla(0.0, 0.0, 1.0, 0.15), 1.0),
            ))
            .with_animation(
                "shimmer-sweep",
                Animation::new(duration)
                    .repeat(animation_repeat)
                    .with_easing(kael::linear),
                move |this, delta| {
                    let start = px(-200.0);
                    let end = px(600.0);
                    let current = start + (end - start) * delta;
                    this.left(current)
                },
            );

        self.base
            .relative()
            .overflow_hidden()
            .child(if window.animations_enabled() {
                sweep.into_any_element()
            } else {
                div().into_any_element()
            })
    }
}

impl Styled for Shimmer {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Shimmer {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Shimmer {}

impl ParentElement for Shimmer {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.base.extend(elements)
    }
}
