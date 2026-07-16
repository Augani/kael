use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

use crate::theme::use_theme;

#[derive(Clone)]
pub struct SortableItemDrag {
    index: usize,
    position: Point<Pixels>,
}

impl std::fmt::Debug for SortableItemDrag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SortableItemDrag")
            .field("index", &self.index)
            .finish()
    }
}

impl Render for SortableItemDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        div().pl(self.position.x).pt(self.position.y).child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .bg(theme.tokens.card.opacity(0.95))
                .border_1()
                .border_color(theme.tokens.primary)
                .rounded(theme.tokens.radius_md)
                .shadow(smallvec::smallvec![BoxShadow {
                    color: hsla(0.0, 0.0, 0.0, 0.2),
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(8.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .text_size(px(14.0))
                .text_color(theme.tokens.foreground)
                .font_family(theme.tokens.font_family.clone())
                .child("Moving..."),
        )
    }
}

pub struct SortableListState<T: Clone + 'static> {
    items: Vec<T>,
    dragging_index: Option<usize>,
    hover_index: Option<usize>,
}

impl<T: Clone + 'static> SortableListState<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            dragging_index: None,
            hover_index: None,
        }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn set_items(&mut self, items: Vec<T>) {
        self.items = items;
        self.dragging_index = None;
        self.hover_index = None;
    }

    pub fn dragging_index(&self) -> Option<usize> {
        self.dragging_index
    }

    pub fn hover_index(&self) -> Option<usize> {
        self.hover_index
    }
}

/// A data-driven convenience sortable list over an in-memory `Vec<T>`.
///
/// Items live in a [`SortableListState`] and are laid out in a plain flex
/// container, which keeps the API ergonomic for small, fully materialized
/// collections that fit comfortably on screen. Drag reordering routes its index
/// math through core's shared [`kael::reorder_target`] / [`kael::apply_reorder`]
/// helpers so a drop resolves to the same move as the low-level, virtualized
/// `kael::SortableList` delegate. For large or lazily-supplied lists that need
/// virtualization, per-item move constraints, or drag auto-scroll, use that
/// delegate-based `kael::SortableList` instead.
#[derive(IntoElement)]
pub struct SortableList<T: Clone + 'static> {
    id: ElementId,
    state: Entity<SortableListState<T>>,
    item_renderer: Rc<dyn Fn(&T, usize, bool) -> AnyElement>,
    on_reorder: Option<Rc<dyn Fn(Vec<T>, &mut Window, &mut App)>>,
    direction: Axis,
    gap: Pixels,
    style: StyleRefinement,
}

impl<T: Clone + 'static> SortableList<T> {
    #[track_caller]
    pub fn new(
        state: Entity<SortableListState<T>>,
        renderer: impl Fn(&T, usize, bool) -> AnyElement + 'static,
    ) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "sortable-list:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            state,
            item_renderer: Rc::new(renderer),
            on_reorder: None,
            direction: Axis::Vertical,
            gap: px(4.0),
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn on_reorder(
        mut self,
        callback: impl Fn(Vec<T>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_reorder = Some(Rc::new(callback));
        self
    }

    pub fn direction(mut self, direction: Axis) -> Self {
        self.direction = direction;
        self
    }

    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        let gap = gap.into();
        if f32::from(gap).is_finite() && gap >= px(0.0) {
            self.gap = gap;
        }
        self
    }
}

impl<T: Clone + 'static> Styled for SortableList<T> {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl<T: Clone + 'static> RenderOnce for SortableList<T> {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let list_id = self.id.clone();
        let item_count = self.state.read(cx).items.len();
        let focus_handles: Vec<FocusHandle> = (0..item_count)
            .map(|index| {
                let id = ElementId::NamedChild(
                    Box::new(list_id.clone()),
                    format!("item-{}", index).into(),
                );
                window
                    .use_keyed_state(id, cx, |_, cx| cx.focus_handle())
                    .read(cx)
                    .clone()
            })
            .collect();
        let theme = use_theme();
        let user_style = self.style;
        let items = self.state.read(cx).items.clone();
        let dragging_index = self.state.read(cx).dragging_index;
        let drag_over_bg = theme.tokens.primary.opacity(0.1);
        let indicator_color = theme.tokens.primary;

        let mut container = div()
            .id(list_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::List).label("Sortable list"),
            )
            .flex()
            .when(self.direction == Axis::Vertical, |d| d.flex_col())
            .gap(self.gap);

        for (idx, item) in items.iter().enumerate() {
            let is_dragging = dragging_index == Some(idx);
            let rendered = (self.item_renderer)(item, idx, is_dragging);

            let state_drop = self.state.clone();
            let on_reorder = self.on_reorder.clone();
            let state_drag = self.state.clone();
            let state_key = self.state.clone();
            let on_reorder_key = self.on_reorder.clone();
            let focus_handle = focus_handles[idx].clone();
            let focus_on_mouse = focus_handle.clone();
            let handles_for_key = focus_handles.clone();
            let item_id =
                ElementId::NamedChild(Box::new(list_id.clone()), format!("item-{}", idx).into());

            let item_el = div()
                .id(item_id)
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::ListItem)
                        .label(format!("Sortable item {} of {}", idx + 1, item_count))
                        .description("Use Alt+Up or Alt+Down to reorder")
                        .actions(vec![AccessibilityAction::Focus]),
                )
                .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                .on_mouse_down(MouseButton::Left, move |_, window, _| {
                    window.focus(&focus_on_mouse);
                })
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    if !event.keystroke.modifiers.alt {
                        return;
                    }
                    let target = match event.keystroke.key.as_str() {
                        "up" | "left" if idx > 0 => Some(idx - 1),
                        "down" | "right" if idx + 1 < item_count => Some(idx + 1),
                        _ => None,
                    };
                    if let Some(target) = target {
                        let changed = state_key.update(cx, |state, cx| {
                            let changed = kael::apply_reorder(&mut state.items, idx, target);
                            if changed {
                                cx.notify();
                            }
                            changed
                        });
                        if changed {
                            window.focus(&handles_for_key[target]);
                            if let Some(callback) = on_reorder_key.as_ref() {
                                callback(state_key.read(cx).items.clone(), window, cx);
                            }
                        }
                        cx.stop_propagation();
                    }
                })
                .child(rendered)
                .on_drag(
                    SortableItemDrag {
                        index: idx,
                        position: Point::default(),
                    },
                    move |data: &SortableItemDrag, pos, _window, cx| {
                        state_drag.update(cx, |s, cx| {
                            s.dragging_index = Some(data.index);
                            cx.notify();
                        });
                        cx.new(|_| SortableItemDrag {
                            index: data.index,
                            position: pos,
                        })
                    },
                )
                .drag_over::<SortableItemDrag>(move |style, _, _, _| {
                    style
                        .bg(drag_over_bg)
                        .border_t(px(2.0))
                        .border_color(indicator_color)
                })
                .on_drop(move |dragged: &SortableItemDrag, window, cx| {
                    let from = dragged.index;
                    let to = idx;

                    let changed = state_drop.update(cx, |s, ctx| {
                        let changed = kael::apply_reorder(&mut s.items, from, to);
                        s.dragging_index = None;
                        s.hover_index = None;
                        ctx.notify();
                        changed
                    });

                    if changed {
                        if let Some(ref callback) = on_reorder {
                            let reordered_items = state_drop.read(cx).items.clone();
                            callback(reordered_items, window, cx);
                        }
                    }
                })
                .when(is_dragging, |d| d.opacity(0.5));

            container = container.child(item_el);
        }

        container.map(|mut this| {
            this.style().refine(&user_style);
            this
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn replacing_items_clears_transient_drag_state() {
        let mut state = SortableListState::new(vec![1, 2, 3]);
        state.dragging_index = Some(2);
        state.hover_index = Some(1);

        state.set_items(vec![4]);

        assert_eq!(state.items(), &[4]);
        assert_eq!(state.dragging_index(), None);
        assert_eq!(state.hover_index(), None);
    }
}
