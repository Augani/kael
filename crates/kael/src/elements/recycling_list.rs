use super::list::list_with_recycling;
use crate::{
    AnyElement, App, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, List, ListAlignment, ListPrepaintState, ListSizingBehavior, ListState, Pixels,
    StyleRefinement, Styled, Window, px,
};
use std::{any::TypeId, cell::RefCell, collections::HashMap, rc::Rc};

const DEFAULT_OVERDRAW_PX: f32 = 200.0;
const DEFAULT_MAX_POOLED_ITEMS_PER_TYPE: usize = 8;

/// Lazily render a heterogeneous list with estimated heights for off-screen items.
///
/// The `id` must remain stable across frames to preserve measured heights and scroll position.
#[track_caller]
pub fn recycling_list<D>(id: impl Into<ElementId>, delegate: D) -> RecyclingList<D>
where
    D: ListDelegate,
{
    RecyclingList {
        element_id: id.into(),
        delegate: Rc::new(delegate),
        style: StyleRefinement::default(),
        sizing_behavior: ListSizingBehavior::default(),
        alignment: ListAlignment::Top,
        overdraw: px(DEFAULT_OVERDRAW_PX),
    }
}

/// Supplies item counts, estimated heights, and item rendering for a [`RecyclingList`].
pub trait ListDelegate: 'static {
    /// Return the current number of items in the list.
    fn item_count(&self) -> usize;

    /// Return the estimated height for an item that has not been measured yet.
    fn estimated_item_height(&self, ix: usize) -> Pixels;

    /// Render the item at the given index.
    fn render_item(&self, ix: usize, window: &mut Window, cx: &mut App) -> AnyElement;

    /// Return the pooling key for this item when element reuse is supported.
    fn recycle_key(&self, _ix: usize) -> Option<TypeId> {
        None
    }

    /// Render the item using an optional recycled element from the matching pool.
    fn render_recycled_item(
        &self,
        ix: usize,
        recycled: Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let _ = recycled;
        self.render_item(ix, window, cx)
    }
}

/// A keyed heterogeneous list that recycles layout state across frames.
pub struct RecyclingList<D> {
    element_id: ElementId,
    delegate: Rc<D>,
    style: StyleRefinement,
    sizing_behavior: ListSizingBehavior,
    alignment: ListAlignment,
    overdraw: Pixels,
}

struct RecyclingListElementState {
    list_state: ListState,
    estimated_heights: Vec<Pixels>,
    element_pool: Rc<RefCell<ElementPool>>,
    alignment: ListAlignment,
    overdraw: Pixels,
}

/// Frame state used by a [`RecyclingList`] between prepaint and paint.
pub struct RecyclingListFrameState {
    inner: ListPrepaintState,
    list_state: ListState,
    element_pool: Rc<RefCell<ElementPool>>,
}

#[derive(Default)]
struct ElementPool {
    pools: HashMap<TypeId, Vec<AnyElement>>,
    max_per_type: usize,
}

impl ElementPool {
    fn new(max_per_type: usize) -> Self {
        Self {
            pools: HashMap::default(),
            max_per_type,
        }
    }

    fn take(&mut self, key: TypeId) -> Option<AnyElement> {
        let pool = self.pools.get_mut(&key)?;
        let element = pool.pop();
        if pool.is_empty() {
            self.pools.remove(&key);
        }
        element
    }

    fn release(&mut self, key: TypeId, element: AnyElement) {
        let pool = self.pools.entry(key).or_default();
        if pool.len() < self.max_per_type {
            pool.push(element);
        }
    }
}

impl<D> RecyclingList<D>
where
    D: ListDelegate,
{
    /// Number of items reported by the delegate.
    pub fn item_count(&self) -> usize {
        self.delegate.item_count()
    }

    /// Returns true when the delegate reports no items.
    pub fn is_empty(&self) -> bool {
        self.item_count() == 0
    }

    /// Stable text key for the list sizing behavior.
    pub fn sizing_behavior_key(&self) -> &'static str {
        self.sizing_behavior.to_text()
    }

    /// Stable text key for the list alignment.
    pub fn alignment_key(&self) -> &'static str {
        self.alignment.to_text()
    }

    /// Coarse overdraw class for content-safe diagnostics.
    pub fn overdraw_class(&self) -> &'static str {
        if self.overdraw == px(0.) {
            "none"
        } else if self.overdraw == px(DEFAULT_OVERDRAW_PX) {
            "default"
        } else {
            "custom"
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "recycling_list(item_count={}, empty={}, sizing={}, alignment={}, overdraw_class={})",
            self.item_count(),
            self.is_empty(),
            self.sizing_behavior_key(),
            self.alignment_key(),
            self.overdraw_class()
        )
    }

    /// Set the sizing behavior for the list.
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }

    /// Set the list alignment.
    pub fn with_alignment(mut self, alignment: ListAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set the amount of extra content to measure above and below the viewport.
    pub fn with_overdraw(mut self, overdraw: Pixels) -> Self {
        self.overdraw = overdraw;
        self
    }

    fn estimated_heights(&self) -> Vec<Pixels> {
        (0..self.delegate.item_count())
            .map(|ix| self.delegate.estimated_item_height(ix))
            .collect()
    }

    fn build_list(&self, list_state: ListState, element_pool: Rc<RefCell<ElementPool>>) -> List {
        let render_delegate = self.delegate.clone();
        let take_delegate = self.delegate.clone();
        let release_delegate = self.delegate.clone();
        let take_pool = element_pool.clone();
        let release_pool = element_pool;

        let mut inner = list_with_recycling(
            list_state,
            move |ix, recycled, window, cx| {
                render_delegate.render_recycled_item(ix, recycled, window, cx)
            },
            move |ix| {
                let key = take_delegate.recycle_key(ix)?;
                take_pool.borrow_mut().take(key)
            },
            move |ix, mut element| {
                let Some(key) = release_delegate.recycle_key(ix) else {
                    return;
                };
                if !element.supports_reuse() {
                    return;
                }
                element.reset_for_reuse();
                release_pool.borrow_mut().release(key, element);
            },
        )
        .with_sizing_behavior(self.sizing_behavior);
        *inner.style() = self.style.clone();
        inner
    }

    fn build_state(&self, estimated_heights: &[Pixels]) -> RecyclingListElementState {
        RecyclingListElementState {
            list_state: ListState::new_estimated(
                estimated_heights.iter().copied(),
                self.alignment,
                self.overdraw,
            ),
            estimated_heights: estimated_heights.to_vec(),
            element_pool: Rc::new(RefCell::new(ElementPool::new(
                DEFAULT_MAX_POOLED_ITEMS_PER_TYPE,
            ))),
            alignment: self.alignment,
            overdraw: self.overdraw,
        }
    }

    fn sync_state(&self, state: &mut RecyclingListElementState, estimated_heights: &[Pixels]) {
        if state.alignment != self.alignment || state.overdraw != self.overdraw {
            let scroll_top = state.list_state.logical_scroll_top();
            state.list_state = ListState::new_estimated(
                estimated_heights.iter().copied(),
                self.alignment,
                self.overdraw,
            );
            state.list_state.scroll_to(scroll_top);
            state.alignment = self.alignment;
            state.overdraw = self.overdraw;
            state.estimated_heights = estimated_heights.to_vec();
            return;
        }

        if state.estimated_heights != estimated_heights {
            state
                .list_state
                .replace_estimated_heights(estimated_heights.iter().copied());
            state.estimated_heights = estimated_heights.to_vec();
        }
    }
}

impl<D> Element for RecyclingList<D>
where
    D: ListDelegate,
{
    type RequestLayoutState = Vec<Pixels>;
    type PrepaintState = RecyclingListFrameState;

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let estimated_heights = self.estimated_heights();
        let temporary_state = ListState::new_estimated(
            estimated_heights.iter().copied(),
            self.alignment,
            self.overdraw,
        );
        let mut inner = self.build_list(
            temporary_state,
            Rc::new(RefCell::new(ElementPool::new(
                DEFAULT_MAX_POOLED_ITEMS_PER_TYPE,
            ))),
        );
        let (layout_id, _) = inner.request_layout(None, None, window, cx);
        (layout_id, estimated_heights)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: crate::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        window.with_optional_element_state(
            global_id,
            |element_state: Option<Option<RecyclingListElementState>>, window| {
                let mut element_state = element_state
                    .flatten()
                    .unwrap_or_else(|| self.build_state(request_layout));
                self.sync_state(&mut element_state, request_layout);

                let list_state = element_state.list_state.clone();
                let element_pool = element_state.element_pool.clone();
                let mut inner = self.build_list(list_state.clone(), element_pool.clone());
                let mut inner_request_layout = ();
                let inner =
                    inner.prepaint(None, None, bounds, &mut inner_request_layout, window, cx);

                (
                    RecyclingListFrameState {
                        inner,
                        list_state,
                        element_pool,
                    },
                    Some(element_state),
                )
            },
        )
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: crate::Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mut inner = self.build_list(prepaint.list_state.clone(), prepaint.element_pool.clone());
        let mut inner_request_layout = ();
        inner.paint(
            None,
            None,
            bounds,
            &mut inner_request_layout,
            &mut prepaint.inner,
            window,
            cx,
        );
    }
}

impl<D> IntoElement for RecyclingList<D>
where
    D: ListDelegate,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<D> Styled for RecyclingList<D>
where
    D: ListDelegate,
{
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[cfg(test)]
mod tests {
    use super::{ListDelegate, recycling_list};
    use crate::{
        AppContext, Context, Element, IntoElement, ListAlignment, ListSizingBehavior,
        ParentElement, Render, ScrollDelta, ScrollWheelEvent, Styled, TestAppContext, Window, div,
        point, px, size,
    };
    use std::{
        any::TypeId,
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Clone)]
    struct TestDelegate {
        rendered: Rc<RefCell<Vec<usize>>>,
    }

    impl ListDelegate for TestDelegate {
        fn item_count(&self) -> usize {
            100
        }

        fn estimated_item_height(&self, _ix: usize) -> crate::Pixels {
            px(20.)
        }

        fn render_item(
            &self,
            ix: usize,
            _window: &mut Window,
            _cx: &mut crate::App,
        ) -> crate::AnyElement {
            self.rendered.borrow_mut().push(ix);
            div()
                .h(px(20.))
                .w_full()
                .child(format!("Item {ix}"))
                .into_any()
        }
    }

    #[test]
    fn recycling_list_summary_is_content_safe() {
        let delegate = TestDelegate {
            rendered: Rc::new(RefCell::new(Vec::new())),
        };
        let list = recycling_list("recycling-list-summary", delegate)
            .with_sizing_behavior(ListSizingBehavior::Infer)
            .with_alignment(ListAlignment::Bottom)
            .with_overdraw(px(0.));

        assert_eq!(list.item_count(), 100);
        assert!(!list.is_empty());
        assert_eq!(list.sizing_behavior_key(), "infer");
        assert_eq!(list.alignment_key(), "bottom");
        assert_eq!(list.overdraw_class(), "none");

        let summary = list.to_text();
        assert!(summary.contains("recycling_list(item_count=100"));
        assert!(summary.contains("sizing=infer"));
        assert!(summary.contains("alignment=bottom"));
        assert!(summary.contains("overdraw_class=none"));
        assert!(!summary.contains("Item "));
    }

    #[kael::test]
    fn test_recycling_list_renders_visible_slice(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let rendered = Rc::new(RefCell::new(Vec::new()));
        let delegate = TestDelegate {
            rendered: rendered.clone(),
        };

        struct TestView(TestDelegate);
        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                recycling_list("recycling-list", self.0.clone())
                    .w_full()
                    .h_full()
            }
        }

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, cx| {
            cx.new(|_| TestView(delegate.clone()))
        });

        let initial_rendered = rendered.borrow().clone();
        assert!(!initial_rendered.is_empty());
        assert!(initial_rendered.iter().all(|ix| *ix < 20));
        assert!(initial_rendered.len() < delegate.item_count());
    }

    #[derive(Clone)]
    struct PoolingDelegate {
        created: Rc<Cell<usize>>,
        reused: Rc<Cell<usize>>,
    }

    impl ListDelegate for PoolingDelegate {
        fn item_count(&self) -> usize {
            100
        }

        fn estimated_item_height(&self, _ix: usize) -> crate::Pixels {
            px(20.)
        }

        fn render_item(
            &self,
            _ix: usize,
            _window: &mut Window,
            _cx: &mut crate::App,
        ) -> crate::AnyElement {
            self.created.set(self.created.get() + 1);
            div().h(px(20.)).w_full().into_boxed_any()
        }

        fn recycle_key(&self, _ix: usize) -> Option<TypeId> {
            Some(TypeId::of::<crate::Div>())
        }

        fn render_recycled_item(
            &self,
            ix: usize,
            recycled: Option<crate::AnyElement>,
            window: &mut Window,
            cx: &mut crate::App,
        ) -> crate::AnyElement {
            if let Some(element) = recycled {
                self.reused.set(self.reused.get() + 1);
                element
            } else {
                self.render_item(ix, window, cx)
            }
        }
    }

    #[kael::test]
    fn test_recycling_list_reuses_pooled_elements(cx: &mut TestAppContext) {
        let cx = cx.add_empty_window();
        let created = Rc::new(Cell::new(0));
        let reused = Rc::new(Cell::new(0));
        let delegate = PoolingDelegate {
            created: created.clone(),
            reused: reused.clone(),
        };

        struct PoolingView(PoolingDelegate);
        impl Render for PoolingView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                recycling_list("recycling-list-pool", self.0.clone())
                    .with_overdraw(px(0.))
                    .w_full()
                    .h_full()
            }
        }

        let view = cx.new(|_| PoolingView(delegate.clone()));

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, _| {
            view.clone()
        });
        let created_after_first_draw = created.get();
        assert!(created_after_first_draw > 0);

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(1.), px(1.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-20.))),
            ..Default::default()
        });

        cx.draw(point(px(0.), px(0.)), size(px(100.), px(20.)), |_, _| {
            view.clone()
        });

        assert!(created.get() <= created_after_first_draw + 1);
        assert!(reused.get() > 0);
    }
}
