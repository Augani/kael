//! CheckboxList component - grouped ASTRYX checkbox rows.

use crate::{
    components::checkbox::{Checkbox, CheckboxSize},
    components::{
        field::{Field, FieldStatusType},
        field_status::FieldStatusVariant,
        list::ListDensity,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

#[derive(Clone)]
pub struct CheckboxListItem {
    pub id: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub end_content: Option<SharedString>,
    pub checked: bool,
    pub disabled: bool,
}

impl CheckboxListItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            end_content: None,
            checked: false,
            disabled: false,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    #[allow(non_snake_case)]
    pub fn isChecked(self, checked: bool) -> Self {
        self.checked(checked)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn end_content(mut self, content: impl Into<SharedString>) -> Self {
        self.end_content = Some(content.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl Into<SharedString>) -> Self {
        self.end_content(content)
    }
}

#[derive(IntoElement)]
pub struct CheckboxList {
    id: ElementId,
    label: Option<SharedString>,
    items: Vec<CheckboxListItem>,
    size: CheckboxSize,
    density: ListDensity,
    has_dividers: bool,
    description: Option<SharedString>,
    status: Option<(FieldStatusType, SharedString)>,
    disabled: bool,
    read_only: bool,
    hidden_label: bool,
    width: Option<Pixels>,
    on_change: Option<Rc<dyn Fn(SharedString, bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl CheckboxList {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "checkbox-list:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: None,
            items: Vec::new(),
            size: CheckboxSize::Md,
            density: ListDensity::Balanced,
            has_dividers: false,
            description: None,
            status: None,
            disabled: false,
            read_only: false,
            hidden_label: false,
            width: None,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn item(mut self, item: CheckboxListItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = CheckboxListItem>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn size(mut self, size: CheckboxSize) -> Self {
        self.size = size;
        self
    }

    pub fn density(mut self, density: ListDensity) -> Self {
        self.density = density;
        self
    }

    pub fn has_dividers(mut self, has_dividers: bool) -> Self {
        self.has_dividers = has_dividers;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasDividers(self, has_dividers: bool) -> Self {
        self.has_dividers(has_dividers)
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
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

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    #[allow(non_snake_case)]
    pub fn isReadOnly(self, read_only: bool) -> Self {
        self.read_only(read_only)
    }

    pub fn hidden_label(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.hidden_label(hidden)
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn value(mut self, values: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        let values = values.into_iter().map(Into::into).collect::<Vec<_>>();
        for item in &mut self.items {
            item.checked = values.iter().any(|value| value == &item.id);
        }
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(SharedString, bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Default for CheckboxList {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for CheckboxList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CheckboxList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let size = match self.density {
            ListDensity::Compact => CheckboxSize::Sm,
            ListDensity::Balanced | ListDensity::Spacious => self.size,
        };
        let on_change = self.on_change.clone();
        let disabled = self.disabled;
        let read_only = self.read_only;
        let row_padding_y = match self.density {
            ListDensity::Compact => px(4.0),
            ListDensity::Balanced => px(6.0),
            ListDensity::Spacious => px(10.0),
        };
        let has_dividers = self.has_dividers;
        let item_count = self.items.len();
        let list_id = self.id.clone();
        let list_label = self.label.clone().unwrap_or_else(|| "Checkbox list".into());

        let list = div()
            .id(list_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::List).label(list_label.to_string()),
            )
            .flex()
            .flex_col()
            .gap(if has_dividers { px(0.0) } else { px(2.0) })
            .children(self.items.into_iter().enumerate().map(move |(idx, item)| {
                let item_id = item.id.clone();
                let on_change = on_change.clone();
                let on_change_for_row = on_change.clone();
                let item_disabled = disabled || item.disabled;
                let interactive = !item_disabled && !read_only;
                let row_item_id = item.id.clone();
                let checkbox_label = item.label.clone();
                let row_element_id =
                    ElementId::NamedChild(Box::new(list_id.clone()), item.id.clone());

                div()
                    .id(row_element_id)
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::ListItem)
                            .label(item.label.to_string()),
                    )
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .py(row_padding_y)
                    .rounded(theme.tokens.radius_md)
                    .when(item.checked && !item_disabled && !read_only, |this| {
                        this.bg(theme.tokens.accent)
                    })
                    .when(has_dividers && idx + 1 < item_count, |this| {
                        this.border_b_1().border_color(theme.tokens.border)
                    })
                    .when(interactive, |this| {
                        this.cursor_pointer().hover(|style| {
                            style.bg(crate::astryx::overlay_hover(
                                theme.tokens.background.l < 0.5,
                            ))
                        })
                    })
                    .child(
                        Checkbox::new(ElementId::Name(item.id.clone()))
                            .label(checkbox_label)
                            .checked(item.checked)
                            .disabled(item_disabled || read_only)
                            .size(size)
                            .on_click(move |checked, window, cx| {
                                if interactive && let Some(handler) = on_change.as_ref() {
                                    handler(item_id.clone(), *checked, window, cx);
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(1.0))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .line_height(px(20.0))
                                    .text_color(if item_disabled {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    })
                                    .child(item.label),
                            )
                            .when_some(item.description, |this, description| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .line_height(px(16.0))
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(description),
                                )
                            }),
                    )
                    .when_some(item.end_content, |this, content| {
                        this.child(
                            div()
                                .ml_auto()
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .text_color(theme.tokens.muted_foreground)
                                .child(content),
                        )
                    })
                    .when_some(
                        on_change_for_row.filter(|_| interactive),
                        |this, handler| {
                            this.on_click(move |_, window, cx| {
                                handler(row_item_id.clone(), !item.checked, window, cx);
                            })
                        },
                    )
                    .when(!interactive, |this| this.cursor(CursorStyle::Arrow))
            }));

        let content = if let Some(label) = self.label {
            Field::new(label, list)
                .hidden_label(self.hidden_label)
                .disabled(disabled)
                .status_variant(FieldStatusVariant::Detached)
                .when_some(self.description, |field, description| {
                    field.description(description)
                })
                .when_some(self.status, |field, (status, message)| {
                    field.status(status, message)
                })
                .when_some(self.width, |field, width| field.width(width))
                .into_any_element()
        } else {
            list.into_any_element()
        };

        div().child(content).map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div
        })
    }
}
