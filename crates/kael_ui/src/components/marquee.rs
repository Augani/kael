use kael::{prelude::FluentBuilder as _, *};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
struct MarqueePhase {
    last_raw_delta: Option<f32>,
    progress: f32,
    paused: bool,
}

impl MarqueePhase {
    fn advance(&mut self, raw_delta: f32) -> f32 {
        if !raw_delta.is_finite() {
            return self.progress;
        }

        let raw_delta = raw_delta.clamp(0.0, 1.0);
        match self.last_raw_delta.replace(raw_delta) {
            None if !self.paused => self.progress = raw_delta.rem_euclid(1.0),
            Some(previous) if !self.paused => {
                let mut step = raw_delta - previous;
                if step < 0.0 {
                    step += 1.0;
                }
                self.progress = (self.progress + step).rem_euclid(1.0);
            }
            _ => {}
        }
        self.progress
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MarqueeDirection {
    Left,
    Right,
}

#[derive(IntoElement)]
pub struct Marquee {
    id: ElementId,
    base: Div,
    speed: f32,
    direction: MarqueeDirection,
    pause_on_hover: bool,
    paused: bool,
    gap: Pixels,
    content_width: Pixels,
    accessibility_label: Option<SharedString>,
    render_content: Rc<dyn Fn() -> AnyElement>,
}

impl Marquee {
    pub fn new(
        id: impl Into<ElementId>,
        render_content: impl Fn() -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            base: div(),
            speed: 50.0,
            direction: MarqueeDirection::Left,
            pause_on_hover: false,
            paused: false,
            gap: px(32.0),
            content_width: px(1000.0),
            accessibility_label: None,
            render_content: Rc::new(render_content),
        }
    }

    pub fn speed(mut self, pixels_per_second: f32) -> Self {
        if pixels_per_second.is_finite() && pixels_per_second > 0.0 {
            self.speed = pixels_per_second;
        }
        self
    }

    pub fn direction(mut self, direction: MarqueeDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn pause_on_hover(mut self, pause: bool) -> Self {
        self.pause_on_hover = pause;
        self
    }

    /// Keep the content visible without scheduling the continuous animation.
    pub fn paused(mut self, paused: bool) -> Self {
        self.paused = paused;
        self
    }

    pub fn gap(mut self, gap: Pixels) -> Self {
        if f32::from(gap).is_finite() && gap >= px(0.0) {
            self.gap = gap;
        }
        self
    }

    pub fn content_width(mut self, width: Pixels) -> Self {
        if f32::from(width).is_finite() && width > px(0.0) {
            self.content_width = width;
        }
        self
    }

    /// Provide one stable description for duplicated scrolling content.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = (!label.trim().is_empty()).then_some(label);
        self
    }
}

impl Styled for Marquee {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for Marquee {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let total_travel = self.content_width + self.gap;
        let duration_ms = (total_travel / px(1.0) / self.speed * 1000.0) as u64;
        let duration = Duration::from_millis(duration_ms.max(100));
        let gap = self.gap;
        let content_width = self.content_width;
        let direction = self.direction;
        let should_animate = !self.paused && window.animations_enabled();
        let pause_on_hover = self.pause_on_hover;
        let phase = Rc::new(RefCell::new(MarqueePhase::default()));
        let root_id = self.id.clone();
        let accessibility_label = self.accessibility_label;
        let has_accessibility_label = accessibility_label.is_some();

        let copy_one = (self.render_content)();
        let copy_two = (self.render_content)();

        let track = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .w(content_width * 2.0 + gap * 2.0)
            .child(
                div()
                    .when(has_accessibility_label, |this| {
                        this.accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Group)
                                .states(AccessibilityState::HIDDEN),
                        )
                    })
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .w(content_width)
                    .mr(gap)
                    .child(copy_one),
            )
            .child(
                div()
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::Group)
                            .states(AccessibilityState::HIDDEN),
                    )
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .w(content_width)
                    .mr(gap)
                    .child(copy_two),
            );
        let track = if should_animate {
            let animation_phase = phase.clone();
            track
                .with_animation(
                    "marquee-scroll",
                    Animation::new(duration)
                        .repeat_forever()
                        .with_easing(kael::linear),
                    move |el, delta| {
                        let progress = animation_phase.borrow_mut().advance(delta);
                        let offset = total_travel * progress;
                        match direction {
                            MarqueeDirection::Left => el.left(-offset),
                            MarqueeDirection::Right => el.left(offset - total_travel),
                        }
                    },
                )
                .into_any_element()
        } else {
            track.into_any_element()
        };

        let hover_phase = phase;
        self.base
            .id(ElementId::NamedChild(
                Box::new(root_id),
                "marquee-container".into(),
            ))
            .overflow_hidden()
            .when_some(accessibility_label, |this, label| {
                this.accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::StaticText)
                        .label(label.to_string()),
                )
            })
            .on_hover(move |hovered: &bool, _window, _cx| {
                if pause_on_hover {
                    hover_phase.borrow_mut().paused = *hovered;
                }
            })
            .child(track)
    }
}

#[cfg(test)]
mod tests {
    use super::{Marquee, MarqueePhase};
    use kael::{IntoElement, div, px};

    #[test]
    fn phase_freezes_while_paused_and_resumes_without_a_jump() {
        let mut phase = MarqueePhase::default();
        assert!((phase.advance(0.2) - 0.2).abs() < f32::EPSILON);
        assert!((phase.advance(0.3) - 0.3).abs() < f32::EPSILON);

        phase.paused = true;
        assert!((phase.advance(0.6) - 0.3).abs() < f32::EPSILON);
        assert!((phase.advance(0.9) - 0.3).abs() < f32::EPSILON);

        phase.paused = false;
        assert!((phase.advance(0.95) - 0.35).abs() < f32::EPSILON);
    }

    #[test]
    fn phase_handles_repeat_wraparound() {
        let mut phase = MarqueePhase::default();
        phase.advance(0.9);
        assert!((phase.advance(0.1) - 0.1).abs() < f32::EPSILON);
        assert!(phase.advance(f32::NAN).is_finite());
    }

    #[test]
    fn invalid_motion_metrics_keep_safe_defaults() {
        let marquee = Marquee::new("marquee", || div().into_any_element())
            .speed(f32::NAN)
            .gap(px(-1.0))
            .content_width(px(f32::INFINITY));

        assert_eq!(marquee.speed, 50.0);
        assert_eq!(marquee.gap, px(32.0));
        assert_eq!(marquee.content_width, px(1000.0));
    }
}
