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
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
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
        self.on_click = Some(Box::new(handler));
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

        div()
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
            .when_some(self.on_click.filter(|_| !disabled), |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    handler(window, cx);
                })
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
