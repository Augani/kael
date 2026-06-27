//! Chat components - ASTRYX-style message list and composer surface.

use crate::{
    components::{button::Button, icon::Icon},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

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
    messages: Vec<ChatMessage>,
    composer_value: SharedString,
    composer_placeholder: SharedString,
    disabled: bool,
    on_send: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Chat {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            composer_value: "".into(),
            composer_placeholder: "Message...".into(),
            disabled: false,
            on_send: None,
            style: StyleRefinement::default(),
        }
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
}

impl Default for Chat {
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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let composer_is_empty = self.composer_value.is_empty();

        div()
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
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_hidden()
                    .p(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .children(
                        self.messages
                            .into_iter()
                            .map(|message| render_chat_message(message, theme).into_any_element()),
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
                            .when(self.disabled, |this| this.opacity(0.5))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .text_size(px(14.0))
                                    .line_height(px(20.0))
                                    .text_color(if composer_is_empty {
                                        theme.tokens.muted_foreground
                                    } else {
                                        theme.tokens.foreground
                                    })
                                    .child(if composer_is_empty {
                                        self.composer_placeholder.clone()
                                    } else {
                                        self.composer_value.clone()
                                    }),
                            )
                            .child(
                                Button::new("chat-send", "")
                                    .icon("send")
                                    .disabled(self.disabled || composer_is_empty)
                                    .on_click(move |_, window, cx| {
                                        if let Some(handler) = self.on_send.as_ref() {
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
