use std::collections::HashMap;

use kael::prelude::*;
use kael::*;
use serde::{Deserialize, Serialize};

const APP_ID: &str = "dev.kael.reference-workspace";
const MAIN_WINDOW_ID: &str = "main";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceSettings {
    theme: String,
    font_size: u32,
    show_line_numbers: bool,
    word_wrap: bool,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            font_size: 14,
            show_line_numbers: true,
            word_wrap: true,
        }
    }
}

#[derive(Debug, Clone)]
struct FileNode {
    name: String,
    path: String,
    is_directory: bool,
    children: Vec<FileNode>,
    expanded: bool,
}

#[derive(Debug, Clone)]
struct EditorTab {
    id: String,
    path: String,
    title: String,
    content: String,
    modified: bool,
}

#[derive(Debug, Clone)]
struct TerminalLine {
    text: String,
    is_error: bool,
}

struct WorkspaceApp {
    file_tree: Vec<FileNode>,
    file_contents: HashMap<String, String>,
    tabs: Vec<EditorTab>,
    active_tab: Option<String>,
    terminal_lines: Vec<TerminalLine>,
    settings: SettingsStore<WorkspaceSettings>,
    undo_manager: UndoRedoManager,
    undo_history: Vec<InsertTextChange>,
    redo_history: Vec<InsertTextChange>,
    status_line: String,
}

impl WorkspaceApp {
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

    fn theme_panel_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xf1f5f9),
            _ => rgb(0x1e293b),
        }
    }

    fn theme_tab_bg(&self) -> Rgba {
        match self.settings.data().theme.as_str() {
            "light" => rgb(0xe2e8f0),
            _ => rgb(0x334155),
        }
    }

    fn active_tab(&self) -> Option<&EditorTab> {
        let active = self.active_tab.as_deref()?;
        self.tabs.iter().find(|tab| tab.id == active)
    }

    fn push_terminal(&mut self, text: impl Into<String>, is_error: bool) {
        self.terminal_lines.push(TerminalLine {
            text: text.into(),
            is_error,
        });
        if self.terminal_lines.len() > 10 {
            let excess = self.terminal_lines.len() - 10;
            self.terminal_lines.drain(0..excess);
        }
    }

    fn insert_text(&mut self, tab_id: &str, text: &str) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.content.push_str(text);
            tab.modified = true;
        }
    }

    fn remove_text(&mut self, tab_id: &str, text: &str) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id)
            && tab.content.ends_with(text)
        {
            let new_len = tab.content.len().saturating_sub(text.len());
            tab.content.truncate(new_len);
            tab.modified = true;
        }
    }

    fn select_tab(&mut self, id: &str) {
        self.active_tab = Some(id.to_string());
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
            self.status_line = format!("Focused {}", tab.title);
        }
    }

    fn close_tab(&mut self, id: &str) {
        let closed_title = self
            .tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| tab.title.clone())
            .unwrap_or_else(|| "file".to_string());
        self.tabs.retain(|t| t.id != id);
        if self.active_tab.as_deref() == Some(id) {
            self.active_tab = self.tabs.first().map(|t| t.id.clone());
        }
        self.status_line = format!("Closed {}", closed_title);
        self.push_terminal(format!("Closed tab {}", closed_title), false);
    }

    fn toggle_folder(&mut self, path: &str) {
        fn toggle_in(nodes: &mut Vec<FileNode>, target: &str) -> bool {
            for node in nodes {
                if node.path == target {
                    node.expanded = !node.expanded;
                    return true;
                }
                if toggle_in(&mut node.children, target) {
                    return true;
                }
            }
            false
        }
        toggle_in(&mut self.file_tree, path);
    }

    fn open_path(&mut self, path: &str) {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.path == path) {
            self.active_tab = Some(tab.id.clone());
            self.status_line = format!("Focused {}", tab.title);
            return;
        }

        if let Some(content) = self.file_contents.get(path).cloned() {
            let title = path.rsplit('/').next().unwrap_or(path).to_string();
            let tab_id = format!("tab-{}", self.tabs.len() + 1);
            self.tabs.push(EditorTab {
                id: tab_id.clone(),
                path: path.to_string(),
                title: title.clone(),
                content,
                modified: false,
            });
            self.active_tab = Some(tab_id);
            self.status_line = format!("Opened {}", title);
            self.push_terminal(format!("Opened {}", path), false);
        }
    }

    fn insert_snippet(&mut self) {
        let Some(active_tab_id) = self.active_tab.clone() else {
            self.status_line = "Open a file before inserting a snippet.".to_string();
            return;
        };

        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| tab.id == active_tab_id)
            .cloned()
        else {
            self.status_line = "The active tab is no longer available.".to_string();
            return;
        };

        let snippet = if tab.title.ends_with(".rs") {
            "\nfn generated_helper() {\n    println!(\"workspace action executed\");\n}\n"
        } else if tab.title.ends_with(".md") {
            "\n- [ ] Review the latest GPUI runtime changes\n"
        } else {
            "\n# generated change\n"
        };

        self.insert_text(&active_tab_id, snippet);

        let change = InsertTextChange::new(
            active_tab_id.clone(),
            snippet,
            format!("insert snippet into {}", tab.title),
        );
        self.undo_manager.push(Box::new(change.clone()));
        self.undo_history.push(change);
        self.redo_history.clear();

        self.status_line = format!("Inserted snippet into {}", tab.title);
        self.push_terminal(format!("Applied generated snippet to {}", tab.path), false);
    }

    fn undo_edit(&mut self) {
        let Some(change) = self.undo_history.pop() else {
            self.status_line = "Nothing left to undo.".to_string();
            return;
        };

        self.remove_text(&change.tab_id, &change.text);
        let description = self
            .undo_manager
            .undo()
            .map(|change| change.description().to_string())
            .unwrap_or_else(|| "undo edit".to_string());
        self.redo_history.push(change);
        self.status_line = format!("Undid {}", description);
        self.push_terminal(format!("Undid {}", description), false);
    }

    fn redo_edit(&mut self) {
        let Some(change) = self.redo_history.pop() else {
            self.status_line = "Nothing left to redo.".to_string();
            return;
        };

        self.insert_text(&change.tab_id, &change.text);
        let description = change.description().to_string();
        let _ = self.undo_manager.redo();
        self.undo_history.push(change);
        self.status_line = format!("Reapplied {}", description);
        self.push_terminal(format!("Reapplied {}", description), false);
    }

    fn toggle_word_wrap(&mut self) {
        let next = !self.settings.data().word_wrap;
        let _ = self.settings.update(|settings| settings.word_wrap = next);
        self.status_line = if next {
            "Word wrap enabled.".to_string()
        } else {
            "Word wrap disabled.".to_string()
        };
    }

    fn handle_deep_link(&mut self, url: &str, handled: bool) {
        if handled {
            if let Some(path) = url.strip_prefix("file://") {
                self.open_path(path);
                self.status_line = format!("Deep link opened {}", path);
            }
        } else {
            self.status_line = format!("No deep link handler registered for {}", url);
        }
        self.push_terminal(format!("deep-link: {}", url), !handled);
    }
}

impl Render for WorkspaceApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme_bg = self.theme_bg();
        let theme_fg = self.theme_fg();
        let sidebar_bg = self.theme_sidebar_bg();
        let panel_bg = self.theme_panel_bg();
        let tab_bg = self.theme_tab_bg();
        let active_tab_id = self.active_tab.clone();
        let file_tree = self.file_tree.clone();
        let tabs = self.tabs.clone();
        let terminal_lines = self.terminal_lines.clone();
        let active_tab = self.active_tab().cloned();
        let can_undo = self.undo_manager.can_undo();
        let can_redo = self.undo_manager.can_redo();
        let undo_descriptions = self
            .undo_manager
            .undo_descriptions()
            .into_iter()
            .take(3)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let redo_descriptions = self
            .undo_manager
            .redo_descriptions()
            .into_iter()
            .take(3)
            .map(str::to_string)
            .collect::<Vec<_>>();
        let status_line = self.status_line.clone();
        let word_wrap = self.settings.data().word_wrap;
        let show_line_numbers = self.settings.data().show_line_numbers;
        let entity = cx.entity();
        let tree_entity = entity.clone();
        let tab_entity = entity.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme_bg)
            .text_color(theme_fg)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .h_full()
                    .child(
                        div()
                            .id(ElementId::Name(SharedString::from("file-tree-sidebar")))
                            .flex()
                            .flex_col()
                            .w(px(240.0))
                            .h_full()
                            .bg(sidebar_bg)
                            .border_r_1()
                            .border_color(rgb(0x334155))
                            .child(
                                div()
                                    .p_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Explorer")
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                            .label("File explorer header"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Tree)
                                            .label("File tree"),
                                    )
                                    .children(file_tree.into_iter().map(|node| {
                                        render_file_node(node, 0, tree_entity.clone())
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
                                    .flex()
                                    .flex_row()
                                    .bg(panel_bg)
                                    .justify_between()
                                    .items_center()
                                    .border_b_1()
                                    .border_color(rgb(0x334155))
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::TabPanel)
                                            .label("Editor tabs"),
                                    )
                                    .child(div().flex().flex_row().children(tabs.into_iter().map(
                                        move |tab| {
                                            let is_active =
                                                active_tab_id.as_deref() == Some(tab.id.as_str());
                                            let id = tab.id.clone();
                                            let title = tab.title.clone();
                                            let modified = tab.modified;
                                            div()
                                                .id(ElementId::Name(SharedString::from(format!(
                                                    "tab-{}",
                                                    id
                                                ))))
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap_2()
                                                .px_3()
                                                .py_2()
                                                .cursor_pointer()
                                                .map(
                                                    |this| {
                                                        if is_active {
                                                            this.bg(tab_bg)
                                                        } else {
                                                            this
                                                        }
                                                    },
                                                )
                                                .child(if modified {
                                                    format!("{} *", title)
                                                } else {
                                                    title.clone()
                                                })
                                                .child(
                                                    div()
                                                        .id(ElementId::Name(SharedString::from(
                                                            format!("close-tab-{}", id),
                                                        )))
                                                        .text_xs()
                                                        .text_color(rgb(0x94a3b8))
                                                        .cursor_pointer()
                                                        .on_click({
                                                            let entity = tab_entity.clone();
                                                            let close_id = id.clone();
                                                            move |_, _, cx| {
                                                                cx.update_entity(
                                                                    &entity,
                                                                    |app, cx| {
                                                                        app.close_tab(&close_id);
                                                                        cx.notify();
                                                                    },
                                                                );
                                                            }
                                                        })
                                                        .child("x"),
                                                )
                                                .on_click({
                                                    let entity = tab_entity.clone();
                                                    let id = id.clone();
                                                    move |_, _, cx| {
                                                        cx.update_entity(&entity, |app, cx| {
                                                            app.select_tab(&id);
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .accessibility(
                                                    AccessibilityAttributes::new(
                                                        AccessibilityRole::Tab,
                                                    )
                                                    .label(title),
                                                )
                                        },
                                    )))
                                    .child(
                                        div()
                                            .flex()
                                            .gap_2()
                                            .px_2()
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(
                                                        "insert-snippet-button",
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
                                                                app.insert_snippet();
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child("Insert Snippet"),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(
                                                        "undo-button",
                                                    )))
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(if can_undo {
                                                        rgb(0x334155)
                                                    } else {
                                                        rgb(0x1e293b)
                                                    })
                                                    .text_xs()
                                                    .cursor_pointer()
                                                    .on_click({
                                                        let entity = entity.clone();
                                                        move |_, _, cx| {
                                                            cx.update_entity(&entity, |app, cx| {
                                                                app.undo_edit();
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child("Undo"),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(
                                                        "redo-button",
                                                    )))
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(if can_redo {
                                                        rgb(0x334155)
                                                    } else {
                                                        rgb(0x1e293b)
                                                    })
                                                    .text_xs()
                                                    .cursor_pointer()
                                                    .on_click({
                                                        let entity = entity.clone();
                                                        move |_, _, cx| {
                                                            cx.update_entity(&entity, |app, cx| {
                                                                app.redo_edit();
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child("Redo"),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(
                                                        "open-deep-link-button",
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
                                                            let url =
                                                                "file://src/lib.rs".to_string();
                                                            let handled = cx
                                                                .try_global::<WorkspaceRuntime>()
                                                                .map(|runtime| {
                                                                    runtime
                                                                        .deep_link_registry
                                                                        .handle(&url)
                                                                })
                                                                .unwrap_or(false);
                                                            cx.update_entity(&entity, |app, cx| {
                                                                app.handle_deep_link(&url, handled);
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child("Open Deep Link"),
                                            )
                                            .child(
                                                div()
                                                    .id(ElementId::Name(SharedString::from(
                                                        "toggle-wrap-button",
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
                                                                app.toggle_word_wrap();
                                                                cx.notify();
                                                            });
                                                        }
                                                    })
                                                    .child(if word_wrap {
                                                        "Wrap On"
                                                    } else {
                                                        "Wrap Off"
                                                    }),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .p_4()
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Group)
                                            .label("Editor content"),
                                    )
                                    .child(
                                        active_tab
                                            .as_ref()
                                            .map(|tab| {
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(rgb(0x94a3b8))
                                                            .child(format!(
                                                                "{}  •  {}",
                                                                tab.path,
                                                                if show_line_numbers {
                                                                    "line numbers on"
                                                                } else {
                                                                    "line numbers off"
                                                                }
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .font_family("monospace")
                                                            .child(tab.content.clone()),
                                                    )
                                            })
                                            .unwrap_or_else(|| div().child("No file open")),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id(ElementId::Name(SharedString::from("properties-panel")))
                            .flex()
                            .flex_col()
                            .w(px(240.0))
                            .h_full()
                            .bg(sidebar_bg)
                            .border_l_1()
                            .border_color(rgb(0x334155))
                            .child(
                                div()
                                    .p_2()
                                    .font_weight(FontWeight::BOLD)
                                    .child("Properties")
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                            .label("Properties panel header"),
                                    ),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x94a3b8))
                                            .child(status_line),
                                    )
                                    .child(
                                        active_tab
                                            .as_ref()
                                            .map(|tab| {
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap_1()
                                                    .child(format!("File: {}", tab.path))
                                                    .child(format!(
                                                        "Modified: {}",
                                                        if tab.modified { "yes" } else { "no" }
                                                    ))
                                                    .child(format!(
                                                        "Wrap: {}",
                                                        if word_wrap {
                                                            "enabled"
                                                        } else {
                                                            "disabled"
                                                        }
                                                    ))
                                            })
                                            .unwrap_or_else(|| div().child("No file selected")),
                                    )
                                    .child(
                                        div()
                                            .pt_2()
                                            .font_weight(FontWeight::BOLD)
                                            .child("Undo Stack"),
                                    )
                                    .children(undo_descriptions.into_iter().map(|description| {
                                        div().text_sm().text_color(rgb(0x94a3b8)).child(description)
                                    }))
                                    .child(
                                        div()
                                            .pt_2()
                                            .font_weight(FontWeight::BOLD)
                                            .child("Redo Stack"),
                                    )
                                    .children(redo_descriptions.into_iter().map(|description| {
                                        div().text_sm().text_color(rgb(0x94a3b8)).child(description)
                                    }))
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::StaticText)
                                            .label("No properties selected"),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .id(ElementId::Name(SharedString::from("terminal-panel")))
                    .flex()
                    .flex_col()
                    .h(px(180.0))
                    .bg(panel_bg)
                    .border_t_1()
                    .border_color(rgb(0x334155))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .bg(rgb(0x334155))
                            .child("Terminal / Output")
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Toolbar)
                                    .label("Terminal panel header"),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .p_2()
                            .font_family("monospace")
                            .accessibility(
                                AccessibilityAttributes::new(AccessibilityRole::Group)
                                    .label("Terminal output"),
                            )
                            .children(terminal_lines.into_iter().map(|line| {
                                let color = if line.is_error {
                                    rgb(0xef4444)
                                } else {
                                    theme_fg
                                };
                                div()
                                    .text_color(color)
                                    .child(line.text.clone())
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::StaticText)
                                            .label(line.text.clone()),
                                    )
                            })),
                    ),
            )
    }
}

fn render_file_node(
    node: FileNode,
    depth: usize,
    entity: Entity<WorkspaceApp>,
) -> impl IntoElement {
    let indent = px(depth as f32 * 12.0);
    let has_children = !node.children.is_empty();
    let expanded = node.expanded;
    let path = node.path.clone();
    let is_directory = node.is_directory;

    div()
        .id(ElementId::Name(SharedString::from(format!(
            "file-{}",
            path.replace(['/', '.'], "-")
        ))))
        .flex()
        .flex_col()
        .child(
            div()
                .id(ElementId::Name(SharedString::from(format!(
                    "file-node-{}",
                    path.replace(['/', '.'], "-")
                ))))
                .flex()
                .flex_row()
                .items_center()
                .pl(indent)
                .px_2()
                .py_1()
                .cursor_pointer()
                .on_click({
                    let entity = entity.clone();
                    let path = path.clone();
                    move |_, _, cx| {
                        cx.update_entity(&entity, |app, cx| {
                            if is_directory {
                                app.toggle_folder(&path);
                            } else {
                                app.open_path(&path);
                            }
                            cx.notify();
                        });
                    }
                })
                .accessibility(
                    AccessibilityAttributes::new(AccessibilityRole::TreeItem)
                        .label(node.name.clone()),
                )
                .child(div().w(px(16.0)).child(if has_children {
                    if expanded {
                        "▼ ".to_string()
                    } else {
                        "▶ ".to_string()
                    }
                } else {
                    "  ".to_string()
                }))
                .child(if is_directory { "📁 " } else { "📄 " })
                .child(node.name),
        )
        .children(node.children.into_iter().filter_map(move |child| {
            if expanded {
                Some(render_file_node(child, depth + 1, entity.clone()))
            } else {
                None
            }
        }))
}

struct WorkspaceRuntime {
    session_store: SessionStore,
    _crash_reporter: CrashReporter,
    tracer: Tracer,
    _quit_subscription: Subscription,
    last_window_state: Option<WindowState>,
    deep_link_registry: DeepLinkRegistry,
}

impl Global for WorkspaceRuntime {}

fn main() {
    let app = Application::new();
    app.on_open_urls(|urls| {
        for url in urls {
            println!("Deep link received: {}", url);
        }
    });
    app.run(|cx: &mut App| {
        if let Err(error) = bootstrap(cx) {
            eprintln!("failed to initialize workspace app: {error:#}");
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
    tracer.record("workspace.startup", "lifecycle", TracePhase::Instant);

    let settings_path = session_store.storage_dir().join("settings.json");
    let settings: SettingsStore<WorkspaceSettings> = SettingsStore::load(&settings_path, &[])
        .unwrap_or_else(|_| SettingsStore::new(&settings_path));

    let workspace = cx.open_workspace();
    workspace.add_panel(Box::new(SidebarPanel::new("explorer", "Explorer")));
    workspace.add_panel(Box::new(InspectorPanel::new("properties", "Properties")));
    workspace.add_panel(Box::new(BottomPanel::new("terminal", "Terminal")));

    cx.register_command("new-file", "New File", || {
        println!("New file command triggered");
    });
    cx.register_command("save-file", "Save File", || {
        println!("Save file command triggered");
    });
    cx.register_command("undo", "Undo", || {
        println!("Undo command triggered");
    });
    cx.register_command("redo", "Redo", || {
        println!("Redo command triggered");
    });
    cx.register_command("find-in-files", "Find in Files", || {
        println!("Find in files command triggered");
    });

    let restored_states = session_store.load_window_states().unwrap_or_default();
    let restored_state = restored_states.get(MAIN_WINDOW_ID).cloned();

    let quit_subscription = cx.on_app_quit(|cx| {
        if let Some(runtime) = cx.try_global::<WorkspaceRuntime>() {
            let mut states = HashMap::new();
            if let Some(state) = &runtime.last_window_state {
                states.insert(MAIN_WINDOW_ID.to_string(), state.clone());
            }
            let _ = runtime.session_store.save_window_states(&states);
            let _ = runtime.tracer.write_to_file(
                runtime
                    .session_store
                    .storage_dir()
                    .join("workspace_trace.json"),
            );
        }
        async {}
    });

    let file_tree = vec![
        FileNode {
            name: "src".to_string(),
            path: "src".to_string(),
            is_directory: true,
            expanded: true,
            children: vec![
                FileNode {
                    name: "main.rs".to_string(),
                    path: "src/main.rs".to_string(),
                    is_directory: false,
                    expanded: false,
                    children: vec![],
                },
                FileNode {
                    name: "lib.rs".to_string(),
                    path: "src/lib.rs".to_string(),
                    is_directory: false,
                    expanded: false,
                    children: vec![],
                },
            ],
        },
        FileNode {
            name: "Cargo.toml".to_string(),
            path: "Cargo.toml".to_string(),
            is_directory: false,
            expanded: false,
            children: vec![],
        },
        FileNode {
            name: "README.md".to_string(),
            path: "README.md".to_string(),
            is_directory: false,
            expanded: false,
            children: vec![],
        },
    ];

    let file_contents = HashMap::from([
        (
            "src/main.rs".to_string(),
            "fn main() {\n    println!(\"Hello, world!\");\n}\n".to_string(),
        ),
        (
            "src/lib.rs".to_string(),
            "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n".to_string(),
        ),
        (
            "Cargo.toml".to_string(),
            "[package]\nname = \"reference-workspace\"\nversion = \"0.1.0\"\n".to_string(),
        ),
        (
            "README.md".to_string(),
            "# Reference Workspace\n\nA GPUI workspace shell with panels and commands.\n"
                .to_string(),
        ),
    ]);

    let tabs = vec![
        EditorTab {
            id: "t1".to_string(),
            path: "src/main.rs".to_string(),
            title: "main.rs".to_string(),
            content: file_contents["src/main.rs"].clone(),
            modified: false,
        },
        EditorTab {
            id: "t2".to_string(),
            path: "src/lib.rs".to_string(),
            title: "lib.rs".to_string(),
            content: file_contents["src/lib.rs"].clone(),
            modified: true,
        },
    ];

    let terminal_lines = vec![
        TerminalLine {
            text: "$ cargo build".to_string(),
            is_error: false,
        },
        TerminalLine {
            text: "   Compiling reference-workspace v0.1.0".to_string(),
            is_error: false,
        },
        TerminalLine {
            text: "    Finished dev [unoptimized + debuginfo] target(s) in 1.23s".to_string(),
            is_error: false,
        },
        TerminalLine {
            text: "$ ".to_string(),
            is_error: false,
        },
    ];

    let workspace_app = WorkspaceApp {
        file_tree,
        file_contents,
        tabs,
        active_tab: Some("t1".to_string()),
        terminal_lines,
        settings,
        undo_manager: UndoRedoManager::new(100),
        undo_history: Vec::new(),
        redo_history: Vec::new(),
        status_line: "Workspace ready with file explorer, commands, and deep-link simulation."
            .to_string(),
    };

    let window_bounds = restored_state
        .as_ref()
        .map(|state| state.bounds)
        .or_else(|| {
            Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1200.0), px(800.0)),
                cx,
            )))
        });

    let mut deep_link_registry = DeepLinkRegistry::new();
    deep_link_registry.register(Box::new(FileDeepLinkHandler));

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
                    cx.update_global(|runtime: &mut WorkspaceRuntime, _| {
                        runtime.last_window_state = Some(window.window_state());
                    });
                })
                .detach();

                workspace_app
            });

            let runtime = WorkspaceRuntime {
                session_store,
                _crash_reporter: crash_reporter,
                tracer,
                _quit_subscription: quit_subscription,
                last_window_state: restored_state.clone(),
                deep_link_registry,
            };
            cx.set_global(runtime);

            entity
        },
    )?;

    cx.activate(true);
    Ok(())
}

struct FileDeepLinkHandler;

impl DeepLinkHandler for FileDeepLinkHandler {
    fn scheme(&self) -> &str {
        "file"
    }

    fn handle(&self, url: &str) {
        println!("Opening file from deep link: {}", url);
    }
}

#[derive(Debug, Clone)]
struct InsertTextChange {
    tab_id: String,
    text: String,
    description: String,
    applied: bool,
}

impl InsertTextChange {
    fn new(
        tab_id: impl Into<String>,
        text: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            tab_id: tab_id.into(),
            text: text.into(),
            description: description.into(),
            applied: false,
        }
    }
}

impl UndoableChange for InsertTextChange {
    fn apply(&mut self) {
        self.applied = true;
    }

    fn revert(&mut self) {
        self.applied = false;
    }

    fn description(&self) -> &str {
        &self.description
    }
}
