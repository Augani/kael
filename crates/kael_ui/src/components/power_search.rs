//! PowerSearch component - ASTRYX-style filter token search surface.

use crate::{
    astryx,
    components::{icon::Icon, token::Token},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PowerSearchSize {
    Sm,
    #[default]
    Md,
    Lg,
}

impl PowerSearchSize {
    fn control_size(self) -> astryx::ControlSize {
        match self {
            Self::Sm => astryx::ControlSize::Sm,
            Self::Md => astryx::ControlSize::Md,
            Self::Lg => astryx::ControlSize::Lg,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmptyOperatorValue;
#[derive(Clone, Debug, PartialEq)]
pub struct StringOperatorValue;
#[derive(Clone, Debug, PartialEq)]
pub struct StringListOperatorValue;
#[derive(Clone, Debug, PartialEq)]
pub struct IntegerOperatorValue {
    pub min_value: Option<i64>,
    pub max_value: Option<i64>,
    pub units: Option<SharedString>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct FloatOperatorValue {
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub units: Option<SharedString>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TimeOperatorValue {
    pub min_value: Option<SharedString>,
    pub max_value: Option<SharedString>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DateAbsoluteOperatorValue {
    pub is_date_only: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DateRelativeOperatorValue {
    pub is_past_allowed: bool,
    pub is_future_allowed: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DateRangeOperatorValue {
    pub interval_date_presets: Vec<DateRangeFilterPreset>,
    pub relative_date_presets: Vec<RelativeDateFilterPreset>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EnumOperatorValue {
    pub values: Vec<EnumItem>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EnumListOperatorValue {
    pub values: Vec<EnumItem>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EntityListOperatorValue {
    pub tokenization: Option<OperatorTokenizationConfig>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CustomOperatorValue {
    pub label: SharedString,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NestedOperatorValue;

#[derive(Clone, Debug, PartialEq)]
pub enum OperatorValue {
    Empty(EmptyOperatorValue),
    String(StringOperatorValue),
    StringList(StringListOperatorValue),
    Integer(IntegerOperatorValue),
    Float(FloatOperatorValue),
    Time(TimeOperatorValue),
    DateAbsolute(DateAbsoluteOperatorValue),
    DateRelative(DateRelativeOperatorValue),
    DateRange(DateRangeOperatorValue),
    Enum(EnumOperatorValue),
    EnumList(EnumListOperatorValue),
    EntityList(EntityListOperatorValue),
    Custom(CustomOperatorValue),
    Nested(NestedOperatorValue),
}

impl OperatorValue {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Empty(_) => "empty",
            Self::String(_) => "string",
            Self::StringList(_) => "string_list",
            Self::Integer(_) => "integer",
            Self::Float(_) => "float",
            Self::Time(_) => "time",
            Self::DateAbsolute(_) => "date_absolute",
            Self::DateRelative(_) => "date_relative",
            Self::DateRange(_) => "date_range",
            Self::Enum(_) => "enum",
            Self::EnumList(_) => "enum_list",
            Self::EntityList(_) => "entity_list",
            Self::Custom(_) => "custom",
            Self::Nested(_) => "nested",
        }
    }
}

impl Default for OperatorValue {
    fn default() -> Self {
        Self::String(StringOperatorValue)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumItem {
    pub value: SharedString,
    pub label: SharedString,
    pub icon: Option<SharedString>,
}

impl EnumItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerSearchEntity {
    pub id: SharedString,
    pub label: SharedString,
    pub photo: Option<SharedString>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorTokenizationConfig {
    pub regex: Option<SharedString>,
    pub sort: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DateTimeRangePart {
    Now,
    Absolute {
        unix_seconds: i64,
    },
    Relative {
        back_value: i64,
        unit: SharedString,
        anchor_key: Option<SharedString>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DateTimeRange {
    pub start: DateTimeRangePart,
    pub end: DateTimeRangePart,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DateRangeFilterPreset {
    pub label: SharedString,
    pub value: DateTimeRange,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RelativeDateFilterPreset {
    pub label: SharedString,
    pub value: SharedString,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FilterValue {
    Empty,
    String(SharedString),
    StringList(Vec<SharedString>),
    Integer(i64),
    Float(f64),
    Time(SharedString),
    DateAbsolute { unix_seconds: i64 },
    DateRelative(SharedString),
    DateRange(DateTimeRange),
    Enum(SharedString),
    EnumList(Vec<SharedString>),
    EntityList(Vec<PowerSearchEntity>),
    Custom(SharedString),
    Nested(Vec<PowerSearchFilter>),
}

impl FilterValue {
    pub fn display_text(&self) -> SharedString {
        match self {
            Self::Empty => "".into(),
            Self::String(value)
            | Self::Time(value)
            | Self::DateRelative(value)
            | Self::Enum(value)
            | Self::Custom(value) => value.clone(),
            Self::StringList(values) | Self::EnumList(values) => values.join(", ").into(),
            Self::Integer(value) => value.to_string().into(),
            Self::Float(value) => value.to_string().into(),
            Self::DateAbsolute { unix_seconds } => unix_seconds.to_string().into(),
            Self::DateRange(_) => "date range".into(),
            Self::EntityList(values) => values
                .iter()
                .map(|entity| entity.label.to_string())
                .collect::<Vec<_>>()
                .join(", ")
                .into(),
            Self::Nested(values) => format!("{} filters", values.len()).into(),
        }
    }
}

pub type FilterValueEmpty = FilterValue;
pub type FilterValueString = FilterValue;
pub type FilterValueStringList = FilterValue;
pub type FilterValueInteger = FilterValue;
pub type FilterValueFloat = FilterValue;
pub type FilterValueTime = FilterValue;
pub type FilterValueDateAbsolute = FilterValue;
pub type FilterValueDateRelative = FilterValue;
pub type FilterValueDateRange = FilterValue;
pub type FilterValueEnum = FilterValue;
pub type FilterValueEnumList = FilterValue;
pub type FilterValueEntityList = FilterValue;
pub type FilterValueCustom = FilterValue;
pub type FilterValueNested = FilterValue;

#[derive(Clone, Debug, PartialEq)]
pub struct PowerSearchOperator {
    pub key: SharedString,
    pub label: SharedString,
    pub value: OperatorValue,
}

impl PowerSearchOperator {
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            value: OperatorValue::default(),
        }
    }

    pub fn value(mut self, value: OperatorValue) -> Self {
        self.value = value;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerSearchConfig {
    pub name: SharedString,
    pub fields: Vec<PowerSearchField>,
    pub content_search_field_key: Option<SharedString>,
}

impl PowerSearchConfig {
    pub fn new(name: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
            content_search_field_key: None,
        }
    }

    pub fn field(mut self, field: PowerSearchField) -> Self {
        self.fields.push(field);
        self
    }

    pub fn fields(mut self, fields: impl IntoIterator<Item = PowerSearchField>) -> Self {
        self.fields = fields.into_iter().collect();
        self
    }

    pub fn content_search_field_key(mut self, key: impl Into<SharedString>) -> Self {
        self.content_search_field_key = Some(key.into());
        self
    }
}

pub fn create_power_search_config(
    name: impl Into<SharedString>,
    fields: impl IntoIterator<Item = PowerSearchField>,
) -> PowerSearchConfig {
    PowerSearchConfig::new(name).fields(fields)
}

#[allow(non_snake_case)]
pub fn createPowerSearchConfig(
    name: impl Into<SharedString>,
    fields: impl IntoIterator<Item = PowerSearchField>,
) -> PowerSearchConfig {
    create_power_search_config(name, fields)
}

pub fn use_power_search_config(config: PowerSearchConfig) -> PowerSearchConfig {
    config
}

#[allow(non_snake_case)]
pub fn usePowerSearchConfig(config: PowerSearchConfig) -> PowerSearchConfig {
    use_power_search_config(config)
}

pub type FieldDefinition = PowerSearchField;
pub type InferData = BTreeMap<SharedString, SharedString>;
pub type SearchableItem = SharedString;
pub type SearchSource = Vec<SearchableItem>;
pub type PowerSearchComponentOverride = SharedString;
pub type PowerSearchComponents = BTreeMap<SharedString, PowerSearchComponentOverride>;
pub type PowerSearchTokenProps = PowerSearchFilter;
pub type PowerSearchEditorProps = PartialFilter;

#[derive(Clone, Debug, PartialEq)]
pub enum PowerSearchChangeType {
    Add,
    Edit,
    Remove,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerSearchHandle {
    pub is_focused: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PartialFilter {
    pub field: SharedString,
    pub operator: Option<SharedString>,
    pub value: Option<FilterValue>,
    pub is_readonly: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerSearchField {
    pub key: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub operators: Vec<PowerSearchOperator>,
    pub icon: Option<SharedString>,
    pub default_operator: Option<SharedString>,
    pub group: Option<SharedString>,
    pub typeahead_aliases: Vec<SharedString>,
    pub typeahead_min_query_length: Option<usize>,
    pub is_value_match_allowed: bool,
}

impl PowerSearchField {
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            description: None,
            operators: Vec::new(),
            icon: None,
            default_operator: None,
            group: None,
            typeahead_aliases: Vec::new(),
            typeahead_min_query_length: None,
            is_value_match_allowed: true,
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn operator(mut self, operator: PowerSearchOperator) -> Self {
        self.operators.push(operator);
        self
    }

    pub fn operators(mut self, operators: impl IntoIterator<Item = PowerSearchOperator>) -> Self {
        self.operators = operators.into_iter().collect();
        self
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn default_operator(mut self, operator: impl Into<SharedString>) -> Self {
        self.default_operator = Some(operator.into());
        self
    }

    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn typeahead_alias(mut self, alias: impl Into<SharedString>) -> Self {
        self.typeahead_aliases.push(alias.into());
        self
    }

    pub fn typeahead_min_query_length(mut self, length: usize) -> Self {
        self.typeahead_min_query_length = Some(length);
        self
    }

    pub fn value_match_allowed(mut self, allowed: bool) -> Self {
        self.is_value_match_allowed = allowed;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PowerSearchFilter {
    pub field: SharedString,
    pub operator: SharedString,
    pub value: FilterValue,
    pub readonly: bool,
}

impl PowerSearchFilter {
    pub fn new(
        field: impl Into<SharedString>,
        operator: impl Into<SharedString>,
        value: impl Into<SharedString>,
    ) -> Self {
        Self {
            field: field.into(),
            operator: operator.into(),
            value: FilterValue::String(value.into()),
            readonly: false,
        }
    }

    pub fn typed(
        field: impl Into<SharedString>,
        operator: impl Into<SharedString>,
        value: FilterValue,
    ) -> Self {
        Self {
            field: field.into(),
            operator: operator.into(),
            value,
            readonly: false,
        }
    }

    pub fn value(mut self, value: FilterValue) -> Self {
        self.value = value;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }
}

#[derive(IntoElement)]
pub struct PowerSearch {
    query: SharedString,
    placeholder: SharedString,
    filters: Vec<PowerSearchFilter>,
    fields: Vec<PowerSearchField>,
    config: Option<PowerSearchConfig>,
    size: PowerSearchSize,
    label: SharedString,
    label_hidden: bool,
    clearable: bool,
    readonly: bool,
    disabled: bool,
    on_filter_remove: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl PowerSearch {
    pub fn new() -> Self {
        Self {
            query: "".into(),
            placeholder: "Search or filter...".into(),
            filters: Vec::new(),
            fields: Vec::new(),
            config: None,
            size: PowerSearchSize::Md,
            label: "Search".into(),
            label_hidden: true,
            clearable: true,
            readonly: false,
            disabled: false,
            on_filter_remove: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn query(mut self, query: impl Into<SharedString>) -> Self {
        self.query = query.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn filters(mut self, filters: impl IntoIterator<Item = PowerSearchFilter>) -> Self {
        self.filters = filters.into_iter().collect();
        self
    }

    pub fn fields(mut self, fields: impl IntoIterator<Item = PowerSearchField>) -> Self {
        self.fields = fields.into_iter().collect();
        self
    }

    pub fn config(mut self, config: PowerSearchConfig) -> Self {
        self.fields = config.fields.clone();
        self.config = Some(config);
        self
    }

    pub fn size(mut self, size: PowerSearchSize) -> Self {
        self.size = size;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.readonly = readonly;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_filter_remove(
        mut self,
        handler: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_filter_remove = Some(Rc::new(handler));
        self
    }
}

impl Default for PowerSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for PowerSearch {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PowerSearch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let on_remove = self.on_filter_remove.clone();
        let query_is_empty = self.query.is_empty();
        let size = self.size.control_size();
        let disabled = self.disabled || self.readonly;

        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .map(|this| {
                let mut div = this;
                div.style().refine(&self.style);
                div
            })
            .child(
                div()
                    .when(!self.label_hidden, |this| {
                        this.child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.tokens.muted_foreground)
                                .child(self.label.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(4.0))
                            .min_h(size.height())
                            .px(px(8.0))
                            .py(px(4.0))
                            .bg(theme.tokens.card)
                            .border_1()
                            .border_color(theme.tokens.input)
                            .rounded(theme.tokens.radius_md)
                            .when(disabled, |this| this.opacity(0.5))
                            .when(!disabled, |this| {
                                this.cursor_text().hover(move |style| {
                                    style.shadow(smallvec::smallvec![hover_ring])
                                })
                            })
                            .children(self.filters.into_iter().enumerate().map(
                                move |(index, filter)| {
                                    let label = format!(
                                        "{} {} {}",
                                        filter.field,
                                        filter.operator,
                                        filter.value.display_text()
                                    );
                                    let on_remove = on_remove.clone();
                                    let readonly = filter.readonly || disabled;
                                    let mut token = Token::new(label).icon("filter");
                                    if !readonly {
                                        token = token.on_remove(move |window, cx| {
                                            if let Some(handler) = on_remove.as_ref() {
                                                handler(index, window, cx);
                                            }
                                        });
                                    }
                                    token.into_any_element()
                                },
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .flex_1()
                                    .min_w(px(96.0))
                                    .text_size(px(14.0))
                                    .line_height(px(20.0))
                                    .text_color(if query_is_empty {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    })
                                    .child(
                                        Icon::new("search")
                                            .size(px(14.0))
                                            .color(theme.tokens.muted_foreground),
                                    )
                                    .child(if query_is_empty {
                                        self.placeholder.clone()
                                    } else {
                                        self.query.clone()
                                    }),
                            )
                            .when(self.clearable && !query_is_empty && !disabled, |this| {
                                this.child(
                                    Icon::new("close")
                                        .size(px(14.0))
                                        .color(theme.tokens.muted_foreground),
                                )
                            }),
                    ),
            )
            .when(!self.fields.is_empty(), |this| {
                this.child(
                    div().flex().flex_wrap().gap(px(4.0)).children(
                        self.fields
                            .into_iter()
                            .map(|field| Token::new(field.label).icon("plus").into_any_element()),
                    ),
                )
            })
    }
}
