//! Popover component with anchored positioning.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;
use std::{cell::RefCell, rc::Rc};

use crate::animations::easings;
use crate::components::button::{Button, ButtonVariant};
use crate::theme::Theme;

const POPOVER_MARGIN: Pixels = px(8.0);
const CONTEXT: &str = "Popover";

actions!(popover, [ClosePopover]);

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", ClosePopover, Some(CONTEXT))]);
}

pub struct PopoverContent {
    focus_handle: FocusHandle,
    content: Rc<dyn Fn(&mut Window, &mut Context<Self>) -> AnyElement>,
    accessibility_label: SharedString,
    dismissing: bool,
}

impl PopoverContent {
    pub fn new<F>(_window: &mut Window, cx: &mut App, content: F) -> Self
    where
        F: Fn(&mut Window, &mut Context<Self>) -> AnyElement + 'static,
    {
        Self {
            focus_handle: cx.focus_handle(),
            content: Rc::new(content),
            accessibility_label: "Popover".into(),
            dismissing: false,
        }
    }

    /// Describe this non-modal surface to assistive technologies.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = if label.trim().is_empty() {
            "Popover".into()
        } else {
            label
        };
        self
    }
}

impl EventEmitter<DismissEvent> for PopoverContent {}

impl Focusable for PopoverContent {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PopoverContent {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let dismissing = self.dismissing;

        div()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.accessibility_label.to_string()),
            )
            .p(px(12.0))
            .min_w(px(200.0))
            .max_w(px(400.0))
            .bg(theme.tokens.popover)
            .text_color(theme.tokens.popover_foreground)
            .rounded(theme.tokens.radius_lg)
            .shadow(theme.tokens.shadow_md.to_vec())
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .key_context(CONTEXT)
            .on_action(cx.listener(|this, _: &ClosePopover, window, cx| {
                if this.dismissing {
                    return;
                }
                this.dismissing = true;
                cx.notify();

                cx.spawn_in(window, async move |entity, cx| {
                    smol::Timer::after(Duration::from_millis(120)).await;
                    let _ = entity.update(cx, |_, cx| {
                        cx.emit(DismissEvent);
                    });
                })
                .detach();
            }))
            .child((self.content)(window, cx))
            .with_animation(
                if dismissing {
                    "popover-exit"
                } else {
                    "popover-enter"
                },
                Animation::new(Duration::from_millis(if dismissing { 120 } else { 150 }))
                    .with_easing(if dismissing {
                        easings::ease_in_cubic as fn(f32) -> f32
                    } else {
                        easings::ease_out_cubic as fn(f32) -> f32
                    }),
                move |el, delta| {
                    if dismissing {
                        el.opacity(1.0 - delta).mt(px(4.0 * delta))
                    } else {
                        el.opacity(delta).mt(px(4.0 * (1.0 - delta)))
                    }
                },
            )
    }
}

pub struct Popover {
    id: ElementId,
    anchor: Corner,
    trigger: Option<Box<dyn FnOnce(bool, &Window, &App) -> AnyElement + 'static>>,
    trigger_button: Option<SharedString>,
    trigger_button_variant: ButtonVariant,
    content: Option<Rc<dyn Fn(&mut Window, &mut App) -> Entity<PopoverContent> + 'static>>,
    on_open_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    mouse_button: MouseButton,
    disabled: bool,
    style: StyleRefinement,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            anchor: Corner::TopLeft,
            trigger: None,
            trigger_button: None,
            trigger_button_variant: ButtonVariant::Outline,
            content: None,
            on_open_change: None,
            mouse_button: MouseButton::Left,
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn anchor(mut self, anchor: Corner) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn mouse_button(mut self, mouse_button: MouseButton) -> Self {
        self.mouse_button = mouse_button;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn trigger<T>(mut self, trigger: T) -> Self
    where
        T: IntoElement + 'static,
    {
        self.trigger = Some(Box::new(move |_is_open, _, _| trigger.into_any_element()));
        self.trigger_button = None;
        self
    }

    /// Use a fully wired, keyboard- and assistive-technology-accessible trigger button.
    pub fn trigger_button(mut self, label: impl Into<SharedString>) -> Self {
        self.trigger = None;
        self.trigger_button = Some(label.into());
        self
    }

    /// Customize the visual variant used by [`Self::trigger_button`].
    pub fn trigger_button_variant(mut self, variant: ButtonVariant) -> Self {
        self.trigger_button_variant = variant;
        self
    }

    pub fn content<F>(mut self, content: F) -> Self
    where
        F: Fn(&mut Window, &mut App) -> Entity<PopoverContent> + 'static,
    {
        self.content = Some(Rc::new(content));
        self
    }

    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }

    fn render_trigger(
        &mut self,
        open: bool,
        content_view: Rc<RefCell<Option<Entity<PopoverContent>>>>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let user_style = self.style.clone();
        let content_build = self.content.clone();
        let on_open_change = self.on_open_change.clone();
        let trigger_element = if let Some(label) = self.trigger_button.take() {
            let button_id = ElementId::NamedChild(Box::new(self.id.clone()), "trigger".into());
            let button_content = content_build.clone();
            let button_view = content_view.clone();
            let button_open_change = on_open_change.clone();
            Button::new(button_id, label)
                .variant(self.trigger_button_variant)
                .expanded(open)
                .disabled(self.disabled || button_content.is_none())
                .on_click(move |_, window, cx| {
                    let Some(content) = button_content.as_ref() else {
                        return;
                    };
                    open_popover_content(
                        content.clone(),
                        button_view.clone(),
                        button_open_change.clone(),
                        window,
                        cx,
                    );
                })
                .into_any_element()
        } else if let Some(trigger) = self.trigger.take() {
            (trigger)(open, window, cx)
        } else {
            div().child("Trigger").into_any_element()
        };

        div()
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(trigger_element)
            .when_some(
                content_build.filter(|_| !self.disabled),
                |this, content_build| {
                    this.on_key_down(move |event, window, cx| {
                        if !matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                            return;
                        }
                        if content_view.borrow().is_none() {
                            open_popover_content(
                                content_build.clone(),
                                content_view.clone(),
                                on_open_change.clone(),
                                window,
                                cx,
                            );
                        }
                        cx.stop_propagation();
                        window.prevent_default();
                    })
                },
            )
            .into_any_element()
    }

    fn resolved_corner(&self, bounds: Bounds<Pixels>) -> Point<Pixels> {
        bounds.corner(match self.anchor {
            Corner::TopLeft => Corner::BottomLeft,
            Corner::TopRight => Corner::BottomRight,
            Corner::BottomLeft => Corner::TopLeft,
            Corner::BottomRight => Corner::TopRight,
        })
    }

    fn with_element_state<R>(
        &mut self,
        id: &GlobalElementId,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut PopoverElementState, &mut Window, &mut App) -> R,
    ) -> R {
        window.with_optional_element_state::<PopoverElementState, _>(
            Some(id),
            |element_state, window| {
                let mut element_state = element_state.unwrap().unwrap_or_default();
                let result = f(self, &mut element_state, window, cx);
                (result, Some(element_state))
            },
        )
    }
}

fn open_popover_content(
    content_build: Rc<dyn Fn(&mut Window, &mut App) -> Entity<PopoverContent> + 'static>,
    content_view: Rc<RefCell<Option<Entity<PopoverContent>>>>,
    on_open_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    window: &mut Window,
    cx: &mut App,
) {
    if content_view.borrow().is_some() {
        return;
    }

    let new_content_view = content_build(window, cx);
    let view_for_dismiss = content_view.clone();
    let previous_focus_handle = window.focused(cx);
    let close_callback = on_open_change.clone();

    window
        .subscribe(
            &new_content_view,
            cx,
            move |modal, _: &DismissEvent, window, cx| {
                if modal.focus_handle(cx).contains_focused(window, cx) {
                    if let Some(previous_focus_handle) = previous_focus_handle.as_ref() {
                        window.focus(previous_focus_handle);
                    }
                }
                *view_for_dismiss.borrow_mut() = None;
                if let Some(callback) = close_callback.as_ref() {
                    callback(false, window, cx);
                }
                window.refresh();
            },
        )
        .detach();

    window.focus(&new_content_view.focus_handle(cx));
    *content_view.borrow_mut() = Some(new_content_view);
    if let Some(callback) = on_open_change.as_ref() {
        callback(true, window, cx);
    }
    window.refresh();
}

impl IntoElement for Popover {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for Popover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

#[derive(Default)]
pub struct PopoverElementState {
    trigger_layout_id: Option<LayoutId>,
    popover_element: Option<AnyElement>,
    trigger_element: Option<AnyElement>,
    content_view: Rc<RefCell<Option<Entity<PopoverContent>>>>,
    trigger_bounds: Option<Bounds<Pixels>>,
}

pub struct PrepaintState {
    hitbox: Hitbox,
    trigger_bounds: Option<Bounds<Pixels>>,
}

impl Element for Popover {
    type RequestLayoutState = PopoverElementState;
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&kael::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = Style::default();

        self.with_element_state(
            id.unwrap(),
            window,
            cx,
            |view, element_state, window, cx| {
                let mut popover_layout_id = None;
                let mut popover_element = None;
                let mut is_open = false;

                if let Some(content_view) = element_state.content_view.borrow_mut().as_mut() {
                    is_open = true;

                    let mut anchored = anchored()
                        .snap_to_window_with_margin(POPOVER_MARGIN)
                        .anchor(view.anchor);

                    if let Some(trigger_bounds) = element_state.trigger_bounds {
                        anchored = anchored.position(view.resolved_corner(trigger_bounds));
                    }

                    let mut element = {
                        let content_view_mut = element_state.content_view.clone();
                        let anchor = view.anchor;

                        deferred(
                            anchored.child(
                                div()
                                    .occlude()
                                    .map(|this| match anchor {
                                        Corner::TopLeft | Corner::TopRight => {
                                            this.mt(POPOVER_MARGIN)
                                        }
                                        Corner::BottomLeft | Corner::BottomRight => {
                                            this.mb(POPOVER_MARGIN)
                                        }
                                    })
                                    .child(content_view.clone())
                                    .on_mouse_down_out(move |_, _, cx| {
                                        let content = content_view_mut.borrow().clone();
                                        if let Some(content) = content {
                                            content.update(cx, |_, cx| cx.emit(DismissEvent));
                                        }
                                    }),
                            ),
                        )
                        .with_priority(1)
                        .into_any()
                    };

                    popover_layout_id = Some(element.request_layout(window, cx));
                    popover_element = Some(element);
                }

                let mut trigger_element =
                    view.render_trigger(is_open, element_state.content_view.clone(), window, cx);
                let trigger_layout_id = trigger_element.request_layout(window, cx);

                let layout_id = window.request_layout(
                    style,
                    Some(trigger_layout_id).into_iter().chain(popover_layout_id),
                    cx,
                );

                (
                    layout_id,
                    PopoverElementState {
                        trigger_layout_id: Some(trigger_layout_id),
                        popover_element,
                        trigger_element: Some(trigger_element),
                        content_view: element_state.content_view.clone(),
                        trigger_bounds: element_state.trigger_bounds,
                    },
                )
            },
        )
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&kael::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(element) = &mut request_layout.trigger_element {
            element.prepaint(window, cx);
        }
        if let Some(element) = &mut request_layout.popover_element {
            element.prepaint(window, cx);
        }

        let trigger_bounds = request_layout
            .trigger_layout_id
            .map(|id| window.layout_bounds(id));

        let hitbox =
            window.insert_hitbox(trigger_bounds.unwrap_or_default(), HitboxBehavior::Normal);

        PrepaintState {
            trigger_bounds,
            hitbox,
        }
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&kael::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.with_element_state(
            id.unwrap(),
            window,
            cx,
            |this, element_state, window, cx| {
                element_state.trigger_bounds = prepaint.trigger_bounds;

                if let Some(mut element) = request_layout.trigger_element.take() {
                    element.paint(window, cx);
                }

                if let Some(mut element) = request_layout.popover_element.take() {
                    element.paint(window, cx);
                    return;
                }

                let Some(content_build) = this.content.clone() else {
                    return;
                };

                let old_content_view = element_state.content_view.clone();
                let on_open_change = this.on_open_change.clone();
                let hitbox_id = prepaint.hitbox.id;
                let mouse_button = this.mouse_button;

                if this.disabled {
                    return;
                }

                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == mouse_button
                        && hitbox_id.is_hovered(window)
                    {
                        cx.stop_propagation();
                        window.prevent_default();
                        open_popover_content(
                            content_build.clone(),
                            old_content_view.clone(),
                            on_open_change.clone(),
                            window,
                            cx,
                        );
                    }
                });
            },
        );
    }
}
