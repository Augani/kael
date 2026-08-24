//! Workspace/IDE template: file tree, tabbed code editor with syntax
//! highlighting, and a status bar — a VS Code-style shell built on kael_ui.

use kael_ui::components::editor::{Editor, EditorState, Language, Redo, Undo};
use kael_ui::navigation::status_bar::{StatusBar, StatusItem};
use kael_ui::navigation::toolbar::{Toolbar, ToolbarButton, ToolbarGroup};
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
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(Into::into)
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

const SAMPLE_RS: &str = r#"use kael_ui::prelude::*;

fn main() -> Result<()> {
    Application::try_new()?.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::custom(ThemeTokens {
            primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
            ..ThemeTokens::dark()
        }));

        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| HelloView)
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}

struct HelloView;

impl Render for HelloView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(Button::new("hi", "Hello, Kael!"))
    }
}
"#;

const THEME_RS: &str = r#"use kael_ui::prelude::*;

pub fn product_theme() -> Theme {
    Theme::custom(ThemeTokens {
        primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
        ..ThemeTokens::dark()
    })
}
"#;

const VIEWS_RS: &str = r#"use kael_ui::prelude::*;

pub struct WelcomeView;

impl Render for WelcomeView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Welcome to Kael")
    }
}
"#;

const SAMPLE_CARGO_TOML: &str = r#"[package]
name = "kael-app"
version = "0.1.0"
edition = "2024"

[dependencies]
kael = "0.4"
kael_ui = "0.4"
"#;

const SAMPLE_README: &str = "# Kael App\n\nA desktop workspace built with Kael.\n";
const SAMPLE_SVG: &str = r##"<svg viewBox="0 0 32 32" aria-label="Kael logo">
  <circle cx="16" cy="16" r="14" fill="#7c3aed" />
</svg>
"##;

fn file_content(path: &Path) -> Option<(&'static str, Language)> {
    match path.to_str()? {
        "/project/src/main.rs" => Some((SAMPLE_RS, Language::Rust)),
        "/project/src/theme.rs" => Some((THEME_RS, Language::Rust)),
        "/project/src/views.rs" => Some((VIEWS_RS, Language::Rust)),
        "/project/Cargo.toml" => Some((SAMPLE_CARGO_TOML, Language::Toml)),
        "/project/README.md" => Some((SAMPLE_README, Language::Markdown)),
        "/project/assets/logo.svg" => Some((SAMPLE_SVG, Language::Html)),
        _ => None,
    }
}

fn main() -> Result<()> {
    Application::try_new()?
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kael_ui"),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::tokyo_night());

            if let Err(error) = cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Kael Workspace".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1320.0), px(860.0)),
                    })),
                    ..Default::default()
                },
                |_, cx| cx.new(WorkspaceApp::new),
            ) {
                eprintln!("failed to open the workspace window: {error}");
                cx.quit();
                return;
            }
            cx.activate(true);
        });
    Ok(())
}

struct WorkspaceApp {
    editor: Entity<EditorState>,
    status_bar: Entity<StatusBar>,
    selected_file: PathBuf,
}

impl WorkspaceApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(cx);
            state.set_content(SAMPLE_RS, cx);
            state.set_language(Language::Rust);
            state
        });

        let status_bar = cx.new(|_| {
            StatusBar::new()
                .left(vec![
                    StatusItem::icon_text("git-branch", "feat/ui-customization"),
                    StatusItem::text("0 errors"),
                ])
                .center(vec![StatusItem::text("Ready")])
                .right(vec![
                    StatusItem::text("Rust"),
                    StatusItem::text("UTF-8"),
                    StatusItem::text("Ln 12, Col 1"),
                ])
        });

        Self {
            editor,
            status_bar,
            selected_file: PathBuf::from("/project/src/main.rs"),
        }
    }

    fn toolbar(&self, cx: &App) -> Toolbar {
        let undo_editor = self.editor.clone();
        let redo_editor = self.editor.clone();
        let (can_undo, can_redo) = {
            let editor = self.editor.read(cx);
            (editor.undo_depth() > 0, editor.redo_depth() > 0)
        };
        Toolbar::new()
            .group(
                ToolbarGroup::new()
                    .button(
                        ToolbarButton::new("save", "save")
                            .tooltip("Save is unavailable in this in-memory demo")
                            .disabled(true),
                    )
                    .button(
                        ToolbarButton::new("undo", "undo")
                            .tooltip("Undo")
                            .disabled(!can_undo)
                            .on_click(move |window, cx| {
                                undo_editor.update(cx, |editor, cx| {
                                    editor.undo(&Undo, window, cx);
                                });
                            }),
                    )
                    .button(
                        ToolbarButton::new("redo", "redo")
                            .tooltip("Redo")
                            .disabled(!can_redo)
                            .on_click(move |window, cx| {
                                redo_editor.update(cx, |editor, cx| {
                                    editor.redo(&Redo, window, cx);
                                });
                            }),
                    ),
            )
            .group(
                ToolbarGroup::new()
                    .button(
                        ToolbarButton::new("run", "play")
                            .tooltip("Run is unavailable in this in-memory demo")
                            .disabled(true),
                    )
                    .button(
                        ToolbarButton::new("terminal", "terminal")
                            .tooltip("Terminal is unavailable in this template")
                            .disabled(true),
                    ),
            )
    }

    fn file_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.clone();
        let status_bar = self.status_bar.clone();
        FileTree::new()
            .nodes(vec![
                FileNode::directory("/project")
                    .with_name("kael-app")
                    .with_children(vec![
                        FileNode::directory("/project/src").with_children(vec![
                            FileNode::file("/project/src/main.rs"),
                            FileNode::file("/project/src/theme.rs"),
                            FileNode::file("/project/src/views.rs"),
                        ]),
                        FileNode::directory("/project/assets")
                            .with_children(vec![FileNode::file("/project/assets/logo.svg")]),
                        FileNode::file("/project/Cargo.toml"),
                        FileNode::file("/project/README.md"),
                    ]),
            ])
            .expanded_paths(vec![
                PathBuf::from("/project"),
                PathBuf::from("/project/src"),
            ])
            .selected_path(self.selected_file.clone())
            .on_select(cx.listener(move |view, path: &PathBuf, _, cx| {
                view.selected_file = path.clone();
                if let Some((content, language)) = file_content(path) {
                    editor.update(cx, |editor, cx| {
                        editor.set_content(content, cx);
                        editor.set_language(language);
                    });
                    status_bar.update(cx, |status_bar, _| {
                        *status_bar = StatusBar::new()
                            .left(vec![
                                StatusItem::icon_text("git-branch", "feat/ui-customization"),
                                StatusItem::text("0 errors"),
                            ])
                            .center(vec![StatusItem::text("Ready")])
                            .right(vec![
                                StatusItem::text(language.to_text()),
                                StatusItem::text("UTF-8"),
                                StatusItem::text("Ln 1, Col 1"),
                            ]);
                    });
                }
                cx.notify();
            }))
    }
}

impl Render for WorkspaceApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let tokens = theme.tokens.clone();

        let explorer = VStack::new()
            .w(px(260.0))
            .h_full()
            .border_r_1()
            .border_color(tokens.border)
            .child(
                div()
                    .px(px(16.0))
                    .py(px(12.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tokens.muted_foreground)
                    .child("EXPLORER"),
            )
            .child(div().flex_1().min_h(px(0.0)).px(px(8.0)).child(
                kael_ui::components::scrollable::scrollable_vertical(
                    div().flex().flex_col().child(self.file_tree(cx)),
                ),
            ));

        let selected_name = self
            .selected_file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Workspace");
        let tab_strip = HStack::new()
            .items_center()
            .gap(px(2.0))
            .px(px(8.0))
            .pt(px(6.0))
            .border_b_1()
            .border_color(tokens.border)
            .child(
                HStack::new()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(14.0))
                    .py(px(8.0))
                    .rounded_tl(px(8.0))
                    .rounded_tr(px(8.0))
                    .bg(tokens.card)
                    .border_1()
                    .border_color(tokens.border)
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(selected_name.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(tokens.muted_foreground)
                            .child("●"),
                    ),
            );

        let editor_pane = VStack::new()
            .flex_1()
            .min_w(px(0.0))
            .h_full()
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .border_b_1()
                    .border_color(tokens.border)
                    .child(self.toolbar(cx)),
            )
            .child(tab_strip)
            .child(
                div().flex_1().min_h(px(0.0)).p(px(8.0)).child(
                    Editor::new(&self.editor)
                        .accessibility_label(format!("Editor for {selected_name}"))
                        .h_full()
                        .w_full(),
                ),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(tokens.background)
            .text_color(tokens.foreground)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .child(explorer)
                    .child(editor_pane),
            )
            .child(self.status_bar.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_files_have_content_and_language() {
        let (content, language) = file_content(Path::new("/project/Cargo.toml")).unwrap();
        assert!(content.contains("[package]"));
        assert_eq!(language, Language::Toml);
        assert!(file_content(Path::new("/project/src")).is_none());
    }

    #[test]
    fn asset_paths_are_confined_to_the_asset_root() {
        let assets = Assets {
            base: PathBuf::from("/tmp/assets"),
        };
        assert!(assets.resolve("icons/save.svg").is_ok());
        assert!(assets.resolve("../secret").is_err());
        assert!(assets.resolve("/etc/passwd").is_err());
    }
}
