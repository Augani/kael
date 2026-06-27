//! Typeahead component - selected token plus searchable input shell.

use crate::{
    astryx,
    components::{field::Field, field::FieldStatusType, icon::Icon, token::Token},
    theme::Theme,
};
use kael::{prelude::*, *};
use std::collections::BTreeMap;
use std::rc::Rc;

pub type TypeaheadSize = astryx::ControlSize;
pub type TypeaheadStatus = FieldStatusType;
pub type TypeaheadStatusType = FieldStatusType;
pub type TypeaheadProps = Typeahead;
pub type BaseTypeaheadProps = Typeahead;
pub type BaseTypeahead = Typeahead;
pub type TypeaheadItemProps = TypeaheadItem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchableItem {
    pub id: SharedString,
    pub label: SharedString,
    pub element: Option<SharedString>,
    pub auxiliary_data: BTreeMap<SharedString, SharedString>,
}

impl SearchableItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            element: None,
            auxiliary_data: BTreeMap::new(),
        }
    }

    pub fn element(mut self, element: impl Into<SharedString>) -> Self {
        self.element = Some(element.into());
        self
    }

    pub fn auxiliary_data(
        mut self,
        key: impl Into<SharedString>,
        value: impl Into<SharedString>,
    ) -> Self {
        self.auxiliary_data.insert(key.into(), value.into());
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSource {
    items: Vec<SearchableItem>,
}

impl SearchSource {
    pub fn new(items: impl IntoIterator<Item = SearchableItem>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    pub fn search(&self, query: &str) -> Vec<SearchableItem> {
        let query = query.to_lowercase();
        self.items
            .iter()
            .filter(|item| item.label.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    pub fn bootstrap(&self) -> Vec<SearchableItem> {
        self.items.clone()
    }

    pub fn cancel(&self) {}
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CreateStaticSourceOptions {
    pub limit: Option<usize>,
}

pub fn create_static_source(items: impl IntoIterator<Item = SearchableItem>) -> SearchSource {
    SearchSource::new(items)
}

#[allow(non_snake_case)]
pub fn createStaticSource(items: impl IntoIterator<Item = SearchableItem>) -> SearchSource {
    create_static_source(items)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeaheadItem {
    pub id: SharedString,
    pub label: SharedString,
    pub value: SharedString,
}

impl TypeaheadItem {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        let label = label.into();
        let value = value.into();
        Self {
            id: value.clone(),
            label,
            value,
        }
    }

    pub fn with_id(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        value: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(IntoElement)]
pub struct Typeahead {
    label: SharedString,
    label_hidden: bool,
    description: Option<SharedString>,
    required: bool,
    optional: bool,
    status: Option<TypeaheadStatus>,
    status_message: Option<SharedString>,
    start_icon: Option<SharedString>,
    width: Option<Pixels>,
    label_tooltip: Option<SharedString>,
    search_source: Option<SearchSource>,
    value: Option<TypeaheadItem>,
    query: SharedString,
    placeholder: SharedString,
    entries_on_focus: bool,
    max_menu_items: usize,
    empty_results_text: SharedString,
    autofocus: bool,
    size: TypeaheadSize,
    clearable: bool,
    readonly: bool,
    disabled: bool,
    debounce_ms: u64,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_change: Option<Rc<dyn Fn(TypeaheadItem, &mut Window, &mut App)>>,
    on_change_query: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_open_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Typeahead {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            label_hidden: false,
            description: None,
            required: false,
            optional: false,
            status: None,
            status_message: None,
            start_icon: None,
            width: None,
            label_tooltip: None,
            search_source: None,
            value: None,
            query: "".into(),
            placeholder: "Search...".into(),
            entries_on_focus: false,
            max_menu_items: 10,
            empty_results_text: "No results found".into(),
            autofocus: false,
            size: astryx::ControlSize::Md,
            clearable: true,
            readonly: false,
            disabled: false,
            debounce_ms: 150,
            on_clear: None,
            on_change: None,
            on_change_query: None,
            on_open_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn search_source(mut self, source: SearchSource) -> Self {
        self.search_source = Some(source);
        self
    }

    pub fn value(mut self, value: TypeaheadItem) -> Self {
        self.value = Some(value);
        self
    }

    pub fn query(mut self, query: impl Into<SharedString>) -> Self {
        self.query = query.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.label_hidden(hidden)
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn status(mut self, status: TypeaheadStatus) -> Self {
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

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into());
        self
    }

    pub fn label_tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.label_tooltip = Some(tooltip.into());
        self
    }

    pub fn entries_on_focus(mut self, entries_on_focus: bool) -> Self {
        self.entries_on_focus = entries_on_focus;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasEntriesOnFocus(self, entries_on_focus: bool) -> Self {
        self.entries_on_focus(entries_on_focus)
    }

    pub fn max_menu_items(mut self, max_menu_items: usize) -> Self {
        self.max_menu_items = max_menu_items;
        self
    }

    #[allow(non_snake_case)]
    pub fn maxMenuItems(self, max_menu_items: usize) -> Self {
        self.max_menu_items(max_menu_items)
    }

    pub fn empty_search_results_text(mut self, text: impl Into<SharedString>) -> Self {
        self.empty_results_text = text.into();
        self
    }

    #[allow(non_snake_case)]
    pub fn emptySearchResultsText(self, text: impl Into<SharedString>) -> Self {
        self.empty_search_results_text(text)
    }

    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasAutoFocus(self, autofocus: bool) -> Self {
        self.autofocus(autofocus)
    }

    pub fn size(mut self, size: TypeaheadSize) -> Self {
        self.size = size;
        self
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasClear(self, clearable: bool) -> Self {
        self.clearable(clearable)
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
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

    pub fn debounce_ms(mut self, debounce_ms: u64) -> Self {
        self.debounce_ms = debounce_ms;
        self
    }

    #[allow(non_snake_case)]
    pub fn debounceMs(self, debounce_ms: u64) -> Self {
        self.debounce_ms(debounce_ms)
    }

    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(TypeaheadItem, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn on_change_query(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change_query = Some(Rc::new(handler));
        self
    }

    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Typeahead {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Typeahead {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let on_clear = self.on_clear;
        let on_change = self.on_change;
        let has_value = self.value.is_some();
        let disabled = self.disabled || self.readonly;
        let clearable = self.clearable;
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let _ = (
            self.label_tooltip.clone(),
            self.entries_on_focus,
            self.max_menu_items,
            self.empty_results_text.clone(),
            self.autofocus,
            self.debounce_ms,
            self.on_change_query,
            self.on_open_change,
        );
        let query = self.query.clone();
        let results = self
            .search_source
            .as_ref()
            .filter(|_| !query.is_empty())
            .map(|source| source.search(&query))
            .unwrap_or_default();
        let show_results = self.search_source.is_some() && !query.is_empty();
        let selected_id = self.value.as_ref().map(|item| item.id.clone());
        let item_padding_y = match self.size {
            astryx::ControlSize::Sm => px(4.0),
            astryx::ControlSize::Md => px(6.0),
            astryx::ControlSize::Lg => px(8.0),
        };
        let status_color = self.status.map(|status| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });

        let label = if self.label_hidden {
            "".into()
        } else if self.required {
            format!("{} *", self.label).into()
        } else if self.optional {
            format!("{} (optional)", self.label).into()
        } else {
            self.label
        };

        let mut field = Field::new(
            label,
            div()
                .when_some(self.width, |this, width| this.w(width))
                .relative()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .min_h(self.size.height())
                        .px(px(8.0))
                        .bg(theme.tokens.card)
                        .border_1()
                        .border_color(status_color.unwrap_or(theme.tokens.input))
                        .rounded(theme.tokens.radius_md)
                        .transition(theme.tokens.transition_fast)
                        .shadow(smallvec::smallvec![astryx::focus_ring(
                            kael::transparent_black()
                        )])
                        .when_some(status_color, |this, color| {
                            this.shadow(smallvec::smallvec![astryx::input_hover_ring(color)])
                        })
                        .when(disabled, |this| this.opacity(0.5))
                        .when(!disabled, |this| {
                            this.cursor_text().hover(move |style| {
                                style
                                    .border_color(status_color.unwrap_or(theme.tokens.input))
                                    .shadow(smallvec::smallvec![
                                        status_color.map_or(hover_ring, astryx::input_hover_ring,)
                                    ])
                            })
                        })
                        .when_some(self.start_icon.clone(), |this, icon| {
                            this.child(
                                Icon::new(icon)
                                    .size(self.size.icon_size())
                                    .color(theme.tokens.muted_foreground),
                            )
                        })
                        .when_some(self.value, |this, value| {
                            let token = Token::new(value.label).disabled(disabled).size(self.size);
                            let token = if clearable && !disabled {
                                token.on_remove(move |window, cx| {
                                    if let Some(handler) = on_clear.as_ref() {
                                        handler(window, cx);
                                    }
                                })
                            } else {
                                token
                            };

                            this.child(token.into_any_element())
                        })
                        .when(!has_value, |this| {
                            this.child(
                                div()
                                    .flex_1()
                                    .text_size(px(14.0))
                                    .text_color(if query.is_empty() {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    })
                                    .child(if query.is_empty() {
                                        self.placeholder.clone()
                                    } else {
                                        query.clone()
                                    }),
                            )
                        })
                        .child(
                            Icon::new("search")
                                .size(self.size.icon_size())
                                .color(theme.tokens.muted_foreground),
                        ),
                )
                .when(show_results && !disabled, |this| {
                    this.child(
                        div()
                            .absolute()
                            .top_full()
                            .left_0()
                            .right_0()
                            .mt(px(4.0))
                            .bg(theme.tokens.popover)
                            .border_1()
                            .border_color(theme.tokens.border)
                            .rounded(theme.tokens.radius_lg)
                            .shadow(theme.tokens.shadow_lg.to_vec())
                            .overflow_hidden()
                            .child(
                                div()
                                    .max_h(px(300.0))
                                    .overflow_hidden()
                                    .py(px(4.0))
                                    .when(results.is_empty(), |this| {
                                        this.child(
                                            div()
                                                .px(px(12.0))
                                                .py(px(16.0))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .text_size(px(13.0))
                                                .font_family(theme.tokens.font_family.clone())
                                                .text_color(theme.tokens.muted_foreground)
                                                .child(self.empty_results_text.clone()),
                                        )
                                    })
                                    .when(!results.is_empty(), |this| {
                                        this.children(
                                            results.into_iter().take(self.max_menu_items).map(
                                                |item| {
                                                    let is_selected =
                                                        selected_id.as_ref() == Some(&item.id);
                                                    let item_for_handler = TypeaheadItem::with_id(
                                                        item.id.clone(),
                                                        item.label.clone(),
                                                        item.id.clone(),
                                                    );
                                                    let on_change = on_change.clone();
                                                    div()
                                                        .px(px(8.0))
                                                        .py(item_padding_y)
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap(px(8.0))
                                                        .rounded(theme.tokens.radius_md)
                                                        .text_color(theme.tokens.popover_foreground)
                                                        .bg(if is_selected {
                                                            theme.tokens.primary.opacity(0.12)
                                                        } else {
                                                            kael::transparent_black()
                                                        })
                                                        .hover(|style| {
                                                            style.bg(astryx::overlay_hover(
                                                                theme.tokens.background.l < 0.5,
                                                            ))
                                                        })
                                                        .cursor_pointer()
                                                        .text_size(px(14.0))
                                                        .when(is_selected, |this| {
                                                            this.font_weight(FontWeight::MEDIUM)
                                                        })
                                                        .font_family(
                                                            theme.tokens.font_family.clone(),
                                                        )
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            move |_, window, cx| {
                                                                if let Some(handler) =
                                                                    on_change.as_ref()
                                                                {
                                                                    handler(
                                                                        item_for_handler.clone(),
                                                                        window,
                                                                        cx,
                                                                    );
                                                                }
                                                            },
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w(px(0.0))
                                                                .child(item.label),
                                                        )
                                                        .child(
                                                            div()
                                                                .size(px(16.0))
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .flex_shrink_0()
                                                                .when(is_selected, |slot| {
                                                                    slot.child(
                                                                        Icon::new("check")
                                                                            .size(px(16.0))
                                                                            .color(
                                                                                theme
                                                                                    .tokens
                                                                                    .primary,
                                                                            ),
                                                                    )
                                                                }),
                                                        )
                                                        .into_any_element()
                                                },
                                            ),
                                        )
                                    }),
                            ),
                    )
                }),
        )
        .disabled(disabled);
        if let Some(description) = self.description {
            field = field.description(description);
        }
        if let Some((status, message)) = self.status.zip(self.status_message) {
            field = field.status(status, message);
        }

        field.map(|this| {
            let mut field = this;
            field.style().refine(&self.style);
            field
        })
    }
}
