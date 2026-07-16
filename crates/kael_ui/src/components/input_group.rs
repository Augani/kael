//! InputGroup component - connected input and addon group.

use crate::{
    astryx,
    components::{
        field::{Field, FieldStatusType},
        field_status::FieldStatusVariant,
        input::InputSize,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};

#[derive(IntoElement)]
pub struct InputGroup {
    label: SharedString,
    children: Vec<AnyElement>,
    size: InputSize,
    hidden_label: bool,
    description: Option<SharedString>,
    disabled: bool,
    optional: bool,
    required: bool,
    status: Option<(FieldStatusType, SharedString)>,
    style: StyleRefinement,
}

#[derive(IntoElement)]
pub struct InputGroupText {
    label: SharedString,
    size: InputSize,
    style: StyleRefinement,
}

impl InputGroupText {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            size: InputSize::Md,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for InputGroupText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for InputGroupText {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let height = input_group_height(self.size);

        div()
            .flex()
            .items_center()
            .h(height)
            .px(px(8.0))
            .bg(theme.tokens.muted.opacity(0.55))
            .border_1()
            .border_color(theme.tokens.input)
            .text_size(px(14.0))
            .font_family(theme.tokens.font_family.clone())
            .text_color(theme.tokens.muted_foreground)
            .line_height(px(20.0))
            .child(self.label)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

impl InputGroup {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            children: Vec::new(),
            size: InputSize::Md,
            hidden_label: false,
            description: None,
            disabled: false,
            optional: false,
            required: false,
            status: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
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

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }
}

impl Styled for InputGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for InputGroup {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let height = input_group_height(self.size);
        let mut field = Field::new(
            self.label,
            div()
                .flex()
                .h(height)
                .bg(transparent_black())
                .when(self.disabled, |this| this.opacity(0.5))
                .children(self.children),
        )
        .hidden_label(self.hidden_label)
        .disabled(self.disabled)
        .optional(self.optional)
        .required(self.required)
        .status_variant(FieldStatusVariant::Detached);

        if let Some(description) = self.description {
            field = field.description(description);
        }
        if let Some((status_type, message)) = self.status {
            field = field.status(status_type, message);
        }

        field.map(|this| {
            let mut field = this;
            field.style().refine(&user_style);
            field
        })
    }
}

fn input_group_height(size: InputSize) -> Pixels {
    match size {
        InputSize::Sm => astryx::ControlSize::Sm.height(),
        InputSize::Md => astryx::ControlSize::Md.height(),
        InputSize::Lg => astryx::ControlSize::Lg.height(),
    }
}
