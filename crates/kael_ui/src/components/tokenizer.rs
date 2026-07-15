//! Tokenizer component - ASTRYX-style token input surface.

use crate::{
    astryx,
    components::{
        field::{Field, FieldStatusType},
        icon::Icon,
        icon_button::IconButton,
        input::{Input, InputSize, InputVariant},
        input_state::{InputEvent, InputState},
        token::Token,
    },
    theme::Theme,
};
use kael::{prelude::*, *};
use std::{panic::Location, rc::Rc, time::Duration};

pub type TokenizerSize = astryx::ControlSize;
pub type TokenizerStatus = FieldStatusType;
pub type TokenizerStatusType = FieldStatusType;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum TokenizerOverflowBehavior {
    #[default]
    None,
    UnfocusedInline,
    UnfocusedLayer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenizerChange {
    Add { item: TokenizerItem },
    Create { item: TokenizerItem },
    Remove { item: TokenizerItem },
    Reorder,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TokenizerHandle {
    pub is_focused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizerItem {
    pub id: SharedString,
    pub label: SharedString,
}

impl TokenizerItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(IntoElement)]
pub struct Tokenizer {
    id: ElementId,
    label: SharedString,
    label_hidden: bool,
    value: Vec<TokenizerItem>,
    search_source: Vec<TokenizerItem>,
    query: SharedString,
    placeholder: SharedString,
    description: Option<SharedString>,
    required: bool,
    optional: bool,
    status: Option<TokenizerStatus>,
    status_message: Option<SharedString>,
    start_icon: Option<SharedString>,
    end_content: Option<AnyElement>,
    size: TokenizerSize,
    max_entries: Option<usize>,
    entries_on_focus: bool,
    max_menu_items: usize,
    empty_results_text: SharedString,
    autofocus: bool,
    overflow_behavior: TokenizerOverflowBehavior,
    debounce_ms: u64,
    creatable: bool,
    disabled: bool,
    read_only: bool,
    clearable: bool,
    max_visible: usize,
    on_remove: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_change: Option<Rc<dyn Fn(TokenizerChange, &mut Window, &mut App)>>,
    on_change_query: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_open_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Tokenizer {
    #[track_caller]
    pub fn new(label: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "tokenizer:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            label: label.into(),
            label_hidden: false,
            value: Vec::new(),
            search_source: Vec::new(),
            query: "".into(),
            placeholder: "Search...".into(),
            description: None,
            required: false,
            optional: false,
            status: None,
            status_message: None,
            start_icon: None,
            end_content: None,
            size: astryx::ControlSize::Md,
            max_entries: None,
            entries_on_focus: false,
            max_menu_items: 10,
            empty_results_text: "No results found".into(),
            autofocus: false,
            overflow_behavior: TokenizerOverflowBehavior::None,
            debounce_ms: 150,
            creatable: false,
            disabled: false,
            read_only: false,
            clearable: false,
            max_visible: 3,
            on_remove: None,
            on_clear: None,
            on_change: None,
            on_change_query: None,
            on_open_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn value(mut self, value: impl IntoIterator<Item = TokenizerItem>) -> Self {
        self.value = value.into_iter().collect();
        self
    }

    pub fn search_source(mut self, source: impl IntoIterator<Item = TokenizerItem>) -> Self {
        self.search_source = source.into_iter().collect();
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

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
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

    pub fn status(mut self, status: TokenizerStatus) -> Self {
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

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    pub fn size(mut self, size: TokenizerSize) -> Self {
        self.size = size;
        self
    }

    pub fn max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = Some(max_entries);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxEntries(self, max_entries: usize) -> Self {
        self.max_entries(max_entries)
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

    pub fn token_overflow_behavior(mut self, behavior: TokenizerOverflowBehavior) -> Self {
        self.overflow_behavior = behavior;
        self
    }

    #[allow(non_snake_case)]
    pub fn tokenOverflowBehavior(self, behavior: TokenizerOverflowBehavior) -> Self {
        self.token_overflow_behavior(behavior)
    }

    pub fn debounce_ms(mut self, debounce_ms: u64) -> Self {
        self.debounce_ms = debounce_ms;
        self
    }

    #[allow(non_snake_case)]
    pub fn debounceMs(self, debounce_ms: u64) -> Self {
        self.debounce_ms(debounce_ms)
    }

    pub fn creatable(mut self, creatable: bool) -> Self {
        self.creatable = creatable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasCreate(self, creatable: bool) -> Self {
        self.creatable(creatable)
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

    pub fn on_remove(
        mut self,
        handler: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_remove = Some(Rc::new(handler));
        self
    }

    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }

    pub fn on_change(
        mut self,
        handler: impl Fn(TokenizerChange, &mut Window, &mut App) + 'static,
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

impl Styled for Tokenizer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
#[derive(Clone)]
enum TokenizerSuggestion {
    Add(TokenizerItem),
    Create(TokenizerItem),
}

impl TokenizerSuggestion {
    fn item(&self) -> &TokenizerItem {
        match self {
            Self::Add(item) | Self::Create(item) => item,
        }
    }

    fn into_change(self) -> TokenizerChange {
        match self {
            Self::Add(item) => TokenizerChange::Add { item },
            Self::Create(item) => TokenizerChange::Create { item },
        }
    }
}

fn next_tokenizer_index(count: usize, current: Option<usize>, delta: isize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let start = current.unwrap_or(if delta < 0 { 0 } else { count - 1 });
    Some((start as isize + delta).rem_euclid(count as isize) as usize)
}

struct TokenizerRuntime {
    input: Entity<InputState>,
    props: Option<Tokenizer>,
    items: Vec<TokenizerItem>,
    open: bool,
    focused_index: Option<usize>,
    input_focused: bool,
    overflow_layer_open: bool,
    initialized: bool,
    autofocus_applied: bool,
    window_handle: Option<AnyWindowHandle>,
    query_generation: u64,
    last_query: SharedString,
    last_external_query: SharedString,
    last_external_ids: Vec<SharedString>,
}

impl TokenizerRuntime {
    fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(InputState::new);
        let input_for_events = input.clone();

        cx.subscribe(&input, move |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => {
                let query: SharedString = input_for_events.read(cx).content().to_string().into();
                this.last_query = query.clone();
                this.focused_index = None;
                this.query_generation = this.query_generation.wrapping_add(1);
                let generation = this.query_generation;

                let should_open = this.props.as_ref().is_some_and(|props| {
                    !props.disabled
                        && !props.read_only
                        && !this.at_capacity()
                        && (!props.search_source.is_empty() || props.creatable)
                        && (!query.is_empty() || props.entries_on_focus)
                });
                let open_changed = this.open != should_open;
                this.open = should_open;

                let Some(props) = this.props.as_ref() else {
                    cx.notify();
                    return;
                };

                if open_changed {
                    if let (Some(callback), Some(window_handle)) =
                        (props.on_open_change.clone(), this.window_handle)
                    {
                        cx.defer(move |cx| {
                            let _ = cx.update_window(window_handle, move |_, window, cx| {
                                callback(should_open, window, cx);
                            });
                        });
                    }
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
                this.overflow_layer_open = false;
                let should_open = this.props.as_ref().is_some_and(|props| {
                    props.entries_on_focus
                        && !props.disabled
                        && !props.read_only
                        && !this.at_capacity()
                        && (!props.search_source.is_empty() || props.creatable)
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
                }
                cx.notify();
            }
            InputEvent::Enter if this.open || this.can_create_current_query(cx) => {
                let index = this
                    .focused_index
                    .unwrap_or_else(|| this.filtered_suggestions(cx).len().saturating_sub(1));
                let runtime = cx.entity();
                if let Some(window_handle) = this.window_handle {
                    cx.defer(move |cx| {
                        let _ = cx.update_window(window_handle, move |_, window, cx| {
                            runtime.update(cx, |runtime, cx| {
                                runtime.select_index(index, window, cx);
                            });
                        });
                    });
                }
            }
            InputEvent::Blur => {
                this.input_focused = false;
                this.focused_index = None;
                if this.open {
                    this.open = false;
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
                }
                cx.notify();
            }
            _ => {}
        })
        .detach();

        Self {
            input,
            props: None,
            items: Vec::new(),
            open: false,
            focused_index: None,
            input_focused: false,
            overflow_layer_open: false,
            initialized: false,
            autofocus_applied: false,
            window_handle: None,
            query_generation: 0,
            last_query: SharedString::default(),
            last_external_query: SharedString::default(),
            last_external_ids: Vec::new(),
        }
    }

    fn at_capacity(&self) -> bool {
        self.props
            .as_ref()
            .and_then(|props| props.max_entries)
            .is_some_and(|max| self.items.len() >= max)
    }

    fn can_create_current_query(&self, cx: &App) -> bool {
        let Some(props) = self.props.as_ref() else {
            return false;
        };
        props.creatable
            && !props.disabled
            && !props.read_only
            && !self.at_capacity()
            && !self.input.read(cx).content().trim().is_empty()
    }

    fn filtered_suggestions(&self, cx: &App) -> Vec<TokenizerSuggestion> {
        let Some(props) = self.props.as_ref() else {
            return Vec::new();
        };
        if props.disabled || props.read_only || self.at_capacity() {
            return Vec::new();
        }

        let query = self.input.read(cx).content().trim().to_lowercase();
        let selected_ids = self
            .items
            .iter()
            .map(|item| item.id.as_ref())
            .collect::<std::collections::HashSet<_>>();
        let mut suggestions = props
            .search_source
            .iter()
            .filter(|item| !selected_ids.contains(item.id.as_ref()))
            .filter(|item| {
                query.is_empty() && props.entries_on_focus
                    || !query.is_empty() && item.label.to_lowercase().contains(&query)
            })
            .cloned()
            .map(TokenizerSuggestion::Add)
            .collect::<Vec<_>>();

        let exact_match = props
            .search_source
            .iter()
            .chain(self.items.iter())
            .any(|item| item.label.to_lowercase() == query || item.id.to_lowercase() == query);
        if props.creatable && !query.is_empty() && !exact_match {
            let value: SharedString = self.input.read(cx).content().trim().to_string().into();
            suggestions.push(TokenizerSuggestion::Create(TokenizerItem::new(
                value.clone(),
                value,
            )));
        }

        suggestions.truncate(props.max_menu_items);
        suggestions
    }

    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open
            && self
                .props
                .as_ref()
                .is_some_and(|props| props.disabled || props.read_only || self.at_capacity())
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
        let count = self.filtered_suggestions(cx).len();
        self.focused_index = next_tokenizer_index(count, self.focused_index, delta);
        if count > 0 && !self.open {
            self.set_open(true, window, cx);
        }
        cx.notify();
    }

    fn select_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .props
            .as_ref()
            .is_some_and(|props| props.disabled || props.read_only)
        {
            return;
        }
        let Some(suggestion) = self.filtered_suggestions(cx).get(index).cloned() else {
            return;
        };
        let item = suggestion.item().clone();
        if self.items.iter().any(|selected| selected.id == item.id) || self.at_capacity() {
            return;
        }

        self.items.push(item.clone());
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.last_query = SharedString::default();
        self.focused_index = None;
        let keep_open = self
            .props
            .as_ref()
            .is_some_and(|props| props.entries_on_focus)
            && !self.at_capacity();
        let open_changed = self.open != keep_open;
        self.open = keep_open;

        if let Some(props) = self.props.as_ref() {
            if let Some(callback) = props.on_change.as_ref() {
                callback(suggestion.into_change(), window, cx);
            }
            if open_changed {
                if let Some(callback) = props.on_open_change.as_ref() {
                    callback(keep_open, window, cx);
                }
            }
        }
        cx.notify();
    }

    fn remove_item(&mut self, id: &SharedString, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .props
            .as_ref()
            .is_some_and(|props| props.disabled || props.read_only)
        {
            return;
        }
        let Some(index) = self.items.iter().position(|item| &item.id == id) else {
            return;
        };
        let item = self.items.remove(index);
        if let Some(props) = self.props.as_ref() {
            if let Some(callback) = props.on_remove.as_ref() {
                callback(item.id.clone(), window, cx);
            }
            if let Some(callback) = props.on_change.as_ref() {
                callback(TokenizerChange::Remove { item: item.clone() }, window, cx);
            }
        }
        cx.notify();
    }

    fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.items.is_empty()
            || self
                .props
                .as_ref()
                .is_some_and(|props| props.disabled || props.read_only)
        {
            return;
        }
        let removed = std::mem::take(&mut self.items);
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        if let Some(props) = self.props.as_ref() {
            if let Some(callback) = props.on_clear.as_ref() {
                callback(window, cx);
            }
            if let Some(callback) = props.on_change.as_ref() {
                for item in removed {
                    callback(TokenizerChange::Remove { item }, window, cx);
                }
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
            .is_some_and(|props| props.disabled || props.read_only)
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
            "escape" if self.open || self.overflow_layer_open => {
                self.open = false;
                self.overflow_layer_open = false;
                self.focused_index = None;
                cx.notify();
                true
            }
            "backspace" if self.input.read(cx).content().is_empty() && !self.items.is_empty() => {
                let id = self
                    .items
                    .last()
                    .map(|item| item.id.clone())
                    .unwrap_or_default();
                self.remove_item(&id, window, cx);
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

impl Render for TokenizerRuntime {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window_handle = Some(window.window_handle());
        let end_content = self
            .props
            .as_mut()
            .and_then(|props| props.end_content.take());
        let Some(props) = self.props.as_ref() else {
            return div().into_any_element();
        };

        if props.autofocus && !self.autofocus_applied && !props.disabled {
            window.focus(&self.input.read(cx).focus_handle(cx));
            self.autofocus_applied = true;
        }

        let theme = Theme::of(cx).clone();
        let runtime = cx.entity();
        let suggestions = self.filtered_suggestions(cx);
        let show_results = self.open
            && !props.disabled
            && !props.read_only
            && (!suggestions.is_empty() || !props.empty_results_text.is_empty());
        let focused_index = self.focused_index;
        let disabled = props.disabled;
        let read_only = props.read_only;
        let input_read_only = read_only || self.at_capacity();
        let user_style = props.style.clone();
        let label = props.label.clone();
        let label_hidden = props.label_hidden;
        let optional = props.optional;
        let required = props.required;
        let placeholder = if self.at_capacity() {
            props
                .max_entries
                .map(|max| format!("Maximum of {max} entries reached").into())
                .unwrap_or_else(|| props.placeholder.clone())
        } else {
            props.placeholder.clone()
        };
        let size = match props.size {
            astryx::ControlSize::Sm => InputSize::Sm,
            astryx::ControlSize::Md => InputSize::Md,
            astryx::ControlSize::Lg => InputSize::Lg,
        };
        let start_icon = props.start_icon.clone();
        let clearable = props.clearable;
        let empty_results_text = props.empty_results_text.clone();
        let item_padding_y = match props.size {
            astryx::ControlSize::Sm => px(4.0),
            astryx::ControlSize::Md => px(6.0),
            astryx::ControlSize::Lg => px(8.0),
        };
        let collapse_tokens =
            !self.input_focused && props.overflow_behavior != TokenizerOverflowBehavior::None;
        let visible_count = if collapse_tokens {
            props.max_visible
        } else {
            usize::MAX
        };
        let overflow = self.items.len().saturating_sub(visible_count);
        let visible_items = self
            .items
            .iter()
            .take(visible_count)
            .cloned()
            .collect::<Vec<_>>();
        let hidden_items = self
            .items
            .iter()
            .skip(visible_count)
            .cloned()
            .collect::<Vec<_>>();
        let overflow_layer_open = self.overflow_layer_open;
        let status_color = props.status.map(|status| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });

        let token_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .children(visible_items.into_iter().enumerate().map(|(index, item)| {
                let id = item.id.clone();
                let runtime = runtime.clone();
                Token::new(item.label)
                    .id(ElementId::Name(
                        format!("tokenizer-token-{}-{index}", self.input.entity_id()).into(),
                    ))
                    .size(props.size)
                    .disabled(disabled)
                    .when(!read_only, |token| {
                        token.on_remove(move |window, cx| {
                            runtime.update(cx, |runtime, cx| {
                                runtime.remove_item(&id, window, cx);
                            });
                        })
                    })
            }))
            .when(overflow > 0, |this| {
                let runtime_for_overflow = runtime.clone();
                match props.overflow_behavior {
                    TokenizerOverflowBehavior::UnfocusedLayer => this.child(
                        button(("tokenizer-overflow", self.input.entity_id()))
                            .label(format!("Show {overflow} more selected items"))
                            .on_click(move |_, _, cx| {
                                runtime_for_overflow.update(cx, |runtime, cx| {
                                    runtime.overflow_layer_open = !runtime.overflow_layer_open;
                                    cx.notify();
                                });
                            })
                            .render_with(move |state, _, _| {
                                div()
                                    .px(px(7.0))
                                    .py(px(2.0))
                                    .rounded_full()
                                    .bg(theme.tokens.muted)
                                    .text_size(px(12.0))
                                    .line_height(px(16.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .when(state.focused, |this| {
                                        this.shadow(smallvec::smallvec![astryx::focus_ring_outer(
                                            theme.tokens.ring
                                        ),])
                                    })
                                    .child(format!("+{overflow} more"))
                                    .into_any_element()
                            }),
                    ),
                    TokenizerOverflowBehavior::UnfocusedInline
                    | TokenizerOverflowBehavior::None => this.child(
                        div()
                            .px(px(7.0))
                            .py(px(2.0))
                            .rounded_full()
                            .bg(theme.tokens.muted)
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child(format!("+{overflow} more")),
                    ),
                }
            });

        let runtime_for_clear = runtime.clone();
        let suffix = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .children(end_content)
            .when(
                clearable && !self.items.is_empty() && !disabled && !read_only,
                |this| {
                    this.child(
                        IconButton::new("x")
                            .id(("tokenizer-clear", self.input.entity_id()))
                            .label("Clear all selected items")
                            .size(px(24.0))
                            .icon_size(px(14.0))
                            .no_background(true)
                            .on_click(move |_, window, cx| {
                                runtime_for_clear.update(cx, |runtime, cx| {
                                    runtime.clear(window, cx);
                                });
                            }),
                    )
                },
            )
            .child(
                Icon::new("search")
                    .size(props.size.icon_size())
                    .color(theme.tokens.muted_foreground),
            );

        let input = Input::new(&self.input)
            .is_label_hidden(true)
            .placeholder(placeholder)
            .size(size)
            .variant(InputVariant::Ghost)
            .disabled(disabled)
            .read_only(input_read_only)
            .aria_label(format!("Add {}", label))
            .suffix(suffix);

        let control = div()
            .id(("tokenizer-control", self.input.entity_id()))
            .relative()
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(format!("{label}, {} selected", self.items.len()))
                    .disabled(disabled)
                    .state(AccessibilityState::READ_ONLY, read_only),
            )
            .bg(theme.tokens.card)
            .border_1()
            .border_color(status_color.unwrap_or(theme.tokens.input))
            .rounded(theme.tokens.radius_md)
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled, |this| {
                this.hover(move |style| {
                    style.shadow(smallvec::smallvec![astryx::input_hover_ring(
                        status_color.unwrap_or(theme.tokens.input),
                    )])
                })
            })
            .when_some(start_icon, |this, icon| {
                this.child(
                    div().px(px(8.0)).pt(px(6.0)).child(
                        Icon::new(icon)
                            .size(props.size.icon_size())
                            .color(theme.tokens.muted_foreground),
                    ),
                )
            })
            .when(!self.items.is_empty(), |this| {
                this.child(div().px(px(6.0)).pt(px(6.0)).child(token_row))
            })
            .child(input)
            .when(show_results, |this| {
                this.child(
                    div()
                        .id(("tokenizer-results", self.input.entity_id()))
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
                                .id(("tokenizer-results-scroll", self.input.entity_id()))
                                .max_h(px(300.0))
                                .overflow_y_scroll()
                                .py(px(4.0))
                                .when(suggestions.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .px(px(12.0))
                                            .py(px(16.0))
                                            .text_center()
                                            .text_size(px(13.0))
                                            .text_color(theme.tokens.muted_foreground)
                                            .child(empty_results_text.clone()),
                                    )
                                })
                                .children(suggestions.into_iter().enumerate().map(
                                    |(index, suggestion)| {
                                        let item = suggestion.item().clone();
                                        let is_focused = focused_index == Some(index);
                                        let is_create =
                                            matches!(suggestion, TokenizerSuggestion::Create(_));
                                        let runtime = runtime.clone();
                                        let item_theme = theme.clone();
                                        button(ElementId::Name(
                                            format!(
                                                "tokenizer-result-{}-{index}",
                                                self.input.entity_id()
                                            )
                                            .into(),
                                        ))
                                        .role(AccessibilityRole::MenuItem)
                                        .label(if is_create {
                                            format!("Create {}", item.label)
                                        } else {
                                            format!("Add {}", item.label)
                                        })
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
                                                    .gap(px(8.0))
                                                    .rounded(item_theme.tokens.radius_md)
                                                    .bg(if is_focused {
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
                                                    .child(
                                                        Icon::new(if is_create {
                                                            "plus"
                                                        } else {
                                                            "check"
                                                        })
                                                        .size(px(14.0))
                                                        .color(item_theme.tokens.primary),
                                                    )
                                                    .child(item.label.clone())
                                                    .into_any_element()
                                            },
                                        )
                                    },
                                )),
                        ),
                )
            })
            .when(
                overflow_layer_open
                    && props.overflow_behavior == TokenizerOverflowBehavior::UnfocusedLayer
                    && !hidden_items.is_empty(),
                |this| {
                    this.child(
                        div()
                            .absolute()
                            .top_full()
                            .right_0()
                            .mt(px(4.0))
                            .max_w(px(320.0))
                            .p(px(8.0))
                            .flex()
                            .flex_wrap()
                            .gap(px(4.0))
                            .bg(theme.tokens.popover)
                            .border_1()
                            .border_color(theme.tokens.border)
                            .rounded(theme.tokens.radius_lg)
                            .shadow(theme.tokens.shadow_lg.to_vec())
                            .children(hidden_items.into_iter().enumerate().map(|(index, item)| {
                                let id = item.id.clone();
                                let runtime = runtime.clone();
                                Token::new(item.label)
                                    .id(ElementId::Name(
                                        format!(
                                            "tokenizer-overflow-token-{}-{index}",
                                            self.input.entity_id()
                                        )
                                        .into(),
                                    ))
                                    .size(props.size)
                                    .disabled(disabled)
                                    .when(!read_only, |token| {
                                        token.on_remove(move |window, cx| {
                                            runtime.update(cx, |runtime, cx| {
                                                runtime.remove_item(&id, window, cx);
                                            });
                                        })
                                    })
                            })),
                    )
                },
            );

        let mut field = Field::new(label, control)
            .hidden_label(label_hidden)
            .required(required)
            .optional(optional)
            .disabled(disabled);
        if let Some(description) = props.description.clone() {
            field = field.description(description);
        }
        if let Some((status, message)) = props.status.zip(props.status_message.clone()) {
            field = field.status(status, message);
        }

        let field = field.map(|this| {
            let mut field = this;
            field.style().refine(&user_style);
            field
        });

        div()
            .id(("tokenizer-runtime", self.input.entity_id()))
            .on_key_down(cx.listener(TokenizerRuntime::handle_key_down))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.set_open(false, window, cx);
                this.overflow_layer_open = false;
            }))
            .child(field)
            .into_any_element()
    }
}

impl RenderOnce for Tokenizer {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let runtime =
            window.use_keyed_state(self.id.clone(), cx, |_, cx| TokenizerRuntime::new(cx));

        let external_query = self.query.clone();
        let external_items = self.value.clone();
        let external_ids = external_items
            .iter()
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();

        runtime.update(cx, |runtime, cx| {
            let query_changed =
                runtime.initialized && runtime.last_external_query != external_query;
            let items_changed = runtime.initialized && runtime.last_external_ids != external_ids;

            runtime.props = Some(self);
            runtime.last_external_query = external_query.clone();
            runtime.last_external_ids = external_ids;

            if !runtime.initialized || items_changed {
                runtime.items = external_items;
            }
            if !runtime.initialized || query_changed {
                runtime.input.update(cx, |input, cx| {
                    input.set_value(external_query.clone(), window, cx);
                });
                runtime.last_query = external_query;
            }
            runtime.initialized = true;
        });

        runtime
    }
}

#[cfg(test)]
mod tests {
    use super::{next_tokenizer_index, Tokenizer, TokenizerChange, TokenizerItem};
    use kael::{Context, IntoElement, Render, TestAppContext, Window};
    use std::{cell::RefCell, rc::Rc};

    struct TokenizerHost {
        change: Rc<RefCell<Option<TokenizerChange>>>,
        creatable: bool,
    }

    struct CapacityTokenizerHost {
        change: Rc<RefCell<Option<TokenizerChange>>>,
        read_only: bool,
    }

    impl Render for TokenizerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let change = self.change.clone();
            Tokenizer::new("Recipients")
                .search_source([
                    TokenizerItem::new("ada", "Ada Lovelace"),
                    TokenizerItem::new("grace", "Grace Hopper"),
                ])
                .creatable(self.creatable)
                .autofocus(true)
                .on_change(move |event, _, _| {
                    change.replace(Some(event));
                })
        }
    }

    impl Render for CapacityTokenizerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let change = self.change.clone();
            Tokenizer::new("Recipients")
                .value([TokenizerItem::new("ada", "Ada Lovelace")])
                .max_entries(1)
                .autofocus(true)
                .read_only(self.read_only)
                .on_change(move |event, _, _| {
                    change.replace(Some(event));
                })
        }
    }

    #[test]
    fn keyboard_navigation_wraps_and_handles_empty_results() {
        assert_eq!(next_tokenizer_index(0, None, 1), None);
        assert_eq!(next_tokenizer_index(3, None, 1), Some(0));
        assert_eq!(next_tokenizer_index(3, None, -1), Some(2));
        assert_eq!(next_tokenizer_index(3, Some(2), 1), Some(0));
        assert_eq!(next_tokenizer_index(3, Some(0), -1), Some(2));
    }

    #[kael::test]
    fn capacity_limited_tokenizer_keeps_keyboard_removal_available(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let change = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            let change = change.clone();
            move |_, _| CapacityTokenizerHost {
                change,
                read_only: false,
            }
        });

        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("backspace");
        window.update(|window, cx| window.draw(cx).clear());

        assert!(matches!(
            change.borrow().as_ref(),
            Some(TokenizerChange::Remove { item }) if item.id.as_ref() == "ada"
        ));
    }

    #[kael::test]
    fn read_only_tokenizer_is_focusable_without_remove_actions(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let change = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            move |_, _| CapacityTokenizerHost {
                change,
                read_only: true,
            }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let input = tree
                .nodes
                .values()
                .find(|node| node.role == kael::AccessibilityRole::TextInput)
                .expect("tokenizer should expose a text input");
            assert!(input.states.contains(kael::AccessibilityState::READ_ONLY));
            assert!(!tree.nodes.values().any(|node| {
                node.label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Remove "))
            }));
        });
    }

    #[kael::test]
    fn live_search_adds_existing_items_with_keyboard(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let change = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            let change = change.clone();
            move |_, _| TokenizerHost {
                change,
                creatable: false,
            }
        });

        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_input("ada");
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("down enter");
        window.update(|window, cx| window.draw(cx).clear());

        assert!(matches!(
            change.borrow().as_ref(),
            Some(TokenizerChange::Add { item }) if item.id.as_ref() == "ada"
        ));
    }

    #[kael::test]
    fn creatable_query_commits_a_new_item_on_enter(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let change = Rc::new(RefCell::new(None));
        let (_host, window) = cx.add_window_view({
            let change = change.clone();
            move |_, _| TokenizerHost {
                change,
                creatable: true,
            }
        });

        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_input("New teammate");
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("enter");
        window.update(|window, cx| window.draw(cx).clear());

        assert!(matches!(
            change.borrow().as_ref(),
            Some(TokenizerChange::Create { item }) if item.label.as_ref() == "New teammate"
        ));
    }
}
