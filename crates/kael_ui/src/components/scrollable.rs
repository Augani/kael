//! Scrollable component with visible scrollbars.

use super::scrollbar::{Scrollbar, ScrollbarAxis, ScrollbarState};
use kael::{
    AnyElement, App, Bounds, Div, Element, ElementId, GlobalElementId, InspectorElementId,
    InteractiveElement, Interactivity, IntoElement, LayoutId, ParentElement, Pixels, Position,
    ScrollHandle, SharedString, Size, Stateful, StatefulInteractiveElement, Style, StyleRefinement,
    Styled, Window, div, relative,
};
use std::panic::Location;

/// A scroll view with visible scrollbars
pub struct Scrollable<E> {
    id: ElementId,
    element: Option<E>,
    axis: ScrollbarAxis,
    always_show_scrollbars: bool,
    external_scroll_handle: Option<ScrollHandle>,
    scroll_size: Option<Size<Pixels>>,
    base: Stateful<Div>,
}

impl<E> Scrollable<E>
where
    E: Element,
{
    #[track_caller]
    pub(crate) fn new(axis: ScrollbarAxis, element: E) -> Self {
        let id = if let Some(element_id) = element.id() {
            ElementId::Name(SharedString::from(format!(
                "scrollable-{axis:?}-{element_id:?}"
            )))
        } else {
            let location = Location::caller();
            ElementId::Name(SharedString::from(format!(
                "scrollable-{axis:?}-{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )))
        };

        Self {
            element: Some(element),
            base: div().id("scrollable-base"),
            id,
            axis,
            always_show_scrollbars: false,
            external_scroll_handle: None,
            scroll_size: None,
        }
    }

    #[track_caller]
    pub fn vertical(element: E) -> Self {
        Self::new(ScrollbarAxis::Vertical, element)
    }

    #[track_caller]
    pub fn horizontal(element: E) -> Self {
        Self::new(ScrollbarAxis::Horizontal, element)
    }

    #[track_caller]
    pub fn both(element: E) -> Self {
        Self::new(ScrollbarAxis::Both, element)
    }

    /// Assign a stable identity to this scroll view.
    ///
    /// This is required when multiple scroll views are created from anonymous
    /// elements at the same level. Without distinct IDs, their retained scroll
    /// state can alias and cause one viewport to inherit another viewport's
    /// offset or scrollbar state.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn always_show_scrollbars(mut self) -> Self {
        self.always_show_scrollbars = true;
        self
    }

    pub fn with_scroll_handle(mut self, handle: ScrollHandle) -> Self {
        self.external_scroll_handle = Some(handle);
        self
    }

    /// Supplies the complete content size when it is known ahead of layout.
    ///
    /// This is useful for virtualized or independently measured content and
    /// guarantees correct thumb geometry on the first rendered frame.
    pub fn scroll_size(mut self, size: Size<Pixels>) -> Self {
        if f32::from(size.width).is_finite()
            && f32::from(size.height).is_finite()
            && size.width >= kael::px(0.0)
            && size.height >= kael::px(0.0)
        {
            self.scroll_size = Some(size);
        }
        self
    }

    fn with_element_state<R>(
        &mut self,
        id: &GlobalElementId,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut ScrollViewState, &mut Window, &mut App) -> R,
    ) -> R {
        window.with_optional_element_state::<ScrollViewState, _>(
            Some(id),
            |element_state, window| {
                let mut element_state = element_state.unwrap().unwrap_or_default();
                let result = f(self, &mut element_state, window, cx);
                (result, Some(element_state))
            },
        )
    }
}

pub struct ScrollViewState {
    state: ScrollbarState,
    handle: ScrollHandle,
}

impl Default for ScrollViewState {
    fn default() -> Self {
        Self {
            handle: ScrollHandle::new(),
            state: ScrollbarState::default(),
        }
    }
}

impl<E> ParentElement for Scrollable<E>
where
    E: Element + ParentElement,
{
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        if let Some(element) = &mut self.element {
            element.extend(elements);
        }
    }
}

impl<E> Styled for Scrollable<E>
where
    E: Element + Styled,
{
    fn style(&mut self) -> &mut StyleRefinement {
        if let Some(element) = &mut self.element {
            element.style()
        } else {
            self.base.style()
        }
    }
}

impl<E> InteractiveElement for Scrollable<E>
where
    E: Element + InteractiveElement,
{
    fn interactivity(&mut self) -> &mut Interactivity {
        if let Some(element) = &mut self.element {
            element.interactivity()
        } else {
            self.base.interactivity()
        }
    }
}

impl<E> StatefulInteractiveElement for Scrollable<E> where E: Element + StatefulInteractiveElement {}

impl<E> IntoElement for Scrollable<E>
where
    E: Element,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E> Element for Scrollable<E>
where
    E: Element,
{
    type RequestLayoutState = AnyElement;
    type PrepaintState = ScrollViewState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style {
            flex_grow: 1.0,
            position: Position::Relative,
            size: kael::Size {
                width: relative(1.0).into(),
                height: relative(1.0).into(),
            },
            ..Style::default()
        };

        let axis = self.axis;
        let scroll_id = self.id.clone();
        let content = self.element.take().map(|c| c.into_any_element());
        let always_show = self.always_show_scrollbars;
        let scroll_size = self.scroll_size;

        self.with_element_state(
            id.unwrap(),
            window,
            cx,
            |scrollable, element_state, window, cx| {
                let scroll_handle =
                    if let Some(ref external_handle) = scrollable.external_scroll_handle {
                        external_handle
                    } else {
                        &element_state.handle
                    };

                let mut scrollbar = Scrollbar::new(axis, &element_state.state, scroll_handle);
                if always_show {
                    scrollbar = scrollbar.always_visible();
                }
                if let Some(scroll_size) = scroll_size {
                    scrollbar = scrollbar.scroll_size(scroll_size);
                }

                let mut element = div()
                    .relative()
                    .size_full()
                    .overflow_hidden()
                    .child(
                        div()
                            .id(scroll_id)
                            .track_scroll(scroll_handle)
                            .overflow_scroll()
                            .relative()
                            .size_full()
                            .child(div().children(content)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .bottom_0()
                            .child(scrollbar),
                    )
                    .into_any_element();

                let element_id = element.request_layout(window, cx);
                let layout_id = window.request_layout(style, vec![element_id], cx);

                (layout_id, element)
            },
        )
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        element.prepaint(window, cx);
        ScrollViewState::default()
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        element: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        element.paint(window, cx)
    }
}

#[track_caller]
pub fn scrollable_vertical<E>(element: E) -> Scrollable<E>
where
    E: Element,
{
    Scrollable::vertical(element)
}

#[track_caller]
pub fn scrollable_horizontal<E>(element: E) -> Scrollable<E>
where
    E: Element,
{
    Scrollable::horizontal(element)
}

#[track_caller]
pub fn scrollable_both<E>(element: E) -> Scrollable<E>
where
    E: Element,
{
    Scrollable::both(element)
}

#[cfg(test)]
mod tests {
    use super::Scrollable;
    use kael::{Element, ElementId, div, px, size};

    #[test]
    fn explicit_ids_keep_sibling_scroll_views_distinct() {
        let hours = Scrollable::vertical(div()).id("hours");
        let minutes = Scrollable::vertical(div()).id("minutes");

        assert_eq!(Element::id(&hours), Some(ElementId::from("hours")));
        assert_eq!(Element::id(&minutes), Some(ElementId::from("minutes")));
        assert_ne!(Element::id(&hours), Element::id(&minutes));
    }

    #[test]
    fn anonymous_scroll_views_get_stable_callsite_ids() {
        let first = Scrollable::vertical(div());
        let second = Scrollable::vertical(div());

        assert_ne!(Element::id(&first), Element::id(&second));
    }

    #[test]
    fn invalid_explicit_scroll_sizes_are_ignored() {
        let view = Scrollable::both(div()).scroll_size(size(px(f32::NAN), px(100.0)));
        assert_eq!(view.scroll_size, None);

        let view = view.scroll_size(size(px(800.0), px(500.0)));
        assert_eq!(view.scroll_size, Some(size(px(800.0), px(500.0))));
    }
}
