use crate::{
    AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, AppContext, Bounds, Context, DismissEvent, Element, ElementId, Entity,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, InspectorElementId, InteractiveElement,
    IntoElement, KeyDownEvent, LayerAnchor, LayerOptions, LayerStack, LayoutId, ParentElement,
    Pixels, Point, Render, StatefulInteractiveElement, Styled, Window, div, point, px,
};
use std::rc::Rc;

type ChangeListener = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of popover anchor state passed to a custom renderer.
pub struct PopoverAnchorRenderState {
    /// Whether the popover is currently open.
    pub open: bool,
}

type PopoverAnchorRenderer = Rc<dyn Fn(PopoverAnchorRenderState, &Window, &App) -> AnyElement>;

#[non_exhaustive]
/// Snapshot of popover popup state passed to a custom renderer.
pub struct PopoverPopupRenderState {
    /// Whether the popover is currently open.
    pub open: bool,
    /// The resolved popup width derived from the anchor bounds.
    pub width: Pixels,
    /// The current anchor bounds, when they have been measured.
    pub anchor_bounds: Option<Bounds<Pixels>>,
}

type PopoverPopupRenderer = Rc<dyn Fn(PopoverPopupRenderState, &Window, &App) -> AnyElement>;

/// Construct a controlled anchored popover primitive.
#[track_caller]
pub fn popover(id: impl Into<ElementId>, open: bool) -> Popover {
    Popover::new(id.into(), open)
}

/// A controlled anchored popover with caller-owned anchor and popup visuals.
pub struct Popover {
    element_id: ElementId,
    open: bool,
    on_open_change: Option<ChangeListener>,
    anchor_renderer: Option<PopoverAnchorRenderer>,
    popup_renderer: Option<PopoverPopupRenderer>,
    dismiss_on_click_outside: bool,
    dismiss_on_escape: bool,
    offset: Point<Pixels>,
    source_location: &'static core::panic::Location<'static>,
}

#[doc(hidden)]
pub struct PopoverElementState {
    root: AnyElement,
    state: Entity<PopoverState>,
}

struct PopoverState {
    open: bool,
    layer_stack: Entity<LayerStack>,
    trigger_bounds: Option<Bounds<Pixels>>,
    on_open_change: Option<ChangeListener>,
    popup_renderer: Option<PopoverPopupRenderer>,
    dismiss_on_click_outside: bool,
    dismiss_on_escape: bool,
    offset: Point<Pixels>,
}

struct PopoverPopup {
    selector_id: String,
    state: Entity<PopoverState>,
    root_focus: FocusHandle,
}

impl Popover {
    #[track_caller]
    fn new(element_id: ElementId, open: bool) -> Self {
        Self {
            element_id,
            open,
            on_open_change: None,
            anchor_renderer: None,
            popup_renderer: None,
            dismiss_on_click_outside: true,
            dismiss_on_escape: true,
            offset: point(px(0.0), px(4.0)),
            source_location: core::panic::Location::caller(),
        }
    }

    /// Register a callback invoked when user interaction requests a new open state.
    pub fn on_open_change(
        mut self,
        listener: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(listener));
        self
    }

    /// Render the anchor with caller-owned visuals and interaction handlers.
    pub fn render_anchor_with(
        mut self,
        renderer: impl Fn(PopoverAnchorRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.anchor_renderer = Some(Rc::new(renderer));
        self
    }

    /// Render the anchored popup with caller-owned visuals.
    pub fn render_popup_with(
        mut self,
        renderer: impl Fn(PopoverPopupRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.popup_renderer = Some(Rc::new(renderer));
        self
    }

    /// Control whether clicking outside the popup requests dismissal.
    pub fn dismiss_on_click_outside(mut self, dismiss_on_click_outside: bool) -> Self {
        self.dismiss_on_click_outside = dismiss_on_click_outside;
        self
    }

    /// Control whether pressing escape inside the popup requests dismissal.
    pub fn dismiss_on_escape(mut self, dismiss_on_escape: bool) -> Self {
        self.dismiss_on_escape = dismiss_on_escape;
        self
    }

    /// Offset the popup relative to the anchor position.
    pub fn offset(mut self, offset: Point<Pixels>) -> Self {
        self.offset = offset;
        self
    }

    fn build_anchor(
        &self,
        window: &mut Window,
        cx: &mut App,
        state: Entity<PopoverState>,
    ) -> AnyElement {
        let render_state = PopoverAnchorRenderState {
            open: state.read(cx).open,
        };

        let mut anchor = div().id(self.element_id.clone());
        let body = if let Some(renderer) = &self.anchor_renderer {
            renderer(render_state, window, cx)
        } else {
            default_popover_anchor(render_state)
        };
        anchor = anchor.child(body);

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("popover-{}", self.element_id);
            anchor = anchor.debug_selector(move || selector);
        }

        anchor.into_any_element()
    }
}

impl Element for Popover {
    type RequestLayoutState = PopoverElementState;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.element_id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        Some(self.source_location)
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let global_id = global_id.expect("popover requires a global id");
        let current_view = window.current_view();
        let selector_id = self.element_id.to_string();
        let open = self.open;
        let on_open_change = self.on_open_change.clone();
        let popup_renderer = self.popup_renderer.clone();
        let dismiss_on_click_outside = self.dismiss_on_click_outside;
        let dismiss_on_escape = self.dismiss_on_escape;
        let offset = self.offset;

        let state =
            window.with_element_state(global_id, |state: Option<Entity<PopoverState>>, _| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let layer_stack = cx.new(|_| LayerStack::new());
                    let state = cx.new(|_| {
                        PopoverState::new(
                            open,
                            layer_stack.clone(),
                            on_open_change.clone(),
                            popup_renderer.clone(),
                            dismiss_on_click_outside,
                            dismiss_on_escape,
                            offset,
                        )
                    });

                    cx.observe(&state, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();

                    cx.observe(&layer_stack, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();

                    (state.clone(), state)
                }
            });

        let state_entity = state.clone();
        state.update(cx, |state, cx| {
            state.sync_from_props(
                open,
                self.on_open_change.clone(),
                self.popup_renderer.clone(),
                self.dismiss_on_click_outside,
                self.dismiss_on_escape,
                self.offset,
                cx,
            );
            state.sync_visibility(state_entity.clone(), &selector_id, window, cx);
        });

        let anchor = self.build_anchor(window, cx, state.clone());
        let overlay_origin = state.read(cx).trigger_bounds.map(|bounds| bounds.origin);
        let overlay =
            build_layer_overlay(state.read(cx).layer_stack.clone(), overlay_origin, window);
        let mut root = div()
            .relative()
            .child(anchor)
            .child(overlay)
            .into_any_element();
        let layout_id = root.request_layout(window, cx);

        (layout_id, PopoverElementState { root, state })
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let selector_id = self.element_id.to_string();
        let state_entity = request_layout.state.clone();

        request_layout.state.update(cx, |state, cx| {
            state.set_trigger_bounds(bounds, state_entity.clone(), &selector_id, window, cx);
        });
        request_layout.root.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        request_layout.root.paint(window, cx);
    }
}

impl IntoElement for Popover {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl PopoverState {
    fn new(
        open: bool,
        layer_stack: Entity<LayerStack>,
        on_open_change: Option<ChangeListener>,
        popup_renderer: Option<PopoverPopupRenderer>,
        dismiss_on_click_outside: bool,
        dismiss_on_escape: bool,
        offset: Point<Pixels>,
    ) -> Self {
        Self {
            open,
            layer_stack,
            trigger_bounds: None,
            on_open_change,
            popup_renderer,
            dismiss_on_click_outside,
            dismiss_on_escape,
            offset,
        }
    }

    fn sync_from_props(
        &mut self,
        open: bool,
        on_open_change: Option<ChangeListener>,
        popup_renderer: Option<PopoverPopupRenderer>,
        dismiss_on_click_outside: bool,
        dismiss_on_escape: bool,
        offset: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let changed = self.open != open
            || self.on_open_change.as_ref().map(Rc::as_ptr)
                != on_open_change.as_ref().map(Rc::as_ptr)
            || self.popup_renderer.as_ref().map(Rc::as_ptr)
                != popup_renderer.as_ref().map(Rc::as_ptr)
            || self.dismiss_on_click_outside != dismiss_on_click_outside
            || self.dismiss_on_escape != dismiss_on_escape
            || self.offset != offset;

        self.open = open;
        self.on_open_change = on_open_change;
        self.popup_renderer = popup_renderer;
        self.dismiss_on_click_outside = dismiss_on_click_outside;
        self.dismiss_on_escape = dismiss_on_escape;
        self.offset = offset;

        if changed {
            cx.notify();
        }
    }

    fn sync_visibility(
        &mut self,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let popup_open = !self.layer_stack.read(cx).is_empty();

        match (self.open, popup_open) {
            (true, false) => self.open_popup(state, selector_id, window, cx),
            (false, true) => self.close_popup(cx),
            _ => {}
        }
    }

    fn set_trigger_bounds(
        &mut self,
        bounds: Bounds<Pixels>,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.trigger_bounds != Some(bounds);
        self.trigger_bounds = Some(bounds);

        if changed && self.open {
            self.open_popup(state, selector_id, window, cx);
        }
    }

    fn open_popup(
        &mut self,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(anchor_position) = self.anchor_position() else {
            return;
        };

        let popup = cx.new({
            let selector_id = selector_id.to_string();
            move |cx| PopoverPopup::new(selector_id.clone(), state, cx)
        });

        let anchor = LayerAnchor::at(anchor_position)
            .offset(self.offset)
            .snap_to_window();

        self.layer_stack.update(cx, |stack, cx| {
            stack.clear(cx);
            let mut options = LayerOptions::default().anchored(anchor).priority(100);
            if self.dismiss_on_click_outside {
                options = options.backdrop(crate::hsla(0.0, 0.0, 0.0, 0.0));
            }
            stack.push(popup, options, window, cx);
        });
        cx.notify();
    }

    fn close_popup(&mut self, cx: &mut Context<Self>) {
        self.layer_stack.update(cx, |stack, cx| {
            stack.clear(cx);
        });
        cx.notify();
    }

    fn request_open_change(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        if !open {
            self.close_popup(cx);
        }

        if let Some(listener) = self.on_open_change.clone() {
            listener(&open, window, cx);
        }
        cx.notify();
    }

    fn popup_width(&self) -> Pixels {
        self.trigger_bounds
            .map(|bounds| bounds.size.width.max(px(160.0)))
            .unwrap_or(px(160.0))
    }

    fn anchor_position(&self) -> Option<Point<Pixels>> {
        self.trigger_bounds
            .map(|bounds| point(bounds.left(), bounds.bottom()))
    }
}

impl PopoverPopup {
    fn new(selector_id: String, state: Entity<PopoverState>, cx: &mut Context<Self>) -> Self {
        cx.observe(&state, |_, _, cx| {
            cx.notify();
        })
        .detach();

        Self {
            selector_id,
            state,
            root_focus: cx.focus_handle(),
        }
    }
}

impl EventEmitter<DismissEvent> for PopoverPopup {}

impl Focusable for PopoverPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.root_focus.clone()
    }
}

impl Render for PopoverPopup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (snapshot, popup_renderer, dismiss_on_click_outside, dismiss_on_escape) = {
            let state = self.state.read(cx);
            (
                PopoverPopupRenderState {
                    open: true,
                    width: state.popup_width(),
                    anchor_bounds: state.trigger_bounds,
                },
                state.popup_renderer.clone(),
                state.dismiss_on_click_outside,
                state.dismiss_on_escape,
            )
        };

        let dismiss_on_click_state = self.state.clone();
        let dismiss_on_escape_state = self.state.clone();

        let mut popup = div()
            .id(ElementId::named_usize(
                format!("{}-popup", self.selector_id),
                0,
            ))
            .track_focus(&self.root_focus)
            .focusable()
            .tab_stop(true)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .states(if self.root_focus.is_focused(window) {
                        AccessibilityState::FOCUSED
                    } else {
                        AccessibilityState::NONE
                    })
                    .actions(vec![AccessibilityAction::Focus]),
            );

        if dismiss_on_click_outside {
            popup = popup.on_mouse_down_out(move |_, window, cx| {
                dismiss_on_click_state.update(cx, |state, cx| {
                    state.request_open_change(false, window, cx);
                });
            });
        }

        if dismiss_on_escape {
            popup = popup.capture_key_down(move |event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }

                if event.keystroke.key == "escape" {
                    dismiss_on_escape_state.update(cx, |state, cx| {
                        state.request_open_change(false, window, cx);
                    });
                    window.prevent_default();
                }
            });
        }

        let body = if let Some(renderer) = popup_renderer {
            renderer(snapshot, window, cx)
        } else {
            default_popover_popup(snapshot)
        };

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("popover-popup-{}", self.selector_id);
            popup = popup.debug_selector(move || selector);
        }

        popup.child(body)
    }
}

fn default_popover_anchor(state: PopoverAnchorRenderState) -> AnyElement {
    div()
        .min_w(px(140.0))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(if state.open {
            crate::rgb(0x1d4ed8)
        } else {
            crate::rgb(0x94a3b8)
        })
        .bg(crate::rgb(0xffffff))
        .text_color(crate::rgb(0x0f172a))
        .child(if state.open {
            "Popover open"
        } else {
            "Popover closed"
        })
        .into_any_element()
}

fn default_popover_popup(state: PopoverPopupRenderState) -> AnyElement {
    div()
        .min_w(state.width)
        .p_2()
        .rounded(px(10.0))
        .border_1()
        .border_color(crate::rgb(0xcbd5e1))
        .bg(crate::rgb(0xffffff))
        .shadow_lg()
        .child("Popover")
        .into_any_element()
}

fn build_layer_overlay(
    stack: Entity<LayerStack>,
    origin: Option<Point<Pixels>>,
    window: &mut Window,
) -> AnyElement {
    let viewport = window.viewport_size();
    let origin = origin.unwrap_or_default();
    div()
        .absolute()
        .top(-origin.y)
        .left(-origin.x)
        .w(viewport.width)
        .h(viewport.height)
        .child(stack)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AppContext, Context, Modifiers, Render, TestAppContext, Window, button, div, point, rgb,
    };

    struct PopoverView {
        open: bool,
        background_clicks: usize,
    }

    struct PersistentPopoverView {
        open: bool,
    }

    impl Render for PopoverView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();

            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("popover-background")
                        .debug_selector(|| "popover-background".to_string())
                        .size_full()
                        .bg(rgb(0xf8fafc))
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.background_clicks += 1;
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    popover("help", self.open)
                        .render_anchor_with({
                            let view = view.clone();
                            move |state, _, _| {
                                let next_open = !state.open;
                                button("help-trigger")
                                    .label(if state.open { "Hide help" } else { "Show help" })
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _, cx| {
                                            view.update(cx, |this, cx| {
                                                this.open = next_open;
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .into_any_element()
                            }
                        })
                        .render_popup_with(|state, _, _| {
                            let selector = format!(
                                "popover-panel-{}",
                                if state.open { "open" } else { "closed" },
                            );
                            div()
                                .debug_selector(move || selector)
                                .w(state.width)
                                .h(px(84.0))
                                .rounded(px(10.0))
                                .border_1()
                                .border_color(rgb(0xcbd5e1))
                                .bg(rgb(0xffffff))
                                .child("Panel")
                                .into_any_element()
                        })
                        .on_open_change({
                            let view = view.clone();
                            move |open, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.open = *open;
                                    cx.notify();
                                });
                            }
                        }),
                )
        }
    }

    impl Render for PersistentPopoverView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let view = cx.entity();

            div().relative().size_full().child(
                popover("persistent-help", self.open)
                    .dismiss_on_click_outside(false)
                    .dismiss_on_escape(false)
                    .render_anchor_with({
                        let view = view.clone();
                        move |state, _, _| {
                            let next_open = !state.open;
                            button("persistent-help-trigger")
                                .label(if state.open { "Hide help" } else { "Show help" })
                                .on_click({
                                    let view = view.clone();
                                    move |_, _, cx| {
                                        view.update(cx, |this, cx| {
                                            this.open = next_open;
                                            cx.notify();
                                        });
                                    }
                                })
                                .into_any_element()
                        }
                    })
                    .render_popup_with(|state, _, _| {
                        let selector = format!(
                            "persistent-popover-panel-{}",
                            if state.open { "open" } else { "closed" },
                        );
                        div()
                            .debug_selector(move || selector)
                            .w(state.width)
                            .h(px(84.0))
                            .rounded(px(10.0))
                            .border_1()
                            .border_color(rgb(0xcbd5e1))
                            .bg(rgb(0xffffff))
                            .child("Persistent panel")
                            .into_any_element()
                    })
                    .on_open_change({
                        let view = view.clone();
                        move |open, _, cx| {
                            view.update(cx, |this, cx| {
                                this.open = *open;
                                cx.notify();
                            });
                        }
                    }),
            )
        }
    }

    #[crate::test]
    fn popover_open_prop_shows_popup_and_escape_closes(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| PopoverView {
            open: false,
            background_clicks: 0,
        });

        window.update(|window, cx| {
            view.update(cx, |this, cx| {
                this.open = true;
                cx.notify();
            });
            window.draw(cx).clear();
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("popover-panel-open").is_some());

        window.simulate_keystrokes("escape");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(!view.read(cx).open);
        });
    }

    #[crate::test]
    fn popover_dismisses_on_backdrop_click_without_clicking_background(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| PopoverView {
            open: true,
            background_clicks: 0,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("popover-panel-open").is_some());

        window.simulate_click(point(px(8.0), px(8.0)), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let (open, background_clicks) =
            cx.read_entity(&view, |view, _| (view.open, view.background_clicks));
        assert!(!open);
        assert_eq!(background_clicks, 0);
    }

    #[crate::test]
    fn popover_respects_dismiss_configuration(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| PersistentPopoverView { open: true });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("persistent-popover-panel-open")
                .is_some()
        );

        window.simulate_keystrokes("escape");
        window.simulate_click(point(px(8.0), px(8.0)), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(
            window
                .debug_bounds("persistent-popover-panel-open")
                .is_some()
        );
    }
}
