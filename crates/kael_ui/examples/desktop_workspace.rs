//! A real dockable desktop: lazy files, live typed properties, preview and notes.
//! Run `cargo run -p kael_ui --example desktop_workspace`.
use kael_ui::prelude::*;
mod desktop_common;
use std::path::PathBuf;

struct Preview {
    properties: Entity<PropertyInspectorState>,
    _observer: Subscription,
}
impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let properties = self.properties.read(cx);
        let name = match properties.value("name").unwrap() {
            PropertyValue::Text(value) => value.clone(),
            _ => unreachable!(),
        };
        let width = match properties.value("width").unwrap() {
            PropertyValue::Number(value) => *value as f32,
            _ => unreachable!(),
        };
        let opacity = match properties.value("opacity").unwrap() {
            PropertyValue::Number(value) => *value as f32,
            _ => unreachable!(),
        };
        let visible = properties.value("visible") == Some(&PropertyValue::Boolean(true));
        let color = match properties.value("color").unwrap() {
            PropertyValue::Choice(value) if value == "blue" => rgb(0x4263eb),
            PropertyValue::Choice(value) if value == "rose" => rgb(0xe64980),
            _ => rgb(0x3b5b40),
        };
        let theme = Theme::of(cx);
        div().size_full().flex().flex_col().items_center().justify_center().p_6().gap_4().bg(theme.tokens.muted.opacity(0.25))
            .when(visible, |element| element.child(div().w(px(width)).max_w_full().p_6().rounded(theme.tokens.radius_lg).bg(color).opacity(opacity).text_color(white()).flex().flex_col().gap_3()
                .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(name))
                .child(div().text_sm().child("Change these properties in the inspector. Pane contents retain state when docked, floated or hidden."))))
            .child(div().text_sm().text_color(theme.tokens.muted_foreground).child("Drag tabs or the group grip. Drop on a pane edge to split. Float, resize, zoom and save your layout."))
    }
}
struct Notes {
    text: SharedString,
}
impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let notes = cx.entity().downgrade();
        div().size_full().p_4().child(
            TextArea::new("workspace-notes")
                .label("Workspace notes")
                .rows(10)
                .value(self.text.clone())
                .on_change(move |text, _, cx| {
                    let _ = notes.update(cx, |notes, cx| {
                        notes.text = text;
                        cx.notify();
                    });
                }),
        )
    }
}
struct Desktop {
    workspace: Entity<DockWorkspaceState>,
    path: PathBuf,
    preset: String,
    status: String,
    _observer: Subscription,
    task: Option<Task<()>>,
}
impl Desktop {
    fn new(path: PathBuf, loaded: Option<String>, cx: &mut Context<Self>) -> Self {
        let properties = cx.new(|cx| {
            PropertyInspectorState::new(
                vec![
                    PropertyGroup::new(
                        "content",
                        "Content",
                        vec![
                            PropertyField::new(
                                "name",
                                "Title",
                                PropertyKind::Text {
                                    max_chars: Some(64),
                                },
                                PropertyValue::Text("A desktop of your own".into()),
                            ),
                            PropertyField::new(
                                "visible",
                                "Visible",
                                PropertyKind::Boolean,
                                PropertyValue::Boolean(true),
                            ),
                        ],
                    ),
                    PropertyGroup::new(
                        "appearance",
                        "Appearance",
                        vec![
                            PropertyField::new(
                                "width",
                                "Width",
                                PropertyKind::Number {
                                    min: Some(160.0),
                                    max: Some(600.0),
                                    step: 20.0,
                                    precision: 0,
                                },
                                PropertyValue::Number(360.0),
                            ),
                            PropertyField::new(
                                "opacity",
                                "Opacity",
                                PropertyKind::Number {
                                    min: Some(0.1),
                                    max: Some(1.0),
                                    step: 0.1,
                                    precision: 1,
                                },
                                PropertyValue::Number(1.0),
                            ),
                            PropertyField::new(
                                "color",
                                "Color",
                                PropertyKind::Choice {
                                    options: vec![
                                        PropertyChoice::new("forest", "Forest"),
                                        PropertyChoice::new("blue", "Blue"),
                                        PropertyChoice::new("rose", "Rose"),
                                    ],
                                },
                                PropertyValue::Choice("forest".into()),
                            ),
                        ],
                    ),
                ],
                cx,
            )
            .unwrap()
        });
        let preview = cx.new({
            let properties = properties.clone();
            move |cx| Preview {
                _observer: cx.observe(&properties, |_, _, cx| cx.notify()),
                properties,
            }
        });
        let notes = cx.new(|_| Notes { text: "State survives docking and switching tabs.\nTry typing, float this pane, then dock it again.\n".into() });
        let root = std::env::current_dir().unwrap();
        let files = cx.new(|cx| FileTreeState::filesystem(&root, cx).unwrap());
        files.update(cx, |files, cx| files.set_expanded(&root, true, cx));
        let workspace = cx.new(|cx| {
            DockWorkspaceState::new(
                vec![
                    DockPane::new("preview", "Preview", move |_, _| {
                        preview.clone().into_any_element()
                    }),
                    DockPane::new("files", "Files", move |_, _| {
                        VirtualFileTree::new("workspace-files", files.clone())
                            .drag_drop(false)
                            .into_any_element()
                    }),
                    DockPane::new("properties", "Properties", move |_, _| {
                        PropertyInspector::new("workspace-properties", properties.clone())
                            .into_any_element()
                    }),
                    DockPane::new("notes", "Notes", move |_, _| {
                        notes.clone().into_any_element()
                    }),
                ],
                DockLayout::group(["preview", "files", "properties", "notes"]),
                cx,
            )
            .unwrap()
        });
        workspace.update(cx, |state, cx| {
            state.move_pane("files", 1, DockPlacement::Left, cx);
            state.move_pane("properties", 1, DockPlacement::Right, cx);
        });
        let preset = workspace.read(cx).layout_json().unwrap();
        let status = if let Some(json) = loaded {
            workspace.update(cx, |state, cx| match state.restore_json(&json, cx) {
                Ok(()) => "Restored saved layout".into(),
                Err(error) => format!("Saved layout rejected: {error}"),
            })
        } else {
            "Arrange panes, then save the layout to restore it on the next run.".into()
        };
        let observer = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self {
            workspace,
            path,
            preset,
            status,
            _observer: observer,
            task: None,
        }
    }
}
impl Render for Desktop {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let save = cx.entity().downgrade();
        let restore = cx.entity().downgrade();
        let reset = cx.entity().downgrade();
        let theme = Theme::of(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .child(
                div()
                    .p_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.tokens.border)
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Kael desktop workspace"),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("save-layout", "Save layout")
                            .size(ButtonSize::Sm)
                            .on_click(move |_, _, cx| {
                                let _ = save.update(cx, |view, cx| {
                                    let json = view.workspace.read(cx).layout_json().unwrap();
                                    let path = view.path.clone();
                                    let background = cx.background_executor().clone();
                                    view.task = Some(cx.spawn(async move |view, cx| {
                                        let result = background
                                            .spawn(async move {
                                                let temporary = path.with_extension(format!(
                                                    "tmp-{}",
                                                    std::process::id()
                                                ));
                                                std::fs::write(&temporary, json)?;
                                                std::fs::rename(temporary, path)
                                            })
                                            .await;
                                        let _ = view.update(cx, |view, cx| {
                                            view.status = match result {
                                                Ok(()) => format!("Saved {}", view.path.display()),
                                                Err(error) => format!("Save failed: {error}"),
                                            };
                                            cx.notify();
                                        });
                                    }));
                                });
                            }),
                    )
                    .child(
                        Button::new("restore-layout", "Restore layout")
                            .size(ButtonSize::Sm)
                            .variant(ButtonVariant::Outline)
                            .on_click(move |_, _, cx| {
                                let _ = restore.update(cx, |view, cx| {
                                    let path = view.path.clone();
                                    let background = cx.background_executor().clone();
                                    view.task = Some(cx.spawn(async move |view, cx| {
                                        let result = background
                                            .spawn(async move {
                                                let metadata = std::fs::metadata(&path)?;
                                                if metadata.len() > 1_048_576 {
                                                    return Err(std::io::Error::other(
                                                        "layout exceeds one MiB",
                                                    ));
                                                }
                                                std::fs::read_to_string(path)
                                            })
                                            .await;
                                        let _ = view.update(cx, |view, cx| {
                                            view.status = match result {
                                                Ok(json) => {
                                                    view.workspace.update(cx, |state, cx| {
                                                        match state.restore_json(&json, cx) {
                                                            Ok(()) => "Layout restored".into(),
                                                            Err(error) => {
                                                                format!("Invalid layout: {error}")
                                                            }
                                                        }
                                                    })
                                                }
                                                Err(error) => format!("Restore failed: {error}"),
                                            };
                                            cx.notify();
                                        });
                                    }));
                                });
                            }),
                    )
                    .child(
                        Button::new("reset-layout", "Reset layout")
                            .size(ButtonSize::Sm)
                            .variant(ButtonVariant::Ghost)
                            .on_click(move |_, _, cx| {
                                let _ = reset.update(cx, |view, cx| {
                                    view.workspace
                                        .update(cx, |state, cx| {
                                            state.restore_json(&view.preset, cx)
                                        })
                                        .unwrap();
                                    view.status = "Default layout restored".into();
                                    cx.notify();
                                });
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(DockWorkspace::new("desktop-dock", self.workspace.clone())),
            )
            .child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(theme.tokens.muted_foreground)
                    .child(self.status.clone()),
            )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join("kael-desktop-workspace-layout.json");
    let loaded = std::fs::metadata(&path)
        .ok()
        .filter(|metadata| metadata.len() <= 1_048_576)
        .and_then(|_| std::fs::read_to_string(&path).ok());
    Application::try_new()?.run(move |cx| {
        kael_ui::init(cx);
        desktop_common::install(cx, "Kael desktop workspace");
        install_theme(cx, Theme::astryx_neutral());
        if let Err(error) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1280.0), px(820.0)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("Kael desktop workspace");
                cx.new(|cx| Desktop::new(path, loaded, cx))
            },
        ) {
            eprintln!("failed to open workspace: {error}");
            cx.quit();
        }
    });
    Ok(())
}
