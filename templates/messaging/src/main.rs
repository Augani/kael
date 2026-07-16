//! Messaging app template: conversation list, chat thread, and composer —
//! a Slack/Discord-style layout built entirely on kael_ui.

use kael::{AccessibilityAction, AccessibilityAttributes, AccessibilityRole, AccessibilityState};
use kael_ui::components::icon_source::IconSource;
use kael_ui::components::input::{Input, InputState};
use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::prelude::*;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ASSET_LIST_ENTRIES: usize = 4096;

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let path = self.resolve(path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Ok(None);
        }
        if metadata.len() > MAX_ASSET_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "asset exceeds the 16 MiB limit",
            )
            .into());
        }
        let mut data = Vec::with_capacity(metadata.len() as usize);
        std::fs::File::open(path)?
            .take(MAX_ASSET_BYTES + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > MAX_ASSET_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "asset grew beyond the 16 MiB limit while reading",
            )
            .into());
        }
        Ok(Some(std::borrow::Cow::Owned(data)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let path = self.resolve(path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Ok(Vec::new());
        }
        std::fs::read_dir(path)?
            .take(MAX_ASSET_LIST_ENTRIES)
            .map(|entry| {
                let name = entry?.file_name().into_string().map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "asset name is not valid UTF-8",
                    )
                })?;
                Ok(SharedString::from(name))
            })
            .collect()
    }
}

impl Assets {
    fn resolve(&self, path: &str) -> Result<PathBuf> {
        let relative = Path::new(path);
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "asset path must be a non-empty relative path",
            )
            .into());
        }
        Ok(self.base.join(relative))
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
    body: String,
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
            body: "Morning! I pushed the new theme tokens to the design branch.".to_string(),
            time: "9:41 AM",
        },
        Message {
            from_me: false,
            author: "Maya Chen",
            body: "The custom brand theme support means we can finally match the marketing site."
                .to_string(),
            time: "9:41 AM",
        },
        Message {
            from_me: true,
            author: "You",
            body:
                "Just saw it — Theme::custom with struct update syntax is exactly what we needed."
                    .to_string(),
            time: "9:44 AM",
        },
        Message {
            from_me: false,
            author: "Maya Chen",
            body:
                "And install_theme refreshing every window makes the theme picker feel instant. 🚀"
                    .to_string(),
            time: "9:45 AM",
        },
        Message {
            from_me: true,
            author: "You",
            body: "Shipping it in the next release. Can you drop the palette in here?".to_string(),
            time: "9:47 AM",
        },
    ]
}

fn main() -> Result<()> {
    Application::try_new()?
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kael_ui"),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::dark());

            if let Err(error) = cx.open_window(
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
            ) {
                eprintln!("failed to open the messaging window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
    Ok(())
}

struct MessagingApp {
    composer: Entity<InputState>,
    search: Entity<InputState>,
    selected: usize,
    sent_messages: Vec<String>,
}

impl MessagingApp {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            composer: cx.new(|cx| InputState::new(cx).placeholder("Message Maya Chen…")),
            search: cx.new(|cx| InputState::new(cx).placeholder("Search")),
            selected: 1,
            sent_messages: Vec::new(),
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
        let keyboard_id = convo.id;
        let accessibility_state = if selected {
            AccessibilityState::SELECTED
        } else {
            AccessibilityState::NONE
        };
        div()
            .id(ElementId::Integer(convo.id as u64))
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Button)
                    .label(format!("Open conversation with {}", convo.name))
                    .states(accessibility_state)
                    .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]),
            )
            .focusable()
            .tab_index(0)
            .tab_stop(true)
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
            .focus_visible(|style| style.inset_ring(tokens.ring, px(2.0)))
            .on_click(cx.listener(move |view, _, _, cx| {
                view.selected = id;
                cx.notify();
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.modified()
                    && matches!(event.keystroke.key.as_str(), "enter" | "space")
                {
                    view.selected = keyboard_id;
                    cx.notify();
                    cx.stop_propagation();
                    window.prevent_default();
                }
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
            .child(msg.body.clone());

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
                                    .tooltip("Compose message")
                                    .icon(IconSource::Named("square-pen".to_string())),
                            ),
                    )
                    .child(
                        Input::new(&self.search)
                            .aria_label("Search conversations")
                            .w_full(),
                    ),
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
                            .tooltip("Start audio call")
                            .icon(IconSource::Named("phone".to_string())),
                    )
                    .child(
                        Button::new("video", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .tooltip("Start video call")
                            .icon(IconSource::Named("video".to_string())),
                    )
                    .child(
                        Button::new("info", "")
                            .variant(ButtonVariant::Ghost)
                            .size(ButtonSize::Icon)
                            .tooltip("Conversation details")
                            .icon(IconSource::Named("info".to_string())),
                    ),
            );

        let mut thread = thread();
        thread.extend(self.sent_messages.iter().cloned().map(|body| Message {
            from_me: true,
            author: "You",
            body,
            time: "Now",
        }));
        let messages: Vec<_> = thread
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
                    .tooltip("Attach a file")
                    .icon(IconSource::Named("paperclip".to_string())),
            )
            .child(
                div().flex_1().child(
                    Input::new(&self.composer)
                        .aria_label("Message composer")
                        .w_full(),
                ),
            )
            .child(
                Button::new("send", "Send")
                    .icon(IconSource::Named("send".to_string()))
                    .on_click(cx.listener(|view, _, _, cx| {
                        let message = view.composer.read(cx).content().trim().to_string();
                        if message.is_empty() {
                            return;
                        }
                        view.sent_messages.push(message);
                        view.composer.update(cx, |input, cx| input.clear(cx));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_paths_are_confined_to_the_asset_root() {
        let assets = Assets {
            base: PathBuf::from("/tmp/assets"),
        };
        assert_eq!(
            assets.resolve("icons/send.svg").unwrap(),
            PathBuf::from("/tmp/assets/icons/send.svg")
        );
        assert!(assets.resolve("../secret").is_err());
        assert!(assets.resolve("/etc/passwd").is_err());
        assert!(assets.resolve("").is_err());
    }
}
