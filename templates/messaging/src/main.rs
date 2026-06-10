//! Messaging app template: conversation list, chat thread, and composer —
//! a Slack/Discord-style layout built entirely on kael_ui.

use kael_ui::components::icon_source::IconSource;
use kael_ui::components::input::{Input, InputState};
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::prelude::*;
use std::path::PathBuf;

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        std::fs::read(self.base.join(path))
            .map(|data| Some(std::borrow::Cow::Owned(data)))
            .map_err(|err| err.into())
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        std::fs::read_dir(self.base.join(path))
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        entry
                            .ok()
                            .and_then(|entry| entry.file_name().into_string().ok())
                            .map(SharedString::from)
                    })
                    .collect()
            })
            .map_err(|err| err.into())
    }
}

#[derive(Clone)]
struct Conversation {
    id: usize,
    name: &'static str,
    preview: &'static str,
    time: &'static str,
    unread: usize,
    #[allow(dead_code)]
    online: bool,
}

#[derive(Clone)]
struct Message {
    from_me: bool,
    author: &'static str,
    body: &'static str,
    time: &'static str,
}

fn conversations() -> Vec<Conversation> {
    vec![
        Conversation {
            id: 0,
            name: "Design Team",
            preview: "Maya: The new tokens look great 🔥",
            time: "2m",
            unread: 3,
            online: true,
        },
        Conversation {
            id: 1,
            name: "Maya Chen",
            preview: "Can you review the spacing PR?",
            time: "14m",
            unread: 1,
            online: true,
        },
        Conversation {
            id: 2,
            name: "Platform Eng",
            preview: "Deploy finished — all green",
            time: "1h",
            unread: 0,
            online: false,
        },
        Conversation {
            id: 3,
            name: "Sam Rivera",
            preview: "lunch tomorrow?",
            time: "3h",
            unread: 0,
            online: true,
        },
        Conversation {
            id: 4,
            name: "Release Crew",
            preview: "v0.2.0 checklist is ready",
            time: "1d",
            unread: 0,
            online: false,
        },
    ]
}

fn thread() -> Vec<Message> {
    vec![
        Message {
            from_me: false,
            author: "Maya Chen",
            body: "Morning! I pushed the new theme tokens to the design branch.",
            time: "9:41 AM",
        },
        Message {
            from_me: false,
            author: "Maya Chen",
            body: "The custom brand theme support means we can finally match the marketing site.",
            time: "9:41 AM",
        },
        Message {
            from_me: true,
            author: "You",
            body:
                "Just saw it — Theme::custom with struct update syntax is exactly what we needed.",
            time: "9:44 AM",
        },
        Message {
            from_me: false,
            author: "Maya Chen",
            body:
                "And install_theme refreshing every window makes the theme picker feel instant. 🚀",
            time: "9:45 AM",
        },
        Message {
            from_me: true,
            author: "You",
            body: "Shipping it in the next release. Can you drop the palette in here?",
            time: "9:47 AM",
        },
    ]
}

fn main() {
    Application::new()
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kael_ui"),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::dark());

            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Pulse — Messaging".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1180.0), px(800.0)),
                    })),
                    ..Default::default()
                },
                |_, cx| cx.new(MessagingApp::new),
            )
            .unwrap();
            cx.activate(true);
        });
}

struct MessagingApp {
    composer: Entity<InputState>,
    search: Entity<InputState>,
    selected: usize,
}

impl MessagingApp {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            composer: cx.new(|cx| InputState::new(cx).placeholder("Message Maya Chen…")),
            search: cx.new(|cx| InputState::new(cx).placeholder("Search")),
            selected: 1,
        }
    }

    fn conversation_row(
        &self,
        convo: &Conversation,
        tokens: &ThemeTokens,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.selected == convo.id;
        let id = convo.id;
        div()
            .id(ElementId::Integer(convo.id as u64))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(10.0))
            .rounded(tokens.radius_md)
            .cursor_pointer()
            .when(selected, |el| el.bg(tokens.accent.opacity(0.25)))
            .hover(|mut style| {
                style.background = Some(tokens.muted.opacity(0.5).into());
                style
            })
            .on_click(cx.listener(move |view, _, _, cx| {
                view.selected = id;
                cx.notify();
            }))
            .child(Avatar::new().name(convo.name).size(AvatarSize::Md))
            .child(
                VStack::new()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap(px(2.0))
                    .child(
                        HStack::new()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(convo.name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(tokens.muted_foreground)
                                    .child(convo.time),
                            ),
                    )
                    .child(
                        HStack::new()
                            .justify_between()
                            .items_center()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(12.0))
                                    .text_color(tokens.muted_foreground)
                                    .truncate()
                                    .child(convo.preview),
                            )
                            .when(convo.unread > 0, |el| {
                                el.child(
                                    div()
                                        .min_w(px(18.0))
                                        .h(px(18.0))
                                        .px(px(5.0))
                                        .rounded(px(9.0))
                                        .bg(tokens.primary)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            div()
                                                .text_size(px(10.0))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(tokens.primary_foreground)
                                                .child(convo.unread.to_string()),
                                        ),
                                )
                            }),
                    ),
            )
    }

    fn message_bubble(&self, msg: &Message, tokens: &ThemeTokens) -> impl IntoElement {
        let bubble = div()
            .max_w(px(440.0))
            .px(px(14.0))
            .py(px(10.0))
            .text_size(px(13.5))
            .child(msg.body);

        let bubble = if msg.from_me {
            bubble
                .bg(tokens.primary)
                .text_color(tokens.primary_foreground)
                .rounded_tl(px(16.0))
                .rounded_tr(px(16.0))
                .rounded_bl(px(16.0))
                .rounded_br(px(4.0))
        } else {
            bubble
                .bg(tokens.muted)
                .text_color(tokens.foreground)
                .rounded_tl(px(16.0))
                .rounded_tr(px(16.0))
                .rounded_br(px(16.0))
                .rounded_bl(px(4.0))
        };

        let row = HStack::new()
            .gap(px(10.0))
            .items_end()
            .w_full()
            .when(msg.from_me, |el| el.justify_end());

        let meta = div()
            .text_size(px(10.0))
            .text_color(tokens.muted_foreground)
            .child(msg.time);

        if msg.from_me {
            row.child(meta).child(bubble)
        } else {
            row.child(Avatar::new().name(msg.author).size(AvatarSize::Sm))
                .child(bubble)
                .child(meta)
        }
    }
}

impl Render for MessagingApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let tokens = theme.tokens.clone();
        let convos = conversations();
        let selected_name = convos
            .iter()
            .find(|c| c.id == self.selected)
            .map(|c| c.name)
            .unwrap_or("Conversation");

        let convo_rows: Vec<_> = convos
            .iter()
            .map(|c| self.conversation_row(c, &tokens, cx).into_any_element())
            .collect();

        let sidebar = VStack::new()
            .w(px(300.0))
            .h_full()
            .border_r_1()
            .border_color(tokens.border)
            .child(
                VStack::new()
                    .gap(px(12.0))
                    .p(px(16.0))
                    .child(
                        HStack::new()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Pulse"),
                            )
                            .child(
                                Button::new("compose", "")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::Icon)
                                    .icon(IconSource::Named("square-pen".to_string())),
                            ),
                    )
                    .child(Input::new(&self.search).w_full()),
            )
            .child(
                div().flex_1().min_h(px(0.0)).child(scrollable_vertical(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .px(px(8.0))
                        .children(convo_rows),
                )),
            );

        let header = HStack::new()
            .items_center()
            .justify_between()
            .px(px(20.0))
            .py(px(12.0))
            .border_b_1()
            .border_color(tokens.border)
            .child(
                HStack::new()
                    .gap(px(10.0))
                    .items_center()
                    .child(Avatar::new().name(selected_name).size(AvatarSize::Sm))
                    .child(
                        VStack::new()
                            .gap(px(0.0))
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(selected_name.to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(hsla(152.0 / 360.0, 0.69, 0.45, 1.0))
                                    .child("Online"),
                            ),
                    ),
            )
            .child(
                HStack::new()
                    .gap(px(4.0))
                    .child(
                        Button::new("call", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(IconSource::Named("phone".to_string())),
                    )
                    .child(
                        Button::new("video", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(IconSource::Named("video".to_string())),
                    )
                    .child(
                        Button::new("info", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .icon(IconSource::Named("info".to_string())),
                    ),
            );

        let messages: Vec<_> = thread()
            .iter()
            .map(|m| self.message_bubble(m, &tokens).into_any_element())
            .collect();

        let composer = HStack::new()
            .gap(px(8.0))
            .items_center()
            .p(px(16.0))
            .border_t_1()
            .border_color(tokens.border)
            .child(
                Button::new("attach", "")
                    .variant(ButtonVariant::Ghost)
                    .size(ButtonSize::Icon)
                    .icon(IconSource::Named("paperclip".to_string())),
            )
            .child(div().flex_1().child(Input::new(&self.composer).w_full()))
            .child(
                Button::new("send", "Send")
                    .icon(IconSource::Named("send".to_string()))
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.notify();
                    })),
            );

        let chat = VStack::new()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .child(header)
            .child(
                div().flex_1().min_h(px(0.0)).child(scrollable_vertical(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(14.0))
                        .p(px(20.0))
                        .children(messages),
                )),
            )
            .child(composer);

        div()
            .size_full()
            .flex()
            .bg(tokens.background)
            .text_color(tokens.foreground)
            .child(sidebar)
            .child(chat)
    }
}
