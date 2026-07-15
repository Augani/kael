//! AnimatedCollapsible - Collapsible section with smooth height and opacity animation.

use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;
use std::time::Duration;

use crate::animations::easings;
use crate::components::icon::Icon;
use crate::theme::Theme;

#[derive(IntoElement)]
pub struct AnimatedCollapsible {
    id: ElementId,
    label: SharedString,
    trigger: Option<AnyElement>,
    content: Option<AnyElement>,
    is_open: bool,
    disabled: bool,
    show_icon: bool,
    duration: Duration,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl AnimatedCollapsible {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "animated-collapsible:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: "Toggle section".into(),
            trigger: None,
            content: None,
            is_open: false,
            disabled: false,
            show_icon: true,
            duration: Duration::from_millis(250),
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Some(trigger.into_any_element());
        self
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.is_open = open;
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

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
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

impl Default for AnimatedCollapsible {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for AnimatedCollapsible {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for AnimatedCollapsible {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let is_open = self.is_open;
        let disabled = self.disabled;
        let show_icon = self.show_icon;
        let on_toggle = self.on_toggle;
        let duration = self.duration;

        let root_id = self.id;
        let trigger_id = ElementId::NamedChild(Box::new(root_id.clone()), "trigger".into());
        let content_id = ElementId::NamedChild(Box::new(root_id.clone()), "content".into());
        let anim_id = ElementId::NamedChild(
            Box::new(root_id.clone()),
            if is_open {
                "open".into()
            } else {
                "close".into()
            },
        );
        let can_toggle = !disabled && on_toggle.is_some();
        let mut state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if disabled || !can_toggle {
            state |= AccessibilityState::DISABLED;
        }
        let mut trigger_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(self.label.to_string())
            .states(state);
        if can_toggle {
            trigger_accessibility = trigger_accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                if is_open {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ]);
        }

        let mut root = div()
            .id(root_id)
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group))
            .flex()
            .flex_col()
            .w_full();

        if let Some(trigger) = self.trigger {
            let trigger_row = div()
                .id(trigger_id)
                .accessibility(trigger_accessibility)
                .flex()
                .items_center()
                .gap(px(8.0))
                .cursor(if can_toggle {
                    CursorStyle::PointingHand
                } else {
                    CursorStyle::Arrow
                })
                .when(can_toggle, |this| {
                    this.focusable()
                        .tab_index(0)
                        .tab_stop(true)
                        .hover(|style| style.bg(theme.tokens.muted.opacity(0.5)))
                        .focus_visible(|style| style.bg(theme.tokens.muted.opacity(0.5)))
                })
                .when(disabled || !can_toggle, |this| this.opacity(0.5))
                .when(can_toggle, |this| {
                    let handler = on_toggle.clone().unwrap();
                    let on_key = handler.clone();
                    this.on_click(move |_, window, cx| {
                        handler(!is_open, window, cx);
                    })
                    .on_key_down(move |event, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            on_key(!is_open, window, cx);
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    })
                })
                .when(show_icon, |this| {
                    this.child(
                        div()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new("chevron-right")
                                    .size(px(16.0))
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
                );

            root = root.child(trigger_row);
        }

        if let Some(content) = self.content {
            let mut content_state = AccessibilityState::NONE;
            if !is_open {
                content_state |= AccessibilityState::HIDDEN;
            }
            let content_wrapper = div()
                .id(content_id)
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::Group)
                        .label(format!("{} content", self.label))
                        .states(content_state),
                )
                .overflow_hidden()
                .child(content);

            let content_wrapper = if cx.reduce_motion() || duration.is_zero() {
                content_wrapper.into_any_element()
            } else {
                content_wrapper
                    .with_animation(
                        anim_id,
                        Animation::new(duration).with_easing(easings::ease_out_cubic),
                        move |el, delta| {
                            if is_open {
                                el.max_h(px(1500.0 * delta)).opacity(delta)
                            } else {
                                el.max_h(px(1500.0 * (1.0 - delta))).opacity(1.0 - delta)
                            }
                        },
                    )
                    .into_any_element()
            };

            root = root.child(content_wrapper);
        }

        root.map(|this| {
            let mut d = this;
            d.style().refine(&user_style);
            d
        })
    }
}
