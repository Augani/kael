//! Field component - label, description, control, and status wrapper.

use crate::{
    components::{
        field_status::{FieldStatus, FieldStatusTone, FieldStatusVariant},
        icon::Icon,
        icon_source::IconSource,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldStatusType {
    Warning,
    Error,
    Success,
}

#[derive(IntoElement)]
pub struct FieldLabel {
    label: SharedString,
    description: Option<SharedString>,
    optional: bool,
    required: bool,
    disabled: bool,
    hidden: bool,
    icon: Option<IconSource>,
    style: StyleRefinement,
}

impl FieldLabel {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            optional: false,
            required: false,
            disabled: false,
            hidden: false,
            icon: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

impl Styled for FieldLabel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FieldLabel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .flex()
            .flex_col()
            .gap(px(3.0))
            .when(self.hidden, |this| {
                this.absolute()
                    .left(px(-10000.0))
                    .top(px(0.0))
                    .size(px(1.0))
                    .overflow_hidden()
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(14.0))
                    .line_height(px(20.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.tokens.muted_foreground)
                    .when_some(self.icon, |this, icon| {
                        this.child(
                            Icon::new(icon)
                                .size(px(16.0))
                                .color(theme.tokens.muted_foreground),
                        )
                    })
                    .child(self.label)
                    .when(self.required && !self.optional, |this| {
                        this.child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme.tokens.muted_foreground)
                                .child("∙ Required"),
                        )
                    })
                    .when(self.optional, |this| {
                        this.child(
                            div()
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .font_weight(FontWeight::NORMAL)
                                .text_color(theme.tokens.muted_foreground)
                                .child("∙ Optional"),
                        )
                    }),
            )
            .when_some(self.description, |this, description| {
                this.child(
                    div()
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .text_color(theme.tokens.muted_foreground)
                        .child(description),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct InputClearButton {
    label: SharedString,
    disabled: bool,
    on_click: Option<std::rc::Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl InputClearButton {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            disabled: false,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(std::rc::Rc::new(handler));
        self
    }
}

impl Styled for InputClearButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for InputClearButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let disabled = self.disabled;
        let handler = self.on_click.filter(|_| !disabled);
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(self.label.to_string())
            .states(if disabled {
                AccessibilityState::DISABLED
            } else {
                AccessibilityState::NONE
            });
        if handler.is_some() {
            accessibility = accessibility.actions(vec![AccessibilityAction::Click]);
        }

        div()
            .accessibility(accessibility)
            .relative()
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .text_color(theme.tokens.muted_foreground)
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(theme.tokens.accent))
            })
            .when_some(handler, |this, handler| {
                let pointer_handler = handler.clone();
                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    pointer_handler(window, cx);
                })
                .on_accessibility_action(
                    AccessibilityAction::Click,
                    move |_, window, cx| {
                        handler(window, cx);
                    },
                )
            })
            .child(
                div()
                    .absolute()
                    .left(px(-10000.0))
                    .top(px(0.0))
                    .size(px(1.0))
                    .overflow_hidden()
                    .child(self.label),
            )
            .child(Icon::new("x").size(px(14.0)))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct Field {
    label: SharedString,
    control: AnyElement,
    description: Option<SharedString>,
    optional: bool,
    required: bool,
    disabled: bool,
    label_icon: Option<IconSource>,
    status: Option<(FieldStatusType, SharedString)>,
    status_variant: FieldStatusVariant,
    hidden_label: bool,
    width: Option<Pixels>,
    style: StyleRefinement,
}

impl Field {
    pub fn new(label: impl Into<SharedString>, control: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            control: control.into_any_element(),
            description: None,
            optional: false,
            required: false,
            disabled: false,
            label_icon: None,
            status: None,
            status_variant: FieldStatusVariant::Attached,
            hidden_label: false,
            width: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn label_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.label_icon = Some(icon.into());
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }

    pub fn status_variant(mut self, variant: FieldStatusVariant) -> Self {
        self.status_variant = variant;
        self
    }

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }
}

impl Styled for Field {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Field {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let status = self.status.clone();
        let status_variant = self.status_variant;
        let hidden_label = self.hidden_label;

        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .when_some(self.width, |this, width| this.w(width))
            .child(
                FieldLabel::new(self.label)
                    .hidden(hidden_label)
                    .optional(self.optional)
                    .required(self.required)
                    .disabled(self.disabled)
                    .when_some(self.description, |label, description| {
                        label.description(description)
                    })
                    .when_some(self.label_icon, |label, icon| label.icon(icon)),
            )
            .child(div().flex().flex_col().child(self.control).when_some(
                status,
                move |this, (tone, message)| {
                    this.child(
                        FieldStatus::new(field_status_tone(tone), message).variant(status_variant),
                    )
                },
            ))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn field_status_tone(status: FieldStatusType) -> FieldStatusTone {
    match status {
        FieldStatusType::Warning => FieldStatusTone::Warning,
        FieldStatusType::Error => FieldStatusTone::Error,
        FieldStatusType::Success => FieldStatusTone::Success,
    }
}

// Mounted behavior baselines; no control implementation changes are included.
#[cfg(test)]
mod input_clear_mounted_baseline {
    use super::*;
    use crate::components::icon_button::IconButton;
    use kael::{TestAppContext, VisualTestContext};
    use std::{cell::Cell, rc::Rc};

    struct Host {
        counts: Rc<Cell<[usize; 3]>>,
        same_label_counts: Rc<Cell<[usize; 2]>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let enabled = self.counts.clone();
            let disabled = self.counts.clone();
            let reference = self.counts.clone();
            div()
                .size_full()
                .p(px(20.0))
                .flex()
                .gap(px(24.0))
                .items_start()
                .child(
                    div()
                        .id("enabled-clear-hit")
                        .debug_selector(|| "enabled-clear-hit".into())
                        .size(px(24.0))
                        .child(InputClearButton::new("Clear filter").on_click(move |_, _| {
                            let mut counts = enabled.get();
                            counts[0] += 1;
                            enabled.set(counts);
                        })),
                )
                .child(
                    div()
                        .id("disabled-clear-hit")
                        .debug_selector(|| "disabled-clear-hit".into())
                        .size(px(24.0))
                        .child(
                            InputClearButton::new("Disabled clear")
                                .disabled(true)
                                .on_click(move |_, _| {
                                    let mut counts = disabled.get();
                                    counts[1] += 1;
                                    disabled.set(counts);
                                }),
                        ),
                )
                .child(
                    div()
                        .id("inert-clear-hit")
                        .debug_selector(|| "inert-clear-hit".into())
                        .size(px(24.0))
                        .child(InputClearButton::new("Unconfigured clear")),
                )
                .child(
                    IconButton::new("x")
                        .id("reference-clear")
                        .label("Reference clear")
                        .size(px(24.0))
                        .on_click(move |_, _, _| {
                            let mut counts = reference.get();
                            counts[2] += 1;
                            reference.set(counts);
                        }),
                )
                .children((0..2).map(|index| {
                    let counts = self.same_label_counts.clone();
                    InputClearButton::new("Clear identical label").on_click(move |_, _| {
                        let mut values = counts.get();
                        values[index] += 1;
                        counts.set(values);
                    })
                }))
        }
    }

    fn mounted() -> (Entity<Host>, VisualTestContext, Rc<Cell<[usize; 3]>>) {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let counts = Rc::new(Cell::new([0; 3]));
        let (host, window) = cx.add_window_view({
            let counts = counts.clone();
            move |_, _| Host {
                counts,
                same_label_counts: Rc::new(Cell::new([0; 2])),
            }
        });
        window.simulate_resize(size(px(500.0), px(320.0)));
        window.deactivate_window();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        window.update(|window, cx| {
            assert!(window.is_window_active());
            window.draw(cx).clear();
        });
        (host, window.clone(), counts)
    }

    fn named_button(window: &Window, label: &str) -> AccessibilityId {
        let matches = window
            .accessibility_tree()
            .nodes
            .values()
            .filter(|node| {
                node.role == AccessibilityRole::Button && node.label.as_deref() == Some(label)
            })
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "expected one named Button: {label}");
        matches[0].id
    }

    fn native_click(window: &mut VisualTestContext, id: AccessibilityId) {
        window.update(|window, _| {
            let node = window.accessibility_tree().get(id).unwrap();
            assert!(node.actions.contains(&AccessibilityAction::Click));
            assert!(window.has_accessibility_action_handler(id, AccessibilityAction::Click));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Click,
            ));
        });
        window.run_until_parked();
    }

    #[::core::prelude::v1::test]
    fn named_enabled_clear_native_click_activates_exactly_once() {
        let (_host, mut window, counts) = mounted();
        let reference = window.update(|window, _| named_button(window, "Reference clear"));
        native_click(&mut window, reference);
        assert_eq!(
            counts.get(),
            [0, 0, 1],
            "real native-dispatch positive control"
        );
        let clear = window.update(|window, _| named_button(window, "Clear filter"));
        native_click(&mut window, clear);
        assert_eq!(counts.get(), [1, 0, 1]);
        window.update(|window, cx| window.draw(cx).clear());
        assert_eq!(counts.get(), [1, 0, 1], "redraw must not activate again");
    }

    #[::core::prelude::v1::test]
    fn clear_pointer_once_and_disabled_callback_free_controls_are_inert() {
        let (_host, mut window, counts) = mounted();
        for (selector, expected) in [
            ("enabled-clear-hit", [1, 0, 0]),
            ("disabled-clear-hit", [1, 0, 0]),
            ("inert-clear-hit", [1, 0, 0]),
        ] {
            let bounds = window
                .debug_bounds(selector)
                .expect("measured clear hit bounds");
            assert_eq!(bounds.size, size(px(24.0), px(24.0)));
            let center = bounds.center();
            assert!(center.x >= px(0.0) && center.y >= px(0.0));
            window.simulate_click(center, Modifiers::default());
            window.run_until_parked();
            assert_eq!(counts.get(), expected, "pointer result for {selector}");
        }
        window.update(|window, cx| {
            window.draw(cx).clear();
            for node in window.accessibility_tree().nodes.values().filter(|node| {
                matches!(
                    node.label.as_deref(),
                    Some("Disabled clear" | "Unconfigured clear")
                )
            }) {
                assert!(!node.actions.contains(&AccessibilityAction::Click));
                assert!(
                    !window.has_accessibility_action_handler(node.id, AccessibilityAction::Click)
                );
                assert!(!node.states.contains(AccessibilityState::FOCUSED));
            }
        });
        assert_eq!(counts.get(), [1, 0, 0]);
    }

    #[::core::prelude::v1::test]
    fn repeated_same_label_clear_actions_keep_distinct_identity_and_callbacks() {
        let (host, mut window, _) = mounted();
        let counts = window.update(|_, cx| host.read(cx).same_label_counts.clone());
        let ids = window.update(|window, _| {
            let mut nodes = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Clear identical label")
                })
                .collect::<Vec<_>>();
            assert_eq!(
                nodes.len(),
                2,
                "both same-label fields need their own named action"
            );
            nodes.sort_by(|left, right| left.bounds.unwrap().x.total_cmp(&right.bounds.unwrap().x));
            assert_ne!(nodes[0].id, nodes[1].id, "a label is not field identity");
            [nodes[0].id, nodes[1].id]
        });
        native_click(&mut window, ids[0]);
        assert_eq!(counts.get(), [1, 0]);
        window.update(|window, cx| window.draw(cx).clear());
        native_click(&mut window, ids[1]);
        assert_eq!(counts.get(), [1, 1]);
        window.update(|window, cx| {
            window.draw(cx).clear();
            for id in ids {
                let node = window
                    .accessibility_tree()
                    .get(id)
                    .expect("identity survives redraw");
                assert_eq!(node.label.as_deref(), Some("Clear identical label"));
            }
        });
    }
}
