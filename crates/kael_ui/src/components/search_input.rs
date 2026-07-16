//! Search input component - Specialized search with filters and advanced capabilities.

use crate::{
    astryx,
    components::{
        button::{Button, ButtonSize, ButtonVariant},
        icon::Icon,
        icon_source::IconSource,
        input_state::{InputEvent, InputState},
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, InteractiveElement, *};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct SearchFilter {
    pub label: SharedString,
    pub active: bool,
    pub value: SharedString,
}

impl SearchFilter {
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            active: false,
            value: value.into(),
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

pub struct SearchInputState {
    input: Entity<InputState>,
    filters: Vec<SearchFilter>,
    case_sensitive: bool,
    use_regex: bool,
    loading: bool,
    results_count: Option<usize>,
    on_search: Option<Rc<dyn Fn(&str, &mut App)>>,
    on_filter_toggle: Option<Rc<dyn Fn(usize, &mut App)>>,
}

impl SearchInputState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(cx).placeholder("Search..."));

        Self {
            input,
            filters: Vec::new(),
            case_sensitive: false,
            use_regex: false,
            loading: false,
            results_count: None,
            on_search: None,
            on_filter_toggle: None,
        }
    }

    pub fn placeholder(
        self,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        self.input.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
        self
    }

    pub fn set_filters(&mut self, filters: Vec<SearchFilter>, cx: &mut Context<Self>) {
        self.filters = filters;
        cx.notify();
    }

    pub fn toggle_filter(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(filter) = self.filters.get_mut(index) {
            filter.active = !filter.active;
            if let Some(handler) = &self.on_filter_toggle {
                handler(index, cx);
            }
            cx.notify();
        }
    }

    pub fn set_case_sensitive(&mut self, case_sensitive: bool, cx: &mut Context<Self>) {
        self.case_sensitive = case_sensitive;
        cx.notify();
    }

    pub fn toggle_case_sensitive(&mut self, cx: &mut Context<Self>) {
        self.case_sensitive = !self.case_sensitive;
        cx.notify();
    }

    pub fn set_use_regex(&mut self, use_regex: bool, cx: &mut Context<Self>) {
        self.use_regex = use_regex;
        cx.notify();
    }

    pub fn toggle_use_regex(&mut self, cx: &mut Context<Self>) {
        self.use_regex = !self.use_regex;
        cx.notify();
    }

    pub fn set_loading(&mut self, loading: bool, cx: &mut Context<Self>) {
        self.loading = loading;
        cx.notify();
    }

    pub fn set_results_count(&mut self, count: Option<usize>, cx: &mut Context<Self>) {
        self.results_count = count;
        cx.notify();
    }

    pub fn clear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.results_count = None;
        cx.notify();
    }

    pub fn query(&self, cx: &App) -> String {
        self.input.read(cx).content().to_string()
    }

    pub fn active_filters(&self) -> Vec<&SearchFilter> {
        self.filters.iter().filter(|f| f.active).collect()
    }

    pub fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    pub fn use_regex(&self) -> bool {
        self.use_regex
    }
}

pub struct SearchInput {
    state: Entity<SearchInputState>,
    style: StyleRefinement,
}

impl SearchInput {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(SearchInputState::new);

        let state_clone = state.clone();
        let input_entity = state.read(cx).input.clone();
        cx.subscribe(&input_entity, move |_this, _input, event, cx| {
            if let InputEvent::Change = event {
                let query = state_clone.read(cx).input.read(cx).content().to_string();
                let handler = state_clone.read(cx).on_search.clone();
                if let Some(handler) = handler {
                    cx.defer(move |cx| {
                        handler(&query, cx);
                    });
                }
            }
        })
        .detach();

        Self {
            state,
            style: StyleRefinement::default(),
        }
    }

    pub fn placeholder(
        self,
        placeholder: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        self.state.update(cx, |state, cx| {
            state.input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx);
            });
        });
        self
    }

    pub fn filters(self, filters: Vec<SearchFilter>, cx: &mut Context<Self>) -> Self {
        self.state.update(cx, |state, cx| {
            state.set_filters(filters, cx);
        });
        self
    }

    pub fn on_search<F>(self, handler: F, cx: &mut Context<Self>) -> Self
    where
        F: Fn(&str, &mut App) + 'static,
    {
        self.state.update(cx, |state, _cx| {
            state.on_search = Some(Rc::new(handler));
        });
        self
    }

    pub fn on_filter_toggle<F>(self, handler: F, cx: &mut Context<Self>) -> Self
    where
        F: Fn(usize, &mut App) + 'static,
    {
        self.state.update(cx, |state, _cx| {
            state.on_filter_toggle = Some(Rc::new(handler));
        });
        self
    }

    pub fn disabled(self, disabled: bool, cx: &mut Context<Self>) -> Self {
        self.state.update(cx, |state, cx| {
            state.input.update(cx, |input, cx| {
                input.disabled = disabled;
                cx.notify();
            });
        });
        self
    }

    pub fn state(&self) -> &Entity<SearchInputState> {
        &self.state
    }
}

impl Styled for SearchInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for SearchInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let state = self.state.read(cx);
        let input = state.input.clone();
        let input_state = input.read(cx);
        let has_query = !input_state.content().is_empty();
        let input_disabled = input_state.disabled;
        let user_style = self.style.clone();
        let input_focus_handle = input_state.focus_handle(cx);
        let is_focused = input_focus_handle.is_focused(window);
        let focus_handle = input_focus_handle.clone();
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let entity_id = self.state.entity_id().as_u64();
        let mut accessibility_state = AccessibilityState::NONE;
        if input_disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .label(
                input_state
                    .aria_label
                    .clone()
                    .unwrap_or_else(|| "Search".into())
                    .to_string(),
            )
            .placeholder(input_state.placeholder.to_string())
            .states(accessibility_state);
        if has_query {
            accessibility =
                accessibility.value(AccessibilityValue::Text(input_state.content().to_string()));
        }
        if !input_disabled {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::SetValue,
            ]);
        }

        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            // Main search input row
            .child(
                div()
                    .id(("search-input", entity_id))
                    .key_context("SearchInput")
                    .accessibility(accessibility)
                    .when(!input_disabled, |this| {
                        this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                    })
                    .when(!input_disabled, {
                        let input = input.clone();
                        move |this| {
                            this.on_action(window.listener_for(&input, InputState::backspace))
                                .on_action(window.listener_for(&input, InputState::delete))
                                .on_action(window.listener_for(&input, InputState::left))
                                .on_action(window.listener_for(&input, InputState::right))
                                .on_action(window.listener_for(&input, InputState::select_left))
                                .on_action(window.listener_for(&input, InputState::select_right))
                                .on_action(window.listener_for(&input, InputState::select_all))
                                .on_action(window.listener_for(&input, InputState::home))
                                .on_action(window.listener_for(&input, InputState::end))
                                .on_action(window.listener_for(&input, InputState::copy))
                                .on_action(window.listener_for(&input, InputState::cut))
                                .on_action(window.listener_for(&input, InputState::paste))
                                .on_action(window.listener_for(&input, InputState::enter))
                                .on_action(window.listener_for(&input, InputState::tab))
                                .on_action(window.listener_for(&input, InputState::shift_tab))
                                .on_action(window.listener_for(&input, InputState::escape))
                        }
                    })
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .h(px(32.0))
                    .bg(theme.tokens.card)
                    .border_1()
                    .border_color(if is_focused {
                        theme.tokens.primary
                    } else {
                        theme.tokens.input
                    })
                    .rounded(theme.tokens.radius_md)
                    .font_family(theme.tokens.font_family.clone())
                    .transition(theme.tokens.transition_fast)
                    .shadow(smallvec::smallvec![astryx::focus_ring(
                        kael::transparent_black()
                    )])
                    .when(is_focused, |this| {
                        this.shadow(smallvec::smallvec![astryx::focus_ring(
                            theme.tokens.primary
                        )])
                    })
                    .when(!is_focused, |this| {
                        this.hover(|style| {
                            style
                                .border_color(theme.tokens.input)
                                .shadow(smallvec::smallvec![hover_ring])
                        })
                    })
                    .child(
                        Icon::new(if state.loading {
                            IconSource::Named("loader".into())
                        } else {
                            IconSource::Named("search".into())
                        })
                        .size(px(16.0))
                        .color(theme.tokens.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h_full()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .child(input.clone()),
                    )
                    .when_some(state.results_count, |parent_div, count| {
                        parent_div.child(
                            div()
                                .h(px(20.0))
                                .flex()
                                .items_center()
                                .px(px(8.0))
                                .rounded(theme.tokens.radius_sm)
                                .bg(theme.tokens.muted)
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .text_color(theme.tokens.muted_foreground)
                                .child(if count == 1 {
                                    "1 result".to_owned()
                                } else {
                                    format!("{count} results")
                                }),
                        )
                    })
                    .child(
                        Button::new(("search-case-sensitive", entity_id), "Aa")
                            .size(ButtonSize::Sm)
                            .variant(ButtonVariant::Ghost)
                            .selected(state.case_sensitive)
                            .disabled(input_disabled)
                            .tooltip("Match case")
                            .on_click({
                                let state = self.state.clone();
                                move |_, _, cx| {
                                    state.update(cx, |state, cx| {
                                        state.toggle_case_sensitive(cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(("search-regex", entity_id), ".*")
                            .size(ButtonSize::Sm)
                            .variant(ButtonVariant::Ghost)
                            .selected(state.use_regex)
                            .disabled(input_disabled)
                            .tooltip("Use regular expression")
                            .on_click({
                                let state = self.state.clone();
                                move |_, _, cx| {
                                    state.update(cx, |state, cx| {
                                        state.toggle_use_regex(cx);
                                    });
                                }
                            }),
                    )
                    .when(has_query && !input_disabled, |parent_div| {
                        parent_div.child(
                            Button::new(("search-clear", entity_id), "Clear search")
                                .size(ButtonSize::Icon)
                                .variant(ButtonVariant::Ghost)
                                .icon("x")
                                .tooltip("Clear search")
                                .on_click({
                                    let state = self.state.clone();
                                    move |_, window, cx| {
                                        state.update(cx, |state, cx| {
                                            state.clear(window, cx);
                                        });
                                    }
                                }),
                        )
                    }),
            )
            .when(!state.filters.is_empty(), |parent_div| {
                parent_div.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .flex_wrap()
                        .children(state.filters.iter().enumerate().map(|(idx, filter)| {
                            let filter = filter.clone();
                            Button::new(
                                ElementId::NamedInteger(
                                    format!("search-filter-{entity_id}").into(),
                                    idx as u64,
                                ),
                                filter.label,
                            )
                            .size(ButtonSize::Sm)
                            .variant(ButtonVariant::Outline)
                            .selected(filter.active)
                            .disabled(input_disabled)
                            .on_click({
                                let state = self.state.clone();
                                move |_, _, cx| {
                                    state.update(cx, |state, cx| {
                                        state.toggle_filter(idx, cx);
                                    });
                                }
                            })
                            .into_any_element()
                        })),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
