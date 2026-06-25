use crate::{
    AnyElement, App, Bounds, ClipShape, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};

/// Clip a child subtree to an arbitrary [`ClipShape`] (circle, ellipse, or convex polygon).
///
/// The shape's coordinates are interpreted relative to this element's top-left, so it moves
/// and lays out with the child. Circles (and equal-radius ellipses) clip exactly through the
/// shader-backed rounded-clip path; other shapes currently clip to their bounding box — see
/// [`Window::with_clip_path`].
///
/// ```no_run
/// # use kael::{clip_path, ClipShape, point, px, div, ParentElement};
/// let masked = clip_path(
///     ClipShape::Circle { center: point(px(40.0), px(40.0)), radius: px(40.0) },
///     div().child("clipped to a circle"),
/// );
/// ```
pub fn clip_path(shape: ClipShape, child: impl IntoElement) -> ClipPathElement {
    ClipPathElement {
        shape,
        child: child.into_any_element(),
    }
}

/// A wrapper element that clips its child to a [`ClipShape`]. Created by [`clip_path`].
pub struct ClipPathElement {
    shape: ClipShape,
    child: AnyElement,
}

impl Element for ClipPathElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let shape = self.shape.translate(bounds.origin);
        window.with_clip_path(&shape, |window| {
            self.child.paint(window, cx);
        });
    }
}

impl IntoElement for ClipPathElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}
