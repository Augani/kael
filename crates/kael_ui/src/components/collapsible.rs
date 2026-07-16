//! Collapsible component - Expandable/collapsible section with trigger and content.

use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

use crate::{components::icon::Icon, theme::use_theme};

#[derive(IntoElement)]
pub struct Collapsible {
    id: ElementId,
    label: SharedString,
    trigger: Option<AnyElement>,
    content: Option<AnyElement>,
    controlled_open: Option<bool>,
    default_open: bool,
    disabled: bool,
    show_icon: bool,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

#[derive(IntoElement)]
pub struct CollapsibleGroup {
    children: Vec<AnyElement>,
    divided: bool,
    style: StyleRefinement,
}

impl CollapsibleGroup {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
            divided: true,
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn divided(mut self, divided: bool) -> Self {
        self.divided = divided;
        self
    }
}

impl Default for CollapsibleGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for CollapsibleGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CollapsibleGroup {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;

        div()
            .flex()
            .flex_col()
            .w_full()
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_lg)
            .overflow_hidden()
            .children(self.children.into_iter().enumerate().map(|(ix, child)| {
                div()
                    .when(self.divided && ix > 0, |this| {
                        this.border_t_1().border_color(theme.tokens.border)
                    })
                    .child(child)
            }))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

impl Collapsible {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "collapsible-{}-{}-{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: "Toggle section".into(),
            trigger: None,
            content: None,
            controlled_open: None,
            default_open: false,
            disabled: false,
            show_icon: true,
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    /// Overrides the stable identity used for focus and uncontrolled state.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Set the name exposed to assistive technology for the disclosure trigger.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// Controls the open state. Pair with [`Self::on_toggle`] and update the
    /// supplied value when the callback fires.
    pub fn open(mut self, open: bool) -> Self {
        self.controlled_open = Some(open);
        self
    }

    /// Sets the initial state for an uncontrolled collapsible.
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn show_icon(mut self, show: bool) -> Self {
        self.show_icon = show;
        self
    }

    pub fn on_toggle<F>(mut self, handler: F) -> Self
    where
        F: Fn(bool, &mut Window, &mut App) + 'static,
    {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl Default for Collapsible {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Collapsible {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Collapsible {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let Collapsible {
            id,
            label,
            trigger,
            content,
            controlled_open,
            default_open,
            disabled,
            show_icon,
            on_toggle,
            style: _,
        } = self;
        let runtime_open = window.use_keyed_state(
            ElementId::NamedChild(Box::new(id.clone()), "open-state".into()),
            cx,
            move |_, _| default_open,
        );
        let is_open = controlled_open.unwrap_or(*runtime_open.read(cx));
        let is_uncontrolled = controlled_open.is_none();
        let interactive = !disabled && (is_uncontrolled || on_toggle.is_some());
        let focus_handle = window
            .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);
        let tab_focus = focus_handle.clone();
        let mut accessibility_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if !interactive {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(label.to_string())
            .states(accessibility_state);
        if interactive {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                if is_open {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ]);
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .when_some(trigger, |this: Div, trigger| {
                this.child(
                    div()
                        .id(id)
                        .when(interactive, |this| {
                            this.track_focus(&tab_focus.tab_index(0).tab_stop(true))
                        })
                        .accessibility(accessibility)
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .cursor(if interactive {
                            CursorStyle::PointingHand
                        } else {
                            CursorStyle::Arrow
                        })
                        .when(interactive, |this: Stateful<Div>| {
                            this.hover(|style| style.bg(theme.tokens.muted.opacity(0.5)))
                        })
                        .when(disabled, |this: Stateful<Div>| this.opacity(0.5))
                        .when(is_focused && interactive, |this| {
                            this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                                theme.tokens.ring,
                            )])
                        })
                        .when(interactive, |this: Stateful<Div>| {
                            let click_handler = on_toggle.clone();
                            let key_handler = on_toggle.clone();
                            let click_state = runtime_open.clone();
                            let key_state = runtime_open.clone();
                            let focus_on_mouse = focus_handle.clone();
                            this.on_click(move |_, window, cx| {
                                window.focus(&focus_on_mouse);
                                let next = !is_open;
                                if is_uncontrolled {
                                    click_state.update(cx, |open, cx| {
                                        *open = next;
                                        cx.notify();
                                    });
                                }
                                if let Some(handler) = &click_handler {
                                    handler(next, window, cx);
                                }
                            })
                            .on_key_down(move |event, window, cx| {
                                if event.keystroke.modifiers.modified() {
                                    return;
                                }
                                if matches!(event.keystroke.key.as_str(), "space" | "enter") {
                                    let next = !is_open;
                                    if is_uncontrolled {
                                        key_state.update(cx, |open, cx| {
                                            *open = next;
                                            cx.notify();
                                        });
                                    }
                                    if let Some(handler) = &key_handler {
                                        handler(next, window, cx);
                                    }
                                    cx.stop_propagation();
                                    window.prevent_default();
                                }
                            })
                        })
                        .when(show_icon, |this: Stateful<Div>| {
                            this.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(20.0))
                                    .child(
                                        Icon::new("chevron-right")
                                            .size(px(14.0))
                                            .color(theme.tokens.muted_foreground)
                                            .when(is_open, |icon| {
                                                icon.rotate(Radians(std::f32::consts::FRAC_PI_2))
                                            }),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Group)
                                        .states(AccessibilityState::HIDDEN),
                                )
                                .child(trigger),
                        ),
                )
            })
            .when(is_open, |this: Div| {
                this.when_some(content, |this: Div, content| {
                    this.child(div().overflow_hidden().child(content))
                })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
