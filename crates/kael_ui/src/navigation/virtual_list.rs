//! View-bound virtual list for efficient rendering of large item counts.
//!
//! This is a thin, view-aware convenience layer over
//! [`crate::virtual_list::VariableVirtualList`]: callers supply an explicit
//! per-item [`Size`] list and a render closure bound to a [`Render`] view, and
//! the heavy lifting (visible-range computation, recycling, layout, paint) is
//! delegated to the shared variable virtual-list engine so there is a single
//! virtualization implementation to maintain.

use std::{ops::Range, rc::Rc};

use kael::{
    Along, App, Axis, Context, ElementId, Entity, IntoElement, ListSizingBehavior, Pixels, Render,
    ScrollHandle, Size, Window,
};

use crate::virtual_list::{ItemExtentProvider, VariableVirtualList};

pub struct ExplicitSizes {
    axis: Axis,
    sizes: Rc<Vec<Size<Pixels>>>,
}

impl ItemExtentProvider for ExplicitSizes {
    fn extent(&self, index: usize) -> Pixels {
        self.sizes
            .get(index)
            .map(|size| size.along(self.axis))
            .unwrap_or_default()
    }
}

#[inline]
pub fn v_virtual_list<R, V>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    f: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> VirtualList
where
    R: IntoElement + 'static,
    V: Render,
{
    virtual_list(view, id, Axis::Vertical, item_sizes, f)
}

fn virtual_list<R, V>(
    view: Entity<V>,
    id: impl Into<ElementId>,
    axis: Axis,
    item_sizes: Rc<Vec<Size<Pixels>>>,
    f: impl 'static + Fn(&mut V, Range<usize>, &mut Window, &mut Context<V>) -> Vec<R>,
) -> VirtualList
where
    R: IntoElement + 'static,
    V: Render,
{
    let id: ElementId = id.into();
    let item_count = item_sizes.len();
    let provider = ExplicitSizes {
        axis,
        sizes: item_sizes,
    };

    let render_range = move |range: Range<usize>, window: &mut Window, cx: &mut App| {
        view.update(cx, |this, cx| f(this, range, window, cx))
    };

    let inner = VariableVirtualList::new(id, axis, item_count, provider, render_range)
        .with_sizing_behavior(ListSizingBehavior::Auto);

    VirtualList { inner }
}

pub struct VirtualList {
    inner: VariableVirtualList<ExplicitSizes>,
}

impl VirtualList {
    pub fn track_scroll(mut self, scroll_handle: &ScrollHandle) -> Self {
        self.inner = self.inner.track_scroll(scroll_handle);
        self
    }
}

impl kael::Styled for VirtualList {
    fn style(&mut self) -> &mut kael::StyleRefinement {
        self.inner.style()
    }
}

impl IntoElement for VirtualList {
    type Element = VariableVirtualList<ExplicitSizes>;

    fn into_element(self) -> Self::Element {
        self.inner
    }
}
