//! Typeahead component - selected token plus searchable input shell.

use crate::{
    astryx,
    components::{
        field::FieldStatusType,
        icon::Icon,
        input::{Input, InputSize},
        input_state::{InputEvent, InputState},
    },
    theme::Theme,
};
use kael::{prelude::*, *};
use std::collections::BTreeMap;
use std::panic::Location;
use std::rc::Rc;
use std::time::Duration;

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

fn next_typeahead_index(count: usize, current: Option<usize>, delta: isize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let start = current.unwrap_or(if delta < 0 { 0 } else { count - 1 });
    Some((start as isize + delta).rem_euclid(count as isize) as usize)
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
    id: ElementId,
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
    #[track_caller]
    pub fn new(label: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "typeahead:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
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

    pub fn read_only(self, read_only: bool) -> Self {
        self.readonly(read_only)
    }

    #[allow(non_snake_case)]
    pub fn isReadOnly(self, read_only: bool) -> Self {
        self.readonly(read_only)
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

struct TypeaheadRuntime {
    input: Entity<InputState>,
    props: Option<Typeahead>,
    open: bool,
    focused_index: Option<usize>,
    input_focused: bool,
    initialized: bool,
    autofocus_applied: bool,
    window_handle: Option<AnyWindowHandle>,
    query_generation: u64,
    last_query: SharedString,
    last_external_query: SharedString,
    last_external_value_id: Option<SharedString>,
    suppress_next_input_change: bool,
}

impl TypeaheadRuntime {
    fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(InputState::new);
        let input_for_events = input.clone();

        cx.subscribe(&input, move |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => {
                let query: SharedString = input_for_events.read(cx).content().to_string().into();
                if this.suppress_next_input_change {
                    this.suppress_next_input_change = false;
                    this.last_query = query;
                    this.focused_index = None;
                    cx.notify();
                    return;
                }
                let previous_query = this.last_query.clone();
                this.last_query = query.clone();
                this.focused_index = None;

                let should_open = this.props.as_ref().is_some_and(|props| {
                    let displays_controlled_value = props.query.is_empty()
                        && props
                            .value
                            .as_ref()
                            .is_some_and(|value| value.label == query);
                    props.search_source.is_some()
                        && ((!query.is_empty() && !displays_controlled_value)
                            || props.entries_on_focus && this.input_focused)
                        && !props.disabled
                        && !props.readonly
                });
                let open_changed = this.open != should_open;
                this.open = should_open;
                this.query_generation = this.query_generation.wrapping_add(1);
                let generation = this.query_generation;

                let Some(props) = this.props.as_ref() else {
                    cx.notify();
                    return;
                };

                if open_changed
                    && let (Some(callback), Some(window_handle)) =
                        (props.on_open_change.clone(), this.window_handle)
                {
                    cx.defer(move |cx| {
                        let _ = cx.update_window(window_handle, move |_, window, cx| {
                            callback(should_open, window, cx);
                        });
                    });
                }

                if query.is_empty()
                    && !previous_query.is_empty()
                    && let (Some(callback), Some(window_handle)) =
                        (props.on_clear.clone(), this.window_handle)
                {
                    cx.defer(move |cx| {
                        let _ = cx.update_window(window_handle, move |_, window, cx| {
                            callback(window, cx);
                        });
                    });
                }

                if let (Some(callback), Some(window_handle)) =
                    (props.on_change_query.clone(), this.window_handle)
                {
                    let delay = Duration::from_millis(props.debounce_ms);
                    cx.spawn(async move |this, cx| {
                        if !delay.is_zero() {
                            cx.background_executor().timer(delay).await;
                        }
                        let _ = this.update(cx, |this, cx| {
                            if this.query_generation != generation {
                                return;
                            }
                            cx.defer(move |cx| {
                                let _ = cx.update_window(window_handle, move |_, window, cx| {
                                    callback(query, window, cx);
                                });
                            });
                        });
                    })
                    .detach();
                }

                cx.notify();
            }
            InputEvent::Focus => {
                this.input_focused = true;
                let should_open = this.props.as_ref().is_some_and(|props| {
                    props.entries_on_focus
                        && props.search_source.is_some()
                        && !props.disabled
                        && !props.readonly
                });
                if should_open && !this.open {
                    this.open = true;
                    if let (Some(callback), Some(window_handle)) = (
                        this.props
                            .as_ref()
                            .and_then(|props| props.on_open_change.clone()),
                        this.window_handle,
                    ) {
                        cx.defer(move |cx| {
                            let _ = cx.update_window(window_handle, move |_, window, cx| {
                                callback(true, window, cx);
                            });
                        });
                    }
                    cx.notify();
                }
            }
            InputEvent::Enter if this.open => {
                if let (Some(index), Some(window_handle)) = (this.focused_index, this.window_handle)
                {
                    let runtime = cx.entity();
                    cx.defer(move |cx| {
                        let _ = cx.update_window(window_handle, move |_, window, cx| {
                            runtime.update(cx, |runtime, cx| {
                                runtime.select_index(index, window, cx);
                            });
                        });
                    });
                }
            }
            InputEvent::Blur if this.open => {
                this.input_focused = false;
                this.open = false;
                this.focused_index = None;
                if let (Some(callback), Some(window_handle)) = (
                    this.props
                        .as_ref()
                        .and_then(|props| props.on_open_change.clone()),
                    this.window_handle,
                ) {
                    cx.defer(move |cx| {
                        let _ = cx.update_window(window_handle, move |_, window, cx| {
                            callback(false, window, cx);
                        });
                    });
                }
                cx.notify();
            }
            InputEvent::Blur => {
                this.input_focused = false;
                cx.notify();
            }
            _ => {}
        })
        .detach();

        Self {
            input,
            props: None,
            open: false,
            focused_index: None,
            input_focused: false,
            initialized: false,
            autofocus_applied: false,
            window_handle: None,
            query_generation: 0,
            last_query: SharedString::default(),
            last_external_query: SharedString::default(),
            last_external_value_id: None,
            suppress_next_input_change: false,
        }
    }

    fn filtered_items(&self, cx: &App) -> Vec<SearchableItem> {
        let Some(props) = self.props.as_ref() else {
            return Vec::new();
        };
        let Some(source) = props.search_source.as_ref() else {
            return Vec::new();
        };
        let raw_query = self.input.read(cx).content();
        let query = if props.query.is_empty()
            && props
                .value
                .as_ref()
                .is_some_and(|value| value.label.as_ref() == raw_query)
        {
            ""
        } else {
            raw_query
        };
        let mut items = if query.is_empty() {
            if props.entries_on_focus {
                source.bootstrap()
            } else {
                Vec::new()
            }
        } else {
            source.search(query)
        };
        items.truncate(props.max_menu_items);
        items
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open
            && self
                .props
                .as_ref()
                .is_some_and(|props| props.disabled || props.readonly)
        {
            return;
        }
        if self.open == open {
            return;
        }
        self.open = open;
        if !open {
            self.focused_index = None;
        }
        if let Some(callback) = self
            .props
            .as_ref()
            .and_then(|props| props.on_open_change.as_ref())
        {
            callback(open, window, cx);
        }
        cx.notify();
    }

    fn move_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.filtered_items(cx).len();
        if count == 0 {
            self.focused_index = None;
            return;
        }
        if !self.open {
            self.set_open(true, window, cx);
        }
        self.focused_index = next_typeahead_index(count, self.focused_index, delta);
        cx.notify();
    }

    fn select_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .props
            .as_ref()
            .is_some_and(|props| props.disabled || props.readonly)
        {
            return;
        }
        let Some(item) = self.filtered_items(cx).get(index).cloned() else {
            return;
        };
        let selected = TypeaheadItem::with_id(item.id.clone(), item.label.clone(), item.id.clone());
        self.suppress_next_input_change = self.input.read(cx).content() != item.label.as_ref();
        self.input.update(cx, |input, cx| {
            input.set_value(item.label.clone(), window, cx);
        });
        self.last_query = item.label;
        self.open = false;
        self.focused_index = None;

        if let Some(props) = self.props.as_ref() {
            if let Some(callback) = props.on_change.as_ref() {
                callback(selected, window, cx);
            }
            if let Some(callback) = props.on_open_change.as_ref() {
                callback(false, window, cx);
            }
        }
        cx.notify();
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .props
            .as_ref()
            .is_some_and(|props| props.disabled || props.readonly)
        {
            return;
        }
        if event.keystroke.modifiers.modified() {
            return;
        }
        let handled = match event.keystroke.key.as_str() {
            "down" => {
                self.move_focus(1, window, cx);
                true
            }
            "up" => {
                self.move_focus(-1, window, cx);
                true
            }
            "enter" if self.open && self.focused_index.is_some() => {
                self.select_index(self.focused_index.unwrap_or(0), window, cx);
                true
            }
            "escape" if self.open => {
                self.set_open(false, window, cx);
                true
            }
            _ => false,
        };
        if handled {
            window.prevent_default();
            cx.stop_propagation();
        }
    }
}

impl Render for TypeaheadRuntime {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window_handle = Some(window.window_handle());
        let Some(props) = self.props.as_ref() else {
            return div().into_any_element();
        };

        if props.autofocus && !self.autofocus_applied && !props.disabled && !props.readonly {
            window.focus(&self.input.read(cx).focus_handle(cx));
            self.autofocus_applied = true;
        }

        let theme = Theme::of(cx).clone();
        let runtime = cx.entity();
        let results = self.filtered_items(cx);
        let show_results =
            self.open && props.search_source.is_some() && !props.disabled && !props.readonly;
        let selected_id = props.value.as_ref().map(|item| item.id.clone());
        let focused_index = self.focused_index;
        let disabled = props.disabled;
        let read_only = props.readonly;
        let user_style = props.style.clone();
        let label = props.label.clone();
        let label_hidden = props.label_hidden;
        let optional = props.optional;
        let required = props.required;
        let placeholder = props.placeholder.clone();
        let size = match props.size {
            astryx::ControlSize::Sm => InputSize::Sm,
            astryx::ControlSize::Md => InputSize::Md,
            astryx::ControlSize::Lg => InputSize::Lg,
        };
        let clearable = props.clearable;
        let helper_text = props
            .status_message
            .clone()
            .or_else(|| props.description.clone());
        let error = props.status == Some(FieldStatusType::Error);
        let aria_description = props
            .label_tooltip
            .clone()
            .or_else(|| props.description.clone());
        let start_icon = props.start_icon.clone();
        let width = props.width;
        let empty_results_text = props.empty_results_text.clone();
        let item_padding_y = match props.size {
            astryx::ControlSize::Sm => px(4.0),
            astryx::ControlSize::Md => px(6.0),
            astryx::ControlSize::Lg => px(8.0),
        };

        let mut input = Input::new(&self.input)
            .label(label.clone())
            .is_label_hidden(label_hidden)
            .is_optional(optional)
            .required(required)
            .placeholder(placeholder)
            .size(size)
            .disabled(disabled)
            .read_only(read_only)
            .error(error)
            .clearable(clearable && !disabled && !read_only)
            .aria_label(label.clone())
            .suffix(
                Icon::new("search")
                    .size(props.size.icon_size())
                    .color(theme.tokens.muted_foreground),
            );

        if let Some(description) = aria_description {
            input = input.aria_description(description);
        }
        if let Some(helper_text) = helper_text {
            input = input.helper_text(helper_text);
        }
        if let Some(icon) = start_icon {
            input = input.prefix(
                Icon::new(icon)
                    .size(props.size.icon_size())
                    .color(theme.tokens.muted_foreground),
            );
        }

        div()
            .id(("typeahead-runtime", self.input.entity_id()))
            .relative()
            .when_some(width, |this, width| this.w(width))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(format!("{} typeahead", label))
                    .disabled(disabled)
                    .state(AccessibilityState::READ_ONLY, read_only),
            )
            .on_key_down(cx.listener(TypeaheadRuntime::handle_key_down))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.set_open(false, window, cx);
            }))
            .child(input)
            .when(show_results, |this| {
                this.child(
                    div()
                        .id(("typeahead-results", self.input.entity_id()))
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::List)
                                .label("Suggestions"),
                        )
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
                                .id(("typeahead-results-scroll", self.input.entity_id()))
                                .max_h(px(300.0))
                                .overflow_y_scroll()
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
                                            .child(empty_results_text.clone()),
                                    )
                                })
                                .children(results.into_iter().enumerate().map(|(index, item)| {
                                    let is_selected = selected_id.as_ref() == Some(&item.id);
                                    let is_focused = focused_index == Some(index);
                                    let runtime = runtime.clone();
                                    let item_theme = theme.clone();
                                    button(ElementId::Name(
                                        format!(
                                            "typeahead-result-{}-{index}",
                                            self.input.entity_id()
                                        )
                                        .into(),
                                    ))
                                    .role(AccessibilityRole::MenuItem)
                                    .label(item.label.clone())
                                    .on_click(move |_, window, cx| {
                                        runtime.update(cx, |runtime, cx| {
                                            runtime.select_index(index, window, cx);
                                        });
                                    })
                                    .render_with(
                                        move |state, _, _| {
                                            div()
                                                .px(px(8.0))
                                                .py(item_padding_y)
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(px(8.0))
                                                .rounded(item_theme.tokens.radius_md)
                                                .text_color(item_theme.tokens.popover_foreground)
                                                .bg(if is_selected || is_focused {
                                                    item_theme.tokens.primary.opacity(0.12)
                                                } else {
                                                    transparent_black()
                                                })
                                                .when(state.focused, |this| {
                                                    this.shadow(smallvec::smallvec![
                                                        astryx::focus_ring_outer(
                                                            item_theme.tokens.ring,
                                                        ),
                                                    ])
                                                })
                                                .hover(|style| {
                                                    style.bg(astryx::overlay_hover(
                                                        item_theme.tokens.background.l < 0.5,
                                                    ))
                                                })
                                                .cursor_pointer()
                                                .text_size(px(14.0))
                                                .when(is_selected || is_focused, |this| {
                                                    this.font_weight(FontWeight::MEDIUM)
                                                })
                                                .font_family(item_theme.tokens.font_family.clone())
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(0.0))
                                                        .child(item.label.clone()),
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
                                                                        item_theme.tokens.primary,
                                                                    ),
                                                            )
                                                        }),
                                                )
                                                .into_any_element()
                                        },
                                    )
                                })),
                        ),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div.into_any_element()
            })
    }
}

impl RenderOnce for Typeahead {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let runtime =
            window.use_keyed_state(self.id.clone(), cx, |_, cx| TypeaheadRuntime::new(cx));

        let external_query = self.query.clone();
        let external_value_id = self.value.as_ref().map(|value| value.id.clone());
        let external_value_label = self.value.as_ref().map(|value| value.label.clone());

        runtime.update(cx, |runtime, cx| {
            let query_changed =
                runtime.initialized && runtime.last_external_query != external_query;
            let value_changed =
                runtime.initialized && runtime.last_external_value_id != external_value_id;

            runtime.props = Some(self);
            runtime.last_external_query = external_query.clone();
            runtime.last_external_value_id = external_value_id;

            if !runtime.initialized || query_changed || value_changed {
                let value = if !external_query.is_empty() {
                    external_query.clone()
                } else {
                    external_value_label.clone().unwrap_or_default()
                };
                runtime.suppress_next_input_change =
                    runtime.input.read(cx).content() != value.as_ref();
                runtime.input.update(cx, |input, cx| {
                    input.set_value(value.clone(), window, cx);
                });
                runtime.last_query = value;
                runtime.initialized = true;
            }
        });

        runtime
    }
}

#[cfg(test)]
mod tests {
    use super::{SearchSource, SearchableItem, Typeahead, TypeaheadItem, next_typeahead_index};
    use kael::{Context, IntoElement, Render, TestAppContext, Window};
    use std::{cell::RefCell, rc::Rc};

    struct TypeaheadHost {
        selected: Rc<RefCell<Option<TypeaheadItem>>>,
    }

    struct ControlledTypeaheadHost {
        read_only: bool,
    }

    impl Render for TypeaheadHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let selected = self.selected.clone();
            Typeahead::new("Assignee")
                .search_source(SearchSource::new([
                    SearchableItem::new("grace", "Grace Hopper"),
                    SearchableItem::new("ada", "Ada Lovelace"),
                ]))
                .autofocus(true)
                .on_change(move |item, _, _| {
                    selected.replace(Some(item));
                })
        }
    }

    impl Render for ControlledTypeaheadHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            Typeahead::new("Assignee")
                .search_source(SearchSource::new([
                    SearchableItem::new("grace", "Grace Hopper"),
                    SearchableItem::new("ada", "Ada Lovelace"),
                ]))
                .value(TypeaheadItem::new("Grace Hopper", "grace"))
                .entries_on_focus(true)
                .readonly(self.read_only)
        }
    }

    #[test]
    fn static_search_is_case_insensitive_and_preserves_source_order() {
        let source = SearchSource::new([
            SearchableItem::new("grace", "Grace Hopper"),
            SearchableItem::new("ada", "Ada Lovelace"),
            SearchableItem::new("alan", "Alan Turing"),
        ]);

        let results = source.search("A");
        assert_eq!(
            results
                .iter()
                .map(|item| item.id.as_ref())
                .collect::<Vec<_>>(),
            ["grace", "ada", "alan"]
        );
        assert_eq!(source.search("hopper")[0].id.as_ref(), "grace");
    }

    #[test]
    fn keyboard_result_navigation_wraps_and_handles_empty_results() {
        assert_eq!(next_typeahead_index(0, None, 1), None);
        assert_eq!(next_typeahead_index(3, None, 1), Some(0));
        assert_eq!(next_typeahead_index(3, None, -1), Some(2));
        assert_eq!(next_typeahead_index(3, Some(2), 1), Some(0));
        assert_eq!(next_typeahead_index(3, Some(0), -1), Some(2));
    }

    #[kael::test]
    fn controlled_value_does_not_open_suggestions_without_focus(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_host, window) =
            cx.add_window_view(|_, _| ControlledTypeaheadHost { read_only: false });

        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(
                !window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .any(|node| node.role == kael::AccessibilityRole::List)
            );
        });
    }

    #[kael::test]
    fn read_only_typeahead_is_not_exposed_as_disabled(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_host, window) =
            cx.add_window_view(|_, _| ControlledTypeaheadHost { read_only: true });

        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == kael::AccessibilityRole::TextInput)
                .expect("typeahead should expose its text input");
            assert!(node.states.contains(kael::AccessibilityState::READ_ONLY));
            assert!(!node.states.contains(kael::AccessibilityState::DISABLED));
            assert!(!node.actions.contains(&kael::AccessibilityAction::SetValue));
        });
    }

    #[kael::test]
    fn live_input_filters_and_keyboard_selection_commits(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let selected = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            let selected = selected.clone();
            move |_, _| TypeaheadHost { selected }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.simulate_input("gra");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.simulate_keystrokes("down enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(
            selected.borrow().as_ref().map(|item| item.id.as_ref()),
            Some("grace")
        );
    }
}
