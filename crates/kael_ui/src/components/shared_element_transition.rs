//! Shared element (hero) transition between views.

use kael::{prelude::FluentBuilder as _, *};
use std::time::{Duration, Instant};

use crate::animations::{durations, easings, lerp_pixels};

#[derive(Clone, Copy, Debug, PartialEq)]
struct ElementBounds {
    x: Pixels,
    y: Pixels,
    width: Pixels,
    height: Pixels,
}

impl Default for ElementBounds {
    fn default() -> Self {
        Self {
            x: px(0.0),
            y: px(0.0),
            width: px(0.0),
            height: px(0.0),
        }
    }
}

pub struct SharedElementState {
    source_bounds: Option<ElementBounds>,
    target_bounds: Option<ElementBounds>,
    is_transitioning: bool,
    progress: f32,
    version: usize,
    duration: Duration,
    transition_started_at: Option<Instant>,
}

impl SharedElementState {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            source_bounds: None,
            target_bounds: None,
            is_transitioning: false,
            progress: 0.0,
            version: 0,
            duration: durations::SLOW,
            transition_started_at: None,
        }
    }

    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = duration;
    }

    pub fn set_source_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.source_bounds = sanitized_bounds(bounds);
    }

    pub fn set_target_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.target_bounds = sanitized_bounds(bounds);
    }

    pub fn transition_to(&mut self, cx: &mut Context<Self>) {
        if self.source_bounds.is_none() || self.target_bounds.is_none() {
            return;
        }

        self.is_transitioning = true;
        self.progress = 0.0;
        self.version = self.version.wrapping_add(1);
        self.transition_started_at = Some(Instant::now());
        let version = self.version;
        cx.notify();

        if cx.reduce_motion() || self.duration.is_zero() {
            self.complete_transition(cx);
            return;
        }

        self.schedule_tick(version, cx);
    }

    fn schedule_tick(&self, version: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            _ = this.update(cx, |state, cx| {
                if !state.is_transitioning || state.version != version {
                    return;
                }

                let elapsed = state
                    .transition_started_at
                    .map_or(state.duration, |started| started.elapsed());
                state.progress =
                    (elapsed.as_secs_f32() / state.duration.as_secs_f32()).clamp(0.0, 1.0);

                if state.progress >= 1.0 {
                    state.complete_transition(cx);
                } else {
                    state.schedule_tick(version, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn complete_transition(&mut self, cx: &mut Context<Self>) {
        self.is_transitioning = false;
        self.progress = 1.0;
        self.source_bounds = self.target_bounds;
        self.transition_started_at = None;
        cx.notify();
    }

    pub fn is_transitioning(&self) -> bool {
        self.is_transitioning
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn version(&self) -> usize {
        self.version
    }
}

fn sanitized_bounds(bounds: Bounds<Pixels>) -> Option<ElementBounds> {
    let values = [
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    ];
    if values.iter().all(|value| value.is_finite())
        && bounds.size.width >= px(0.0)
        && bounds.size.height >= px(0.0)
    {
        Some(ElementBounds {
            x: bounds.origin.x,
            y: bounds.origin.y,
            width: bounds.size.width,
            height: bounds.size.height,
        })
    } else {
        None
    }
}

#[derive(IntoElement)]
pub struct SharedElementTransition {
    id: ElementId,
    state: Entity<SharedElementState>,
    content: Option<AnyElement>,
    style: StyleRefinement,
}

impl SharedElementTransition {
    pub fn new(id: impl Into<ElementId>, state: Entity<SharedElementState>) -> Self {
        Self {
            id: id.into(),
            state,
            content: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn content(mut self, element: impl IntoElement) -> Self {
        self.content = Some(element.into_any_element());
        self
    }
}

impl Styled for SharedElementTransition {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SharedElementTransition {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let state = self.state.read(cx);
        let is_transitioning = state.is_transitioning;
        let version = state.version;
        let source = state.source_bounds.unwrap_or_default();
        let target = state.target_bounds.unwrap_or_default();
        let duration = state.duration;
        let content = self.content;

        if is_transitioning {
            div()
                .id(self.id.clone())
                .absolute()
                .left(source.x)
                .top(source.y)
                .w(source.width)
                .h(source.height)
                .children(content)
                .map(|this| {
                    let mut el = this;
                    el.style().refine(&user_style);
                    el
                })
                .with_animation(
                    ElementId::Name(format!("set-{}-{}", self.id, version).into()),
                    Animation::new(duration).with_easing(easings::ease_in_out_cubic),
                    move |el, delta| {
                        let curr_x = lerp_pixels(source.x, target.x, delta);
                        let curr_y = lerp_pixels(source.y, target.y, delta);
                        let curr_w = lerp_pixels(source.width, target.width, delta);
                        let curr_h = lerp_pixels(source.height, target.height, delta);

                        el.left(curr_x).top(curr_y).w(curr_w).h(curr_h)
                    },
                )
                .into_any_element()
        } else if state.source_bounds.is_some() {
            div()
                .id(self.id)
                .absolute()
                .left(source.x)
                .top(source.y)
                .w(source.width)
                .h(source.height)
                .children(content)
                .map(|this| {
                    let mut el = this;
                    el.style().refine(&user_style);
                    el
                })
                .into_any_element()
        } else {
            div()
                .id(self.id)
                .children(content)
                .map(|this| {
                    let mut el = this;
                    el.style().refine(&user_style);
                    el
                })
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_bounds_are_not_used_for_transitions() {
        let mut cx = TestAppContext::single();
        let state = cx.new(SharedElementState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_source_bounds(Bounds {
                    origin: point(px(f32::NAN), px(0.0)),
                    size: size(px(20.0), px(20.0)),
                });
                state.set_target_bounds(Bounds {
                    origin: point(px(20.0), px(20.0)),
                    size: size(px(40.0), px(40.0)),
                });
                state.transition_to(cx);
                assert!(!state.is_transitioning());
            });
        });
    }

    #[::core::prelude::v1::test]
    fn reduced_motion_completes_the_transition_immediately() {
        let mut cx = TestAppContext::single();
        cx.set_reduce_motion(true);
        let state = cx.new(SharedElementState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_source_bounds(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: size(px(20.0), px(20.0)),
                });
                state.set_target_bounds(Bounds {
                    origin: point(px(20.0), px(20.0)),
                    size: size(px(40.0), px(40.0)),
                });
                state.transition_to(cx);
                assert!(!state.is_transitioning());
                assert_eq!(state.progress(), 1.0);
            });
        });
    }

    #[::core::prelude::v1::test]
    fn zero_duration_completes_the_transition_immediately() {
        let mut cx = TestAppContext::single();
        let state = cx.new(SharedElementState::new);

        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_duration(Duration::ZERO);
                state.set_source_bounds(Bounds {
                    origin: point(px(4.0), px(8.0)),
                    size: size(px(20.0), px(20.0)),
                });
                state.set_target_bounds(Bounds {
                    origin: point(px(40.0), px(60.0)),
                    size: size(px(80.0), px(50.0)),
                });
                state.transition_to(cx);
                assert!(!state.is_transitioning());
                assert_eq!(state.progress(), 1.0);
                assert_eq!(state.source_bounds, state.target_bounds);
            });
        });
    }
}
