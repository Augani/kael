use kael::{
    App, Application, Bounds, Context, Render, SharedString, Window, WindowBounds, WindowOptions,
    div, prelude::*, px, rgb, size,
};

use kael::{
    ContributedCommand, EXTENSION_RPC_VERSION, ExecutionModel, ExtensionHandshake,
    ExtensionHostRuntime, ExtensionInfo, ExtensionMessage, ExtensionNotification, ExtensionRequest,
    ExtensionResponse, ExtensionTransport, PermissionBroker, PluginManifest,
};

#[cfg(unix)]
use kael::UnixDomainSocketTransport;

#[cfg(unix)]
fn run_extension_process() {
    use kael::process_model::IpcMessage;

    let socket_path =
        std::env::var("GPUI_EXTENSION_SOCKET").expect("GPUI_EXTENSION_SOCKET not set");
    let transport = UnixDomainSocketTransport::connect(&socket_path).expect("failed to connect");
    let mut transport = ExtensionTransport::new(Box::new(transport));

    let msg = transport.recv_message().expect("handshake receive failed");
    match msg {
        ExtensionMessage::Handshake(ExtensionHandshake::Host { .. }) => {
            transport
                .send_handshake(ExtensionHandshake::Extension {
                    version: EXTENSION_RPC_VERSION,
                    accepted: true,
                })
                .expect("handshake send failed");
        }
        _ => panic!("expected handshake"),
    }

    loop {
        match transport.recv_message() {
            Ok(ExtensionMessage::Rpc(IpcMessage::Request { id, body })) => match body {
                ExtensionRequest::Shutdown => {
                    let _ = transport.send_response(id, Ok(ExtensionResponse::Ack));
                    break;
                }
                ExtensionRequest::GetContributions => {
                    let contributions = kael::Contributions::default();
                    let _ = transport
                        .send_response(id, Ok(ExtensionResponse::Contributions(contributions)));
                }
                ExtensionRequest::ExecuteCommand { command_id, .. } => {
                    eprintln!("Extension executing command: {}", command_id);
                    let _ = transport.send_response(id, Ok(ExtensionResponse::Ack));
                }
                _ => {
                    let _ = transport.send_response(id, Ok(ExtensionResponse::Ack));
                }
            },
            Ok(ExtensionMessage::Notification(ExtensionNotification::SettingsChanged {
                key,
                value,
            })) => {
                eprintln!("Extension received settings change: {} = {}", key, value);
            }
            Err(_) => break,
            _ => break,
        }
    }

    eprintln!("Extension shutting down");
}

#[cfg(not(unix))]
fn run_extension_process() {
    eprintln!("Extension mode only supported on Unix");
}

struct PluginHostApp {
    runtime: ExtensionHostRuntime,
    log: Vec<String>,
}

impl PluginHostApp {
    fn add_log(&mut self, message: impl Into<String>) {
        self.log.push(message.into());
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }
}

impl Render for PluginHostApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let extensions: Vec<ExtensionInfo> =
            self.runtime.all().iter().map(|e| (*e).clone()).collect();
        let log_lines = self.log.clone();

        div()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .size_full()
            .p_4()
            .gap_3()
            .child(div().text_xl().child("Plugin Host"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(button("Load Mock", {
                        let entity = entity.clone();
                        move |_, cx| {
                            cx.update_entity(&entity, |this, _cx| {
                                let manifest = PluginManifest::builder(
                                    "com.example.mock",
                                    "Mock Plugin",
                                    "1.0.0",
                                    "1.0.0",
                                    "mock.wasm",
                                    ExecutionModel::Wasm,
                                )
                                .command(ContributedCommand {
                                    id: "mock.hello".to_string(),
                                    title: "Say Hello".to_string(),
                                    keybinding: None,
                                })
                                .build()
                                .unwrap();

                                match this.runtime.load(manifest) {
                                    Ok(_) => this.add_log("Loaded mock plugin"),
                                    Err(e) => this.add_log(format!("Load failed: {}", e)),
                                }
                            });
                        }
                    }))
                    .child(button("Load External", {
                        let entity = entity.clone();
                        move |_, cx| {
                            cx.update_entity(&entity, |this, _cx| {
                                let exe = std::env::current_exe().unwrap();
                                let manifest = PluginManifest::builder(
                                    "com.example.external",
                                    "External Plugin",
                                    "1.0.0",
                                    "1.0.0",
                                    exe.to_string_lossy(),
                                    ExecutionModel::ExternalProcess,
                                )
                                .arg("--extension-mode")
                                .command(ContributedCommand {
                                    id: "ext.echo".to_string(),
                                    title: "Echo".to_string(),
                                    keybinding: None,
                                })
                                .capability(kael::Capability::Notification)
                                .build()
                                .unwrap();

                                match this.runtime.load(manifest) {
                                    Ok(_) => this.add_log("Loaded external plugin"),
                                    Err(e) => this.add_log(format!("Load failed: {}", e)),
                                }
                            });
                        }
                    }))
                    .child(button("Validate All", {
                        let entity = entity.clone();
                        move |_, cx| {
                            cx.update_entity(&entity, |this, _cx| {
                                let broker = PermissionBroker::new();
                                let ids: Vec<String> = this
                                    .runtime
                                    .all()
                                    .into_iter()
                                    .map(|ext| ext.manifest.id.clone())
                                    .collect();
                                for id in ids {
                                    match this.runtime.activate_with_broker(&id, &broker) {
                                        Ok(_) => {
                                            this.add_log(format!("{} validated and activated", id))
                                        }
                                        Err(e) => {
                                            this.add_log(format!("{} validation failed: {}", id, e))
                                        }
                                    }
                                }
                            });
                        }
                    }))
                    .child(button("Broadcast", {
                        let entity = entity.clone();
                        move |_, cx| {
                            cx.update_entity(&entity, |this, _cx| {
                                this.runtime.broadcast_notification(
                                    ExtensionNotification::SettingsChanged {
                                        key: "theme".to_string(),
                                        value: serde_json::json!("dark"),
                                    },
                                );
                                this.add_log("Broadcast settings change");
                            });
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(extensions.into_iter().map(|ext| {
                        let id = ext.manifest.id.clone();
                        let id_activate = id.clone();
                        let is_active = ext.is_active;
                        let entity = entity.clone();

                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .border_1()
                            .border_color(rgb(0xe0e0e0))
                            .rounded_sm()
                            .p_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(format!("{} ({})", ext.manifest.name, ext.manifest.id)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x888888))
                                    .child(if is_active { "Active" } else { "Inactive" }),
                            )
                            .when(!is_active, |this| {
                                this.child(button("Activate", {
                                    let entity = entity.clone();
                                    let id = id_activate;
                                    move |_, cx| {
                                        cx.update_entity(&entity, |this, _cx| {
                                            match this.runtime.activate(&id) {
                                                Ok(_) => this.add_log(format!("Activated {}", id)),
                                                Err(e) => this.add_log(format!(
                                                    "Activate {} failed: {}",
                                                    id, e
                                                )),
                                            }
                                        });
                                    }
                                }))
                            })
                            .when(is_active, |this| {
                                this.child(button("Deactivate", {
                                    let entity = entity.clone();
                                    let id = id;
                                    move |_, cx| {
                                        cx.update_entity(&entity, |this, _cx| {
                                            match this.runtime.deactivate(&id) {
                                                Ok(_) => {
                                                    this.add_log(format!("Deactivated {}", id))
                                                }
                                                Err(e) => this.add_log(format!(
                                                    "Deactivate {} failed: {}",
                                                    id, e
                                                )),
                                            }
                                        });
                                    }
                                }))
                            })
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .bg(rgb(0xf5f5f5))
                    .rounded_sm()
                    .p_2()
                    .children(
                        log_lines
                            .into_iter()
                            .map(|line| div().text_sm().child(line)),
                    ),
            )
    }
}

fn button(text: &str, on_click: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(SharedString::from(text.to_string()))
        .px_2()
        .py_1()
        .bg(rgb(0xf0f0f0))
        .border_1()
        .border_color(rgb(0xd0d0d0))
        .rounded_sm()
        .cursor_pointer()
        .child(text.to_string())
        .on_click(move |_, window, cx| on_click(window, cx))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--extension-mode") {
        run_extension_process();
        return;
    }

    Application::new().run(|cx: &mut App| {
        let tmp_dir = std::env::temp_dir().join("gpui-plugin-host-example");
        let _ = std::fs::create_dir_all(&tmp_dir);

        let runtime = ExtensionHostRuntime::new(&tmp_dir, "plugin-host-example");

        let bounds = Bounds::centered(None, size(px(600.0), px(500.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| PluginHostApp {
                    runtime,
                    log: vec!["Plugin Host started".to_string()],
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
