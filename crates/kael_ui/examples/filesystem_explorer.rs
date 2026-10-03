//! Lazy native explorer with real move operations confined to a demo workspace.
//! Run `cargo run -p kael_ui --example filesystem_explorer`.
//! Pass a directory to inspect it in read-only mode.
//! Pass `--stress` for a synthetic 100,000-file background/model workload.
use kael_ui::prelude::*;
use std::path::PathBuf;
mod desktop_common;

struct Explorer {
    files: Entity<FileTreeState>,
    root: PathBuf,
    writable: bool,
    hidden: bool,
    status: String,
    _events: Subscription,
    _changes: Subscription,
    move_task: Option<Task<()>>,
}
impl Explorer {
    fn new(root: PathBuf, writable: bool, stress: bool, cx: &mut Context<Self>) -> Self {
        let files = cx.new(|cx| {
            if stress {
                FileTreeState::new(
                    vec![FileTreeEntry::new(&root, FileNodeKind::Directory)],
                    |request| {
                        Ok((0..100_000)
                            .map(|index| {
                                FileTreeEntry::new(
                                    request.path.join(format!("file-{index:06}.rs")),
                                    FileNodeKind::File,
                                )
                                .with_size(index)
                            })
                            .collect())
                    },
                    cx,
                )
                .unwrap()
            } else {
                FileTreeState::filesystem(&root, cx).expect("demo root is readable")
            }
        });
        let events = cx.subscribe(&files, |view, _, event: &FileTreeEvent, cx| {
            match event {
                FileTreeEvent::Selected(path) => {
                    view.status = format!("Selected {}", path.display())
                }
                FileTreeEvent::Opened(path) => {
                    view.status = format!("Open requested: {}", path.display())
                }
                FileTreeEvent::ContextAction { path, action } => {
                    if action.as_ref() == "copy-path" {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            path.display().to_string(),
                        ));
                        view.status = "Path copied".into();
                    }
                }
                FileTreeEvent::DropRequested(request) if view.writable => {
                    let source = request.source.clone();
                    let target_parent = request.target_directory.clone();
                    let source_parent = source.parent().unwrap().to_path_buf();
                    let target = target_parent.join(source.file_name().unwrap());
                    let root = view.root.clone();
                    let files = view.files.downgrade();
                    let background = cx.background_executor().clone();
                    view.status = format!("Moving {}…", source.display());
                    view.move_task = Some(cx.spawn(async move |view, cx| {
                        let result = background
                            .spawn(async move {
                                if !source.starts_with(&root) || !target.starts_with(&root) {
                                    return Err("move escapes demo workspace".to_string());
                                }
                                if target.try_exists().map_err(|error| error.to_string())? {
                                    return Err("destination already exists".into());
                                }
                                std::fs::rename(source, target).map_err(|error| error.to_string())
                            })
                            .await;
                        let _ = files.update(cx, |files, cx| {
                            files.reload(&source_parent, cx);
                            files.reload(&target_parent, cx);
                        });
                        let _ = view.update(cx, |view, cx| {
                            view.status = match result {
                                Ok(()) => "Moved; both directories refreshed".into(),
                                Err(error) => format!("Move failed: {error}"),
                            };
                            cx.notify();
                        });
                    }));
                }
                FileTreeEvent::DropRequested(_) => {
                    view.status =
                        "Read-only directory: move request received, no files changed".into()
                }
                FileTreeEvent::LoadFailed { message, .. } => {
                    view.status = format!("Load failed: {message}")
                }
                _ => {}
            }
            cx.notify();
        });
        let changes = cx.observe(&files, |_, _, cx| cx.notify());
        files.update(cx, |files, cx| files.set_expanded(&root, true, cx));
        Self { files, root, writable, hidden: false, status: "Expand folders to load them. Drag a file into a folder, or use Pick up / Move here in the menu.".into(), _events: events, _changes: changes, move_task: None }
    }
}
impl Render for Explorer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let refresh = self.files.downgrade();
        let root = self.root.clone();
        let evict = self.files.downgrade();
        let evict_root = self.root.clone();
        let hidden = cx.entity().downgrade();
        let theme = Theme::of(cx);
        div()
            .size_full()
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Filesystem explorer"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("evict", "Evict cache")
                                    .variant(ButtonVariant::Ghost)
                                    .on_click(move |_, _, cx| {
                                        let _ = evict.update(cx, |files, cx| {
                                            files.evict_directory(&evict_root, cx)
                                        });
                                    }),
                            )
                            .child(Button::new("refresh", "Reload").on_click(move |_, _, cx| {
                                let _ = refresh.update(cx, |files, cx| files.reload(&root, cx));
                            })),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.tokens.muted_foreground)
                    .child(format!(
                        "{} · {} · {} entries · {} pending · worker {:.2}ms · apply {:.2}µs",
                        self.root.display(),
                        if self.writable {
                            "demo workspace, moves enabled"
                        } else {
                            "read only"
                        },
                        self.files.read(cx).cached_entry_count(),
                        self.files.read(cx).loading_count(),
                        self.files
                            .read(cx)
                            .last_model_prepare_duration()
                            .as_secs_f64()
                            * 1000.0,
                        self.files
                            .read(cx)
                            .last_model_apply_duration()
                            .as_secs_f64()
                            * 1_000_000.0,
                    )),
            )
            .child(
                Checkbox::new("show-hidden")
                    .label("Show hidden files")
                    .checked(self.hidden)
                    .on_click(move |value, _, cx| {
                        let _ = hidden.update(cx, |view, cx| {
                            view.hidden = *value;
                            view.files
                                .update(cx, |files, cx| files.set_show_hidden(*value, cx));
                            cx.notify();
                        });
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .border_1()
                    .border_color(theme.tokens.border)
                    .rounded(theme.tokens.radius_md)
                    .overflow_hidden()
                    .child(
                        VirtualFileTree::new("filesystem", self.files.clone())
                            .show_file_size(true)
                            .context_actions(vec![FileTreeContextAction::new(
                                "copy-path",
                                "Copy path",
                            )])
                            .drag_drop(self.writable),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.tokens.muted_foreground)
                    .child(self.status.clone()),
            )
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (root, writable, stress) = match std::env::args_os().nth(1) {
        Some(path) if path == "--stress" => (PathBuf::from("/benchmark"), false, true),
        Some(path) => (std::fs::canonicalize(path)?, false, false),
        None => {
            let root =
                std::env::temp_dir().join(format!("kael-explorer-demo-{}", std::process::id()));
            std::fs::create_dir_all(root.join("src"))?;
            std::fs::create_dir_all(root.join("archive"))?;
            std::fs::write(root.join("src/main.rs"), b"fn main() {}\n")?;
            std::fs::write(root.join("readme.md"), b"Drag this file into archive.\n")?;
            std::fs::write(root.join(".hidden"), b"Hidden fixture\n")?;
            (root, true, false)
        }
    };
    Application::try_new()?.run(move |cx| {
        kael_ui::init(cx);
        desktop_common::install(cx, "Kael filesystem explorer");
        install_theme(cx, Theme::astryx_neutral());
        if let Err(error) = cx.open_window(WindowOptions::default(), move |window, cx| {
            window.set_window_title("Kael filesystem explorer");
            cx.new(|cx| Explorer::new(root, writable, stress, cx))
        }) {
            eprintln!("failed to open explorer: {error}");
            cx.quit();
        }
    });
    Ok(())
}
