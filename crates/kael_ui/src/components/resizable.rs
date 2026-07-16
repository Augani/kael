//! Resizable panel component - Split-pane layouts with drag handles.

use std::{cell::Cell, ops::Range, rc::Rc};

use kael::{prelude::FluentBuilder as _, *};

use crate::{theme::use_theme, util::AxisExt};

const PANEL_MIN_SIZE: Pixels = px(100.0);
const HANDLE_PADDING: Pixels = px(5.0);
const HANDLE_SIZE: Pixels = px(2.0);

pub fn h_resizable(id: impl Into<ElementId>, state: Entity<ResizableState>) -> ResizablePanelGroup {
    ResizablePanelGroup::new(id, state).axis(Axis::Horizontal)
}

pub fn v_resizable(id: impl Into<ElementId>, state: Entity<ResizableState>) -> ResizablePanelGroup {
    ResizablePanelGroup::new(id, state).axis(Axis::Vertical)
}

pub fn resizable_panel() -> ResizablePanel {
    ResizablePanel::new()
}

#[derive(IntoElement)]
pub struct ResizeHandle {
    axis: Axis,
    active: bool,
    style: StyleRefinement,
}

impl ResizeHandle {
    pub fn new(axis: Axis) -> Self {
        Self {
            axis,
            active: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

impl Styled for ResizeHandle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ResizeHandle {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let color = if self.active {
            theme.tokens.primary
        } else {
            theme.tokens.border
        };

        div()
            .flex()
            .items_center()
            .justify_center()
            .when(self.axis.is_horizontal(), |this| {
                this.cursor_col_resize().h_full().w(px(12.0))
            })
            .when(self.axis.is_vertical(), |this| {
                this.cursor_row_resize().w_full().h(px(12.0))
            })
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Separator).label("Resize panels"),
            )
            .child(
                div()
                    .bg(color)
                    .rounded(px(9999.0))
                    .when(self.axis.is_horizontal(), |this| {
                        this.h_full().w(HANDLE_SIZE)
                    })
                    .when(self.axis.is_vertical(), |this| this.w_full().h(HANDLE_SIZE)),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(Clone, Debug)]
pub enum ResizablePanelEvent {
    Resized {
        panel_index: usize,
        new_size: Pixels,
    },
}

#[derive(Debug, Clone)]
pub struct ResizableState {
    axis: Axis,
    panels: Vec<ResizablePanelState>,
    sizes: Vec<Pixels>,
    resizing_panel_ix: Option<usize>,
    bounds: Bounds<Pixels>,
}

impl ResizableState {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            axis: Axis::Horizontal,
            panels: vec![],
            sizes: vec![],
            resizing_panel_ix: None,
            bounds: Bounds::default(),
        })
    }

    pub fn insert_panel(
        &mut self,
        size: Option<Pixels>,
        index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let panel_state = ResizablePanelState {
            size,
            ..Default::default()
        };

        if let Some(index) = index {
            self.panels.insert(index, panel_state);
            self.sizes.insert(index, size.unwrap_or(PANEL_MIN_SIZE));
        } else {
            self.panels.push(panel_state);
            self.sizes.push(size.unwrap_or(PANEL_MIN_SIZE));
        }

        cx.notify();
    }

    pub fn remove_panel(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.panels.len() {
            return;
        }

        self.panels.remove(index);
        self.sizes.remove(index);

        if let Some(resizing_ix) = self.resizing_panel_ix {
            if resizing_ix > index {
                self.resizing_panel_ix = Some(resizing_ix - 1);
            } else if resizing_ix == index {
                self.resizing_panel_ix = None;
            }
        }

        cx.notify();
    }

    pub fn sizes(&self) -> &[Pixels] {
        &self.sizes
    }

    pub fn total_size(&self) -> Pixels {
        self.sizes.iter().fold(px(0.0), |acc, &size| acc + size)
    }

    pub fn clear(&mut self) {
        self.panels.clear();
        self.sizes.clear();
    }

    fn sync_panels_count(&mut self, axis: Axis, panels_count: usize) {
        self.axis = axis;

        if panels_count > self.panels.len() {
            let diff = panels_count - self.panels.len();
            self.panels
                .extend(vec![ResizablePanelState::default(); diff]);
            self.sizes.extend(vec![PANEL_MIN_SIZE; diff]);
        } else if panels_count < self.panels.len() {
            self.panels.truncate(panels_count);
            self.sizes.truncate(panels_count);
            if self
                .resizing_panel_ix
                .is_some_and(|index| index >= panels_count.saturating_sub(1))
            {
                self.resizing_panel_ix = None;
            }
        }
    }

    fn update_panel_size(
        &mut self,
        index: usize,
        bounds: Bounds<Pixels>,
        size_range: Range<Pixels>,
        _cx: &mut Context<Self>,
    ) {
        if index >= self.panels.len() {
            return;
        }

        let size = bounds.size.along(self.axis);
        self.sizes[index] = size;
        self.panels[index].size = Some(size);
        self.panels[index].bounds = bounds;
        self.panels[index].size_range = size_range;
    }

    fn done_resizing(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.resizing_panel_ix {
            let new_size = self.sizes.get(index).copied().unwrap_or(PANEL_MIN_SIZE);

            cx.emit(ResizablePanelEvent::Resized {
                panel_index: index,
                new_size,
            });
        }

        self.resizing_panel_ix = None;
    }

    fn panel_size_range(&self, index: usize) -> Range<Pixels> {
        self.panels
            .get(index)
            .map(|p| p.size_range.clone())
            .unwrap_or(PANEL_MIN_SIZE..Pixels::MAX)
    }

    fn sync_real_panel_sizes(&mut self, _: &App) {
        for (i, panel) in self.panels.iter().enumerate() {
            if i < self.sizes.len() {
                self.sizes[i] = panel.bounds.size.along(self.axis).floor();
            }
        }
    }

    fn resize_panel(&mut self, index: usize, size: Pixels, _: &mut Window, cx: &mut Context<Self>) {
        if self.sizes.len() < 2 || index >= self.sizes.len() - 1 {
            return;
        }

        let size = size.floor();
        let container_size = self.bounds.size.along(self.axis);

        self.sync_real_panel_sizes(cx);
        let old_sizes = self.sizes.clone();

        let move_changed = size - old_sizes[index];
        if move_changed == px(0.0) {
            return;
        }

        let size_range = self.panel_size_range(index);
        let new_size = size.clamp(size_range.start, size_range.end);
        let is_expand = move_changed > px(0.0);

        let main_ix = index;
        let mut new_sizes = old_sizes.clone();
        let mut ix = index;

        if is_expand {
            let mut changed = new_size - old_sizes[index];
            new_sizes[index] = new_size;

            while changed > px(0.0) && ix < old_sizes.len() - 1 {
                ix += 1;
                let size_range = self.panel_size_range(ix);
                let available_size = (new_sizes[ix] - size_range.start).max(px(0.0));
                let to_reduce = changed.min(available_size);
                new_sizes[ix] -= to_reduce;
                changed -= to_reduce;
            }
        } else {
            let mut changed = old_sizes[index] - new_size;
            new_sizes[index + 1] += changed;
            new_sizes[index] = new_size;

            let right_size_range = self.panel_size_range(index + 1);
            if new_sizes[index + 1] > right_size_range.end {
                let overflow = new_sizes[index + 1] - right_size_range.end;
                new_sizes[index + 1] = right_size_range.end;
                changed = overflow;

                while changed > px(0.0) && ix > 0 {
                    ix -= 1;
                    let size_range = self.panel_size_range(ix);
                    let available_size = (new_sizes[ix] - size_range.start).max(px(0.0));
                    let to_reduce = changed.min(available_size);
                    changed -= to_reduce;
                    new_sizes[ix] -= to_reduce;
                }
            }
        }

        let total_size: Pixels = new_sizes.iter().fold(px(0.0), |acc, &size| acc + size);
        if total_size > container_size {
            let overflow = total_size - container_size;
            let size_range = self.panel_size_range(main_ix);
            new_sizes[main_ix] = (new_sizes[main_ix] - overflow).max(size_range.start);
        }

        for (i, _) in old_sizes.iter().enumerate() {
            if i < new_sizes.len() && i < self.panels.len() {
                let size = new_sizes[i];
                self.panels[i].size = Some(size);
            }
        }

        self.sizes = new_sizes;
        cx.notify();
    }
}

impl EventEmitter<ResizablePanelEvent> for ResizableState {}

#[derive(Debug, Clone, Default)]
struct ResizablePanelState {
    size: Option<Pixels>,
    size_range: Range<Pixels>,
    bounds: Bounds<Pixels>,
}

/// A container for resizable panels with drag handles between them.
#[derive(IntoElement)]
pub struct ResizablePanelGroup {
    id: ElementId,
    state: Entity<ResizableState>,
    axis: Axis,
    children: Vec<ResizablePanel>,
}

impl ResizablePanelGroup {
    fn new(id: impl Into<ElementId>, state: Entity<ResizableState>) -> Self {
        Self {
            id: id.into(),
            axis: Axis::Horizontal,
            children: vec![],
            state,
        }
    }

    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    pub fn child(mut self, panel: impl Into<ResizablePanel>) -> Self {
        self.children.push(panel.into());
        self
    }

    pub fn children<I>(mut self, panels: impl IntoIterator<Item = I>) -> Self
    where
        I: Into<ResizablePanel>,
    {
        self.children = panels.into_iter().map(|panel| panel.into()).collect();
        self
    }

    pub fn group(self, group: ResizablePanelGroup) -> Self {
        self.child(resizable_panel().child(group.into_any_element()))
    }
}

impl<T> From<T> for ResizablePanel
where
    T: Into<AnyElement>,
{
    fn from(value: T) -> Self {
        resizable_panel().child(value.into())
    }
}

impl EventEmitter<ResizablePanelEvent> for ResizablePanelGroup {}

impl RenderOnce for ResizablePanelGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = self.state.clone();
        let group_key: SharedString = format!("{}", self.id).into();

        let panels_count = self.children.len();
        self.state.update(cx, |state, _| {
            state.sync_panels_count(self.axis, panels_count);
        });

        let container = div()
            .id(self.id.clone())
            .flex()
            .size_full()
            .when(self.axis.is_horizontal(), |this| this.flex_row())
            .when(self.axis.is_vertical(), |this| this.flex_col());

        container
            .children(
                self.children
                    .into_iter()
                    .enumerate()
                    .map(|(index, mut panel)| {
                        panel.index = index;
                        panel.axis = self.axis;
                        panel.state = Some(self.state.clone());
                        panel.group_key = group_key.clone();
                        panel
                    }),
            )
            .child({
                canvas_with_prepaint(
                    move |bounds, _, cx| {
                        state.update(cx, |state, _| {
                            state.bounds = bounds;
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .child(ResizePanelGroupElement {
                state: self.state.clone(),
                axis: self.axis,
            })
    }
}

/// A single resizable panel within a ResizablePanelGroup.
#[derive(IntoElement)]
pub struct ResizablePanel {
    axis: Axis,
    index: usize,
    state: Option<Entity<ResizableState>>,
    group_key: SharedString,
    initial_size: Option<Pixels>,
    size_range: Range<Pixels>,
    children: Vec<AnyElement>,
    visible: bool,
    style: StyleRefinement,
}

impl ResizablePanel {
    fn new() -> Self {
        Self {
            index: 0,
            initial_size: None,
            state: None,
            group_key: "resizable-group".into(),
            size_range: (PANEL_MIN_SIZE..Pixels::MAX),
            axis: Axis::Horizontal,
            children: vec![],
            visible: true,
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.initial_size = Some(size.into());
        self
    }

    pub fn size_range(mut self, range: impl Into<Range<Pixels>>) -> Self {
        self.size_range = range.into();
        self
    }

    pub fn min_size(mut self, min: impl Into<Pixels>) -> Self {
        self.size_range.start = min.into();
        self
    }

    pub fn max_size(mut self, max: impl Into<Pixels>) -> Self {
        self.size_range.end = max.into();
        self
    }
}

impl Styled for ResizablePanel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ResizablePanel {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let panel_id = ElementId::NamedInteger(
            format!("resizable-panel-{}", self.group_key).into(),
            self.index as u64,
        );
        if !self.visible {
            return div().id(panel_id).into_any_element();
        }

        let state = self
            .state
            .as_ref()
            .expect("ResizablePanel must be used within a ResizablePanelGroup");

        let panel_state = state.read(cx).panels.get(self.index).cloned();

        let size_range = self.size_range.clone();
        let has_custom_size =
            self.initial_size.is_some() || panel_state.as_ref().and_then(|p| p.size).is_some();

        let mut panel_div = div().id(panel_id).flex().flex_grow().size_full().relative();

        panel_div = panel_div.when(self.axis.is_vertical(), |this| {
            this.min_h(size_range.start).max_h(size_range.end)
        });

        panel_div = panel_div.when(self.axis.is_horizontal(), |this| {
            this.min_w(size_range.start).max_w(size_range.end)
        });

        panel_div = panel_div.when(!has_custom_size, |this| this.flex_shrink());

        if let Some(initial_size) = self.initial_size {
            let should_use_flex_none = panel_state
                .as_ref()
                .map(|p| p.size.is_none() && !initial_size.is_zero())
                .unwrap_or(false);

            panel_div = panel_div
                .when(should_use_flex_none, |this| this.flex_none())
                .flex_basis(initial_size);
        }

        if let Some(panel_state) = panel_state.as_ref() {
            if let Some(size) = panel_state.size {
                panel_div = panel_div.flex_basis(size);
            }
        }

        let user_style = self.style;

        panel_div
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .children(self.children)
            .when(self.index > 0, |this| {
                let handle_index = self.index - 1;
                let state = state.clone();
                let drag_state = state.clone();

                this.child(ResizePanelHandle::new(
                    ElementId::NamedInteger(
                        format!("resizable-handle-{}", self.group_key).into(),
                        handle_index as u64,
                    ),
                    self.axis,
                    DragPanel,
                    move |drag_panel, _, _, cx| {
                        cx.stop_propagation();
                        drag_state.update(cx, |state, cx| {
                            state.resizing_panel_ix = Some(handle_index);
                            cx.notify();
                        });
                        cx.new(|_| (*drag_panel).clone())
                    },
                    move |delta, window, cx| {
                        state.update(cx, |state, cx| {
                            state.sync_real_panel_sizes(cx);
                            let current = state
                                .sizes
                                .get(handle_index)
                                .copied()
                                .unwrap_or(PANEL_MIN_SIZE);
                            state.resizing_panel_ix = Some(handle_index);
                            state.resize_panel(handle_index, current + delta, window, cx);
                            state.done_resizing(cx);
                            cx.notify();
                        });
                    },
                ))
            })
            .child({
                let state = state.clone();
                let index = self.index;
                let size_range = self.size_range.clone();

                canvas_with_prepaint(
                    move |bounds, _, cx| {
                        state.update(cx, |state, cx| {
                            state.update_panel_size(index, bounds, size_range.clone(), cx)
                        })
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
            .into_any_element()
    }
}

#[derive(Clone)]
struct DragPanel;

impl Render for DragPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        Empty
    }
}

struct ResizePanelHandle<T: 'static, E: 'static + Render> {
    id: ElementId,
    axis: Axis,
    drag_value: Rc<T>,
    on_drag: Rc<dyn Fn(Rc<T>, &Point<Pixels>, &mut Window, &mut App) -> Entity<E>>,
    on_adjust: Rc<dyn Fn(Pixels, &mut Window, &mut App)>,
}

impl<T: 'static, E: 'static + Render> ResizePanelHandle<T, E> {
    fn new(
        id: impl Into<ElementId>,
        axis: Axis,
        value: T,
        f: impl Fn(Rc<T>, &Point<Pixels>, &mut Window, &mut App) -> Entity<E> + 'static,
        on_adjust: impl Fn(Pixels, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            axis,
            drag_value: Rc::new(value),
            on_drag: Rc::new(f),
            on_adjust: Rc::new(on_adjust),
        }
    }
}

#[derive(Default, Debug, Clone)]
struct ResizeHandleState {
    active: Cell<bool>,
}

impl ResizeHandleState {
    fn set_active(&self, active: bool) {
        self.active.set(active);
    }

    fn is_active(&self) -> bool {
        self.active.get()
    }
}

impl<T: 'static, E: 'static + Render> IntoElement for ResizePanelHandle<T, E> {
    type Element = ResizePanelHandle<T, E>;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<T: 'static, E: 'static + Render> Element for ResizePanelHandle<T, E> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&kael::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (kael::LayoutId, Self::RequestLayoutState) {
        let neg_offset = -HANDLE_PADDING;
        let axis = self.axis;
        let theme = use_theme();
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();

        window.with_element_state(id.unwrap(), |state, window| {
            let state = state.unwrap_or_else(ResizeHandleState::default);

            let bg_color = if state.is_active() {
                theme.tokens.primary
            } else {
                theme.tokens.border
            };

            let mut handle_element = div()
                .id(self.id.clone())
                .occlude()
                .absolute()
                .flex_shrink_0()
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Separator)
                        .label("Resize adjacent panels")
                        .actions(vec![
                            AccessibilityAction::Focus,
                            AccessibilityAction::Increment,
                            AccessibilityAction::Decrement,
                        ]),
                )
                .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                .group("handle");

            let on_drag = self.on_drag.clone();
            let drag_value = self.drag_value.clone();
            handle_element = handle_element
                .on_drag(drag_value.clone(), move |_, position, window, cx| {
                    (on_drag)(drag_value.clone(), &position, window, cx)
                });

            let increment = self.on_adjust.clone();
            let decrement = self.on_adjust.clone();
            let keyboard_adjust = self.on_adjust.clone();
            handle_element = handle_element
                .on_accessibility_action(AccessibilityAction::Increment, move |_, window, cx| {
                    increment(px(16.0), window, cx)
                })
                .on_accessibility_action(AccessibilityAction::Decrement, move |_, window, cx| {
                    decrement(px(-16.0), window, cx)
                })
                .on_key_down(move |event, window, cx| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let delta = match (axis, event.keystroke.key.as_str()) {
                        (Axis::Horizontal, "left") | (Axis::Vertical, "up") => px(-16.0),
                        (Axis::Horizontal, "right") | (Axis::Vertical, "down") => px(16.0),
                        _ => return,
                    };
                    keyboard_adjust(delta, window, cx);
                    window.prevent_default();
                    cx.stop_propagation();
                });

            handle_element = match axis {
                Axis::Horizontal => handle_element
                    .cursor_col_resize()
                    .top_0()
                    .left(neg_offset)
                    .h_full()
                    .w(HANDLE_SIZE)
                    .px(HANDLE_PADDING),
                Axis::Vertical => handle_element
                    .cursor_row_resize()
                    .top(neg_offset)
                    .left_0()
                    .w_full()
                    .h(HANDLE_SIZE)
                    .py(HANDLE_PADDING),
            };

            handle_element = handle_element.child(
                div()
                    .bg(bg_color)
                    .rounded_full()
                    .group_hover("handle", |this| this.bg(theme.tokens.primary))
                    .when(axis.is_horizontal(), |this| this.h_full().w(HANDLE_SIZE))
                    .when(axis.is_vertical(), |this| this.w_full().h(HANDLE_SIZE)),
            );

            let mut el = handle_element.into_any_element();
            let layout_id = el.request_layout(window, cx);

            ((layout_id, el), state)
        })
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&kael::InspectorElementId>,
        _: kael::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        request_layout.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&kael::InspectorElementId>,
        bounds: kael::Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        request_layout.paint(window, cx);

        window.with_element_state(id.unwrap(), |state: Option<ResizeHandleState>, window| {
            let state = state.unwrap_or_default();

            window.on_mouse_event({
                let state = state.clone();
                move |event: &MouseDownEvent, phase, window, _| {
                    if bounds.contains(&event.position) && phase.bubble() {
                        state.set_active(true);
                        window.refresh();
                    }
                }
            });

            window.on_mouse_event({
                let state = state.clone();
                move |_: &MouseUpEvent, _, window, _| {
                    if state.is_active() {
                        state.set_active(false);
                        window.refresh();
                    }
                }
            });

            ((), state)
        });
    }
}

struct ResizePanelGroupElement {
    state: Entity<ResizableState>,
    axis: Axis,
}

impl IntoElement for ResizePanelGroupElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ResizePanelGroupElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<kael::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&kael::GlobalElementId>,
        _: Option<&kael::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (kael::LayoutId, Self::RequestLayoutState) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&kael::GlobalElementId>,
        _: Option<&kael::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _: Option<&kael::GlobalElementId>,
        _: Option<&kael::InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        window.on_mouse_event({
            let state = self.state.clone();
            let axis = self.axis;
            move |event: &MouseMoveEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }

                let Some(panel_index) = state.read(cx).resizing_panel_ix else {
                    return;
                };

                state.update(cx, |state, cx| {
                    if let Some(panel) = state.panels.get(panel_index) {
                        let new_size = match axis {
                            Axis::Horizontal => event.position.x - panel.bounds.left(),
                            Axis::Vertical => event.position.y - panel.bounds.top(),
                        };

                        state.resize_panel(panel_index, new_size, window, cx);
                    }

                    cx.notify();
                });
            }
        });

        window.on_mouse_event({
            let state = self.state.clone();
            move |_: &MouseUpEvent, phase, _, cx| {
                if state.read(cx).resizing_panel_ix.is_none() {
                    return;
                }

                if phase.bubble() {
                    state.update(cx, |state, cx| {
                        state.done_resizing(cx);
                    });
                }
            }
        });
    }
}
