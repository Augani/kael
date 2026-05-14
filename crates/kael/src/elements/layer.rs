use crate::{
    AnchoredFitMode, AnchoredPositionMode, AnyElement, AnyView, Context, Corner, DismissEvent,
    Edges, Entity, Hsla, InteractiveElement, IntoElement, ManagedView, ParentElement, Pixels,
    Point, Render, Styled, Subscription, WeakEntity, Window, anchored, deferred, div, hsla,
};

/// A stable identifier for a layer managed by [`LayerStack`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(usize);

/// Controls how a layer is placed inside a [`LayerStack`].
#[derive(Clone, Copy, Default, PartialEq)]
pub enum LayerPlacement {
    /// Center the layer content within the window.
    #[default]
    Centered,
    /// Stretch the layer content to fill the window.
    Fullscreen,
    /// Position the layer using GPUI's anchored placement rules.
    Anchored(LayerAnchor),
}

/// Anchored placement configuration for a layer.
#[derive(Clone, Copy, PartialEq)]
pub struct LayerAnchor {
    anchor_corner: Corner,
    anchor_position: Option<Point<Pixels>>,
    position_mode: AnchoredPositionMode,
    fit_mode: AnchoredFitMode,
    offset: Point<Pixels>,
}

impl Default for LayerAnchor {
    fn default() -> Self {
        Self {
            anchor_corner: Corner::TopLeft,
            anchor_position: None,
            position_mode: AnchoredPositionMode::Window,
            fit_mode: AnchoredFitMode::SwitchAnchor,
            offset: Point::default(),
        }
    }
}

impl LayerAnchor {
    /// Creates an anchored placement using GPUI's default anchored behavior.
    pub fn new() -> Self {
        Self::default()
    }

    /// Anchors the layer to the given position.
    pub fn at(position: Point<Pixels>) -> Self {
        Self {
            anchor_position: Some(position),
            ..Self::default()
        }
    }

    /// Sets which corner of the anchored layer should be attached to the anchor position.
    pub fn anchor(mut self, anchor: Corner) -> Self {
        self.anchor_corner = anchor;
        self
    }

    /// Sets how the anchor position should be interpreted.
    pub fn position_mode(mut self, mode: AnchoredPositionMode) -> Self {
        self.position_mode = mode;
        self
    }

    /// Offsets the final anchored layer position.
    pub fn offset(mut self, offset: Point<Pixels>) -> Self {
        self.offset = offset;
        self
    }

    /// Snaps the anchored layer to the window edge when it would overflow.
    pub fn snap_to_window(mut self) -> Self {
        self.fit_mode = AnchoredFitMode::SnapToWindow;
        self
    }

    /// Snaps the anchored layer to the window edge while preserving the given margins.
    pub fn snap_to_window_with_margin(mut self, edges: impl Into<Edges<Pixels>>) -> Self {
        self.fit_mode = AnchoredFitMode::SnapToWindowWithMargin(edges.into());
        self
    }
}

/// Options that control how a layer behaves inside a [`LayerStack`].
#[derive(Clone, Copy, PartialEq)]
pub struct LayerOptions {
    placement: LayerPlacement,
    backdrop: Option<Hsla>,
    priority: usize,
    dismiss_on_click_outside: bool,
    dismiss_on_escape: bool,
}

impl Default for LayerOptions {
    fn default() -> Self {
        Self {
            placement: LayerPlacement::Centered,
            backdrop: None,
            priority: 0,
            dismiss_on_click_outside: false,
            dismiss_on_escape: false,
        }
    }
}

impl LayerOptions {
    /// Creates centered layer options appropriate for modal content.
    pub fn modal() -> Self {
        Self::default()
            .backdrop(hsla(0.0, 0.0, 0.0, 0.32))
            .dismiss_on_click_outside()
            .dismiss_on_escape()
            .priority(100)
    }

    /// Sets the placement used for the layer content.
    pub fn placement(mut self, placement: LayerPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Places the layer content using anchored positioning.
    pub fn anchored(mut self, anchor: LayerAnchor) -> Self {
        self.placement = LayerPlacement::Anchored(anchor);
        self
    }

    /// Places the layer content in the center of the window.
    pub fn centered(mut self) -> Self {
        self.placement = LayerPlacement::Centered;
        self
    }

    /// Places the layer content so it fills the window.
    pub fn fullscreen(mut self) -> Self {
        self.placement = LayerPlacement::Fullscreen;
        self
    }

    /// Paints a backdrop behind the layer content.
    pub fn backdrop(mut self, color: Hsla) -> Self {
        self.backdrop = Some(color);
        self
    }

    /// Sets the deferred draw priority for the layer.
    pub fn priority(mut self, priority: usize) -> Self {
        self.priority = priority;
        self
    }

    /// Dismisses the layer when the backdrop area is clicked.
    pub fn dismiss_on_click_outside(mut self) -> Self {
        self.dismiss_on_click_outside = true;
        self
    }

    /// Dismisses the layer when the escape key is pressed while it is focused.
    pub fn dismiss_on_escape(mut self) -> Self {
        self.dismiss_on_escape = true;
        self
    }

    fn needs_backdrop_capture(&self) -> bool {
        self.backdrop.is_some() || self.dismiss_on_click_outside
    }
}

struct LayerEntry {
    id: LayerId,
    view: AnyView,
    options: LayerOptions,
    _subscription: Subscription,
}

/// A stack of in-window layers such as modals and popovers.
pub struct LayerStack {
    next_layer_id: usize,
    layers: Vec<LayerEntry>,
}

impl LayerStack {
    /// Creates an empty layer stack.
    pub fn new() -> Self {
        Self {
            next_layer_id: 0,
            layers: Vec::new(),
        }
    }

    /// Returns the number of active layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Returns whether the stack currently has no layers.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// Pushes a managed view onto the stack and returns its layer identifier.
    pub fn push<V: ManagedView>(
        &mut self,
        view: Entity<V>,
        options: LayerOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> LayerId {
        let id = LayerId(self.next_layer_id);
        self.next_layer_id += 1;

        let dismiss_subscription = cx.subscribe(&view, move |stack, _, _: &DismissEvent, cx| {
            stack.dismiss(id, cx);
        });
        let release_subscription = cx.observe_release(&view, move |stack, _, cx| {
            stack.dismiss(id, cx);
        });

        cx.focus_view(&view, window);
        self.layers.push(LayerEntry {
            id,
            view: view.into(),
            options,
            _subscription: Subscription::join(dismiss_subscription, release_subscription),
        });
        cx.notify();
        id
    }

    /// Dismisses the layer with the given identifier if it is present.
    pub fn dismiss(&mut self, layer_id: LayerId, cx: &mut Context<Self>) -> bool {
        let original_len = self.layers.len();
        self.layers.retain(|entry| entry.id != layer_id);
        let dismissed = self.layers.len() != original_len;
        if dismissed {
            cx.notify();
        }
        dismissed
    }

    /// Removes all layers from the stack.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.layers.is_empty() {
            return;
        }

        self.layers.clear();
        cx.notify();
    }
}

impl Render for LayerStack {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut stack_root = div().absolute().top_0().left_0().w_full().h_full();
        let stack = cx.weak_entity();

        for layer in &self.layers {
            stack_root = stack_root.child(render_layer(layer, stack.clone()));
        }

        stack_root.into_any_element()
    }
}

fn render_layer(layer: &LayerEntry, stack: WeakEntity<LayerStack>) -> AnyElement {
    let mut overlay = div().absolute().top_0().left_0().w_full().h_full();

    if layer.options.needs_backdrop_capture() {
        let mut backdrop = div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .occlude();

        if let Some(color) = layer.options.backdrop {
            backdrop = backdrop.bg(color);
        }

        overlay = overlay.child(backdrop);
    }

    overlay = overlay.child(render_layer_content(layer, stack));

    deferred(overlay)
        .priority(layer.options.priority)
        .into_any_element()
}

fn render_layer_content(layer: &LayerEntry, stack: WeakEntity<LayerStack>) -> AnyElement {
    let content = if layer.options.dismiss_on_click_outside || layer.options.dismiss_on_escape {
        let layer_id = layer.id;
        let mut content = div().child(layer.view.clone());

        if layer.options.dismiss_on_click_outside
            && !matches!(layer.options.placement, LayerPlacement::Fullscreen)
        {
            let dismiss_stack = stack.clone();
            content = content.on_mouse_down_out(move |_, _, cx| {
                let _ = dismiss_stack.update(cx, |stack, cx| {
                    stack.dismiss(layer_id, cx);
                });
            });
        }

        if layer.options.dismiss_on_escape {
            content = content.capture_key_down(move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    let _ = stack.update(cx, |stack, cx| {
                        stack.dismiss(layer_id, cx);
                    });
                }
            });
        }

        content.into_any_element()
    } else {
        layer.view.clone().into_any_element()
    };

    match layer.options.placement {
        LayerPlacement::Centered => div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(content)
            .into_any_element(),
        LayerPlacement::Fullscreen => div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h_full()
            .child(content)
            .into_any_element(),
        LayerPlacement::Anchored(anchor) => {
            let mut anchored_layer = anchored()
                .anchor(anchor.anchor_corner)
                .position_mode(anchor.position_mode)
                .offset(anchor.offset);

            if let Some(position) = anchor.anchor_position {
                anchored_layer = anchored_layer.position(position);
            }

            anchored_layer = match anchor.fit_mode {
                AnchoredFitMode::SwitchAnchor => anchored_layer,
                AnchoredFitMode::SnapToWindow => anchored_layer.snap_to_window(),
                AnchoredFitMode::SnapToWindowWithMargin(edges) => {
                    anchored_layer.snap_to_window_with_margin(edges)
                }
            };

            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h_full()
                .child(anchored_layer.child(content))
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement,
        Modifiers, ParentElement, StatefulInteractiveElement, TestAppContext, Window, div, point,
        px, rgb,
    };
    use std::{cell::Cell, rc::Rc};

    struct TestLayerView {
        focus: FocusHandle,
        clicks: Rc<Cell<usize>>,
    }

    impl EventEmitter<DismissEvent> for TestLayerView {}

    impl Focusable for TestLayerView {
        fn focus_handle(&self, _: &crate::App) -> FocusHandle {
            self.focus.clone()
        }
    }

    impl Render for TestLayerView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let clicks = self.clicks.clone();
            div()
                .track_focus(&self.focus)
                .id("panel")
                .debug_selector(|| "panel".to_string())
                .w(px(120.))
                .h(px(72.))
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(rgb(0xd0d0d0))
                .on_click(move |_, _, _| {
                    clicks.set(clicks.get() + 1);
                })
        }
    }

    struct RootView {
        layer_stack: Entity<LayerStack>,
        background_clicks: Rc<Cell<usize>>,
    }

    impl Render for RootView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let background_clicks = self.background_clicks.clone();
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("background")
                        .debug_selector(|| "background".to_string())
                        .size_full()
                        .bg(rgb(0xf2f2f2))
                        .on_click(move |_, _, _| {
                            background_clicks.set(background_clicks.get() + 1);
                        }),
                )
                .child(self.layer_stack.clone())
        }
    }

    #[kael::test]
    fn layer_stack_dismisses_modal_on_backdrop_click(cx: &mut TestAppContext) {
        let background_clicks = Rc::new(Cell::new(0));
        let panel_clicks = Rc::new(Cell::new(0));

        let (root, cx) = cx.add_window_view({
            let background_clicks = background_clicks.clone();
            let panel_clicks = panel_clicks.clone();
            move |window, cx| {
                let layer_stack = cx.new(|_| LayerStack::new());
                let layer = cx.new(|cx| TestLayerView {
                    focus: cx.focus_handle(),
                    clicks: panel_clicks.clone(),
                });

                layer_stack.update(cx, |stack, cx| {
                    stack.push(layer, LayerOptions::modal(), window, cx);
                });

                RootView {
                    layer_stack,
                    background_clicks: background_clicks.clone(),
                }
            }
        });

        let panel_bounds = cx.debug_bounds("panel").unwrap();
        cx.simulate_click(panel_bounds.center(), Modifiers::default());

        let layer_stack = cx.read_entity(&root, |root, _| root.layer_stack.clone());
        assert_eq!(panel_clicks.get(), 1);
        assert_eq!(background_clicks.get(), 0);
        assert_eq!(cx.read_entity(&layer_stack, |stack, _| stack.len()), 1);

        cx.simulate_click(point(px(8.), px(8.)), Modifiers::default());

        assert_eq!(background_clicks.get(), 0);
        assert!(cx.read_entity(&layer_stack, |stack, _| stack.is_empty()));
    }

    #[kael::test]
    fn layer_stack_dismisses_when_view_emits_dismiss_event(cx: &mut TestAppContext) {
        let panel_clicks = Rc::new(Cell::new(0));

        let (root, cx) = cx.add_window_view({
            let panel_clicks = panel_clicks.clone();
            move |window, cx| {
                let layer_stack = cx.new(|_| LayerStack::new());
                let layer = cx.new(|cx| TestLayerView {
                    focus: cx.focus_handle(),
                    clicks: panel_clicks.clone(),
                });

                layer_stack.update(cx, |stack, cx| {
                    stack.push(layer.clone(), LayerOptions::modal(), window, cx);
                });

                RootView {
                    layer_stack,
                    background_clicks: Rc::new(Cell::new(0)),
                }
            }
        });

        let layer_stack = cx.read_entity(&root, |root, _| root.layer_stack.clone());
        let layer = cx.read_entity(&layer_stack, |stack, _| {
            stack.layers[0]
                .view
                .clone()
                .downcast::<TestLayerView>()
                .unwrap()
        });

        layer.update(cx, |_, cx: &mut Context<TestLayerView>| {
            cx.emit(DismissEvent);
        });

        assert!(cx.read_entity(&layer_stack, |stack, _| stack.is_empty()));
    }

    #[kael::test]
    fn layer_stack_dismisses_modal_on_escape(cx: &mut TestAppContext) {
        let panel_clicks = Rc::new(Cell::new(0));

        let (root, cx) = cx.add_window_view({
            let panel_clicks = panel_clicks.clone();
            move |window, cx| {
                let layer_stack = cx.new(|_| LayerStack::new());
                let layer = cx.new(|cx| TestLayerView {
                    focus: cx.focus_handle(),
                    clicks: panel_clicks.clone(),
                });

                layer_stack.update(cx, |stack, cx| {
                    stack.push(layer, LayerOptions::modal(), window, cx);
                });

                RootView {
                    layer_stack,
                    background_clicks: Rc::new(Cell::new(0)),
                }
            }
        });

        let layer_stack = cx.read_entity(&root, |root, _| root.layer_stack.clone());
        assert_eq!(cx.read_entity(&layer_stack, |stack, _| stack.len()), 1);

        cx.simulate_keystrokes("escape");

        assert!(cx.read_entity(&layer_stack, |stack, _| stack.is_empty()));
    }

    #[kael::test]
    fn layer_stack_positions_anchored_layers_in_window_space(cx: &mut TestAppContext) {
        let panel_clicks = Rc::new(Cell::new(0));
        let anchor_position = point(px(180.), px(96.));
        let offset = point(px(12.), px(8.));

        let (_root, cx) = cx.add_window_view({
            let panel_clicks = panel_clicks.clone();
            move |window, cx| {
                let layer_stack = cx.new(|_| LayerStack::new());
                let layer = cx.new(|cx| TestLayerView {
                    focus: cx.focus_handle(),
                    clicks: panel_clicks.clone(),
                });

                layer_stack.update(cx, |stack, cx| {
                    stack.push(
                        layer,
                        LayerOptions::default()
                            .anchored(LayerAnchor::at(anchor_position).offset(offset)),
                        window,
                        cx,
                    );
                });

                RootView {
                    layer_stack,
                    background_clicks: Rc::new(Cell::new(0)),
                }
            }
        });

        let panel_bounds = cx.debug_bounds("panel").unwrap();
        assert_eq!(panel_bounds.origin, anchor_position + offset);
    }
}
