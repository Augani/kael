use crate::{
    div, hsla, px, AccessibilityAction, AccessibilityAttributes, AccessibilityRole,
    AccessibilityState, AnyElement, App, AppContext, Bounds, Context, DismissEvent, Element,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla,
    InspectorElementId, InteractiveElement, IntoElement, KeyDownEvent, LayerOptions, LayerStack,
    LayoutId, ParentElement, Pixels, Point, Render, SharedString,
    StatefulInteractiveElement, Styled, Window,
};
use std::rc::Rc;

type ChangeListener = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of modal state passed to a custom renderer.
pub struct ModalRenderState {
    /// Whether the modal is currently open.
    pub open: bool,
    /// The configured accessibility label for the dialog, if any.
    pub label: Option<SharedString>,
    /// Whether the modal currently owns keyboard focus.
    pub focused: bool,
}

type ModalCustomRenderer = Rc<dyn Fn(ModalRenderState, &Window, &App) -> AnyElement>;

/// Construct a controlled modal primitive.
#[track_caller]
pub fn modal(id: impl Into<ElementId>, open: bool) -> Modal {
    Modal::new(id.into(), open)
}

/// A controlled modal primitive with caller-owned dialog visuals.
pub struct Modal {
    element_id: ElementId,
    open: bool,
    label: Option<SharedString>,
    on_change: Option<ChangeListener>,
    custom_renderer: Option<ModalCustomRenderer>,
    backdrop: Hsla,
    dismiss_on_click_outside: bool,
    dismiss_on_escape: bool,
    source_location: &'static core::panic::Location<'static>,
}

#[doc(hidden)]
pub struct ModalElementState {
    root: AnyElement,
    state: Entity<ModalState>,
}

struct ModalState {
    focus_handle: FocusHandle,
    layer_stack: Entity<LayerStack>,
    open: bool,
    label: Option<SharedString>,
    on_change: Option<ChangeListener>,
    renderer: Option<ModalCustomRenderer>,
    backdrop: Hsla,
    dismiss_on_click_outside: bool,
    dismiss_on_escape: bool,
    root_origin: Point<Pixels>,
}

struct ModalLayer {
    selector_id: String,
    state: Entity<ModalState>,
    root_focus: FocusHandle,
}

impl Modal {
    #[track_caller]
    fn new(element_id: ElementId, open: bool) -> Self {
        Self {
            element_id,
            open,
            label: None,
            on_change: None,
            custom_renderer: None,
            backdrop: hsla(0.0, 0.0, 0.0, 0.32),
            dismiss_on_click_outside: true,
            dismiss_on_escape: true,
            source_location: core::panic::Location::caller(),
        }
    }

    /// Set the accessibility label exposed for the dialog.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Register a callback invoked when the modal requests an open-state change.
    pub fn on_change(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(listener));
        self
    }

    /// Render the dialog body with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(ModalRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }

    /// Set the backdrop color painted behind the dialog.
    pub fn backdrop(mut self, color: Hsla) -> Self {
        self.backdrop = color;
        self
    }

    /// Control whether clicking outside the dialog requests dismissal.
    pub fn dismiss_on_click_outside(mut self, dismiss_on_click_outside: bool) -> Self {
        self.dismiss_on_click_outside = dismiss_on_click_outside;
        self
    }

    /// Control whether pressing escape requests dismissal.
    pub fn dismiss_on_escape(mut self, dismiss_on_escape: bool) -> Self {
        self.dismiss_on_escape = dismiss_on_escape;
        self
    }
}

impl Element for Modal {
    type RequestLayoutState = ModalElementState;
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
        let global_id = global_id.expect("modal requires a global id");
        let modal_id = self.element_id.to_string();
        let current_view = window.current_view();
        let open = self.open;
        let label = self.label.clone();
        let on_change = self.on_change.clone();
        let custom_renderer = self.custom_renderer.clone();
        let backdrop = self.backdrop;
        let dismiss_on_click_outside = self.dismiss_on_click_outside;
        let dismiss_on_escape = self.dismiss_on_escape;

        let state = window.with_element_state(global_id, |state: Option<Entity<ModalState>>, _| {
            if let Some(state) = state {
                (state.clone(), state)
            } else {
                let layer_stack = cx.new(|_| LayerStack::new());
                let focus_handle = cx.focus_handle();
                let state_layer_stack = layer_stack.clone();
                let state = cx.new(move |_| {
                    ModalState::new(
                        focus_handle.clone(),
                        state_layer_stack.clone(),
                        open,
                        label,
                        on_change,
                        custom_renderer,
                        backdrop,
                        dismiss_on_click_outside,
                        dismiss_on_escape,
                    )
                });

                cx.observe(&layer_stack, {
                    move |_, cx| {
                        cx.notify(current_view);
                    }
                })
                .detach();

                (state.clone(), state)
            }
        });

        let modal_state = state.clone();
        state.update(cx, |state, cx| {
            state.sync_from_props(
                open,
                self.label.clone(),
                self.on_change.clone(),
                self.custom_renderer.clone(),
                self.backdrop,
                self.dismiss_on_click_outside,
                self.dismiss_on_escape,
                cx,
            );
            state.sync_visibility(modal_state.clone(), &modal_id, window, cx);
        });

        let overlay = build_layer_overlay(state.read(cx).layer_stack.clone(), state.read(cx).root_origin, window);
        let mut root = div()
            .relative()
            .w(px(0.0))
            .h(px(0.0))
            .child(overlay)
            .into_any_element();
        let layout_id = root.request_layout(window, cx);

        (layout_id, ModalElementState { root, state })
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
        let current_view = window.current_view();
        let origin = bounds.origin;
        let origin_changed = request_layout
            .state
            .update(cx, |state, _| state.set_root_origin(origin));
        if origin_changed {
            cx.notify(current_view);
        }

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

impl IntoElement for Modal {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl ModalState {
    fn new(
        focus_handle: FocusHandle,
        layer_stack: Entity<LayerStack>,
        open: bool,
        label: Option<SharedString>,
        on_change: Option<ChangeListener>,
        renderer: Option<ModalCustomRenderer>,
        backdrop: Hsla,
        dismiss_on_click_outside: bool,
        dismiss_on_escape: bool,
    ) -> Self {
        Self {
            focus_handle,
            layer_stack,
            open,
            label,
            on_change,
            renderer,
            backdrop,
            dismiss_on_click_outside,
            dismiss_on_escape,
            root_origin: Point::default(),
        }
    }

    fn sync_from_props(
        &mut self,
        open: bool,
        label: Option<SharedString>,
        on_change: Option<ChangeListener>,
        renderer: Option<ModalCustomRenderer>,
        backdrop: Hsla,
        dismiss_on_click_outside: bool,
        dismiss_on_escape: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = self.open != open
            || self.label != label
            || self.on_change.as_ref().map(Rc::as_ptr) != on_change.as_ref().map(Rc::as_ptr)
            || self.renderer.as_ref().map(Rc::as_ptr) != renderer.as_ref().map(Rc::as_ptr)
            || self.backdrop != backdrop
            || self.dismiss_on_click_outside != dismiss_on_click_outside
            || self.dismiss_on_escape != dismiss_on_escape;

        self.open = open;
        self.label = label;
        self.on_change = on_change;
        self.renderer = renderer;
        self.backdrop = backdrop;
        self.dismiss_on_click_outside = dismiss_on_click_outside;
        self.dismiss_on_escape = dismiss_on_escape;

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
        let is_open = !self.layer_stack.read(cx).is_empty();
        match (self.open, is_open) {
            (true, false) => self.open_layer(state, selector_id, window, cx),
            (false, true) => self.close_layer(cx),
            _ => {}
        }
    }

    fn open_layer(
        &mut self,
        state: Entity<Self>,
        selector_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modal = cx.new({
            let selector_id = selector_id.to_string();
            let focus_handle = self.focus_handle.clone();
            move |cx| ModalLayer::new(selector_id.clone(), state, focus_handle.clone(), cx)
        });

        self.layer_stack.update(cx, |stack, cx| {
            stack.clear(cx);
            stack.push(
                modal,
                LayerOptions::default()
                    .centered()
                    .backdrop(self.backdrop)
                    .priority(100),
                window,
                cx,
            );
        });

        window.focus(&self.focus_handle);
    }

    fn close_layer(&mut self, cx: &mut Context<Self>) {
        self.layer_stack.update(cx, |stack, cx| {
            stack.clear(cx);
        });
    }

    fn request_open_change(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !open {
            self.close_layer(cx);
            self.open = false;
        }

        if let Some(listener) = self.on_change.clone() {
            listener(&open, window, cx);
        }
    }

    fn set_root_origin(&mut self, origin: Point<Pixels>) -> bool {
        if self.root_origin == origin {
            return false;
        }

        self.root_origin = origin;
        true
    }
}

impl ModalLayer {
    fn new(
        selector_id: String,
        state: Entity<ModalState>,
        root_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&state, |_, _, cx| {
            cx.notify();
        })
        .detach();

        Self {
            selector_id,
            state,
            root_focus,
        }
    }
}

impl Focusable for ModalLayer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.root_focus.clone()
    }
}

impl EventEmitter<DismissEvent> for ModalLayer {}

impl Render for ModalLayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (label, renderer, dismiss_on_click_outside, dismiss_on_escape) = {
            let state = self.state.read(cx);
            (
                state.label.clone(),
                state.renderer.clone(),
                state.dismiss_on_click_outside,
                state.dismiss_on_escape,
            )
        };

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Dialog).states(
            if self.root_focus.is_focused(window) {
                AccessibilityState::FOCUSED
            } else {
                AccessibilityState::NONE
            },
        );
        if let Some(label) = label.as_ref() {
            accessibility = accessibility.label(label.to_string());
        }
        accessibility = accessibility.actions(vec![AccessibilityAction::Focus]);

        let dismiss_on_click_state = self.state.clone();
        let dismiss_on_escape_state = self.state.clone();
        let mut root = div()
            .id(ElementId::named_usize(format!("{}-modal", self.selector_id), 0))
            .track_focus(&self.root_focus)
            .focusable()
            .tab_stop(true)
            .accessibility(accessibility);

        if dismiss_on_click_outside {
            root = root.on_mouse_down_out(move |_, window, cx| {
                dismiss_on_click_state.update(cx, |state, cx| {
                    state.request_open_change(false, window, cx);
                });
            });
        }

        if dismiss_on_escape {
            root = root.on_key_down(move |event: &KeyDownEvent, window, cx| {
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

        let body = if let Some(renderer) = renderer {
            renderer(
                ModalRenderState {
                    open: true,
                    label: label.clone(),
                    focused: self.root_focus.is_focused(window),
                },
                window,
                cx,
            )
        } else {
            default_modal_body(label.clone())
        };

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("modal-{}", self.selector_id);
            root = root.debug_selector(move || selector);
        }

        root.child(body)
    }
}

fn default_modal_body(label: Option<SharedString>) -> AnyElement {
    div()
        .min_w(px(240.0))
        .px(px(20.0))
        .py(px(16.0))
        .rounded(px(12.0))
        .border_1()
        .border_color(crate::rgb(0xcbd5e1))
        .bg(crate::rgb(0xffffff))
        .shadow_lg()
        .child(label.unwrap_or_else(|| SharedString::from("Dialog")))
        .into_any_element()
}

fn build_layer_overlay(
    stack: Entity<LayerStack>,
    origin: Point<Pixels>,
    window: &mut Window,
) -> AnyElement {
    let viewport = window.viewport_size();
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
        AccessibilityRole, AppContext, Context, Modifiers, Render, TestAppContext, Window, div,
        point, rgb,
    };

    struct ModalView {
        open: bool,
        dismissals: usize,
        background_clicks: usize,
    }

    struct PersistentModalView {
        open: bool,
    }

    impl Render for ModalView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("modal-background")
                        .debug_selector(|| "modal-background".to_string())
                        .size_full()
                        .bg(rgb(0xf8fafc))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.background_clicks += 1;
                            cx.notify();
                        })),
                )
                .child(
                    modal("confirm", self.open)
                        .label("Confirm")
                        .render_with(|state, _, _| {
                            let selector = format!(
                                "modal-custom-{}-{}",
                                state.label.unwrap_or_else(|| SharedString::from("missing")),
                                if state.focused { "focused" } else { "blurred" },
                            );
                            div()
                                .debug_selector(move || selector)
                                .w(px(180.0))
                                .h(px(96.0))
                                .bg(rgb(0xffffff))
                                .border_1()
                                .border_color(rgb(0xcbd5e1))
                                .child("Confirm")
                                .into_any_element()
                        })
                        .on_change(cx.listener(|this, open, _, cx| {
                            this.open = *open;
                            this.dismissals += 1;
                            cx.notify();
                        })),
                )
        }
    }

    impl Render for PersistentModalView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().relative().size_full().child(
                modal("persistent", self.open)
                    .label("Persistent")
                    .dismiss_on_escape(false)
                    .dismiss_on_click_outside(false)
                    .render_with(|state, _, _| {
                        let selector = format!(
                            "persistent-modal-{}",
                            if state.focused { "focused" } else { "blurred" },
                        );
                        div()
                            .debug_selector(move || selector)
                            .w(px(160.0))
                            .h(px(88.0))
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xcbd5e1))
                            .child("Persistent")
                            .into_any_element()
                    })
                    .on_change(cx.listener(|this, open, _, cx| {
                        this.open = *open;
                        cx.notify();
                    })),
            )
        }
    }

    #[crate::test]
    fn modal_exposes_dialog_focus_state_and_escape_dismissal(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| ModalView {
            open: true,
            dismissals: 0,
            background_clicks: 0,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window
            .debug_bounds("modal-custom-Confirm-focused")
            .is_some());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window
                .accessibility_tree
                .nodes
                .values()
                .any(|node| node.role == AccessibilityRole::Dialog
                    && node.label.as_deref() == Some("Confirm")));
        });

        window.simulate_keystrokes("escape");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let (open, dismissals) = cx.read_entity(&view, |view, _| (view.open, view.dismissals));
        assert!(!open);
        assert_eq!(dismissals, 1);
    }

    #[crate::test]
    fn modal_dismisses_on_backdrop_click_without_clicking_background(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| ModalView {
            open: true,
            dismissals: 0,
            background_clicks: 0,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.simulate_click(point(px(8.0), px(8.0)), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let (open, dismissals, background_clicks) = cx.read_entity(&view, |view, _| {
            (view.open, view.dismissals, view.background_clicks)
        });
        assert!(!open);
        assert_eq!(dismissals, 1);
        assert_eq!(background_clicks, 0);
    }

    #[crate::test]
    fn modal_respects_dismiss_configuration(cx: &mut TestAppContext) {
        let (_view, mut window) = cx.add_window_view(|_, _| PersistentModalView { open: true });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("persistent-modal-focused").is_some());

        window.simulate_keystrokes("escape");
        window.simulate_click(point(px(8.0), px(8.0)), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("persistent-modal-focused").is_some());
    }
}