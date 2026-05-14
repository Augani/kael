use crate::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
    cache::{SubtreeCacheKey, SubtreeCacheState},
};
use collections::FxHashSet;
use std::mem;

#[track_caller]
/// Reuses a child subtree's previous prepaint and paint output until one of its tracked entities changes.
pub fn cached(child: impl IntoElement) -> Cached {
    Cached {
        child: Some(child.into_any_element()),
        element_id: None,
        source_location: core::panic::Location::caller(),
    }
}

/// A cache wrapper that replays a previously rendered child subtree when its dependencies are unchanged.
pub struct Cached {
    child: Option<AnyElement>,
    element_id: Option<ElementId>,
    source_location: &'static core::panic::Location<'static>,
}

impl Cached {
    /// Overrides the cache key used to preserve this cached subtree across frames.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.element_id = Some(id.into());
        self
    }
}

impl Element for Cached {
    type RequestLayoutState = (Option<AnyElement>, FxHashSet<EntityId>);
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        Some(
            self.element_id
                .clone()
                .unwrap_or_else(|| ElementId::CodeLocation(*self.source_location)),
        )
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let ((layout_id, element), accessed_entities) = cx.detect_accessed_entities(|cx| {
            let mut element = self.child.take().unwrap();
            let layout_id = element.request_layout(window, cx);
            (layout_id, element)
        });

        (layout_id, (Some(element), accessed_entities))
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let cache_key = SubtreeCacheKey {
            bounds,
            content_mask: window.content_mask(),
            global_generation: cx.global_generation(),
            text_style: window.text_style(),
        };
        let caching_disabled = window.is_inspector_picking(cx);

        window.with_element_state::<SubtreeCacheState, _>(
            global_id.unwrap(),
            |element_state, window| {
                if !caching_disabled
                    && !window.refreshing
                    && let Some(mut element_state) = element_state
                    && element_state.is_reusable(&cache_key, cx)
                {
                    let prepaint_start = window.prepaint_index();
                    window.reuse_prepaint(element_state.prepaint_range.clone());
                    cx.entities
                        .extend_accessed(&element_state.accessed_entities);
                    let prepaint_end = window.prepaint_index();
                    element_state.prepaint_range = prepaint_start..prepaint_end;
                    return (None, element_state);
                }

                let mut accessed_entities = mem::take(&mut request_layout.1);
                let mut element = request_layout.0.take().unwrap();

                let prepaint_start = window.prepaint_index();
                let (_, prepaint_accessed_entities) = cx.detect_accessed_entities(|cx| {
                    element.prepaint(window, cx);
                });
                let prepaint_end = window.prepaint_index();

                accessed_entities.extend(prepaint_accessed_entities);

                (
                    Some(element),
                    SubtreeCacheState::new(
                        accessed_entities,
                        cache_key,
                        prepaint_start..prepaint_end,
                        cx,
                    ),
                )
            },
        )
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_element_state::<SubtreeCacheState, _>(
            global_id.unwrap(),
            |element_state, window| {
                let mut element_state = element_state.unwrap();
                let paint_start = window.paint_index();

                if let Some(element) = prepaint {
                    element.paint(window, cx);
                    let paint_end = window.paint_index();
                    let paint_scene_range = paint_start.scene_index..paint_end.scene_index;
                    element_state.paint_range = paint_start..paint_end;
                    element_state.prepare_surface(
                        global_id.unwrap(),
                        bounds,
                        window.scale_factor(),
                        window.sprite_atlas(),
                    );
                    if let Some(surface) = element_state.surface.as_ref() {
                        window.request_cached_surface_snapshot(paint_scene_range, surface);
                    }
                } else {
                    if let Some(surface) = element_state.surface.as_ref() {
                        window.reuse_paint_without_scene(element_state.paint_range.clone());
                        window.paint_cached_surface(surface);
                    } else {
                        window.reuse_paint(element_state.paint_range.clone());
                    }

                    let paint_end = window.paint_index();
                    element_state.paint_range = paint_start..paint_end;
                }

                ((), element_state)
            },
        )
    }
}

impl IntoElement for Cached {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::cached;
    use crate::{
        AppContext, Bounds, Context, Element, Global, GlobalElementId, InspectorElementId,
        IntoElement, LayoutId, ParentElement, Render, Style, Styled, TestAppContext, VisualContext,
        Window, div,
    };
    use crate::{px, size};
    use std::{cell::Cell, rc::Rc};

    struct Model(usize);

    struct ThemeEpoch(usize);

    impl Global for ThemeEpoch {}

    struct ProbeElement {
        model: crate::Entity<Model>,
        prepaint_count: Rc<Cell<usize>>,
        paint_count: Rc<Cell<usize>>,
    }

    impl Element for ProbeElement {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<crate::ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            _cx: &mut crate::App,
        ) -> (LayoutId, Self::RequestLayoutState) {
            (
                window
                    .request_measured_layout(Style::default(), |_, _, _, _| size(px(20.), px(20.))),
                (),
            )
        }

        fn prepaint(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<crate::Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _window: &mut Window,
            cx: &mut crate::App,
        ) -> Self::PrepaintState {
            let _ = self.model.read(cx).0;
            self.prepaint_count.set(self.prepaint_count.get() + 1);
        }

        fn paint(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            bounds: Bounds<crate::Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _prepaint: &mut Self::PrepaintState,
            window: &mut Window,
            _cx: &mut crate::App,
        ) {
            window.paint_quad(crate::fill(bounds, crate::black()));
            self.paint_count.set(self.paint_count.get() + 1);
        }
    }

    impl IntoElement for ProbeElement {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    struct ObservedElement {
        model: crate::Entity<Model>,
    }

    impl Element for ObservedElement {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<crate::ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            _cx: &mut crate::App,
        ) -> (LayoutId, Self::RequestLayoutState) {
            (
                window
                    .request_measured_layout(Style::default(), |_, _, _, _| size(px(20.), px(20.))),
                (),
            )
        }

        fn prepaint(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<crate::Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _window: &mut Window,
            cx: &mut crate::App,
        ) -> Self::PrepaintState {
            let _ = self.model.read(cx).0;
        }

        fn paint(
            &mut self,
            _global_id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<crate::Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _prepaint: &mut Self::PrepaintState,
            _window: &mut Window,
            _cx: &mut crate::App,
        ) {
        }
    }

    impl IntoElement for ObservedElement {
        type Element = Self;

        fn into_element(self) -> Self::Element {
            self
        }
    }

    struct TestView {
        cached_model: crate::Entity<Model>,
        other_model: crate::Entity<Model>,
        prepaint_count: Rc<Cell<usize>>,
        paint_count: Rc<Cell<usize>>,
    }

    impl Render for TestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .w_full()
                .h_full()
                .child(
                    cached(div().w(px(20.)).h(px(20.)).child(ProbeElement {
                        model: self.cached_model.clone(),
                        prepaint_count: self.prepaint_count.clone(),
                        paint_count: self.paint_count.clone(),
                    }))
                    .id("cached-probe"),
                )
                .child(ObservedElement {
                    model: self.other_model.clone(),
                })
        }
    }

    #[kael::test]
    fn cached_subtree_reuses_prepaint_and_paint_when_dependencies_are_unchanged(
        cx: &mut TestAppContext,
    ) {
        let prepaint_count = Rc::new(Cell::new(0));
        let paint_count = Rc::new(Cell::new(0));
        let cached_model = cx.new(|_| Model(0));
        let other_model = cx.new(|_| Model(0));

        let (_view, cx) = cx.add_window_view(|_, _| TestView {
            cached_model: cached_model.clone(),
            other_model: other_model.clone(),
            prepaint_count: prepaint_count.clone(),
            paint_count: paint_count.clone(),
        });

        cx.update(|window, _| {
            assert_eq!(window.rendered_scene().cached_surface_snapshots.len(), 1);
            assert_eq!(window.rendered_scene().polychrome_sprites.len(), 0);
        });

        assert_eq!(prepaint_count.get(), 1);
        assert_eq!(paint_count.get(), 1);

        other_model.update(cx, |model, cx| {
            model.0 += 1;
            cx.notify();
        });

        cx.update(|window, _| {
            assert_eq!(window.rendered_scene().cached_surface_snapshots.len(), 0);
            assert_eq!(window.rendered_scene().polychrome_sprites.len(), 1);
        });

        assert_eq!(prepaint_count.get(), 1);
        assert_eq!(paint_count.get(), 1);

        cached_model.update(cx, |model, cx| {
            model.0 += 1;
            cx.notify();
        });

        assert_eq!(prepaint_count.get(), 2);
        assert_eq!(paint_count.get(), 2);

        cx.update(|window, _| {
            assert_eq!(window.rendered_scene().cached_surface_snapshots.len(), 1);
            assert_eq!(window.rendered_scene().polychrome_sprites.len(), 0);
        });

        assert_eq!(prepaint_count.get(), 2);
        assert_eq!(paint_count.get(), 2);
    }

    #[kael::test]
    fn cached_subtree_invalidates_when_globals_change(cx: &mut TestAppContext) {
        cx.set_global(ThemeEpoch(0));

        let prepaint_count = Rc::new(Cell::new(0));
        let paint_count = Rc::new(Cell::new(0));
        let cached_model = cx.new(|_| Model(0));
        let other_model = cx.new(|_| Model(0));

        let (_view, cx) = cx.add_window_view(|_, _| TestView {
            cached_model: cached_model.clone(),
            other_model: other_model.clone(),
            prepaint_count: prepaint_count.clone(),
            paint_count: paint_count.clone(),
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(prepaint_count.get(), 1);
        assert_eq!(paint_count.get(), 1);

        cx.update_global::<ThemeEpoch, _>(|theme, _| {
            theme.0 += 1;
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(prepaint_count.get(), 2);
        assert_eq!(paint_count.get(), 2);
    }

    #[kael::test]
    fn cached_subtree_can_be_explicitly_invalidated(cx: &mut TestAppContext) {
        let prepaint_count = Rc::new(Cell::new(0));
        let paint_count = Rc::new(Cell::new(0));
        let cached_model = cx.new(|_| Model(0));
        let other_model = cx.new(|_| Model(0));

        let (_view, cx) = cx.add_window_view(|_, _| TestView {
            cached_model: cached_model.clone(),
            other_model: other_model.clone(),
            prepaint_count: prepaint_count.clone(),
            paint_count: paint_count.clone(),
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(prepaint_count.get(), 1);
        assert_eq!(paint_count.get(), 1);

        cx.invalidate_cache("cached-probe").unwrap();

        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(prepaint_count.get(), 2);
        assert_eq!(paint_count.get(), 2);
    }
}
