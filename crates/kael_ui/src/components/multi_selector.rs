//! MultiSelector component - multi-value selector trigger with token badges.

use crate::{
    astryx,
    components::{
        field::{Field, FieldStatusType},
        icon::Icon,
        input::InputSize,
        token::Token,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

pub type MultiSelectorDivider = crate::components::select::SelectorDivider;
pub type MultiSelectorSection = crate::components::select::SelectorSection;
pub type MultiSelectorSize = InputSize;
pub type MultiSelectorStatus = FieldStatusType;
pub type MultiSelectorStatusType = FieldStatusType;
pub type MultiSelectorOptionData = MultiSelectorOption;
pub type MultiSelectorProps = MultiSelector;

#[derive(Clone, Debug)]
pub struct MultiSelectorOption {
    pub label: SharedString,
    pub value: SharedString,
    pub selected: bool,
    pub disabled: bool,
}

impl MultiSelectorOption {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            selected: false,
            disabled: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(IntoElement)]
pub struct MultiSelector {
    id: ElementId,
    label: SharedString,
    hidden_label: bool,
    description: Option<SharedString>,
    options: Vec<MultiSelectorOption>,
    placeholder: SharedString,
    disabled: bool,
    optional: bool,
    required: bool,
    loading: bool,
    open: bool,
    size: MultiSelectorSize,
    status: Option<MultiSelectorStatus>,
    status_message: Option<SharedString>,
    start_icon: Option<SharedString>,
    clearable: bool,
    max_visible: usize,
    on_open: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl MultiSelector {
    #[track_caller]
    pub fn new(label: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "multi-selector:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: label.into(),
            hidden_label: false,
            description: None,
            options: Vec::new(),
            placeholder: "Select options".into(),
            disabled: false,
            optional: false,
            required: false,
            loading: false,
            open: false,
            size: MultiSelectorSize::default(),
            status: None,
            status_message: None,
            start_icon: None,
            clearable: false,
            max_visible: 2,
            on_open: None,
            on_clear: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn option(mut self, option: MultiSelectorOption) -> Self {
        self.options.push(option);
        self
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn options(mut self, options: impl IntoIterator<Item = MultiSelectorOption>) -> Self {
        self.options.extend(options);
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.hidden_label(hidden)
    }

    pub fn is_loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.is_loading(loading)
    }

    pub fn size(mut self, size: MultiSelectorSize) -> Self {
        self.size = size;
        self
    }

    pub fn status(mut self, status: MultiSelectorStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn status_message(mut self, message: impl Into<SharedString>) -> Self {
        self.status_message = Some(message.into());
        self
    }

    pub fn start_icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.start_icon = Some(icon.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn startIcon(self, icon: impl Into<SharedString>) -> Self {
        self.start_icon(icon)
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasClear(self, clearable: bool) -> Self {
        self.clearable(clearable)
    }

    pub fn max_visible(mut self, max_visible: usize) -> Self {
        self.max_visible = max_visible;
        self
    }

    #[allow(non_snake_case)]
    pub fn maxBadges(self, max_badges: usize) -> Self {
        self.max_visible(max_badges)
    }

    pub fn on_open(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Rc::new(handler));
        self
    }

    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }
}

impl Styled for MultiSelector {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MultiSelector {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let selected: Vec<_> = self
            .options
            .iter()
            .filter(|opt| opt.selected)
            .cloned()
            .collect();
        let overflow = selected.len().saturating_sub(self.max_visible);
        let has_selected = !selected.is_empty();
        let on_open = self.on_open;
        let on_clear = self.on_clear;
        let is_busy = self.loading;
        let is_open = self.open;
        let selector_id = self.id;
        let trigger_id = ElementId::NamedChild(Box::new(selector_id.clone()), "trigger".into());
        let clear_id = ElementId::NamedChild(Box::new(selector_id.clone()), "clear".into());
        let field_label = self.label.clone();
        let can_open = !self.disabled && !is_busy && on_open.is_some();
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let (height, padding_x, text_size, icon_size) = match self.size {
            InputSize::Sm => (px(28.0), px(8.0), px(14.0), px(16.0)),
            InputSize::Md => (px(32.0), px(12.0), px(14.0), px(16.0)),
            InputSize::Lg => (px(36.0), px(12.0), px(14.0), px(16.0)),
        };
        let status_color = self.status.map(|status| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });
        let mut accessibility_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if self.disabled || !can_open {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if self.required {
            accessibility_state |= AccessibilityState::REQUIRED;
        }
        if is_busy {
            accessibility_state |= AccessibilityState::BUSY;
        }
        if self.status == Some(FieldStatusType::Error) {
            accessibility_state |= AccessibilityState::INVALID;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::ComboBox)
            .label(field_label.to_string())
            .placeholder(self.placeholder.to_string())
            .states(accessibility_state);
        if let Some(description) = self.description.as_ref() {
            accessibility = accessibility.description(description.to_string());
        }
        if has_selected {
            accessibility = accessibility.value(AccessibilityValue::Text(
                selected
                    .iter()
                    .map(|option| option.label.as_ref())
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if can_open {
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
        let mut clear_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(format!("Clear {}", field_label))
            .states(if on_clear.is_some() {
                AccessibilityState::NONE
            } else {
                AccessibilityState::DISABLED
            });
        if on_clear.is_some() {
            clear_accessibility = clear_accessibility
                .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let field = Field::new(
            self.label,
            div()
                .id(trigger_id)
                .accessibility(accessibility)
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(height)
                .px(padding_x)
                .bg(theme.tokens.card)
                .border_1()
                .border_color(status_color.unwrap_or(theme.tokens.input))
                .rounded(theme.tokens.radius_md)
                .text_size(text_size)
                .font_family(theme.tokens.font_family.clone())
                .transition(theme.tokens.transition_fast)
                .shadow(smallvec::smallvec![astryx::focus_ring(
                    kael::transparent_black()
                )])
                .when_some(status_color, |this, color| {
                    this.shadow(smallvec::smallvec![astryx::input_hover_ring(color)])
                })
                .when(self.disabled || is_busy, |this| this.opacity(0.5))
                .when(can_open, |this| {
                    this.focusable()
                        .tab_index(0)
                        .tab_stop(true)
                        .cursor_pointer()
                        .hover(move |style| {
                            style
                                .border_color(status_color.unwrap_or(theme.tokens.input))
                                .shadow(smallvec::smallvec![
                                    status_color.map_or(hover_ring, astryx::input_hover_ring,)
                                ])
                        })
                        .when_some(on_open, |this, handler| {
                            let on_key = handler.clone();
                            this.on_click(move |_, window, cx| {
                                handler(window, cx);
                            })
                            .on_key_down(move |event, window, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    on_key(window, cx);
                                    cx.stop_propagation();
                                    window.prevent_default();
                                }
                            })
                        })
                })
                .when_some(self.start_icon.clone(), |this, icon| {
                    this.child(
                        div()
                            .size(px(16.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .child(
                                Icon::new(icon)
                                    .size(icon_size)
                                    .color(theme.tokens.muted_foreground),
                            ),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .overflow_hidden()
                        .when(selected.is_empty(), |this| {
                            this.child(
                                div()
                                    .text_size(text_size)
                                    .text_color(theme.tokens.muted_foreground)
                                    .child(self.placeholder.clone()),
                            )
                        })
                        .children(
                            selected
                                .iter()
                                .take(self.max_visible)
                                .cloned()
                                .map(|opt| Token::new(opt.label).into_any_element()),
                        )
                        .when(overflow > 0, |this| {
                            this.child(
                                div()
                                    .text_size(text_size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.tokens.muted_foreground)
                                    .child(format!("+{overflow}")),
                            )
                        }),
                )
                .when(
                    self.clearable && has_selected && !self.disabled && !is_busy,
                    |this| {
                        this.child(
                            div()
                                .id(clear_id)
                                .accessibility(clear_accessibility)
                                .size(px(24.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(theme.tokens.radius_sm)
                                .when(on_clear.is_some(), |this| {
                                    this.focusable()
                                        .tab_index(0)
                                        .tab_stop(true)
                                        .cursor_pointer()
                                        .hover(|style| {
                                            style.bg(astryx::overlay_hover(
                                                theme.tokens.background.l < 0.5,
                                            ))
                                        })
                                        .focus_visible(|style| {
                                            style.bg(astryx::overlay_hover(
                                                theme.tokens.background.l < 0.5,
                                            ))
                                        })
                                })
                                .when_some(on_clear, |this, handler| {
                                    let on_key = handler.clone();
                                    this.on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        handler(window, cx);
                                    })
                                    .on_key_down(
                                        move |event, window, cx| {
                                            if matches!(
                                                event.keystroke.key.as_str(),
                                                "enter" | "space"
                                            ) {
                                                on_key(window, cx);
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            }
                                        },
                                    )
                                })
                                .child(
                                    Icon::new("x")
                                        .size(icon_size)
                                        .color(theme.tokens.muted_foreground),
                                ),
                        )
                    },
                )
                .when(is_busy, |this| {
                    this.child(
                        crate::components::spinner::Spinner::new()
                            .size(crate::components::spinner::SpinnerSize::Sm),
                    )
                })
                .child(
                    div()
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .child(
                            Icon::new("chevron-down")
                                .size(icon_size)
                                .color(theme.tokens.muted_foreground),
                        ),
                ),
        )
        .disabled(self.disabled)
        .hidden_label(self.hidden_label)
        .optional(self.optional)
        .required(self.required)
        .when_some(self.description, |field, description| {
            field.description(description)
        })
        .when_some(
            self.status.zip(self.status_message),
            |field, (status, message)| field.status(status, message),
        );
        field.map(|this| {
            let mut field = this;
            field.style().refine(&self.style);
            field
        })
    }
}
