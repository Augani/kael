//! Chat components - ASTRYX-style message list and composer surface.

use crate::{
    components::{button::Button, icon::Icon, input::Input, input_state::InputState},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
use std::rc::Rc;

type ChatSubmitHandler = Rc<dyn Fn(SharedString, &mut App)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChatMessageRole {
    User,
    #[default]
    Assistant,
    System,
    Tool,
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub id: SharedString,
    pub role: ChatMessageRole,
    pub author: Option<SharedString>,
    pub body: SharedString,
    pub timestamp: Option<SharedString>,
}

impl ChatMessage {
    pub fn new(id: impl Into<SharedString>, body: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            role: ChatMessageRole::Assistant,
            author: None,
            body: body.into(),
            timestamp: None,
        }
    }

    pub fn role(mut self, role: ChatMessageRole) -> Self {
        self.role = role;
        self
    }

    pub fn author(mut self, author: impl Into<SharedString>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn timestamp(mut self, timestamp: impl Into<SharedString>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }
}

#[derive(IntoElement)]
pub struct Chat {
    id: ElementId,
    messages: Vec<ChatMessage>,
    composer_state: Option<Entity<InputState>>,
    composer_value: SharedString,
    composer_placeholder: SharedString,
    disabled: bool,
    on_send: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_submit: Option<ChatSubmitHandler>,
    on_composer_change: Option<ChatSubmitHandler>,
    style: StyleRefinement,
}

impl Chat {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "chat:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            messages: Vec::new(),
            composer_state: None,
            composer_value: "".into(),
            composer_placeholder: "Message...".into(),
            disabled: false,
            on_send: None,
            on_submit: None,
            on_composer_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn message(mut self, message: ChatMessage) -> Self {
        self.messages.push(message);
        self
    }

    pub fn messages(mut self, messages: impl IntoIterator<Item = ChatMessage>) -> Self {
        self.messages.extend(messages);
        self
    }

    pub fn composer_value(mut self, value: impl Into<SharedString>) -> Self {
        self.composer_value = value.into();
        self
    }

    /// Use a caller-owned input state for a fully controlled composer.
    pub fn composer_state(mut self, state: &Entity<InputState>) -> Self {
        self.composer_state = Some(state.clone());
        self
    }

    pub fn composer_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.composer_placeholder = placeholder.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_send(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_send = Some(Rc::new(handler));
        self
    }

    /// Submit the current composer value from either Enter or the send button.
    pub fn on_submit(mut self, handler: impl Fn(SharedString, &mut App) + 'static) -> Self {
        self.on_submit = Some(Rc::new(handler));
        self
    }

    pub fn on_composer_change(
        mut self,
        handler: impl Fn(SharedString, &mut App) + 'static,
    ) -> Self {
        self.on_composer_change = Some(Rc::new(handler));
        self
    }
}

impl Default for Chat {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for Chat {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Chat {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let user_style = self.style;
        let chat_id = self.id.clone();
        let composer_id = ElementId::NamedChild(Box::new(chat_id.clone()), "composer".into());
        let send_id = ElementId::NamedChild(Box::new(chat_id.clone()), "send".into());
        let composer_state = self.composer_state.unwrap_or_else(|| {
            window.use_keyed_state(composer_id.clone(), cx, |_, cx| InputState::new(cx))
        });
        if composer_state.read(cx).content() != self.composer_value.as_ref()
            && composer_state.read(cx).content().is_empty()
            && !self.composer_value.is_empty()
        {
            composer_state.update(cx, |state, cx| {
                state.set_value(self.composer_value.clone(), window, cx);
            });
        }
        let composer_value: SharedString = composer_state.read(cx).content().to_owned().into();
        let composer_is_empty = composer_value.trim().is_empty();
        let on_send = self.on_send;
        let on_submit = self.on_submit;
        let on_submit_for_enter = on_submit.clone();
        let on_change = self.on_composer_change;
        let can_submit = on_send.is_some() || on_submit.is_some();
        let disabled = self.disabled;

        div()
            .id(chat_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group).label("Chat conversation"),
            )
            .flex()
            .flex_col()
            .size_full()
            .min_h(px(0.0))
            .bg(theme.tokens.background)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(
                div()
                    .id(ElementId::NamedChild(
                        Box::new(chat_id.clone()),
                        "messages".into(),
                    ))
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::List)
                            .label("Conversation messages"),
                    )
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scroll()
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .children(
                        self.messages
                            .into_iter()
                            .map(|message| render_chat_message(message, &theme).into_any_element()),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .p(px(12.0))
                    .border_t_1()
                    .border_color(theme.tokens.border)
                    .bg(theme.tokens.card)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .min_h(px(40.0))
                            .px(px(10.0))
                            .py(px(6.0))
                            .rounded(theme.tokens.radius_lg)
                            .border_1()
                            .border_color(theme.tokens.input)
                            .bg(theme.tokens.background)
                            .when(disabled, |this| this.opacity(0.5))
                            .child(
                                Input::new(&composer_state)
                                    .placeholder(self.composer_placeholder)
                                    .aria_label("Message")
                                    .disabled(disabled)
                                    .when_some(on_change, |input, handler| {
                                        input.on_change(move |value, cx| handler(value, cx))
                                    })
                                    .when_some(on_submit_for_enter, |input, handler| {
                                        input.on_enter(move |value, cx| {
                                            if !value.trim().is_empty() {
                                                handler(value, cx);
                                            }
                                        })
                                    })
                                    .flex_1()
                                    .min_w(px(0.0)),
                            )
                            .child(
                                Button::new(send_id, "Send message")
                                    .icon("send")
                                    .disabled(disabled || composer_is_empty || !can_submit)
                                    .on_click(move |_, window, cx| {
                                        if let Some(handler) = on_submit.as_ref() {
                                            handler(composer_value.clone(), cx);
                                        }
                                        if let Some(handler) = on_send.as_ref() {
                                            handler(window, cx);
                                        }
                                    }),
                            ),
                    ),
            )
    }
}

fn render_chat_message(message: ChatMessage, theme: &Theme) -> impl IntoElement {
    let is_user = message.role == ChatMessageRole::User;
    let is_system = message.role == ChatMessageRole::System;
    let accent = if is_user {
        theme.tokens.primary
    } else {
        theme.tokens.muted
    };
    let fg = if is_user {
        theme.tokens.primary_foreground
    } else {
        theme.tokens.foreground
    };

    div()
        .accessibility(
            AccessibilityAttributes::new(AccessibilityRole::ListItem)
                .label(format!("{} message", message_role_label(message.role))),
        )
        .flex()
        .when(is_user, |this| this.justify_end())
        .when(!is_user, |this| this.justify_start())
        .child(
            div()
                .max_w(px(680.0))
                .flex()
                .gap(px(8.0))
                .when(is_user, |this| this.flex_row_reverse())
                .when(!is_user, |this| this.flex_row())
                .child(
                    div()
                        .size(px(28.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(if is_system {
                            theme.tokens.warning
                        } else {
                            accent
                        })
                        .child(
                            Icon::new(match message.role {
                                ChatMessageRole::User => "user",
                                ChatMessageRole::Assistant => "sparkles",
                                ChatMessageRole::System => "info",
                                ChatMessageRole::Tool => "wrench",
                            })
                            .size(px(14.0))
                            .color(if is_system {
                                theme.tokens.warning_foreground
                            } else {
                                fg
                            }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.0))
                        .when(is_user, |this| this.items_end())
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .text_size(px(12.0))
                                .line_height(px(16.0))
                                .text_color(theme.tokens.muted_foreground)
                                .when_some(message.author, |this, author| this.child(author))
                                .when_some(message.timestamp, |this, timestamp| {
                                    this.child(timestamp)
                                }),
                        )
                        .child(
                            div()
                                .px(px(12.0))
                                .py(px(8.0))
                                .rounded(theme.tokens.radius_lg)
                                .bg(if is_system {
                                    theme.tokens.warning.opacity(0.14)
                                } else {
                                    accent
                                })
                                .text_color(if is_system {
                                    theme.tokens.foreground
                                } else {
                                    fg
                                })
                                .text_size(px(14.0))
                                .line_height(px(20.0))
                                .child(message.body),
                        ),
                ),
        )
}

fn message_role_label(role: ChatMessageRole) -> &'static str {
    match role {
        ChatMessageRole::User => "User",
        ChatMessageRole::Assistant => "Assistant",
        ChatMessageRole::System => "System",
        ChatMessageRole::Tool => "Tool",
    }
}
