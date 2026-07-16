//! Layout transition - container that applies staggered entry animations to children.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::animations::{delayed_animation_progress, durations, easings, stagger_delay};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LayoutAnimation {
    #[default]
    FadeUp,
    FadeDown,
    SlideLeft,
    SlideRight,
    Scale,
}

#[derive(IntoElement)]
pub struct LayoutTransition {
    id: ElementId,
    children: Vec<AnyElement>,
    duration: Duration,
    stagger: Duration,
    animation: LayoutAnimation,
    version: usize,
    style: StyleRefinement,
}

impl LayoutTransition {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
            duration: durations::NORMAL,
            stagger: Duration::from_millis(50),
            animation: LayoutAnimation::default(),
            version: 0,
            style: StyleRefinement::default(),
        }
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn stagger(mut self, stagger: Duration) -> Self {
        self.stagger = stagger;
        self
    }

    pub fn animation(mut self, animation: LayoutAnimation) -> Self {
        self.animation = animation;
        self
    }

    pub fn version(mut self, version: usize) -> Self {
        self.version = version;
        self
    }
}

impl Styled for LayoutTransition {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for LayoutTransition {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for LayoutTransition {
    fn render(self, window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let duration = self.duration;
        let stagger = self.stagger;
        let animation = self.animation;
        let version = self.version;
        let animations_enabled = window.animations_enabled();

        div()
            .id(self.id)
            .children(
                self.children
                    .into_iter()
                    .enumerate()
                    .map(move |(idx, child)| {
                        let delay = stagger_delay(stagger, idx);
                        let total_duration = duration.saturating_add(delay);

                        let child = div()
                            .id(ElementId::Name(
                                format!("lt-child-{}-{}", idx, version).into(),
                            ))
                            .child(child);

                        if animations_enabled {
                            child
                                .with_animation(
                                    ElementId::Name(format!("lt-anim-{}-{}", idx, version).into()),
                                    Animation::new(total_duration)
                                        .with_easing(easings::ease_out_cubic),
                                    move |el, raw_delta| {
                                        let delta =
                                            delayed_animation_progress(raw_delta, delay, duration);

                                        match animation {
                                            LayoutAnimation::FadeUp => {
                                                el.opacity(delta).mt(px(-12.0 * (1.0 - delta)))
                                            }
                                            LayoutAnimation::FadeDown => {
                                                el.opacity(delta).mt(px(12.0 * (1.0 - delta)))
                                            }
                                            LayoutAnimation::SlideLeft => {
                                                el.opacity(delta).ml(px(20.0 * (1.0 - delta)))
                                            }
                                            LayoutAnimation::SlideRight => {
                                                el.opacity(delta).ml(px(-20.0 * (1.0 - delta)))
                                            }
                                            LayoutAnimation::Scale => {
                                                let scale_val = 0.8 + 0.2 * delta;
                                                el.opacity(delta).scale(scale_val)
                                            }
                                        }
                                    },
                                )
                                .into_any_element()
                        } else {
                            child.into_any_element()
                        }
                    }),
            )
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}
