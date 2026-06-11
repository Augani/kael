use crate::{
    AnyElement, App, AppContext, Bounds, Component, Context, ElementId, InteractiveElement,
    IntoElement, List, ListAlignment, ListSizingBehavior, ListState, ParentElement, Pixels, Point,
    Render, RenderOnce, StatefulInteractiveElement, StyleRefinement, Styled, Window, div, px,
};
use std::rc::Rc;

const DEFAULT_OVERDRAW_PX: f32 = 200.0;
const DEFAULT_ESTIMATED_ITEM_HEIGHT_PX: f32 = 44.0;
const INSERTION_INDICATOR_HEIGHT_PX: f32 = 2.0;
const INSERTION_GAP_PX: f32 = 10.0;

/// Distance to auto-scroll when dragging near an edge, in pixels per frame.
pub const AUTO_SCROLL_STEP_PX: f32 = 18.0;
/// Edge band, in pixels, within which a drag triggers auto-scrolling.
pub const AUTO_SCROLL_THRESHOLD_PX: f32 = 36.0;

/// Translates a drag from a source item to an insertion slot into the final
/// `(from, to)` index pair, or `None` when the move is a no-op or out of range.
///
/// An insertion slot sits *between* items (`0..=len`); when the slot is past the
/// source the target shifts down by one to account for the source being lifted
/// out first. This is the canonical reorder index math shared by every sortable
/// list so virtualized (delegate) and data-driven lists agree on what a drop
/// means.
pub fn reorder_target(
    source_index: usize,
    insertion_index: usize,
    item_count: usize,
) -> Option<(usize, usize)> {
    if source_index >= item_count || insertion_index > item_count {
        return None;
    }

    let target_index = if insertion_index > source_index {
        insertion_index.saturating_sub(1)
    } else {
        insertion_index
    };

    (target_index != source_index).then_some((source_index, target_index))
}

/// Moves the item at `from` to `to` within `items`, clamping `to` into range.
///
/// Returns whether the vector changed. Both core's delegate-based list and
/// higher-level data-driven lists route their reorder through this primitive so
/// the move semantics never drift apart.
pub fn apply_reorder<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
    if from >= items.len() || from == to {
        return false;
    }
    let moved = items.remove(from);
    let insert_at = to.min(items.len());
    items.insert(insert_at, moved);
    true
}

/// Returns the per-frame auto-scroll distance for a drag at `position` within
/// `bounds`: negative near the top edge, positive near the bottom, zero in the
/// middle. Shared so every sortable list auto-scrolls with the same feel.
pub fn auto_scroll_distance(position: Point<Pixels>, bounds: Bounds<Pixels>) -> Pixels {
    let threshold = px(AUTO_SCROLL_THRESHOLD_PX);
    if position.y <= bounds.top() + threshold {
        -px(AUTO_SCROLL_STEP_PX)
    } else if position.y >= bounds.bottom() - threshold {
        px(AUTO_SCROLL_STEP_PX)
    } else {
        Pixels::ZERO
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SortableDragPayload {
    list_id: String,
    index: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DragSnapshot {
    source_index: Option<usize>,
    insertion_index: Option<usize>,
}

fn drag_reorder_target(drag: &DragSnapshot, item_count: usize) -> Option<(usize, usize)> {
    reorder_target(drag.source_index?, drag.insertion_index?, item_count)
}

struct SortableListElementState {
    list_state: ListState,
    estimated_heights: Vec<Pixels>,
    alignment: ListAlignment,
    overdraw: Pixels,
    drag: DragSnapshot,
}

impl SortableListElementState {
    fn new(estimated_heights: &[Pixels], alignment: ListAlignment, overdraw: Pixels) -> Self {
        Self {
            list_state: ListState::new_estimated(
                estimated_heights.iter().copied(),
                alignment,
                overdraw,
            ),
            estimated_heights: estimated_heights.to_vec(),
            alignment,
            overdraw,
            drag: DragSnapshot::default(),
        }
    }

    fn sync(&mut self, estimated_heights: &[Pixels], alignment: ListAlignment, overdraw: Pixels) {
        if self.alignment != alignment || self.overdraw != overdraw {
            let scroll_top = self.list_state.logical_scroll_top();
            self.list_state =
                ListState::new_estimated(estimated_heights.iter().copied(), alignment, overdraw);
            self.list_state.scroll_to(scroll_top);
            self.alignment = alignment;
            self.overdraw = overdraw;
        } else if self.estimated_heights != estimated_heights {
            self.list_state
                .replace_estimated_heights(estimated_heights.iter().copied());
        }

        self.estimated_heights = estimated_heights.to_vec();
        let item_count = self.list_state.item_count();
        if self
            .drag
            .source_index
            .is_some_and(|source_index| source_index >= item_count)
        {
            self.clear_drag();
        }
    }

    fn begin_drag(&mut self, source_index: usize) {
        self.drag.source_index = Some(source_index);
        self.drag.insertion_index = Some(source_index);
    }

    fn clear_drag(&mut self) {
        self.drag = DragSnapshot::default();
    }

    fn reorder_target(&self) -> Option<(usize, usize)> {
        drag_reorder_target(&self.drag, self.list_state.item_count())
    }

    fn update_drag<D>(
        &mut self,
        payload: &SortableDragPayload,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        delegate: &D,
    ) where
        D: SortableDelegate,
    {
        let scroll_delta = auto_scroll_distance(position, bounds);
        if scroll_delta != Pixels::ZERO {
            self.list_state.scroll_by(scroll_delta);
        }

        let Some(insertion_index) = self.insertion_index_for_position(position, bounds) else {
            self.drag.insertion_index = None;
            return;
        };

        self.update_insertion_index(payload, insertion_index, delegate);
    }

    fn update_insertion_index<D>(
        &mut self,
        payload: &SortableDragPayload,
        insertion_index: usize,
        delegate: &D,
    ) where
        D: SortableDelegate,
    {
        let source_index = *self.drag.source_index.get_or_insert(payload.index);
        let candidate_target = if insertion_index > source_index {
            insertion_index.saturating_sub(1)
        } else {
            insertion_index
        };

        if candidate_target == source_index || delegate.can_move(source_index, candidate_target) {
            self.drag.insertion_index = Some(insertion_index);
        } else {
            self.drag.insertion_index = None;
        }
    }

    fn insertion_index_for_position(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
    ) -> Option<usize> {
        let item_count = self.list_state.item_count();
        if item_count == 0 {
            return Some(0);
        }

        let relative_y = position.y - bounds.top();
        let scroll_top = self.list_state.logical_scroll_top();
        let start_index = scroll_top.item_ix.min(item_count.saturating_sub(1));
        let mut insertion_index = item_count;
        let mut item_top = -scroll_top.offset_in_item;

        for ix in start_index..item_count {
            let item_height = self
                .list_state
                .bounds_for_item(ix)
                .map(|item_bounds| item_bounds.size.height)
                .unwrap_or_else(|| {
                    self.estimated_heights
                        .get(ix)
                        .copied()
                        .unwrap_or(px(DEFAULT_ESTIMATED_ITEM_HEIGHT_PX))
                });
            let item_center = item_top + item_height / 2.0;
            if relative_y < item_center {
                return Some(ix);
            }

            item_top += item_height;
            insertion_index = ix + 1;
            if item_top > bounds.size.height {
                break;
            }
        }

        Some(insertion_index)
    }
}

/// Creates a list with built-in internal drag-and-drop reordering.
#[track_caller]
pub fn sortable_list<D>(id: impl Into<ElementId>, delegate: D) -> SortableList<D>
where
    D: SortableDelegate,
{
    SortableList {
        element_id: id.into(),
        delegate: Rc::new(delegate),
        style: StyleRefinement::default(),
        sizing_behavior: ListSizingBehavior::default(),
        alignment: ListAlignment::Top,
        overdraw: px(DEFAULT_OVERDRAW_PX),
    }
}

/// Supplies item rendering and reorder behavior for a [`SortableList`].
pub trait SortableDelegate: 'static {
    /// Returns the current number of items in the list.
    fn item_count(&self) -> usize;

    /// Renders the item at `ix`.
    fn render_item(
        &self,
        ix: usize,
        is_dragging: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;

    /// Returns the estimated height for an item that has not yet been measured.
    fn estimated_item_height(&self, _ix: usize) -> Pixels {
        px(DEFAULT_ESTIMATED_ITEM_HEIGHT_PX)
    }

    /// Returns whether an item can move from `from` to `to`.
    fn can_move(&self, _from: usize, _to: usize) -> bool {
        true
    }

    /// Applies a reorder after a successful drop.
    fn did_reorder(&self, from: usize, to: usize, window: &mut Window, cx: &mut App);
}

/// A low-level, delegate-driven list that supports reordering its own items via
/// drag and drop.
///
/// This is the foundational sortable: items are supplied lazily through a
/// [`SortableDelegate`] and laid out by a virtualized [`ListState`], so it
/// scales to large lists, supports per-item move constraints
/// ([`SortableDelegate::can_move`]), drag auto-scroll, and insertion indicators.
/// For a data-driven convenience layer over an in-memory `Vec`, prefer the
/// higher-level `kael_ui` `SortableList`, which delegates its reorder math to
/// the shared [`reorder_target`] / [`apply_reorder`] helpers in this module.
pub struct SortableList<D> {
    element_id: ElementId,
    delegate: Rc<D>,
    style: StyleRefinement,
    sizing_behavior: ListSizingBehavior,
    alignment: ListAlignment,
    overdraw: Pixels,
}

struct SortableDragPreview<D> {
    delegate: Rc<D>,
    index: usize,
    width: Option<Pixels>,
}

impl<D> SortableDragPreview<D> {
    fn new(delegate: Rc<D>, index: usize, width: Option<Pixels>) -> Self {
        Self {
            delegate,
            index,
            width,
        }
    }
}

impl<D> Render for SortableDragPreview<D>
where
    D: SortableDelegate,
{
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut preview = div()
            .rounded(px(10.0))
            .shadow_lg()
            .opacity(0.92)
            .child(self.delegate.render_item(self.index, true, window, cx));

        if let Some(width) = self.width {
            preview = preview.w(width);
        }

        preview
    }
}

impl<D> SortableList<D>
where
    D: SortableDelegate,
{
    /// Sets how the list chooses its rendered size.
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }

    /// Sets how items align within the available list space.
    pub fn with_alignment(mut self, alignment: ListAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the extra measurement area above and below the viewport.
    pub fn with_overdraw(mut self, overdraw: Pixels) -> Self {
        self.overdraw = overdraw;
        self
    }

    fn estimated_heights(&self) -> Vec<Pixels> {
        (0..self.delegate.item_count())
            .map(|ix| self.delegate.estimated_item_height(ix))
            .collect()
    }

    fn build_list(
        &self,
        list_state: ListState,
        drag: DragSnapshot,
        selector_id: String,
        list_id: String,
        state: crate::Entity<SortableListElementState>,
    ) -> List {
        let item_count = self.delegate.item_count();
        let delegate = self.delegate.clone();
        crate::list(list_state, move |ix, window, cx| {
            let is_dragging = drag.source_index == Some(ix);
            let show_gap_before = drag.insertion_index == Some(ix)
                && drag_reorder_target(&drag, item_count).is_some();
            let show_gap_after = drag.insertion_index == Some(item_count)
                && ix + 1 == item_count
                && drag_reorder_target(&drag, item_count).is_some();

            let payload = SortableDragPayload {
                list_id: list_id.clone(),
                index: ix,
            };
            let delegate_for_drag = delegate.clone();
            let delegate_for_hover = delegate.clone();
            let delegate_for_drop = delegate.clone();
            let state_for_drag = state.clone();
            let state_for_hover = state.clone();
            let state_for_drop = state.clone();
            let list_id_for_hover = list_id.clone();
            let list_id_for_drop = list_id.clone();

            let mut item = div()
                .id(ElementId::named_usize(format!("{}-item", selector_id), ix))
                .flex()
                .flex_col()
                .cursor_move();

            if show_gap_before {
                item = item.child(
                    div().h(px(INSERTION_GAP_PX)).child(
                        div()
                            .h(px(INSERTION_INDICATOR_HEIGHT_PX))
                            .rounded(px(999.0))
                            .bg(crate::blue()),
                    ),
                );
            }

            item = item
                .on_drag(payload, move |payload, _, window, cx| {
                    state_for_drag.update(cx, |state, _| {
                        state.begin_drag(payload.index);
                    });
                    window.refresh();

                    let width = state_for_drag
                        .read(cx)
                        .list_state
                        .bounds_for_item(payload.index)
                        .map(|bounds| bounds.size.width);
                    let preview_delegate = delegate_for_drag.clone();
                    cx.new(move |_| {
                        SortableDragPreview::new(preview_delegate.clone(), payload.index, width)
                    })
                })
                .on_drag_move(
                    move |event: &crate::DragMoveEvent<SortableDragPayload>, window, cx| {
                        let payload = event.drag(cx).clone();
                        if payload.list_id != list_id_for_hover
                            || !event.bounds.contains(&event.event.position)
                        {
                            return;
                        }

                        let insertion_index = if event.event.position.y < event.bounds.center().y {
                            ix
                        } else {
                            ix + 1
                        };
                        state_for_hover.update(cx, |state, _| {
                            state.update_insertion_index(
                                &payload,
                                insertion_index,
                                delegate_for_hover.as_ref(),
                            );
                        });
                        window.refresh();
                    },
                )
                .on_drop(move |payload: &SortableDragPayload, window, cx| {
                    if payload.list_id != list_id_for_drop {
                        return;
                    }

                    let mut reorder = None;
                    state_for_drop.update(cx, |state, _| {
                        reorder = state.reorder_target();
                        state.clear_drag();
                    });

                    if let Some((from, to)) = reorder {
                        delegate_for_drop.did_reorder(from, to, window, cx);
                    }
                    window.refresh();
                })
                .child(
                    div()
                        .opacity(if is_dragging { 0.35 } else { 1.0 })
                        .child(delegate.render_item(ix, is_dragging, window, cx)),
                );

            if show_gap_after {
                item = item.child(
                    div().h(px(INSERTION_GAP_PX)).child(
                        div()
                            .h(px(INSERTION_INDICATOR_HEIGHT_PX))
                            .rounded(px(999.0))
                            .bg(crate::blue()),
                    ),
                );
            }

            item.into_any_element()
        })
        .with_sizing_behavior(self.sizing_behavior)
    }
}

impl<D> RenderOnce for SortableList<D>
where
    D: SortableDelegate,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let estimated_heights = self.estimated_heights();
        let selector_id = self.element_id.to_string();
        let list_id = selector_id.clone();
        let state = window.use_keyed_state(self.element_id.clone(), cx, |_, _| {
            SortableListElementState::new(&estimated_heights, self.alignment, self.overdraw)
        });
        state.update(cx, |state, _| {
            state.sync(&estimated_heights, self.alignment, self.overdraw);
        });

        if !cx.has_active_drag() {
            state.update(cx, |state, _| {
                if state.drag.source_index.is_some() || state.drag.insertion_index.is_some() {
                    state.clear_drag();
                }
            });
        }

        let (list_state, drag) = {
            let snapshot = state.read(cx);
            (snapshot.list_state.clone(), snapshot.drag.clone())
        };

        let delegate_for_drag_move = self.delegate.clone();
        let delegate_for_drop = self.delegate.clone();
        let state_for_drag_move = state.clone();
        let state_for_drop = state;
        let list_id_for_can_drop = list_id.clone();
        let list_id_for_drag_move = list_id.clone();
        let list_id_for_drop = list_id.clone();
        let inner = self.build_list(
            list_state,
            drag,
            selector_id,
            list_id,
            state_for_drag_move.clone(),
        );

        let mut root = div();
        *root.style() = self.style.clone();
        root.id(self.element_id)
            .can_drop(move |value, _, _| {
                value
                    .downcast_ref::<SortableDragPayload>()
                    .is_some_and(|payload| payload.list_id == list_id_for_can_drop)
            })
            .on_drag_move(
                move |event: &crate::DragMoveEvent<SortableDragPayload>, window, cx| {
                    let payload = event.drag(cx).clone();
                    if payload.list_id != list_id_for_drag_move {
                        return;
                    }

                    state_for_drag_move.update(cx, |state, _| {
                        state.update_drag(
                            &payload,
                            event.event.position,
                            event.bounds,
                            delegate_for_drag_move.as_ref(),
                        );
                    });
                    window.refresh();
                },
            )
            .on_drop(move |payload: &SortableDragPayload, window, cx| {
                if payload.list_id != list_id_for_drop {
                    return;
                }

                let mut reorder = None;
                state_for_drop.update(cx, |state, _| {
                    reorder = state.reorder_target();
                    state.clear_drag();
                });

                if let Some((from, to)) = reorder {
                    delegate_for_drop.did_reorder(from, to, window, cx);
                }
                window.refresh();
            })
            .child(inner.w_full().h_full())
    }
}

impl<D> IntoElement for SortableList<D>
where
    D: SortableDelegate,
{
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

impl<D> Styled for SortableList<D>
where
    D: SortableDelegate,
{
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AUTO_SCROLL_STEP_PX, SortableDelegate, SortableDragPayload, SortableListElementState,
        apply_reorder, auto_scroll_distance, reorder_target,
    };
    use crate::{
        AnyElement, App, Bounds, IntoElement, ListAlignment, Pixels, Window, div, point, px,
    };

    #[test]
    fn reorder_target_shifts_when_inserting_past_source() {
        assert_eq!(reorder_target(0, 3, 4), Some((0, 2)));
        assert_eq!(reorder_target(3, 0, 4), Some((3, 0)));
        assert_eq!(reorder_target(1, 1, 4), None);
        assert_eq!(reorder_target(1, 2, 4), None);
        assert_eq!(reorder_target(0, 5, 4), None);
        assert_eq!(reorder_target(9, 0, 4), None);
    }

    #[test]
    fn apply_reorder_moves_and_clamps() {
        let mut items = vec![0, 1, 2, 3];
        assert!(apply_reorder(&mut items, 0, 2));
        assert_eq!(items, vec![1, 2, 0, 3]);

        let mut clamp = vec![0, 1, 2];
        assert!(apply_reorder(&mut clamp, 0, 99));
        assert_eq!(clamp, vec![1, 2, 0]);

        let mut noop = vec![0, 1, 2];
        assert!(!apply_reorder(&mut noop, 1, 1));
        assert!(!apply_reorder(&mut noop, 9, 0));
        assert_eq!(noop, vec![0, 1, 2]);
    }

    #[test]
    fn auto_scroll_distance_triggers_only_near_edges() {
        let bounds = Bounds::from_corners(point(px(0.0), px(0.0)), point(px(100.0), px(300.0)));
        assert_eq!(
            auto_scroll_distance(point(px(50.0), px(10.0)), bounds),
            -px(AUTO_SCROLL_STEP_PX)
        );
        assert_eq!(
            auto_scroll_distance(point(px(50.0), px(290.0)), bounds),
            px(AUTO_SCROLL_STEP_PX)
        );
        assert_eq!(
            auto_scroll_distance(point(px(50.0), px(150.0)), bounds),
            Pixels::ZERO
        );
    }

    struct AllowMoves;

    impl SortableDelegate for AllowMoves {
        fn item_count(&self) -> usize {
            4
        }

        fn render_item(
            &self,
            _ix: usize,
            _is_dragging: bool,
            _window: &mut Window,
            _cx: &mut App,
        ) -> AnyElement {
            div().into_any_element()
        }

        fn estimated_item_height(&self, _ix: usize) -> Pixels {
            px(36.0)
        }

        fn did_reorder(&self, _from: usize, _to: usize, _window: &mut Window, _cx: &mut App) {}
    }

    struct DenyMoves;

    impl SortableDelegate for DenyMoves {
        fn item_count(&self) -> usize {
            4
        }

        fn render_item(
            &self,
            _ix: usize,
            _is_dragging: bool,
            _window: &mut Window,
            _cx: &mut App,
        ) -> AnyElement {
            div().into_any_element()
        }

        fn estimated_item_height(&self, _ix: usize) -> Pixels {
            px(36.0)
        }

        fn can_move(&self, _from: usize, _to: usize) -> bool {
            false
        }

        fn did_reorder(&self, _from: usize, _to: usize, _window: &mut Window, _cx: &mut App) {}
    }

    #[test]
    fn reorder_target_translates_insertion_slot_to_final_index() {
        let estimated_heights = [px(36.0), px(36.0), px(36.0), px(36.0)];
        let mut state =
            SortableListElementState::new(&estimated_heights, ListAlignment::Top, px(200.0));

        state.begin_drag(0);
        state.drag.insertion_index = Some(3);
        assert_eq!(state.reorder_target(), Some((0, 2)));

        state.drag.insertion_index = Some(0);
        assert_eq!(state.reorder_target(), None);
    }

    #[test]
    fn update_insertion_index_respects_delegate_constraints() {
        let estimated_heights = [px(36.0), px(36.0), px(36.0), px(36.0)];
        let mut state =
            SortableListElementState::new(&estimated_heights, ListAlignment::Top, px(200.0));
        let payload = SortableDragPayload {
            list_id: "sortable".to_string(),
            index: 0,
        };

        state.begin_drag(0);
        state.update_insertion_index(&payload, 3, &AllowMoves);
        assert_eq!(state.drag.insertion_index, Some(3));

        state.update_insertion_index(&payload, 3, &DenyMoves);
        assert_eq!(state.drag.insertion_index, None);
    }

    #[test]
    fn insertion_index_for_position_uses_estimated_heights() {
        let estimated_heights = [px(36.0), px(36.0), px(36.0), px(36.0)];
        let state =
            SortableListElementState::new(&estimated_heights, ListAlignment::Top, px(200.0));
        let bounds = Bounds::from_corners(point(px(0.0), px(0.0)), point(px(220.0), px(220.0)));

        assert_eq!(
            state.insertion_index_for_position(point(px(10.0), px(10.0)), bounds),
            Some(0)
        );
        assert_eq!(
            state.insertion_index_for_position(point(px(10.0), px(80.0)), bounds),
            Some(2)
        );
        assert_eq!(
            state.insertion_index_for_position(point(px(10.0), px(150.0)), bounds),
            Some(4)
        );
    }
}
