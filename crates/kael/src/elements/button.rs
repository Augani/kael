use crate::{
    div, px, AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState,
    AnyElement, App, Component, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
};
use std::rc::Rc;

type ClickListener = Rc<dyn Fn(&crate::ClickEvent, &mut Window, &mut App)>;

#[non_exhaustive]
/// Snapshot of button state passed to a custom renderer.
pub struct ButtonRenderState {
    /// The visible label configured for the button, if any.
    pub label: Option<SharedString>,
    /// Whether the button currently owns keyboard focus.
    pub focused: bool,
    /// Whether the button is disabled.
    pub disabled: bool,
}

type ButtonCustomRenderer = Rc<dyn Fn(ButtonRenderState, &Window, &App) -> AnyElement>;

/// Construct a button primitive with caller-owned visuals.
#[track_caller]
pub fn button(id: impl Into<ElementId>) -> Button {
    Button::new(id.into())
}

/// A focusable button primitive backed by `div` click semantics.
pub struct Button {
    element_id: ElementId,
    label: Option<SharedString>,
    disabled: bool,
    on_click: Option<ClickListener>,
    custom_renderer: Option<ButtonCustomRenderer>,
}

impl Button {
    fn new(element_id: ElementId) -> Self {
        Self {
            element_id,
            label: None,
            disabled: false,
            on_click: None,
            custom_renderer: None,
        }
    }

    /// Set the visible label for the button.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Disable the button.
    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    /// Register a callback invoked when the button is clicked by mouse or keyboard.
    pub fn on_click(
        mut self,
        listener: impl Fn(&crate::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(listener));
        self
    }

    /// Render the button body with caller-owned visuals.
    pub fn render_with(
        mut self,
        renderer: impl Fn(ButtonRenderState, &Window, &App) -> AnyElement + 'static,
    ) -> Self {
        self.custom_renderer = Some(Rc::new(renderer));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Button {
            element_id,
            label,
            disabled,
            on_click,
            custom_renderer,
        } = self;

        let button_id = element_id.to_string();
        let focus_handle = window
            .use_keyed_state(
                ElementId::named_usize(format!("{}-focus", button_id), 0),
                cx,
                |_, cx| cx.focus_handle(),
            )
            .read(cx)
            .clone();

        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button).states(
            if disabled {
                AccessibilityState::DISABLED
            } else if focus_handle.is_focused(window) {
                AccessibilityState::FOCUSED
            } else {
                AccessibilityState::NONE
            },
        );
        if let Some(label) = label.as_ref() {
            accessibility = accessibility.label(label.to_string());
        }
        if !disabled {
            accessibility = accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let mut root = div()
            .id(element_id)
            .track_focus(&focus_handle)
            .focusable()
            .tab_stop(!disabled)
            .cursor_pointer()
            .accessibility(accessibility)
            .focus_visible(|style: crate::StyleRefinement| style.bg(crate::rgba(0x1d4ed810)));

        if !disabled {
            if let Some(listener) = on_click {
                let key_listener = listener.clone();
                root = root
                    .on_click(move |event, window, cx| {
                        listener(event, window, cx);
                    })
                    .on_key_down(move |event, window, cx| {
                        if event.keystroke.modifiers.modified() {
                            return;
                        }

                        let Some(button) = (match event.keystroke.key.as_str() {
                            "enter" => Some(crate::KeyboardButton::Enter),
                            "space" => Some(crate::KeyboardButton::Space),
                            _ => None,
                        }) else {
                            return;
                        };

                        key_listener(
                            &crate::ClickEvent::Keyboard(crate::KeyboardClickEvent {
                                button,
                                ..Default::default()
                            }),
                            window,
                            cx,
                        );
                        window.prevent_default();
                    });
            }
        }

        let body = if let Some(renderer) = custom_renderer {
            renderer(
                ButtonRenderState {
                    label: label.clone(),
                    focused: focus_handle.is_focused(window),
                    disabled,
                },
                window,
                cx,
            )
        } else {
            default_button_body(label.clone(), disabled)
        };
        root = root.child(body);

        #[cfg(any(test, feature = "test-support"))]
        {
            let selector = format!("button-{}", button_id);
            root = root.debug_selector(move || selector);
        }

        root
    }
}

fn default_button_body(label: Option<SharedString>, disabled: bool) -> AnyElement {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(if disabled {
            crate::rgb(0xcbd5e1)
        } else {
            crate::rgb(0x94a3b8)
        })
        .bg(if disabled {
            crate::rgb(0xf8fafc)
        } else {
            crate::rgb(0xffffff)
        })
        .text_color(if disabled {
            crate::rgb(0x94a3b8)
        } else {
            crate::rgb(0x0f172a)
        })
        .child(label.unwrap_or_else(|| SharedString::from("Button")))
        .into_any_element()
}

impl IntoElement for Button {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Context, Modifiers, Render, TestAppContext, div};
    use std::cell::RefCell;

    struct ButtonView {
        clicks: usize,
    }

    struct CustomButtonView {
        snapshot: Rc<RefCell<Option<(bool, bool, SharedString)>>>,
    }

    impl Render for ButtonView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            button("primary")
                .label("Apply")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.clicks += 1;
                    cx.notify();
                }))
        }
    }

    impl Render for CustomButtonView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let snapshot = self.snapshot.clone();
            button("custom")
                .label("Save")
                .disabled()
                .render_with(move |state, _, _| {
                    snapshot.replace(Some((
                        state.focused,
                        state.disabled,
                        state.label.unwrap_or_else(|| SharedString::default()),
                    )));
                    div().child("Save").into_any_element()
                })
        }
    }

    #[crate::test]
    fn button_clicks_with_mouse_and_keyboard(cx: &mut TestAppContext) {
        let (view, mut window) = cx.add_window_view(|_, _| ButtonView { clicks: 0 });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let bounds = window.debug_bounds("button-primary").unwrap();
        window.simulate_click(bounds.center(), Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).clicks, 1);
            assert!(window.focused(cx).is_some());
        });

        window.simulate_keystrokes("space");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(view.read(cx).clicks, 2);
        });
    }

    #[crate::test]
    fn button_render_with_receives_disabled_and_label_state(cx: &mut TestAppContext) {
        let snapshot = Rc::new(RefCell::new(None));
        let snapshot_ref = snapshot.clone();
        let (_view, mut window) = cx.add_window_view(|_, _| CustomButtonView {
            snapshot: snapshot_ref,
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(snapshot.take(), Some((false, true, SharedString::from("Save"))));
    }
}