//! Expandable card with animated expand/collapse transitions.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::animations::{durations, easings};
use crate::theme::Theme;

pub struct ExpandableCardState {
    is_expanded: bool,
    is_animating: bool,
    is_expanding: bool,
    animation_version: usize,
}

impl Default for ExpandableCardState {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpandableCardState {
    pub fn new() -> Self {
        Self {
            is_expanded: false,
            is_animating: false,
            is_expanding: false,
            animation_version: 0,
        }
    }

    pub fn is_expanded(&self) -> bool {
        self.is_expanded
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.toggle_with_duration(durations::NORMAL, cx);
    }

    pub fn toggle_with_duration(&mut self, duration: Duration, cx: &mut Context<Self>) {
        if self.is_expanded {
            self.collapse_with_duration(duration, cx);
        } else {
            self.expand_with_duration(duration, cx);
        }
    }

    pub fn expand(&mut self, cx: &mut Context<Self>) {
        self.expand_with_duration(durations::NORMAL, cx);
    }

    pub fn expand_with_duration(&mut self, duration: Duration, cx: &mut Context<Self>) {
        if self.is_expanded {
            return;
        }

        self.start_transition(true, duration, cx);
    }

    pub fn collapse(&mut self, cx: &mut Context<Self>) {
        self.collapse_with_duration(durations::NORMAL, cx);
    }

    pub fn collapse_with_duration(&mut self, duration: Duration, cx: &mut Context<Self>) {
        if !self.is_expanded {
            return;
        }

        self.start_transition(false, duration, cx);
    }

    fn start_transition(
        &mut self,
        target_expanded: bool,
        duration: Duration,
        cx: &mut Context<Self>,
    ) {
        self.is_expanded = target_expanded;
        self.is_expanding = target_expanded;
        self.is_animating = !duration.is_zero();
        self.animation_version = self.animation_version.wrapping_add(1);
        let version = self.animation_version;
        cx.notify();

        if duration.is_zero() {
            self.is_expanding = false;
            return;
        }

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            _ = this.update(cx, |state, cx| {
                if state.animation_version == version {
                    state.is_animating = false;
                    state.is_expanding = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[derive(IntoElement)]
pub struct ExpandableCard {
    id: ElementId,
    label: SharedString,
    state: Entity<ExpandableCardState>,
    collapsed_content: Option<AnyElement>,
    expanded_content: Option<AnyElement>,
    duration: Duration,
    style: StyleRefinement,
}

impl ExpandableCard {
    pub fn new(id: impl Into<ElementId>, state: Entity<ExpandableCardState>) -> Self {
        Self {
            id: id.into(),
            label: "Expandable card".into(),
            state,
            collapsed_content: None,
            expanded_content: None,
            duration: durations::NORMAL,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn collapsed(mut self, content: impl IntoElement) -> Self {
        self.collapsed_content = Some(content.into_any_element());
        self
    }

    pub fn expanded(mut self, content: impl IntoElement) -> Self {
        self.expanded_content = Some(content.into_any_element());
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
}

impl Styled for ExpandableCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ExpandableCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let card_id = self.id.clone();
        let focus_handle = window
            .use_keyed_state(card_id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let focus_on_mouse = focus_handle.clone();
        let is_focused = focus_handle.is_focused(window);
        let theme = Theme::of(cx);
        let user_style = self.style;
        let state = self.state.read(cx);
        let is_expanded = state.is_expanded;
        let is_animating = state.is_animating;
        let is_expanding = state.is_expanding;
        let animation_version = state.animation_version;
        let duration = self.duration;
        let state_for_click = self.state.clone();
        let state_for_key = self.state.clone();

        let shadow = BoxShadow {
            color: hsla(0.0, 0.0, 0.0, 0.08),
            offset: point(px(0.0), px(1.0)),
            blur_radius: px(3.0),
            spread_radius: px(0.0),
            inset: false,
        };

        div()
            .id(card_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Button)
                    .label(if is_expanded {
                        format!("Collapse {}", self.label)
                    } else {
                        format!("Expand {}", self.label)
                    })
                    .expanded(is_expanded)
                    .focused(is_focused)
                    .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]),
            )
            .track_focus(&focus_handle.tab_index(0).tab_stop(true))
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_lg)
            .shadow(smallvec::smallvec![shadow])
            .overflow_hidden()
            .cursor_pointer()
            .when(is_focused, |this| {
                this.shadow(smallvec::smallvec![theme.tokens.focus_ring_light()])
            })
            .on_click(move |_, window, cx| {
                window.focus(&focus_on_mouse);
                state_for_click.update(cx, |s, cx| s.toggle_with_duration(duration, cx));
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    state_for_key.update(cx, |state, cx| state.toggle_with_duration(duration, cx));
                    cx.stop_propagation();
                    window.prevent_default();
                }
            })
            .when(!is_expanded && !is_animating, |this| {
                this.when_some(self.collapsed_content, |this, content| {
                    this.child(div().px(px(24.0)).py(px(16.0)).child(content))
                })
            })
            .when(is_expanded || is_animating, |this| {
                this.when_some(self.expanded_content, |this, content| {
                    this.child(
                        div()
                            .id(ElementId::NamedChild(Box::new(card_id), "content".into()))
                            .px(px(24.0))
                            .py(px(16.0))
                            .overflow_hidden()
                            .child(content)
                            .with_animation(
                                ElementId::Name(format!("expand-{}", animation_version).into()),
                                Animation::new(duration).with_easing(easings::ease_out_cubic),
                                move |el, delta| {
                                    if is_expanding {
                                        el.opacity(delta)
                                    } else if is_animating && !is_expanding {
                                        el.opacity(1.0 - delta)
                                    } else {
                                        el.opacity(1.0)
                                    }
                                },
                            ),
                    )
                })
            })
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}
