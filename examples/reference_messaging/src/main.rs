use std::collections::HashMap;

use kael::prelude::*;
use kael::*;
use serde::{Deserialize, Serialize};

const APP_ID: &str = "dev.kael.reference-messaging";
const MAIN_WINDOW_ID: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessagingSettings {
    theme: String,
    font_size: u32,
    compact_mode: bool,
}

impl Default for MessagingSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            font_size: 14,
            compact_mode: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Conversation {
    id: String,
    name: String,
    last_message: String,
    timestamp: String,
    unread: bool,
}

#[derive(Debug, Clone)]
struct Message {
    id: String,
    sender: String,
    content: String,
    is_me: bool,
}

struct MessagingApp {
    conversations: Vec<Conversation>,
    messages: HashMap<String, Vec<Message>>,
    draft_presets: HashMap<String, Vec<String>>,
    selected_conversation: Option<String>,
    composer_text: String,
    settings: SettingsStore<MessagingSettings>,
    status_message: String,
}

impl MessagingApp {
    fn theme_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xffffff),
            _ => rgb(0x0f172a),
        }
    }

    fn theme_fg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0x1e293b),
            _ => rgb(0xe2e8f0),
        }
    }

    fn theme_sidebar_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xf8fafc),
            _ => rgb(0x1e293b),
        }
    }

    fn theme_bubble_me(&self) -> Rgba {
        rgb(0x3b82f6)
    }

    fn theme_bubble_them(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xe2e8f0),
            _ => rgb(0x334155),
        }
    }

    fn current_conversation(&self) -> Option<&Conversation> {
        let selected = self.selected_conversation.as_deref()?;
        self.conversations
            .iter()
            .find(|conversation| conversation.id == selected)
    }

    fn visible_messages(&self) -> Vec<Message> {
        self.selected_conversation
            .as_deref()
            .and_then(|id| self.messages.get(id))
            .cloned()
            .unwrap_or_default()
    }

    fn current_presets(&self) -> Vec<String> {
        self.selected_conversation
            .as_deref()
            .and_then(|id| self.draft_presets.get(id))
            .cloned()
            .unwrap_or_default()
    }

    fn set_preset(&mut self, text: &str) {
        self.composer_text = text.to_string();
        self.status_message = format!("Prepared draft: \"{}\"", text);
    }

    fn send_message(&mut self) {
        let conversation_id = match self.selected_conversation.clone() {
            Some(id) => id,
            None => {
                self.status_message = "Pick a conversation before sending a message.".to_string();
                return;
            }
        };

        let text = self.composer_text.trim().to_string();
        if text.is_empty() {
            self.status_message =
                "Choose a quick reply first so the composer has content.".to_string();
            return;
        }

        let entry = self.messages.entry(conversation_id.clone()).or_default();
        entry.push(Message {
            id: format!("msg-{}-{}", conversation_id, entry.len()),
            sender: "Me".to_string(),
            content: text.clone(),
            is_me: true,
        });

        if let Some(conversation) = self
            .conversations
            .iter_mut()
            .find(|conversation| conversation.id == conversation_id)
        {
            conversation.last_message = format!("You: {}", text);
            conversation.timestamp = "Just now".to_string();
            conversation.unread = false;
        }

        self.status_message = "Message queued into the active conversation.".to_string();
        self.composer_text.clear();
    }

    fn select_conversation(&mut self, id: &str) {
        self.selected_conversation = Some(id.to_string());
        for conv in &mut self.conversations {
            if conv.id == id {
                conv.unread = false;
            }
        }

        self.composer_text = self
            .current_presets()
            .into_iter()
            .next()
            .unwrap_or_default();
        if let Some(conversation) = self.current_conversation() {
            self.status_message = format!("Focused {}", conversation.name);
        }
    }

    fn toggle_theme(&mut self) {
        let next_theme = if self.settings.data().theme == "light" {
            "dark"
        } else {
            "light"
        };
        let _ = self
            .settings
            .update(|settings| settings.theme = next_theme.to_string());
        self.status_message = format!("Theme switched to {}", next_theme);
    }

    fn toggle_density(&mut self) {
        let next_mode = !self.settings.data().compact_mode;
        let _ = self
            .settings
            .update(|settings| settings.compact_mode = next_mode);
        self.status_message = if next_mode {
            "Conversation list switched to compact mode.".to_string()
        } else {
            "Conversation list switched to comfortable mode.".to_string()
        };
    }
}

impl Render for MessagingApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme_bg = self.theme_bg();
        let theme_fg = self.theme_fg();
        let sidebar_bg = self.theme_sidebar_bg();
        let bubble_me = self.theme_bubble_me();
        let bubble_them = self.theme_bubble_them();
        let compact = self.settings.data().compact_mode;
        let is_light = self.settings.data().theme == "light";
        let conversations = self.conversations.clone();
        let selected_conversation_id = self.selected_conversation.clone();
        let active_conversation = self.current_conversation().cloned();
        let visible_messages = self.visible_messages();
        let draft_presets = self.current_presets();
        let composer_text = self.composer_text.clone();
        let status_message = self.status_message.clone();
        let entity = cx.entity();
        let conversation_entity = entity.clone();
        let preset_entity = entity.clone();

        div()
            .flex()
            .size_full()
            .bg(theme_bg)
            .text_color(theme_fg)
            .child(
                div()
                    .id(ElementId::Name(SharedString::from("conversation-sidebar")))
                    .flex()
                    .flex_col()
                    .w(px(260.0))
                    .h_full()
                    .bg(sidebar_bg)
                    .border_r_1()
                    .border_color(rgb(0x334155))
                    .child(
                        div()
                            .p_3()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(div().font_weight(FontWeight::BOLD).child("Messages"))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(
                                                "toggle-theme-button",
                                            )))
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(rgb(0x334155))
                                            .text_xs()
                                            .cursor_pointer()
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.toggle_theme();
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(if is_light { "Dark" } else { "Light" }),
                                    )
                                    .child(
                                        div()
                                            .id(ElementId::Name(SharedString::from(
                                                "toggle-density-button",
                                            )))
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(rgb(0x334155))
                                            .text_xs()
                                            .cursor_pointer()
                                            .on_click({
                                                let entity = entity.clone();
                                                move |_, _, cx| {
                                                    cx.update_entity(&entity, |app, cx| {
                                                        app.toggle_density();
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .child(if compact { "Roomy" } else { "Compact" }),
                                    ),
                            )
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                    .label("Conversation sidebar header"),
                            ),
                    )
                    .child(
                        div()
                            .id(ElementId::Name(SharedString::from("conversation-list")))
                            .flex()
                            .flex_col()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::List)
                                    .label("Conversations"),
                            )
                            .children(conversations.into_iter().map(move |conv| {
                                let selected =
                                    selected_conversation_id.as_deref() == Some(conv.id.as_str());
                                let unread = conv.unread;
                                let id = conv.id.clone();
                                let name = conv.name.clone();
                                let timestamp = conv.timestamp.clone();
                                let last_message = conv.last_message.clone();
                                div()
                                    .id(ElementId::Name(SharedString::from(format!("conv-{}", id))))
                                    .flex()
                                    .flex_col()
                                    .px_3()
                                    .py(if compact { px(6.0) } else { px(10.0) })
                                    .map(|this| {
                                        if selected {
                                            this.bg(rgb(0x334155))
                                        } else {
                                            this
                                        }
                                    })
                                    .map(|this| {
                                        if unread {
                                            this.font_weight(FontWeight::BOLD)
                                        } else {
                                            this
                                        }
                                    })
                                    .cursor_pointer()
                                    .on_click({
                                        let entity = conversation_entity.clone();
                                        let id = id.clone();
                                        move |_, _, cx| {
                                            cx.update_entity(&entity, |app, cx| {
                                                app.select_conversation(&id);
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::ListItem)
                                            .label(name.clone()),
                                    )
                                    .child(div().flex().justify_between().child(name).child(
                                        div().text_xs().text_color(rgb(0x94a3b8)).child(timestamp),
                                    ))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x94a3b8))
                                            .child(last_message),
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .child(
                        div()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(rgb(0x334155))
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .child(
                                                div().font_weight(FontWeight::BOLD).child(
                                                    active_conversation
                                                        .as_ref()
                                                        .map(|conversation| {
                                                            conversation.name.clone()
                                                        })
                                                        .unwrap_or_else(|| {
                                                            "No conversation selected".to_string()
                                                        }),
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(0x94a3b8))
                                                    .child(status_message.clone()),
                                            ),
                                    )
                                    .child(
                                        div().text_sm().text_color(rgb(0x94a3b8)).child(
                                            active_conversation
                                                .as_ref()
                                                .map(|conversation| conversation.timestamp.clone())
                                                .unwrap_or_else(|| "Idle".to_string()),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Group)
                                    .label("Chat messages"),
                            )
                            .children(visible_messages.into_iter().map(move |msg| {
                                let bubble_bg = if msg.is_me { bubble_me } else { bubble_them };
                                let text_color = if msg.is_me { rgb(0xffffff) } else { theme_fg };
                                let is_me = msg.is_me;
                                let message_id = msg.id.clone();
                                let content = msg.content.clone();
                                let sender = msg.sender.clone();
                                div()
                                    .id(ElementId::Name(SharedString::from(format!(
                                        "message-{}",
                                        message_id
                                    ))))
                                    .flex()
                                    .map(|this| {
                                        if is_me {
                                            this.justify_end()
                                        } else {
                                            this.justify_start()
                                        }
                                    })
                                    .child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .max_w(px(480.0))
                                            .bg(bubble_bg)
                                            .text_color(text_color)
                                            .accessibility(
                                                AccessibilityAttributes::new(
                                                    AccessibilityRole::StaticText,
                                                )
                                                .label(format!("{}: {}", sender, content)),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(if is_me {
                                                                rgba(0xffffffb3)
                                                            } else {
                                                                rgb(0x94a3b8)
                                                            })
                                                            .child(sender),
                                                    )
                                                    .child(content),
                                            ),
                                    )
                            })),
                    )
                    .child(div().px_3().pt_2().flex().gap_2().children(
                        draft_presets.into_iter().map(move |preset| {
                            let entity = preset_entity.clone();
                            let label = preset.clone();
                            div()
                                .id(ElementId::Name(SharedString::from(format!(
                                    "preset-{}",
                                    preset.replace(' ', "-")
                                ))))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(0x475569))
                                .bg(sidebar_bg)
                                .text_sm()
                                .cursor_pointer()
                                .on_click({
                                    let preset = preset.clone();
                                    move |_, _, cx| {
                                        cx.update_entity(&entity, |app, cx| {
                                            app.set_preset(&preset);
                                            cx.notify();
                                        });
                                    }
                                })
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Button)
                                        .label(format!("Use quick reply {}", label)),
                                )
                                .child(label)
                        }),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .p_3()
                            .border_t_1()
                            .border_color(rgb(0x334155))
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .px_3()
                                    .py_2()
                                    .rounded_md()
                                    .bg(sidebar_bg)
                                    .border_1()
                                    .border_color(rgb(0x475569))
                                    .child(if composer_text.is_empty() {
                                        div()
                                            .text_color(rgb(0x94a3b8))
                                            .child("Choose a quick reply to populate the composer")
                                    } else {
                                        div().child(composer_text)
                                    })
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::TextInput)
                                            .label("Message composer")
                                            .placeholder("Type a message..."),
                                    ),
                            )
                            .child(
                                div()
                                    .id(ElementId::Name(SharedString::from("send-button")))
                                    .px_4()
                                    .py_2()
                                    .rounded_md()
                                    .bg(bubble_me)
                                    .text_color(rgb(0xffffff))
                                    .cursor_pointer()
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            cx.update_entity(&entity, |app, cx| {
                                                app.send_message();
                                                cx.notify();
                                            });
                                        }
                                    })
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Button)
                                            .label("Send message"),
                                    )
                                    .child("Send"),
                            ),
                    ),
            )
    }
}

struct MessagingRuntime {
    session_store: SessionStore,
    _crash_reporter: CrashReporter,
    tracer: Tracer,
    _quit_subscription: Subscription,
    last_window_state: Option<WindowState>,
}

impl Global for MessagingRuntime {}

fn main() {
    let app = Application::new();
    app.run(|cx: &mut App| {
        if let Err(error) = bootstrap(cx) {
            eprintln!("failed to initialize messaging app: {error:#}");
        }
    });
}

fn bootstrap(cx: &mut App) -> Result<()> {
    let session_store = SessionStore::new(APP_ID)?;

    let mut crash_reporter = CrashReporter::new(APP_ID)?;
    crash_reporter.install_hook();

    let tracer = Tracer::default();
    tracer.enable();
    tracer.install_global();
    tracer.record("messaging.startup", "lifecycle", TracePhase::Instant);

    let settings_path = session_store.storage_dir().join("settings.json");
    let settings: SettingsStore<MessagingSettings> = SettingsStore::load(&settings_path, &[])
        .unwrap_or_else(|_| SettingsStore::new(&settings_path));

    let workspace = cx.open_workspace();
    workspace.add_panel(Box::new(SidebarPanel::new("contacts", "Contacts")));
    workspace.add_panel(Box::new(InspectorPanel::new("details", "Details")));

    cx.register_command("new-conversation", "New Conversation", || {
        println!("New conversation command triggered");
    });
    cx.register_command("search-messages", "Search Messages", || {
        println!("Search messages command triggered");
    });
    cx.register_command("toggle-theme", "Toggle Theme", || {
        println!("Toggle theme command triggered");
    });

    cx.set_tray_icon(Some(&[]));
    cx.set_tray_tooltip("Reference Messaging");
    cx.set_tray_menu(vec![
        TrayMenuItem::Action {
            label: "Show Window".into(),
            id: "show".into(),
        },
        TrayMenuItem::Action {
            label: "Quit".into(),
            id: "quit".into(),
        },
    ]);
    cx.on_tray_menu_action(|id, cx| match id.as_ref() {
        "show" => {
            cx.activate(true);
        }
        "quit" => {
            cx.quit();
        }
        _ => {}
    });

    let restored_states = session_store.load_window_states().unwrap_or_default();
    let restored_state = restored_states.get(MAIN_WINDOW_ID).cloned();

    let quit_subscription = cx.on_app_quit(|cx| {
        if let Some(runtime) = cx.try_global::<MessagingRuntime>() {
            let mut states = HashMap::new();
            if let Some(state) = &runtime.last_window_state {
                states.insert(MAIN_WINDOW_ID.to_string(), state.clone());
            }
            let _ = runtime.session_store.save_window_states(&states);
            let _ = runtime.tracer.write_to_file(
                runtime
                    .session_store
                    .storage_dir()
                    .join("messaging_trace.json"),
            );
        }
        async {}
    });

    let conversations = vec![
        Conversation {
            id: "c1".to_string(),
            name: "Design Team".to_string(),
            last_message: "Alice: New mockups are ready".to_string(),
            timestamp: "10:42".to_string(),
            unread: true,
        },
        Conversation {
            id: "c2".to_string(),
            name: "Bob Smith".to_string(),
            last_message: "See you tomorrow!".to_string(),
            timestamp: "09:15".to_string(),
            unread: false,
        },
        Conversation {
            id: "c3".to_string(),
            name: "General".to_string(),
            last_message: "Charlie: Standup in 5 min".to_string(),
            timestamp: "Yesterday".to_string(),
            unread: true,
        },
    ];

    let messages = HashMap::from([
        (
            "c1".to_string(),
            vec![
                Message {
                    id: "m1".to_string(),
                    sender: "Alice".to_string(),
                    content: "Hey everyone, check out the new designs.".to_string(),
                    is_me: false,
                },
                Message {
                    id: "m2".to_string(),
                    sender: "Me".to_string(),
                    content: "Looks great! Love the color palette.".to_string(),
                    is_me: true,
                },
                Message {
                    id: "m3".to_string(),
                    sender: "Alice".to_string(),
                    content: "Thanks! I'll push the final assets shortly.".to_string(),
                    is_me: false,
                },
            ],
        ),
        (
            "c2".to_string(),
            vec![
                Message {
                    id: "m4".to_string(),
                    sender: "Bob Smith".to_string(),
                    content: "Tomorrow still works for the launch sync?".to_string(),
                    is_me: false,
                },
                Message {
                    id: "m5".to_string(),
                    sender: "Me".to_string(),
                    content: "Yes, I’ll share the agenda before noon.".to_string(),
                    is_me: true,
                },
            ],
        ),
        (
            "c3".to_string(),
            vec![
                Message {
                    id: "m6".to_string(),
                    sender: "Charlie".to_string(),
                    content: "Standup starts in five minutes. Bring blockers.".to_string(),
                    is_me: false,
                },
                Message {
                    id: "m7".to_string(),
                    sender: "Dana".to_string(),
                    content: "On my way, finishing the perf notes now.".to_string(),
                    is_me: false,
                },
            ],
        ),
    ]);

    let messaging_app = MessagingApp {
        conversations,
        messages,
        draft_presets: HashMap::from([
            (
                "c1".to_string(),
                vec![
                    "I’m reviewing the latest mockups now.".to_string(),
                    "Please share the exported assets in the design channel.".to_string(),
                    "Let’s keep the motion pass subtle on desktop.".to_string(),
                ],
            ),
            (
                "c2".to_string(),
                vec![
                    "Yes, tomorrow works for me.".to_string(),
                    "Can we move it thirty minutes later?".to_string(),
                    "I’ll send notes right after the meeting.".to_string(),
                ],
            ),
            (
                "c3".to_string(),
                vec![
                    "I have one blocker around Windows tray behavior.".to_string(),
                    "Linux packaging is green on my side.".to_string(),
                    "I’ll take the follow-up on release automation.".to_string(),
                ],
            ),
        ]),
        selected_conversation: Some("c1".to_string()),
        composer_text: "I’m reviewing the latest mockups now.".to_string(),
        settings,
        status_message: "Messaging reference app booted with session restore and quick actions."
            .to_string(),
    };

    let window_bounds = restored_state
        .as_ref()
        .map(|state| state.bounds)
        .or_else(|| {
            Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1000.0), px(700.0)),
                cx,
            )))
        });

    cx.open_window(
        WindowOptions {
            window_bounds,
            ..Default::default()
        },
        move |window, cx| {
            if let Some(state) = restored_state.as_ref() {
                window.restore_window_state(state);
            }

            let entity = cx.new(|cx| {
                cx.observe_window_bounds(window, |_, window, cx| {
                    cx.update_global(|runtime: &mut MessagingRuntime, _| {
                        runtime.last_window_state = Some(window.window_state());
                    });
                })
                .detach();

                messaging_app
            });

            let runtime = MessagingRuntime {
                session_store,
                _crash_reporter: crash_reporter,
                tracer,
                _quit_subscription: quit_subscription,
                last_window_state: restored_state.clone(),
            };
            cx.set_global(runtime);

            entity
        },
    )?;

    cx.activate(true);
    Ok(())
}
