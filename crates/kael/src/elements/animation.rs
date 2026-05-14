use std::{cell::Cell, rc::Rc, time::Instant};

use crate::{
    AnyElement, App, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement, Styled,
    Window,
};

pub use crate::animation::easing::*;
pub use crate::animation::{
    Animation, AnimationSequence, Easing, Keyframes, Repeat, StyledKeyframe, keyframes,
};
use smallvec::SmallVec;

/// A handle that can be used to cancel an in-flight animation.
#[derive(Clone)]
pub struct AnimationHandle {
    cancelled: Rc<Cell<bool>>,
}

impl AnimationHandle {
    fn new() -> Self {
        Self {
            cancelled: Rc::new(Cell::new(false)),
        }
    }

    /// Cancel the animation, causing it to jump to its final state.
    pub fn cancel(&self) {
        self.cancelled.set(true);
    }

    /// Whether the animation has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

/// An extension trait for adding the animation wrapper to both Elements and Components
pub trait AnimationExt {
    /// Render this component or element with an animation
    fn with_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(move |this, _, value| animator(this, value)),
            animations: smallvec::smallvec![animation],
            cancel_handle: None,
        }
    }

    /// Render this component or element with a chain of animations
    fn with_animations(
        self,
        id: impl Into<ElementId>,
        animations: Vec<Animation>,
        animator: impl Fn(Self, usize, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(animator),
            animations: animations.into(),
            cancel_handle: None,
        }
    }

    /// Render this component or element with a scheduled animation sequence.
    fn with_animation_sequence(
        self,
        id: impl Into<ElementId>,
        sequence: AnimationSequence,
        animator: impl Fn(Self, usize, f32) -> Self + 'static,
    ) -> AnimationElement<Self>
    where
        Self: Sized,
    {
        self.with_animations(id, sequence.into_animations(), animator)
    }

    /// Render this styled element with keyframe-driven transforms.
    fn with_keyframes(
        self,
        id: impl Into<ElementId>,
        keyframes: Keyframes,
        animation: Animation,
    ) -> AnimationElement<Self>
    where
        Self: Sized + Styled + 'static,
    {
        self.with_animation(id, animation, move |element, delta| {
            keyframes.apply(element, delta)
        })
    }

    /// Render this styled element with keyframes using the compact explicit animation API.
    fn animation(
        self,
        id: impl Into<ElementId>,
        keyframes: Keyframes,
        animation: Animation,
    ) -> AnimationElement<Self>
    where
        Self: Sized + Styled + 'static,
    {
        self.with_keyframes(id, keyframes, animation)
    }

    /// Render this component or element with a cancellable animation.
    /// Returns the animated element and a handle that can be used to cancel the animation.
    fn with_cancellable_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> (AnimationElement<Self>, AnimationHandle)
    where
        Self: Sized,
    {
        let handle = AnimationHandle::new();
        let element = AnimationElement {
            id: id.into(),
            element: Some(self),
            animator: Box::new(move |this, _, value| animator(this, value)),
            animations: smallvec::smallvec![animation],
            cancel_handle: Some(handle.cancelled.clone()),
        };
        (element, handle)
    }
}

impl<E: IntoElement + 'static> AnimationExt for E {}

/// A GPUI element that applies an animation to another element
pub struct AnimationElement<E> {
    id: ElementId,
    element: Option<E>,
    animations: SmallVec<[Animation; 1]>,
    animator: Box<dyn Fn(E, usize, f32) -> E + 'static>,
    cancel_handle: Option<Rc<Cell<bool>>>,
}

impl<E> AnimationElement<E> {
    /// Returns a new [`AnimationElement<E>`] after applying the given function
    /// to the element being animated.
    pub fn map_element(mut self, f: impl FnOnce(E) -> E) -> AnimationElement<E> {
        self.element = self.element.map(f);
        self
    }
}

impl<E: IntoElement + 'static> IntoElement for AnimationElement<E> {
    type Element = AnimationElement<E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

struct AnimationState {
    start: Instant,
}

impl<E: IntoElement + 'static> Element for AnimationElement<E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (crate::LayoutId, Self::RequestLayoutState) {
        window.with_element_state(global_id.unwrap(), |state, window| {
            let state = state.unwrap_or_else(|| AnimationState {
                start: Instant::now(),
            });

            let cancelled = self.cancel_handle.as_ref().map_or(false, |h| h.get());
            let elapsed = state.start.elapsed();
            let element = self.element.take().expect("should only be called once");
            let (element, done) = if cancelled {
                let element = self
                    .animations
                    .iter()
                    .enumerate()
                    .fold(element, |element, (animation_ix, _)| {
                        (self.animator)(element, animation_ix, 1.0)
                    });
                (element, true)
            } else {
                let mut element = element;
                let mut done = true;

                for (animation_ix, animation) in self.animations.iter().enumerate() {
                    let sample = animation.sample(elapsed);
                    if sample.started || sample.finished {
                        debug_assert!(
                            (0.0..=1.0).contains(&sample.delta),
                            "delta should always be between 0 and 1"
                        );
                        element = (self.animator)(element, animation_ix, sample.delta);
                    }
                    if !sample.finished {
                        done = false;
                    }
                }

                (element, done)
            };
            let mut element = element.into_any_element();

            if !done {
                window.request_animation_frame();
            }

            ((element.request_layout(window, cx), element), state)
        })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: crate::Bounds<crate::Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx);
    }
}
