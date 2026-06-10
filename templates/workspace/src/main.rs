//! Workspace/IDE template: file tree, tabbed code editor with syntax
//! highlighting, and a status bar — a VS Code-style shell built on kael_ui.

use kael_ui::components::editor::{Editor, EditorState, Language};
use kael_ui::navigation::status_bar::{StatusBar, StatusItem};
use kael_ui::navigation::toolbar::{Toolbar, ToolbarButton, ToolbarGroup};
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

const SAMPLE_RS: &str = r#"use kael_ui::prelude::*;

fn main() {
    Application::new().run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::custom(ThemeTokens {
            primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
            ..ThemeTokens::dark()
        }));

        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| HelloView)
        })
        .unwrap();
    });
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

fn main() {
    Application::new()
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kael_ui"),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, Theme::tokyo_night());

            cx.open_window(
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
            )
            .unwrap();
            cx.activate(true);
        });
}

struct WorkspaceApp {
    editor: Entity<EditorState>,
    toolbar: Entity<Toolbar>,
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

        let toolbar = cx.new(|_| {
            Toolbar::new()
                .group(
                    ToolbarGroup::new()
                        .button(ToolbarButton::new("save", "save").tooltip("Save"))
                        .button(ToolbarButton::new("undo", "undo").tooltip("Undo"))
                        .button(ToolbarButton::new("redo", "redo").tooltip("Redo")),
                )
                .group(
                    ToolbarGroup::new()
                        .button(ToolbarButton::new("run", "play").tooltip("Run"))
                        .button(
                            ToolbarButton::new("terminal", "terminal").tooltip("Toggle terminal"),
                        ),
                )
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
            toolbar,
            status_bar,
            selected_file: PathBuf::from("/project/src/main.rs"),
        }
    }

    fn file_tree(&self, cx: &mut Context<Self>) -> impl IntoElement {
        FileTree::new()
            .nodes(vec![FileNode::directory("/project")
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
                ])])
            .expanded_paths(vec![
                PathBuf::from("/project"),
                PathBuf::from("/project/src"),
            ])
            .selected_path(self.selected_file.clone())
            .on_select(cx.listener(|view, path: &PathBuf, _, cx| {
                view.selected_file = path.clone();
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
                            .child("main.rs"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(tokens.muted_foreground)
                            .child("●"),
                    ),
            )
            .child(
                HStack::new().items_center().px(px(14.0)).py(px(8.0)).child(
                    div()
                        .text_size(px(12.5))
                        .text_color(tokens.muted_foreground)
                        .child("theme.rs"),
                ),
            )
            .child(
                HStack::new().items_center().px(px(14.0)).py(px(8.0)).child(
                    div()
                        .text_size(px(12.5))
                        .text_color(tokens.muted_foreground)
                        .child("Cargo.toml"),
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
                    .child(self.toolbar.clone()),
            )
            .child(tab_strip)
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .p(px(8.0))
                    .child(Editor::new(&self.editor).h_full().w_full()),
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
