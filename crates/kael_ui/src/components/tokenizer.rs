//! Tokenizer component - ASTRYX-style token input surface.

use crate::{
    astryx,
    components::{field::Field, field::FieldStatusType, icon::Icon, token::Token},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

pub type TokenizerSize = astryx::ControlSize;
pub type TokenizerStatus = FieldStatusType;
pub type TokenizerStatusType = FieldStatusType;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenizerOverflowBehavior {
    None,
    UnfocusedInline,
    UnfocusedLayer,
}

impl Default for TokenizerOverflowBehavior {
    fn default() -> Self {
        Self::None
    }
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
    label: SharedString,
    label_hidden: bool,
    value: Vec<TokenizerItem>,
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
    clearable: bool,
    max_visible: usize,
    on_remove: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Tokenizer {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            label_hidden: false,
            value: Vec::new(),
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
            clearable: false,
            max_visible: 3,
            on_remove: None,
            on_clear: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn value(mut self, value: impl IntoIterator<Item = TokenizerItem>) -> Self {
        self.value = value.into_iter().collect();
        self
    }

    pub fn search_source(self, _source: impl IntoIterator<Item = TokenizerItem>) -> Self {
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
}

impl Styled for Tokenizer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tokenizer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let status_color = self.status.map(|status| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });
        let overflow = self.value.len().saturating_sub(self.max_visible);
        let on_remove = self.on_remove.clone();
        let on_clear = self.on_clear.clone();
        let has_value = !self.value.is_empty();
        let clearable = self.clearable;
        let has_end_section = self.end_content.is_some() || (self.clearable && has_value);
        let end_content = self.end_content;
        let query_is_empty = self.query.is_empty();
        let max_entries_reached = self
            .max_entries
            .map(|max_entries| self.value.len() >= max_entries)
            .unwrap_or(false);
        let disabled = self.disabled || max_entries_reached;
        let _ = (
            self.entries_on_focus,
            self.max_menu_items,
            self.empty_results_text.clone(),
            self.autofocus,
            self.debounce_ms,
            self.creatable,
        );

        let control = div()
            .relative()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0))
            .min_h(self.size.height())
            .when(!has_value, |this| this.px(px(8.0)).py(px(4.0)))
            .when(has_value, |this| this.p(px(3.0)))
            .when(has_end_section && !has_value, |this| this.pr(px(32.0)))
            .when(has_end_section && has_value, |this| this.pr(px(27.0)))
            .bg(theme.tokens.card)
            .border_1()
            .border_color(status_color.unwrap_or(theme.tokens.input))
            .rounded(theme.tokens.radius_md)
            .font_family(theme.tokens.font_family.clone())
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
                    div()
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(has_value, |this| this.ml(px(5.0)))
                        .child(
                            Icon::new(icon)
                                .size(px(16.0))
                                .color(theme.tokens.muted_foreground),
                        ),
                )
            })
            .children(
                self.value
                    .into_iter()
                    .take(
                        if self.overflow_behavior == TokenizerOverflowBehavior::None {
                            usize::MAX
                        } else {
                            self.max_visible
                        },
                    )
                    .map(move |item| {
                        let id = item.id.clone();
                        let on_remove = on_remove.clone();
                        Token::new(item.label)
                            .size(self.size)
                            .disabled(disabled)
                            .on_remove(move |window, cx| {
                                if let Some(handler) = on_remove.as_ref() {
                                    handler(id.clone(), window, cx);
                                }
                            })
                            .into_any_element()
                    }),
            )
            .when(
                self.overflow_behavior != TokenizerOverflowBehavior::None && overflow > 0,
                |this| {
                    this.child(
                        div()
                            .px(px(7.0))
                            .py(px(2.0))
                            .rounded_full()
                            .bg(theme.tokens.muted)
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child(format!("+{overflow} more")),
                    )
                },
            )
            .child(
                div()
                    .flex_1()
                    .min_w(if has_value { px(40.0) } else { px(48.0) })
                    .when(has_value, |this| this.pl(px(5.0)))
                    .when(disabled, |this| {
                        this.w(px(0.0)).min_w(px(0.0)).opacity(0.0).absolute()
                    })
                    .text_size(px(14.0))
                    .line_height(px(20.0))
                    .text_color(if query_is_empty {
                        theme.tokens.muted_foreground
                    } else {
                        theme.tokens.foreground
                    })
                    .child(if query_is_empty && !has_value {
                        self.placeholder
                    } else if query_is_empty {
                        "".into()
                    } else {
                        self.query
                    }),
            )
            .when(has_end_section, |this| {
                this.child(
                    div()
                        .absolute()
                        .right(px(3.0))
                        .top(self.size.height() / 2.0 - px(11.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .children(end_content)
                        .when(clearable && has_value && !self.disabled, |this| {
                            this.child(
                                div()
                                    .h(px(20.0))
                                    .w(px(20.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(theme.tokens.accent))
                                    .when_some(on_clear, |this, handler| {
                                        this.on_mouse_down(
                                            MouseButton::Left,
                                            move |_event, window, cx| {
                                                handler(window, cx);
                                            },
                                        )
                                    })
                                    .child(
                                        Icon::new("x")
                                            .size(px(14.0))
                                            .color(theme.tokens.muted_foreground),
                                    ),
                            )
                        }),
                )
            });

        let mut field = Field::new(self.label, control)
            .hidden_label(self.label_hidden)
            .required(self.required)
            .optional(self.optional)
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
