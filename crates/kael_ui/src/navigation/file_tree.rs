use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::styled_ext::StyledExt;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::collections::HashSet;
use std::panic::Location;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileNodeKind {
    File,
    Directory,
    Symlink,
}

#[derive(Clone, Debug)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub kind: FileNodeKind,
    pub children: Vec<FileNode>,
    pub size: Option<u64>,
    pub modified: Option<String>,
    pub is_hidden: bool,
    pub has_unloaded_children: bool,
}

impl FileNode {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            path,
            name,
            kind: FileNodeKind::File,
            children: Vec::new(),
            size: None,
            modified: None,
            is_hidden: false,
            has_unloaded_children: false,
        }
    }

    pub fn directory(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        Self {
            path,
            name,
            kind: FileNodeKind::Directory,
            children: Vec::new(),
            size: None,
            modified: None,
            is_hidden: false,
            has_unloaded_children: false,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub fn with_children(mut self, children: Vec<FileNode>) -> Self {
        self.children = children
            .into_iter()
            .map(|mut child| {
                if child.path.is_relative() && !child.path.starts_with(&self.path) {
                    let rebased = self.path.join(&child.path);
                    child.rebase_to(rebased);
                }
                child
            })
            .collect();
        self
    }

    fn rebase_to(&mut self, path: PathBuf) {
        let previous = std::mem::replace(&mut self.path, path.clone());
        for child in &mut self.children {
            if child.path.is_absolute() {
                continue;
            }
            let suffix = child
                .path
                .strip_prefix(&previous)
                .map(PathBuf::from)
                .unwrap_or_else(|_| child.path.clone());
            child.rebase_to(path.join(suffix));
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_modified(mut self, modified: impl Into<String>) -> Self {
        self.modified = Some(modified.into());
        self
    }

    pub fn hidden(mut self, is_hidden: bool) -> Self {
        self.is_hidden = is_hidden;
        self
    }

    pub fn with_unloaded_children(mut self, has_unloaded: bool) -> Self {
        self.has_unloaded_children = has_unloaded;
        self
    }

    pub fn is_directory(&self) -> bool {
        self.kind == FileNodeKind::Directory
    }

    pub fn extension(&self) -> Option<&str> {
        self.path.extension().and_then(|e| e.to_str())
    }

    pub fn file_icon(&self, is_expanded: bool) -> IconSource {
        match self.kind {
            FileNodeKind::Directory => {
                if is_expanded {
                    IconSource::Named("folder-open".into())
                } else {
                    IconSource::Named("folder".into())
                }
            }
            FileNodeKind::Symlink => IconSource::Named("link".into()),
            FileNodeKind::File => match self.extension() {
                Some("json") | Some("yaml") | Some("yml") | Some("toml") | Some("xml") => {
                    IconSource::Named("file-json".into())
                }
                Some("md") | Some("txt") | Some("doc") | Some("docx") | Some("pdf") => {
                    IconSource::Named("file-text".into())
                }
                Some("sh") | Some("bash") | Some("zsh") => IconSource::Named("hash".into()),
                Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("svg")
                | Some("ico") | Some("webp") => IconSource::Named("image".into()),
                Some("mp3") | Some("wav") | Some("ogg") | Some("flac") => {
                    IconSource::Named("music".into())
                }
                Some("mp4") | Some("mov") | Some("avi") | Some("webm") => {
                    IconSource::Named("video".into())
                }
                Some("zip") | Some("tar") | Some("gz") | Some("rar") | Some("7z") => {
                    IconSource::Named("archive".into())
                }
                _ => IconSource::Named("file-code".into()),
            },
        }
    }

    pub fn file_icon_color(&self, theme: &crate::theme::Theme) -> Hsla {
        match self.kind {
            FileNodeKind::Directory => rgb(0x60a5fa).into(),
            FileNodeKind::Symlink => theme.tokens.muted_foreground,
            FileNodeKind::File => match self.extension() {
                Some("json") | Some("yaml") | Some("yml") | Some("toml") | Some("xml") => {
                    rgb(0xfbbf24).into()
                }
                Some("md") | Some("txt") | Some("doc") | Some("docx") | Some("pdf") => {
                    rgb(0xa78bfa).into()
                }
                Some("sh") | Some("bash") | Some("zsh") => rgb(0x4ade80).into(),
                Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("svg")
                | Some("ico") | Some("webp") => rgb(0x22c55e).into(),
                Some("mp3") | Some("wav") | Some("ogg") | Some("flac") => rgb(0xf472b6).into(),
                Some("mp4") | Some("mov") | Some("avi") | Some("webm") => rgb(0xf472b6).into(),
                Some("zip") | Some("tar") | Some("gz") | Some("rar") | Some("7z") => {
                    rgb(0xfbbf24).into()
                }
                _ => rgb(0x9ca3af).into(),
            },
        }
    }
}

#[derive(Clone)]
struct FlatFileNode<'a> {
    node: &'a FileNode,
    level: usize,
}

fn sort_file_nodes(nodes: &mut [FileNode]) {
    // Normalize each label once per ingestion rather than on every comparison.
    nodes.sort_by_cached_key(|node| {
        (
            !node.is_directory(),
            node.name.to_lowercase(),
            node.path.clone(),
        )
    });
    for node in nodes.iter_mut() {
        if !node.children.is_empty() {
            sort_file_nodes(&mut node.children);
        }
    }
}

fn flatten_file_tree<'a>(
    nodes: &'a [FileNode],
    expanded_paths: &HashSet<PathBuf>,
    show_hidden: bool,
    level: usize,
) -> Vec<FlatFileNode<'a>> {
    let mut result = Vec::new();
    let mut pending = vec![(nodes.iter(), level)];
    while let Some((siblings, level)) = pending.last_mut() {
        let Some(node) = siblings.next() else {
            pending.pop();
            continue;
        };
        if !show_hidden && node.is_hidden {
            continue;
        }
        let depth = *level;
        result.push(FlatFileNode { node, level: depth });
        if node.is_directory() && expanded_paths.contains(&node.path) {
            pending.push((node.children.iter(), depth + 1));
        }
    }
    result
}

fn format_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.1} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.1} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.1} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

const ROW_HEIGHT: f32 = 28.0;

#[derive(IntoElement)]
pub struct FileTree {
    id: ElementId,
    nodes: Vec<FileNode>,
    selected_path: Option<PathBuf>,
    expanded_paths: Vec<PathBuf>,
    show_hidden: bool,
    show_file_size: bool,
    on_select: Option<Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + Send + Sync>>,
    on_open: Option<Arc<dyn Fn(&PathBuf, &mut Window, &mut App) + Send + Sync>>,
    on_toggle: Option<Arc<dyn Fn(&PathBuf, bool, &mut Window, &mut App) + Send + Sync>>,
    on_context_menu:
        Option<Arc<dyn Fn(&PathBuf, Point<Pixels>, &mut Window, &mut App) + Send + Sync>>,
    style: StyleRefinement,
}

impl FileTree {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "file-tree:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            nodes: Vec::new(),
            selected_path: None,
            expanded_paths: Vec::new(),
            show_hidden: false,
            show_file_size: false,
            on_select: None,
            on_open: None,
            on_toggle: None,
            on_context_menu: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn nodes(mut self, mut nodes: Vec<FileNode>) -> Self {
        sort_file_nodes(&mut nodes);
        self.nodes = nodes;
        self
    }

    /// Set a stable identity when multiple file trees are rendered from one callsite.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn selected_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.selected_path = Some(path.into());
        self
    }

    pub fn expanded_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.expanded_paths = paths;
        self
    }

    pub fn show_hidden(mut self, show: bool) -> Self {
        self.show_hidden = show;
        self
    }

    pub fn show_file_size(mut self, show: bool) -> Self {
        self.show_file_size = show;
        self
    }

    pub fn on_select<F>(mut self, handler: F) -> Self
    where
        F: Fn(&PathBuf, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_select = Some(Arc::new(handler));
        self
    }

    pub fn on_open<F>(mut self, handler: F) -> Self
    where
        F: Fn(&PathBuf, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_open = Some(Arc::new(handler));
        self
    }

    pub fn on_toggle<F>(mut self, handler: F) -> Self
    where
        F: Fn(&PathBuf, bool, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_toggle = Some(Arc::new(handler));
        self
    }

    pub fn on_context_menu<F>(mut self, handler: F) -> Self
    where
        F: Fn(&PathBuf, Point<Pixels>, &mut Window, &mut App) + Send + Sync + 'static,
    {
        self.on_context_menu = Some(Arc::new(handler));
        self
    }
}

impl Default for FileTree {
    #[track_caller]
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for FileTree {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FileTree {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        let expanded_set: HashSet<PathBuf> = self.expanded_paths.into_iter().collect();
        let flat_nodes = flatten_file_tree(&self.nodes, &expanded_set, self.show_hidden, 0);

        let selected_path = self.selected_path;
        let on_select = self.on_select;
        let on_open = self.on_open;
        let on_toggle = self.on_toggle;
        let on_context_menu = self.on_context_menu;
        let show_file_size = self.show_file_size;
        let tree_id = self.id;

        div()
            .id(tree_id.clone())
            .accessibility(AccessibilityAttributes::new(AccessibilityRole::Tree).label("File tree"))
            .tab_group()
            .flex()
            .flex_col()
            .w_full()
            .bg(kael::transparent_black())
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .children(
                flat_nodes
                    .into_iter()
                    .enumerate()
                    .map(|(index, flat_node)| {
                        let is_selected = selected_path.as_ref() == Some(&flat_node.node.path);
                        let is_expanded = expanded_set.contains(&flat_node.node.path);
                        let has_children = !flat_node.node.children.is_empty()
                            || flat_node.node.has_unloaded_children;
                        let indent = px((flat_node.level as f32) * 16.0);
                        let node = flat_node.node;
                        let path = node.path.clone();
                        let row_id = ElementId::NamedChild(
                            Box::new(tree_id.clone()),
                            path.to_string_lossy().to_string().into(),
                        );

                        let icon_color = node.file_icon_color(theme);
                        let node_icon = node.file_icon(is_expanded);

                        let mut state = AccessibilityState::NONE;
                        if is_selected {
                            state |= AccessibilityState::SELECTED;
                        }
                        if node.is_directory() {
                            state |= if is_expanded {
                                AccessibilityState::EXPANDED
                            } else {
                                AccessibilityState::COLLAPSED
                            };
                        }
                        let can_activate = on_select.is_some()
                            || (node.is_directory() && on_toggle.is_some())
                            || (!node.is_directory() && on_open.is_some());
                        let mut accessibility =
                            AccessibilityAttributes::new(AccessibilityRole::TreeItem)
                                .label(node.name.clone())
                                .states(state);
                        if can_activate {
                            accessibility = accessibility.actions(vec![
                                AccessibilityAction::Focus,
                                AccessibilityAction::Click,
                            ]);
                        }

                        div()
                            .id(row_id)
                            .accessibility(accessibility)
                            .when(can_activate, |this| {
                                this.focusable()
                                    .tab_index(index as isize)
                                    .tab_stop(true)
                                    .focus_visible(|style| {
                                        style.inset_ring(theme.tokens.ring, px(2.0))
                                    })
                            })
                            .w_full()
                            .h(px(ROW_HEIGHT))
                            .flex()
                            .items_center()
                            .mx(px(8.0))
                            .px(px(8.0))
                            .pl(indent + px(8.0))
                            .rounded(theme.tokens.radius_sm)
                            .cursor(if can_activate {
                                CursorStyle::PointingHand
                            } else {
                                CursorStyle::Arrow
                            })
                            .transition(theme.tokens.transition_fast)
                            .bg(if is_selected {
                                theme.tokens.accent
                            } else {
                                kael::transparent_black()
                            })
                            .text_color(if is_selected {
                                theme.tokens.accent_foreground
                            } else if node.is_hidden {
                                theme.tokens.muted_foreground
                            } else {
                                theme.tokens.foreground
                            })
                            .when(!is_selected, |d| {
                                d.hover(|s| s.bg(theme.tokens.accent.opacity(0.5)))
                            })
                            .on_click({
                                let path = path.clone();
                                let on_select = on_select.clone();
                                let on_toggle = on_toggle.clone();
                                let on_open = on_open.clone();
                                let is_dir = node.is_directory();

                                move |event, window, cx| {
                                    if let Some(ref handler) = on_select {
                                        handler(&path, window, cx);
                                    }

                                    if is_dir {
                                        if let Some(ref handler) = on_toggle {
                                            handler(&path, !is_expanded, window, cx);
                                        }
                                    } else if event.click_count() == 2
                                        && let Some(ref handler) = on_open
                                    {
                                        handler(&path, window, cx);
                                    }
                                }
                            })
                            .when(can_activate, |this| {
                                let path = path.clone();
                                let on_select = on_select.clone();
                                let on_toggle = on_toggle.clone();
                                let on_open = on_open.clone();
                                let is_dir = node.is_directory();
                                this.on_key_down(move |event, window, cx| {
                                    if event.keystroke.modifiers.modified() {
                                        return;
                                    }
                                    match event.keystroke.key.as_str() {
                                        "space" => {
                                            if let Some(handler) = &on_select {
                                                handler(&path, window, cx);
                                            }
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                        "enter" => {
                                            if let Some(handler) = &on_select {
                                                handler(&path, window, cx);
                                            }
                                            if is_dir {
                                                if let Some(handler) = &on_toggle {
                                                    handler(&path, !is_expanded, window, cx);
                                                }
                                            } else if let Some(handler) = &on_open {
                                                handler(&path, window, cx);
                                            }
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                        "arrowright" if is_dir && !is_expanded => {
                                            if let Some(handler) = &on_toggle {
                                                handler(&path, true, window, cx);
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            }
                                        }
                                        "arrowleft" if is_dir && is_expanded => {
                                            if let Some(handler) = &on_toggle {
                                                handler(&path, false, window, cx);
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            }
                                        }
                                        _ => {}
                                    }
                                })
                            })
                            .on_mouse_down(MouseButton::Right, {
                                let path = path.clone();
                                let on_context_menu = on_context_menu.clone();

                                move |event, window, cx| {
                                    if let Some(ref handler) = on_context_menu {
                                        handler(&path, event.position, window, cx);
                                    }
                                }
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .flex_1()
                                    .child(
                                        div()
                                            .w(px(16.0))
                                            .h(px(16.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(has_children, |d| {
                                                d.child(
                                                    Icon::new(if is_expanded {
                                                        "chevron-down"
                                                    } else {
                                                        "chevron-right"
                                                    })
                                                    .size(px(12.0))
                                                    .color(theme.tokens.muted_foreground),
                                                )
                                            }),
                                    )
                                    .child(Icon::new(node_icon).size(px(16.0)).color(
                                        if is_selected {
                                            theme.tokens.accent_foreground
                                        } else {
                                            icon_color
                                        },
                                    ))
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(13.0))
                                            .font_family(theme.tokens.font_family.clone())
                                            .when(node.is_hidden, |d| d.opacity(0.6))
                                            .child(
                                                StyledText::new(node.name.clone())
                                                    .accessibility_hidden(true),
                                            ),
                                    )
                                    .when(
                                        show_file_size
                                            && node.size.is_some()
                                            && !node.is_directory(),
                                        |d| {
                                            d.child(
                                                div()
                                                    .text_size(px(11.0))
                                                    .text_color(theme.tokens.muted_foreground)
                                                    .child(format_size(node.size.unwrap())),
                                            )
                                        },
                                    ),
                            )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn relative_child_paths_are_rebased_under_their_parent() {
        let tree = FileNode::directory("src").with_children(vec![
            FileNode::file("main.rs"),
            FileNode::directory("ui").with_children(vec![FileNode::file("button.rs")]),
        ]);

        assert_eq!(tree.children[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(tree.children[1].path, PathBuf::from("src/ui"));
        assert_eq!(
            tree.children[1].children[0].path,
            PathBuf::from("src/ui/button.rs")
        );
    }

    #[::core::prelude::v1::test]
    fn absolute_child_paths_are_preserved() {
        let tree = FileNode::directory("/project")
            .with_children(vec![FileNode::file("/shared/readme.md")]);

        assert_eq!(tree.children[0].path, PathBuf::from("/shared/readme.md"));
    }
}

// The filesystem explorer stores each entry once; children are path references,
// and immutable render snapshots contain only displayed, shallow rows.
use super::tree::FlatTreeNode;
use super::virtual_tree::{VirtualTreeList, VirtualTreeModel, VirtualTreeState};
use crate::overlays::context_menu::{ContextMenu, ContextMenuItem};
use std::collections::HashMap;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};

/// A shallow filesystem entry with its normalized sort key cached at ingestion.
#[derive(Clone, Debug)]
pub struct FileTreeEntry {
    path: PathBuf,
    name: SharedString,
    sort_key: SharedString,
    kind: FileNodeKind,
    size: Option<u64>,
    hidden: bool,
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some_and(|name| name != "..") {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

impl FileTreeEntry {
    pub fn new(path: impl Into<PathBuf>, kind: FileNodeKind) -> Self {
        let path = normalize_path(&path.into());
        let name = path
            .file_name()
            .unwrap_or_else(|| path.as_os_str())
            .to_string_lossy();
        let hidden = name.starts_with('.');
        let sort_key = name.to_lowercase().into();
        let name = name.into_owned().into();
        Self {
            path,
            name,
            sort_key,
            kind,
            size: None,
            hidden,
        }
    }

    pub fn with_name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = name.into();
        self.sort_key = self.name.to_lowercase().into();
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn kind(&self) -> FileNodeKind {
        self.kind
    }
    pub fn size(&self) -> Option<u64> {
        self.size
    }
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }
    pub fn is_directory(&self) -> bool {
        self.kind == FileNodeKind::Directory
    }
}

/// A background directory request. Custom loaders should check `is_cancelled`
/// during expensive work; cancellation and stale result rejection are separate.
#[derive(Clone)]
pub struct FileTreeLoadRequest {
    pub path: PathBuf,
    cancelled: Arc<AtomicBool>,
    /// A hard per-directory entry limit; exceeding it reports an error, never a
    /// silently incomplete listing. Set on `FileTreeState` before loading.
    pub max_entries: usize,
}
impl FileTreeLoadRequest {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

type DirectoryLoader = Arc<
    dyn Fn(
            FileTreeLoadRequest,
        ) -> futures::future::BoxFuture<'static, Result<Vec<FileTreeEntry>, String>>
        + Send
        + Sync,
>;

/// Loading status for a directory; failures remain retryable through Reload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileTreeLoadState {
    Unloaded,
    Loading,
    Loaded,
    Failed(SharedString),
}

#[derive(Clone)]
struct FileCatalog {
    entries: HashMap<Arc<Path>, Arc<FileTreeEntry>>,
    children: HashMap<Arc<Path>, Arc<[Arc<Path>]>>,
    executor: BackgroundExecutor,
}
impl Drop for FileCatalog {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        let children = std::mem::take(&mut self.children);
        self.executor
            .spawn(async move {
                drop((entries, children));
            })
            .detach();
    }
}
#[derive(Clone)]
struct FileExpansion {
    ids: HashSet<PathBuf>,
    executor: BackgroundExecutor,
}
/// Maximum distinct branch changes queued while a worker prepares a snapshot.
/// Sources can retry additional programmatic changes after `is_preparing` clears.
pub const FILE_TREE_MAX_PENDING_EXPANSIONS: usize = 4096;
struct FileExpansionChanges {
    values: HashMap<PathBuf, bool>,
    executor: BackgroundExecutor,
}
impl Drop for FileExpansionChanges {
    fn drop(&mut self) {
        let values = std::mem::take(&mut self.values);
        self.executor
            .spawn(async move {
                drop(values);
            })
            .detach();
    }
}
impl Drop for FileExpansion {
    fn drop(&mut self) {
        let ids = std::mem::take(&mut self.ids);
        self.executor
            .spawn(async move {
                drop(ids);
            })
            .detach();
    }
}
struct FileListing {
    entries: Vec<FileTreeEntry>,
    executor: BackgroundExecutor,
}
impl Drop for FileListing {
    fn drop(&mut self) {
        let entries = std::mem::take(&mut self.entries);
        self.executor
            .spawn(async move {
                drop(entries);
            })
            .detach();
    }
}
struct FileDirectory {
    status: FileTreeLoadState,
    generation: u64,
}
struct FileLoadJob {
    _task: Task<()>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for FileLoadJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
struct FilePrepareJob {
    _task: Task<()>,
    cancel_rows: Arc<AtomicBool>,
}
impl Drop for FilePrepareJob {
    fn drop(&mut self) {
        self.cancel_rows.store(true, Ordering::Relaxed);
    }
}
#[derive(Clone)]
enum FileMutation {
    Snapshot,
    Listing {
        path: PathBuf,
        generation: u64,
        listing: Arc<FileListing>,
    },
    Evict {
        path: PathBuf,
        generation: u64,
    },
}
struct PreparedFiles {
    catalog: Arc<FileCatalog>,
    snapshot: Option<VirtualTreeModel<PathBuf>>,
    expanded: Option<Arc<FileExpansion>>,
    requested_expanded: Arc<FileExpansion>,
    mutation: FileMutation,
    base_revision: u64,
    view_generation: u64,
    label: SharedString,
    result: Result<(), String>,
    elapsed: std::time::Duration,
}

/// Explorer events. File writes remain application-owned: a drop is a validated
/// request, and the app can authorize/perform move or copy, then reload parents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileTreeEvent {
    Selected(PathBuf),
    Opened(PathBuf),
    Loaded(PathBuf),
    LoadFailed {
        path: PathBuf,
        message: SharedString,
    },
    ContextAction {
        path: PathBuf,
        action: SharedString,
    },
    DropRequested(FileTreeDrop),
}
/// A validated intra-explorer drop request. Symlinks are not directory targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeDrop {
    pub source: PathBuf,
    pub target_directory: PathBuf,
}

/// A reusable lazy explorer. Listing validation, sorting, immutable catalog
/// preparation, viewport snapshot construction and large-value reclamation run
/// on workers. Foreground commits swap prepared Arcs and bounded request metadata.
///
/// Expansion/eviction are asynchronous: the previous coherent snapshot remains
/// visible until a generation-checked replacement is ready. Keep this entity in
/// the view and observe `is_preparing`/`loading_count` for application feedback.
pub struct FileTreeState {
    catalog: Arc<FileCatalog>,
    roots: Arc<[PathBuf]>,
    directories: HashMap<PathBuf, FileDirectory>,
    expanded: Arc<FileExpansion>,
    expansion_changes: HashMap<PathBuf, bool>,
    preparing_expansion_changes: Option<Arc<FileExpansionChanges>>,
    snapshot: VirtualTreeModel<PathBuf>,
    accessibility_label: SharedString,
    snapshot_label: SharedString,
    tree_state: Entity<VirtualTreeState<PathBuf>>,
    loader: DirectoryLoader,
    jobs: HashMap<PathBuf, FileLoadJob>,
    queued: std::collections::VecDeque<FileTreeLoadRequest>,
    mutations: std::collections::VecDeque<FileMutation>,
    prepare_task: Option<FilePrepareJob>,
    preparing_listing: bool,
    snapshot_requested: bool,
    catalog_revision: u64,
    view_generation: u64,
    next_generation: u64,
    selected: Option<PathBuf>,
    show_hidden: bool,
    max_entries: usize,
    cached_entry_budget: usize,
    max_pipeline_loads: usize,
    max_queued_loads: usize,
    menu: Option<(PathBuf, Point<Pixels>)>,
    cut: Option<PathBuf>,
    last_apply: std::time::Duration,
    last_prepare: std::time::Duration,
}
impl EventEmitter<FileTreeEvent> for FileTreeState {}
impl Focusable for FileTreeState {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.tree_state.focus_handle(cx)
    }
}

impl FileTreeState {
    /// Adapt a blocking source to the background executor.
    pub fn new(
        roots: Vec<FileTreeEntry>,
        loader: impl Fn(FileTreeLoadRequest) -> Result<Vec<FileTreeEntry>, String>
        + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let loader = Arc::new(loader);
        Self::with_async_loader(
            roots,
            move |request| {
                let loader = loader.clone();
                Box::pin(async move { loader(request) })
            },
            cx,
        )
    }
    /// Load from asynchronous Rust storage clients without requiring a specific
    /// runtime. Futures must be Send and should cooperate with cancellation.
    pub fn with_async_loader(
        roots: Vec<FileTreeEntry>,
        loader: impl Fn(
            FileTreeLoadRequest,
        )
            -> futures::future::BoxFuture<'static, Result<Vec<FileTreeEntry>, String>>
        + Send
        + Sync
        + 'static,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        if roots.len() > 1024 {
            return Err("filesystem explorer supports at most 1024 roots".into());
        }
        let executor = cx.background_executor().clone();
        let mut roots = roots;
        sort_entries(&mut roots);
        let paths: Arc<[PathBuf]> = roots
            .iter()
            .map(|entry| entry.path.clone())
            .collect::<Vec<_>>()
            .into();
        let mut entries: HashMap<Arc<Path>, Arc<FileTreeEntry>> = HashMap::new();
        for entry in roots {
            if entries
                .keys()
                .any(|path| path.starts_with(&entry.path) || entry.path.starts_with(path.as_ref()))
            {
                return Err("filesystem roots must not duplicate or overlap".into());
            }
            entries.insert(Arc::from(entry.path.as_path()), Arc::new(entry));
        }
        let catalog = Arc::new(FileCatalog {
            entries,
            children: HashMap::new(),
            executor: executor.clone(),
        });
        let expanded = Arc::new(FileExpansion {
            ids: HashSet::new(),
            executor: executor.clone(),
        });
        let tree_state = cx.new(|cx| VirtualTreeState::new(cx));
        let label: SharedString = "Files".into();
        let mut snapshot = build_file_snapshot(
            &catalog,
            &paths,
            &expanded.ids,
            false,
            &AtomicBool::new(false),
        )
        .expect("root snapshot is not cancelled");
        snapshot
            .prepare_accessibility(
                tree_state
                    .read(cx)
                    .accessibility_preparation_context(label.clone(), true),
                &executor,
            )
            .expect("fresh root snapshot is unshared");
        Ok(Self {
            catalog,
            roots: paths,
            directories: HashMap::new(),
            expanded,
            expansion_changes: HashMap::new(),
            preparing_expansion_changes: None,
            snapshot,
            accessibility_label: label.clone(),
            snapshot_label: label,
            tree_state,
            loader: Arc::new(loader),
            jobs: HashMap::new(),
            queued: Default::default(),
            mutations: Default::default(),
            prepare_task: None,
            preparing_listing: false,
            snapshot_requested: false,
            catalog_revision: 0,
            view_generation: 0,
            next_generation: 1,
            selected: None,
            show_hidden: false,
            max_entries: 100_000,
            cached_entry_budget: 250_000,
            max_pipeline_loads: 4,
            max_queued_loads: 32,
            menu: None,
            cut: None,
            last_apply: Default::default(),
            last_prepare: Default::default(),
        })
    }
    /// Validate one native root without enumerating it. Expand to load its
    /// directory asynchronously. Symbolic-link roots are deliberately rejected.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn filesystem(root: impl Into<PathBuf>, cx: &mut Context<Self>) -> Result<Self, String> {
        let root = root.into();
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(root)
        };
        if !std::fs::symlink_metadata(&root)
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            return Err("filesystem root must be a directory, not a symlink".into());
        }
        Self::new(
            vec![FileTreeEntry::new(root, FileNodeKind::Directory)],
            read_directory,
            cx,
        )
    }
    pub fn snapshot(&self) -> VirtualTreeModel<PathBuf> {
        self.snapshot.clone()
    }
    pub fn tree_state(&self) -> &Entity<VirtualTreeState<PathBuf>> {
        &self.tree_state
    }
    pub fn entry(&self, path: &Path) -> Option<&FileTreeEntry> {
        self.catalog.entries.get(path).map(AsRef::as_ref)
    }
    pub fn root_paths(&self) -> &[PathBuf] {
        &self.roots
    }
    pub fn selected_path(&self) -> Option<&Path> {
        self.selected.as_deref()
    }
    pub fn cached_entry_count(&self) -> usize {
        self.catalog.entries.len()
    }
    pub fn loading_count(&self) -> usize {
        self.directories
            .values()
            .filter(|directory| directory.status == FileTreeLoadState::Loading)
            .count()
    }
    pub fn is_preparing(&self) -> bool {
        self.prepare_task.is_some() || !self.mutations.is_empty() || self.snapshot_requested
    }
    /// Duration of the most recent foreground prepared-Arc commit, excluding
    /// application event callbacks and the subsequent frame's layout/paint.
    pub fn last_model_apply_duration(&self) -> std::time::Duration {
        self.last_apply
    }
    pub fn last_model_prepare_duration(&self) -> std::time::Duration {
        self.last_prepare
    }
    pub fn load_state(&self, path: &Path) -> Option<&FileTreeLoadState> {
        if !self.entry(path).is_some_and(FileTreeEntry::is_directory) {
            return None;
        }
        if let Some(directory) = self.directories.get(path) {
            return Some(&directory.status);
        }
        if self.catalog.children.contains_key(path) {
            Some(&FileTreeLoadState::Loaded)
        } else {
            Some(&FileTreeLoadState::Unloaded)
        }
    }
    pub fn set_cached_entry_budget(&mut self, budget: usize) -> Result<(), &'static str> {
        if budget < self.cached_entry_count() {
            return Err("evict cached branches before lowering the entry budget");
        }
        self.cached_entry_budget = budget;
        Ok(())
    }
    pub fn cached_entry_budget(&self) -> usize {
        self.cached_entry_budget
    }
    pub fn set_max_entries(&mut self, max_entries: usize) {
        self.max_entries = max_entries.max(1);
    }
    /// Bound concurrent loading plus prepared listings (default four), and cheap
    /// queued path requests (default 32). Completed listings hold pipeline slots
    /// until committed, preventing a slow preparer from accumulating payloads.
    pub fn set_load_limits(&mut self, pipeline: usize, queued: usize) {
        self.max_pipeline_loads = pipeline.clamp(1, 64);
        self.max_queued_loads = queued.min(4096);
    }
    pub fn set_show_hidden(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.show_hidden != visible {
            self.show_hidden = visible;
            self.request_snapshot(cx);
        }
    }
    pub fn select(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.entry(path).is_none() {
            return;
        }
        self.selected = Some(path.to_path_buf());
        cx.emit(FileTreeEvent::Selected(path.to_path_buf()));
        cx.notify();
    }
    pub fn open(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.select(path, cx);
        if self.entry(path).is_some_and(FileTreeEntry::is_directory) {
            self.set_expanded(path, !self.is_expanded(path), cx);
        } else if self.entry(path).is_some() {
            cx.emit(FileTreeEvent::Opened(path.to_path_buf()));
        }
    }
    pub fn set_expanded(&mut self, path: &Path, expanded: bool, cx: &mut Context<Self>) {
        if let Err(message) = self.try_set_expanded(path, expanded, cx)
            && self.entry(path).is_some_and(FileTreeEntry::is_directory)
        {
            cx.emit(FileTreeEvent::LoadFailed {
                path: path.to_path_buf(),
                message: message.into(),
            });
        }
    }
    /// Desired expansion includes pending worker changes without copying the
    /// retained expanded-directory set on the foreground thread.
    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expansion_changes
            .get(path)
            .copied()
            .or_else(|| {
                self.preparing_expansion_changes
                    .as_ref()
                    .and_then(|changes| changes.values.get(path).copied())
            })
            .unwrap_or_else(|| self.expanded.ids.contains(path))
    }
    /// Queue one branch change. The bounded delta batch is moved to a worker;
    /// applications expanding thousands of branches can retry once preparation
    /// completes. A rejected change leaves the desired expansion intact.
    pub fn try_set_expanded(
        &mut self,
        path: &Path,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        if !self.entry(path).is_some_and(FileTreeEntry::is_directory) {
            return Err("expansion target is not a cached directory");
        }
        if self.is_expanded(path) == expanded {
            if expanded {
                self.load(path, false, cx);
            }
            return Ok(());
        }
        self.stage_expansion(path, expanded)?;
        if expanded {
            self.load(path, false, cx);
        }
        self.request_snapshot(cx);
        Ok(())
    }
    fn stage_expansion(&mut self, path: &Path, expanded: bool) -> Result<(), &'static str> {
        if !self.expansion_changes.contains_key(path)
            && self.expansion_changes.len() == FILE_TREE_MAX_PENDING_EXPANSIONS
        {
            return Err("expansion queue is full; retry after preparation completes");
        }
        self.expansion_changes.insert(path.to_path_buf(), expanded);
        Ok(())
    }
    pub fn reload(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.load(path, true, cx);
    }
    /// Asynchronously evict a cached subtree and cancel pending descendants.
    /// The prior snapshot remains coherent until the worker commits its replacement.
    pub fn evict_directory(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.entry(path).is_some_and(FileTreeEntry::is_directory) {
            return;
        }
        if self.mutations.iter().any(|mutation| matches!(mutation, FileMutation::Evict { path: pending, .. } if path.starts_with(pending))) { return; }
        if self
            .mutations
            .iter()
            .filter(|mutation| matches!(mutation, FileMutation::Evict { path: pending, .. } if !pending.starts_with(path)))
            .count()
            >= 32
        {
            let generation = self.allocate_generation();
            self.fail_load(
                path,
                generation,
                "eviction queue is full; retry after pending work finishes".into(),
                cx,
            );
            cx.notify();
            return;
        }
        if let Err(message) = self.stage_expansion(path, false) {
            cx.emit(FileTreeEvent::LoadFailed {
                path: path.to_path_buf(),
                message: message.into(),
            });
            return;
        }
        self.mutations.retain(|mutation| !matches!(mutation, FileMutation::Evict { path: pending, .. } if pending.starts_with(path)));
        self.jobs
            .retain(|candidate, _| !candidate.starts_with(path));
        self.queued.retain(|request| {
            let keep = !request.path.starts_with(path);
            if !keep {
                request.cancelled.store(true, Ordering::Relaxed);
            }
            keep
        });
        self.directories
            .retain(|candidate, _| !candidate.starts_with(path));
        let generation = self.allocate_generation();
        self.directories.insert(
            path.to_path_buf(),
            FileDirectory {
                status: FileTreeLoadState::Unloaded,
                generation,
            },
        );
        self.view_generation = self.view_generation.wrapping_add(1);
        self.mutations.push_back(FileMutation::Evict {
            path: path.to_path_buf(),
            generation,
        });
        self.snapshot_requested = true;
        if let Some(job) = self.prepare_task.as_ref() {
            job.cancel_rows.store(true, Ordering::Relaxed);
        }
        self.start_prepare(cx);
        self.start_queued(cx);
        cx.notify();
    }
    fn allocate_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);
        generation
    }
    fn pipeline_count(&self) -> usize {
        self.jobs.len()
            + self
                .mutations
                .iter()
                .filter(|mutation| matches!(mutation, FileMutation::Listing { .. }))
                .count()
            + usize::from(self.preparing_listing)
    }
    fn load(&mut self, path: &Path, force: bool, cx: &mut Context<Self>) {
        if !self.entry(path).is_some_and(FileTreeEntry::is_directory) {
            return;
        }
        if !force
            && matches!(
                self.load_state(path),
                Some(FileTreeLoadState::Loaded | FileTreeLoadState::Loading)
            )
        {
            return;
        }
        self.jobs.remove(path);
        self.queued.retain(|request| {
            if request.path == path {
                request.cancelled.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
        let generation = self.allocate_generation();
        self.directories.insert(
            path.to_path_buf(),
            FileDirectory {
                status: FileTreeLoadState::Loading,
                generation,
            },
        );
        let request = FileTreeLoadRequest {
            path: path.to_path_buf(),
            cancelled: Arc::new(AtomicBool::new(false)),
            max_entries: self.max_entries,
        };
        if self.pipeline_count() < self.max_pipeline_loads {
            self.start_load(request, generation, cx);
        } else if self.queued.len() < self.max_queued_loads {
            self.queued.push_back(request);
        } else {
            self.fail_load(
                path,
                generation,
                "directory request queue is full; retry after pending loads finish".into(),
                cx,
            );
        }
        cx.notify();
    }
    fn start_load(
        &mut self,
        request: FileTreeLoadRequest,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let path = request.path.clone();
        let job_path = path.clone();
        let cancelled = request.cancelled.clone();
        let loader = self.loader.clone();
        let background = cx.background_executor().clone();
        let reclaim_executor = background.clone();
        let task = cx.spawn(async move |state, cx| {
            let result = background.spawn(async move {
                let mut entries = loader(request.clone()).await?;
                if request.is_cancelled() {
                    return Err("directory load cancelled".into());
                }
                if entries.len() > request.max_entries {
                    return Err(format!(
                        "directory exceeds {} entry limit",
                        request.max_entries
                    ));
                }
                let mut unique = HashSet::with_capacity(entries.len());
                for entry in &entries {
                    if entry.path.parent() != Some(request.path.as_path())
                        || !unique.insert(entry.path.clone())
                    {
                        return Err("directory loader returned duplicate or non-child paths".into());
                    }
                }
                sort_entries(&mut entries);
                Ok(Arc::new(FileListing {
                    entries,
                    executor: reclaim_executor,
                }))
            });
            let result = result.await;
            let _ = state.update(cx, |state, cx| {
                state.listing_ready(&path, generation, result, cx)
            });
        });
        self.jobs.insert(
            job_path,
            FileLoadJob {
                _task: task,
                cancelled,
            },
        );
    }
    fn start_queued(&mut self, cx: &mut Context<Self>) {
        while self.pipeline_count() < self.max_pipeline_loads {
            let Some(request) = self.queued.pop_front() else {
                break;
            };
            let Some(directory) = self.directories.get(&request.path) else {
                continue;
            };
            let generation = directory.generation;
            if directory.status == FileTreeLoadState::Loading {
                self.start_load(request, generation, cx);
            }
        }
    }
    fn listing_ready(
        &mut self,
        path: &Path,
        generation: u64,
        result: Result<Arc<FileListing>, String>,
        cx: &mut Context<Self>,
    ) {
        if !self
            .directories
            .get(path)
            .is_some_and(|directory| directory.generation == generation)
        {
            return;
        }
        self.jobs.remove(path);
        match result {
            Ok(listing) => self.mutations.push_back(FileMutation::Listing {
                path: path.to_path_buf(),
                generation,
                listing,
            }),
            Err(message) => self.fail_load(path, generation, message, cx),
        }
        self.start_prepare(cx);
        self.start_queued(cx);
        cx.notify();
    }
    fn fail_load(&mut self, path: &Path, generation: u64, message: String, cx: &mut Context<Self>) {
        // Keep only a bounded error history; pending requests are never removed.
        if self
            .directories
            .values()
            .filter(|directory| matches!(directory.status, FileTreeLoadState::Failed(_)))
            .count()
            >= 128
            && let Some(old) = self
                .directories
                .iter()
                .filter(|(_, directory)| matches!(directory.status, FileTreeLoadState::Failed(_)))
                .min_by_key(|(_, directory)| directory.generation)
                .map(|(path, _)| path.clone())
        {
            self.directories.remove(&old);
        }
        let message: SharedString = message.into();
        self.directories.insert(
            path.to_path_buf(),
            FileDirectory {
                status: FileTreeLoadState::Failed(message.clone()),
                generation,
            },
        );
        cx.emit(FileTreeEvent::LoadFailed {
            path: path.to_path_buf(),
            message,
        });
    }
    fn request_snapshot(&mut self, cx: &mut Context<Self>) {
        self.view_generation = self.view_generation.wrapping_add(1);
        self.snapshot_requested = true;
        if let Some(job) = self.prepare_task.as_ref() {
            job.cancel_rows.store(true, Ordering::Relaxed);
        }
        self.start_prepare(cx);
        cx.notify();
    }
    fn start_prepare(&mut self, cx: &mut Context<Self>) {
        if self.prepare_task.is_some() {
            return;
        }
        let mutation = loop {
            if let Some(mutation) = self.mutations.pop_front() {
                if let FileMutation::Listing {
                    path, generation, ..
                } = &mutation
                    && !self
                        .directories
                        .get(path)
                        .is_some_and(|directory| directory.generation == *generation)
                {
                    continue;
                }
                break mutation;
            }
            if !self.snapshot_requested {
                return;
            }
            break FileMutation::Snapshot;
        };
        self.snapshot_requested = false;
        self.preparing_listing = matches!(mutation, FileMutation::Listing { .. });
        let catalog = self.catalog.clone();
        let roots = self.roots.clone();
        let expanded = self.expanded.clone();
        let expansion_changes = Arc::new(FileExpansionChanges {
            values: std::mem::take(&mut self.expansion_changes),
            executor: catalog.executor.clone(),
        });
        self.preparing_expansion_changes = Some(expansion_changes.clone());
        let visible = self.show_hidden;
        let budget = self.cached_entry_budget;
        let base_revision = self.catalog_revision;
        let view_generation = self.view_generation;
        let label = self.accessibility_label.clone();
        let accessibility = self
            .tree_state
            .read(cx)
            .accessibility_preparation_context(label.clone(), true);
        let background = cx.background_executor().clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel_rows = cancelled.clone();
        let task = cx.spawn(async move |state, cx| {
            let prepared = background
                .spawn(async move {
                    let started = std::time::Instant::now();
                    let (catalog, result) = match prepare_catalog(&catalog, &mutation, budget) {
                        Ok(catalog) => (catalog, Ok(())),
                        Err(message) => (catalog, Err(message)),
                    };
                    let mut requested = expanded.ids.clone();
                    for (path, expanded) in &expansion_changes.values {
                        if *expanded {
                            requested.insert(path.clone());
                        } else {
                            requested.remove(path);
                        }
                    }
                    let requested_expanded = Arc::new(FileExpansion {
                        ids: requested,
                        executor: catalog.executor.clone(),
                    });
                    let expanded = if requested_expanded
                        .ids
                        .iter()
                        .all(|path| catalog.entries.contains_key(path.as_path()))
                    {
                        requested_expanded.clone()
                    } else {
                        Arc::new(FileExpansion {
                            ids: requested_expanded
                                .ids
                                .iter()
                                .filter(|path| catalog.entries.contains_key(path.as_path()))
                                .cloned()
                                .collect(),
                            executor: catalog.executor.clone(),
                        })
                    };
                    let mut snapshot =
                        build_file_snapshot(&catalog, &roots, &expanded.ids, visible, &cancel_rows);
                    if let Some(model) = snapshot.as_mut() {
                        model
                            .prepare_accessibility(accessibility, &catalog.executor)
                            .expect("fresh worker snapshot is unshared");
                    }
                    PreparedFiles {
                        catalog,
                        snapshot,
                        expanded: Some(expanded),
                        requested_expanded,
                        mutation,
                        base_revision,
                        view_generation,
                        label,
                        result,
                        elapsed: started.elapsed(),
                    }
                })
                .await;
            let _ = state.update(cx, |state, cx| state.commit_prepared(prepared, cx));
        });
        self.prepare_task = Some(FilePrepareJob {
            _task: task,
            cancel_rows: cancelled,
        });
    }
    fn commit_prepared(&mut self, mut prepared: PreparedFiles, cx: &mut Context<Self>) {
        if prepared.result.is_ok() && prepared.catalog.entries.len() > self.cached_entry_budget {
            prepared.result =
                Err("prepared directory exceeds the updated cached entry budget".into());
        }
        let started = std::time::Instant::now();
        self.prepare_task = None;
        self.preparing_listing = false;
        let valid = prepared.base_revision == self.catalog_revision
            && match &prepared.mutation {
                FileMutation::Listing {
                    path, generation, ..
                } => self
                    .directories
                    .get(path)
                    .is_some_and(|directory| directory.generation == *generation),
                _ => true,
            };
        // Requested expansion is independent of source generations. Even if a
        // listing is stale, its worker batch must not erase a user's earlier
        // branch request. Newer foreground deltas remain a separate overlay.
        self.expanded = if valid && prepared.result.is_ok() {
            prepared.expanded.take().unwrap()
        } else {
            prepared.requested_expanded.clone()
        };
        self.preparing_expansion_changes = None;
        if valid {
            if prepared.result.is_ok() {
                self.catalog = prepared.catalog;
                if !matches!(prepared.mutation, FileMutation::Snapshot) {
                    self.catalog_revision = self.catalog_revision.wrapping_add(1);
                }
                // This metadata is bounded by active/queued loads and 128 errors.
                self.directories
                    .retain(|path, _| self.catalog.entries.contains_key(path.as_path()));
                self.jobs
                    .retain(|path, _| self.catalog.entries.contains_key(path.as_path()));
                self.queued
                    .retain(|request| self.catalog.entries.contains_key(request.path.as_path()));
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|path| !self.catalog.entries.contains_key(path.as_path()))
                {
                    self.selected = None;
                }
                if self
                    .cut
                    .as_ref()
                    .is_some_and(|path| !self.catalog.entries.contains_key(path.as_path()))
                {
                    self.cut = None;
                }
                if prepared.view_generation == self.view_generation
                    && let Some(snapshot) = prepared.snapshot
                {
                    self.snapshot = snapshot;
                    self.snapshot_label = prepared.label;
                } else {
                    self.snapshot_requested = true;
                }
                match prepared.mutation {
                    FileMutation::Listing { path, .. } => {
                        self.directories.remove(&path);
                        cx.emit(FileTreeEvent::Loaded(path));
                    }
                    FileMutation::Evict { path, generation } => {
                        if self
                            .directories
                            .get(&path)
                            .is_some_and(|directory| directory.generation == generation)
                        {
                            self.directories.remove(&path);
                        }
                    }
                    FileMutation::Snapshot => {}
                }
            } else if let FileMutation::Listing {
                path, generation, ..
            } = prepared.mutation
                && let Err(message) = prepared.result
            {
                self.fail_load(&path, generation, message, cx);
            }
            self.last_prepare = prepared.elapsed;
        } else {
            self.snapshot_requested = true;
        }
        self.last_apply = started.elapsed();
        self.start_prepare(cx);
        self.start_queued(cx);
        cx.notify();
    }
    #[cfg(test)]
    fn apply_listing(
        &mut self,
        path: &Path,
        generation: u64,
        result: Result<Vec<FileTreeEntry>, String>,
        cx: &mut Context<Self>,
    ) {
        self.listing_ready(
            path,
            generation,
            result.map(|entries| {
                Arc::new(FileListing {
                    entries,
                    executor: cx.background_executor().clone(),
                })
            }),
            cx,
        );
    }
    pub fn validate_drop(
        &self,
        source: &Path,
        target: &Path,
    ) -> Result<FileTreeDrop, &'static str> {
        let source = normalize_path(source);
        let target = normalize_path(target);
        if self.entry(&source).is_none() {
            return Err("source is not in this explorer");
        }
        if !self.entry(&target).is_some_and(FileTreeEntry::is_directory) {
            return Err("drop target must be a directory");
        }
        if target == source || target.starts_with(&source) {
            return Err("cannot move an entry into itself or its descendants");
        }
        if source.parent() == Some(target.as_path()) {
            return Err("entry is already in this directory");
        }
        if self.roots.contains(&source) {
            return Err("explorer roots cannot be moved");
        }
        Ok(FileTreeDrop {
            source,
            target_directory: target,
        })
    }
    pub fn cut(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        if self.entry(path).is_none() || self.roots.iter().any(|root| root == path) {
            return false;
        }
        self.cut = Some(path.to_path_buf());
        cx.notify();
        true
    }
    pub fn paste_into(
        &mut self,
        target: &Path,
        cx: &mut Context<Self>,
    ) -> Result<FileTreeDrop, &'static str> {
        let source = self.cut.clone().ok_or("no picked-up entry")?;
        let request = self.request_drop(&source, target, cx)?;
        self.cut = None;
        cx.notify();
        Ok(request)
    }
    pub fn request_drop(
        &mut self,
        source: &Path,
        target: &Path,
        cx: &mut Context<Self>,
    ) -> Result<FileTreeDrop, &'static str> {
        let request = self.validate_drop(source, target)?;
        cx.emit(FileTreeEvent::DropRequested(request.clone()));
        Ok(request)
    }
}

fn catalog_subtrees(
    catalog: &FileCatalog,
    roots: impl IntoIterator<Item = Arc<Path>>,
) -> HashSet<Arc<Path>> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut removed = HashSet::new();
    while let Some(path) = pending.pop() {
        if !removed.insert(path.clone()) {
            continue;
        }
        if let Some(children) = catalog.children.get(path.as_ref()) {
            pending.extend(children.iter().cloned());
        }
    }
    removed
}
fn prepare_catalog(
    base: &Arc<FileCatalog>,
    mutation: &FileMutation,
    budget: usize,
) -> Result<Arc<FileCatalog>, String> {
    if matches!(mutation, FileMutation::Snapshot) {
        return Ok(base.clone());
    }
    let (path, incoming) = match mutation {
        FileMutation::Listing { path, listing, .. } => (path, Some(&listing.entries)),
        FileMutation::Evict { path, .. } => (path, None),
        _ => unreachable!(),
    };
    let previous = base
        .children
        .get(path.as_path())
        .cloned()
        .unwrap_or_else(|| Arc::from([]));
    let keep: HashMap<&Path, FileNodeKind> = incoming
        .into_iter()
        .flat_map(|entries| entries.iter())
        .map(|entry| (entry.path.as_path(), entry.kind))
        .collect();
    let removed = catalog_subtrees(
        base,
        previous
            .iter()
            .filter(|old| {
                !keep.contains_key(old.as_ref())
                    || base.entries.get(old.as_ref()).is_some_and(|entry| {
                        keep.get(old.as_ref())
                            .is_some_and(|kind| *kind != entry.kind)
                    })
            })
            .cloned(),
    );
    let additions = incoming
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter(|entry| {
            !base.entries.contains_key(entry.path.as_path())
                || removed.contains(entry.path.as_path())
        })
        .count();
    if base
        .entries
        .len()
        .saturating_sub(removed.len())
        .saturating_add(additions)
        > budget
    {
        return Err(format!(
            "directory would exceed {budget} cached entry budget; evict collapsed branches"
        ));
    }
    let mut catalog = (**base).clone();
    for path in removed {
        catalog.entries.remove(path.as_ref());
        catalog.children.remove(path.as_ref());
    }
    if let Some(incoming) = incoming {
        let mut children = Vec::with_capacity(incoming.len());
        for entry in incoming {
            let key: Arc<Path> = Arc::from(entry.path.as_path());
            children.push(key.clone());
            catalog.entries.insert(key, Arc::new(entry.clone()));
        }
        catalog
            .children
            .insert(Arc::from(path.as_path()), children.into());
    } else {
        catalog.children.remove(path.as_path());
    }
    Ok(Arc::new(catalog))
}
fn build_file_snapshot(
    catalog: &FileCatalog,
    roots: &[PathBuf],
    expanded: &HashSet<PathBuf>,
    show_hidden: bool,
    cancelled: &AtomicBool,
) -> Option<VirtualTreeModel<PathBuf>> {
    let mut rows = Vec::new();
    let mut pending: Vec<(Arc<Path>, usize)> = roots
        .iter()
        .rev()
        .map(|path| (Arc::from(path.as_path()), 0))
        .collect();
    while let Some((path, level)) = pending.pop() {
        if rows.len() % 512 == 0 && cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let Some(entry) = catalog.entries.get(path.as_ref()) else {
            continue;
        };
        if !show_hidden && entry.hidden {
            continue;
        }
        let children = catalog.children.get(path.as_ref());
        let has_children =
            entry.is_directory() && children.is_none_or(|children| !children.is_empty());
        rows.push(FlatTreeNode {
            node_id: path.to_path_buf(),
            label: entry.name.clone(),
            icon: Some(
                match entry.kind {
                    FileNodeKind::Directory => {
                        if expanded.contains(path.as_ref()) {
                            "folder-open"
                        } else {
                            "folder"
                        }
                    }
                    FileNodeKind::Symlink => "link",
                    FileNodeKind::File => "file",
                }
                .into(),
            ),
            icon_color: None,
            disabled: false,
            has_children,
            level,
            match_ranges: Vec::new(),
        });
        if expanded.contains(path.as_ref())
            && let Some(children) = children
        {
            pending.extend(children.iter().rev().map(|path| (path.clone(), level + 1)));
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let mut model =
        VirtualTreeModel::from_rows(rows, expanded, false).expect("catalog paths are unique");
    model
        .set_accessibility_catalog_ids(catalog.entries.keys().map(|path| path.to_path_buf()))
        .expect("catalog IDs are unique and contain every displayed row");
    model.reclaim_on(&catalog.executor);
    Some(model)
}

fn sort_entries(entries: &mut [FileTreeEntry]) {
    entries.sort_by(|a, b| {
        (!a.is_directory(), &a.sort_key, &a.path).cmp(&(!b.is_directory(), &b.sort_key, &b.path))
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn read_directory(request: FileTreeLoadRequest) -> Result<Vec<FileTreeEntry>, String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&request.path).map_err(|error| error.to_string())? {
        if request.is_cancelled() {
            return Err("directory load cancelled".into());
        }
        if entries.len() >= request.max_entries {
            return Err(format!(
                "directory exceeds {} entry limit",
                request.max_entries
            ));
        }
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata =
            std::fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        let kind = if metadata.file_type().is_symlink() {
            FileNodeKind::Symlink
        } else if metadata.is_dir() {
            FileNodeKind::Directory
        } else {
            FileNodeKind::File
        };
        let mut node = FileTreeEntry::new(entry.path(), kind);
        if kind == FileNodeKind::File {
            node.size = Some(metadata.len());
        }
        entries.push(node);
    }
    Ok(entries)
}

/// Application-specific context action shown after Open and Reload.
#[derive(Clone)]
pub struct FileTreeContextAction {
    pub id: SharedString,
    pub label: SharedString,
    pub destructive: bool,
}
impl FileTreeContextAction {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            destructive: false,
        }
    }
    pub fn destructive(mut self, value: bool) -> Self {
        self.destructive = value;
        self
    }
}

#[derive(Clone)]
struct FileTreeDrag {
    owner: EntityId,
    path: PathBuf,
    name: SharedString,
}
impl Render for FileTreeDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .p_2()
            .rounded(theme.tokens.radius_sm)
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.border)
            .child(self.name.clone())
    }
}

/// A complete virtual filesystem explorer: lazy loading, single logical focus,
/// selection/open, right-click or Shift-F10 menus, and validated drag/drop.
/// Listen to `FileTreeEvent` on its state for file actions. Give it bounded height.
#[derive(IntoElement)]
pub struct VirtualFileTree {
    id: ElementId,
    state: Entity<FileTreeState>,
    label: SharedString,
    context_actions: Vec<FileTreeContextAction>,
    show_file_size: bool,
    drag_drop: bool,
    style: StyleRefinement,
}
impl VirtualFileTree {
    pub fn new(id: impl Into<ElementId>, state: Entity<FileTreeState>) -> Self {
        Self {
            id: id.into(),
            state,
            label: "Files".into(),
            context_actions: Vec::new(),
            show_file_size: false,
            drag_drop: true,
            style: StyleRefinement::default(),
        }
    }
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
    pub fn context_actions(mut self, actions: Vec<FileTreeContextAction>) -> Self {
        self.context_actions = actions;
        self
    }
    pub fn show_file_size(mut self, show: bool) -> Self {
        self.show_file_size = show;
        self
    }
    pub fn drag_drop(mut self, enabled: bool) -> Self {
        self.drag_drop = enabled;
        self
    }
}
impl Styled for VirtualFileTree {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl RenderOnce for VirtualFileTree {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        crate::components::model_observer::observe_model(self.id.clone(), &self.state, window, cx);
        if self.state.read(cx).accessibility_label != self.label {
            self.state.update(cx, |state, cx| {
                state.accessibility_label = self.label;
                state.request_snapshot(cx);
            });
        }
        let label = self.state.read(cx).snapshot_label.clone();
        let model = self.state.read(cx).snapshot();
        let tree_state = self.state.read(cx).tree_state.clone();
        let selected = self.state.read(cx).selected.clone();
        let select = self.state.downgrade();
        let toggle = self.state.downgrade();
        let open = self.state.downgrade();
        let decorate = self.state.downgrade();
        let key = self.state.downgrade();
        let owner = self.state.entity_id();
        let show_size = self.show_file_size;
        let drag_drop = self.drag_drop;
        let muted = Theme::of(cx).tokens.muted_foreground;
        let accent = Theme::of(cx).tokens.accent;
        let mut container = div()
            .id(self.id)
            .relative()
            .size_full()
            .min_h(px(0.0))
            .on_key_down(move |event, _, cx| {
                if event.keystroke.key == "f10" && event.keystroke.modifiers.shift {
                    let _ = key.update(cx, |state, cx| {
                        if let Some(path) = state.tree_state.read(cx).active_id().cloned() {
                            state.menu = Some((path, point(px(16.0), px(32.0))));
                            cx.notify();
                        }
                    });
                    cx.stop_propagation();
                }
            })
            .child(
                VirtualTreeList::new("filesystem-rows", model, tree_state)
                    .label(label)
                    .when_some(selected, |tree, path| tree.selected_id(path))
                    .on_select(move |path, _, cx| {
                        let _ = select.update(cx, |state, cx| state.select(path, cx));
                    })
                    .on_toggle(move |path, expanded, _, cx| {
                        let _ =
                            toggle.update(cx, |state, cx| state.set_expanded(path, expanded, cx));
                    })
                    .on_activate(move |path, _, cx| {
                        let _ = open.update(cx, |state, cx| state.open(path, cx));
                    })
                    .decorate_row(move |path, row, _, cx| {
                        let Some(state) = decorate.upgrade() else {
                            return row;
                        };
                        let Some(entry) =
                            state.read(cx).catalog.entries.get(path.as_path()).cloned()
                        else {
                            return row;
                        };
                        let menu_state = state.downgrade();
                        let menu_path = path.clone();
                        let double_state = state.downgrade();
                        let double_path = path.clone();
                        let mut row = row
                            .on_mouse_down(MouseButton::Right, move |event, _, cx| {
                                let _ = menu_state.update(cx, |state, cx| {
                                    state.select(&menu_path, cx);
                                    state.menu = Some((menu_path.clone(), event.position));
                                    cx.notify();
                                });
                                cx.stop_propagation();
                            })
                            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                if event.click_count == 2 {
                                    let _ = double_state
                                        .update(cx, |state, cx| state.open(&double_path, cx));
                                }
                            });
                        if let Some(directory) = state.read(cx).load_state(path) {
                            let status = match directory {
                                FileTreeLoadState::Loading => Some("Loading…".into()),
                                FileTreeLoadState::Failed(message) => {
                                    Some(format!("Error: {message}"))
                                }
                                _ => None,
                            };
                            if let Some(status) = status {
                                row = row.child(div().text_xs().text_color(muted).child(status));
                            }
                        } else if show_size && let Some(size) = entry.size {
                            row = row
                                .child(div().text_xs().text_color(muted).child(format_size(size)));
                        }
                        if drag_drop && !state.read(cx).roots.contains(path) {
                            let drag = FileTreeDrag {
                                owner,
                                path: path.clone(),
                                name: entry.name.clone(),
                            };
                            row = row.on_drag(drag, |data: &FileTreeDrag, _, _, cx| {
                                cx.new(|_| data.clone())
                            });
                        }
                        if drag_drop && entry.is_directory() {
                            let drop_state = state.downgrade();
                            let target = path.clone();
                            row = row
                                .drag_over::<FileTreeDrag>(move |style, _, _, _| {
                                    style.bg(accent.opacity(0.45))
                                })
                                .on_drop(move |data: &FileTreeDrag, _, cx| {
                                    if data.owner == owner {
                                        let _ = drop_state.update(cx, |state, cx| {
                                            state.request_drop(&data.path, &target, cx)
                                        });
                                    }
                                });
                        }
                        row
                    }),
            );
        if let Some((path, position)) = self.state.read(cx).menu.clone() {
            let open_state = self.state.downgrade();
            let open_path = path.clone();
            let mut items = vec![
                ContextMenuItem::new("open", "Open")
                    .icon("folder-open")
                    .on_click(move |_, cx| {
                        let _ = open_state.update(cx, |state, cx| {
                            state.menu = None;
                            state.open(&open_path, cx);
                        });
                    }),
            ];
            if self
                .state
                .read(cx)
                .entry(&path)
                .is_some_and(FileTreeEntry::is_directory)
            {
                let refresh_state = self.state.downgrade();
                let refresh_path = path.clone();
                items.push(
                    ContextMenuItem::new("reload", "Reload directory")
                        .icon("refresh-cw")
                        .on_click(move |_, cx| {
                            let _ = refresh_state.update(cx, |state, cx| {
                                state.menu = None;
                                state.reload(&refresh_path, cx);
                            });
                        }),
                );
            }
            if !self.state.read(cx).roots.contains(&path) {
                let cut_state = self.state.downgrade();
                let cut_path = path.clone();
                items.push(
                    ContextMenuItem::new("cut", "Pick up to move")
                        .icon("scissors")
                        .on_click(move |_, cx| {
                            let _ = cut_state.update(cx, |state, cx| {
                                state.menu = None;
                                state.cut(&cut_path, cx);
                            });
                        }),
                );
            }
            if self
                .state
                .read(cx)
                .entry(&path)
                .is_some_and(FileTreeEntry::is_directory)
                && self.state.read(cx).cut.is_some()
            {
                let paste_state = self.state.downgrade();
                let paste_path = path.clone();
                let disabled =
                    self.state.read(cx).cut.as_ref().is_none_or(|source| {
                        self.state.read(cx).validate_drop(source, &path).is_err()
                    });
                items.push(
                    ContextMenuItem::new("paste", "Move picked-up entry here")
                        .icon("clipboard")
                        .disabled(disabled)
                        .on_click(move |_, cx| {
                            let _ = paste_state.update(cx, |state, cx| {
                                state.menu = None;
                                let _ = state.paste_into(&paste_path, cx);
                                cx.notify();
                            });
                        }),
                );
            }
            for action in self.context_actions {
                let action_state = self.state.downgrade();
                let action_path = path.clone();
                let action_id = action.id.clone();
                items.push(
                    ContextMenuItem::new(action.id, action.label)
                        .destructive(action.destructive)
                        .on_click(move |_, cx| {
                            let _ = action_state.update(cx, |state, cx| {
                                state.menu = None;
                                cx.emit(FileTreeEvent::ContextAction {
                                    path: action_path.clone(),
                                    action: action_id.clone(),
                                });
                                cx.notify();
                            });
                        }),
                );
            }
            let close = self.state.downgrade();
            container = container.child(ContextMenu::new(position).items(items).on_close(
                move |_, cx| {
                    let _ = close.update(cx, |state, cx| {
                        state.menu = None;
                        cx.notify();
                    });
                },
            ));
        }
        container.map(|mut element| {
            element.style().refine(&self.style);
            element
        })
    }
}

#[cfg(test)]
mod virtual_file_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn entry(path: &str, kind: FileNodeKind) -> FileTreeEntry {
        FileTreeEntry::new(path, kind)
    }
    fn state(cx: &mut Context<FileTreeState>) -> FileTreeState {
        FileTreeState::new(
            vec![entry("/project", FileNodeKind::Directory)],
            |request| {
                Ok(match request.path.to_str().unwrap() {
                    "/project" => vec![
                        entry("/project/src", FileNodeKind::Directory),
                        entry("/project/readme.md", FileNodeKind::File),
                        entry("/project/archive", FileNodeKind::Directory),
                        entry("/project/.hidden", FileNodeKind::File),
                    ],
                    "/project/src" => vec![entry("/project/src/main.rs", FileNodeKind::File)],
                    _ => Vec::new(),
                })
            },
            cx,
        )
        .unwrap()
    }

    #[::core::prelude::v1::test]
    fn cached_keys_sort_directories_first_and_normalize_once() {
        let mut entries = vec![
            entry("/root/z", FileNodeKind::Directory),
            entry("/root/A", FileNodeKind::File),
            entry("/root/a", FileNodeKind::Directory),
            entry("/root/İ", FileNodeKind::File),
        ];
        let key = entries[3].sort_key.clone();
        sort_entries(&mut entries);
        assert_eq!(
            entries.iter().map(|entry| entry.name()).collect::<Vec<_>>(),
            ["a", "z", "A", "İ"]
        );
        assert_eq!(key.as_ref(), "i\u{307}");
        assert_eq!(
            entry("/root/a/../b", FileNodeKind::File).path(),
            Path::new("/root/b")
        );
        let mut many = (0..100_000)
            .rev()
            .map(|index| entry(&format!("/root/file-{index:06}"), FileNodeKind::File))
            .collect::<Vec<_>>();
        sort_entries(&mut many);
        assert_eq!(many[0].name(), "file-000000");
        assert_eq!(many[99_999].name(), "file-099999");
    }

    #[::core::prelude::v1::test]
    fn async_load_pipeline_is_bounded_and_unmount_cancels_pending_sources() {
        type Reply = futures::channel::oneshot::Sender<Result<Vec<FileTreeEntry>, String>>;
        let mut cx = TestAppContext::single();
        let pending = Arc::new(std::sync::Mutex::new(
            Vec::<(FileTreeLoadRequest, Reply)>::new(),
        ));
        let state = cx.new({
            let pending = pending.clone();
            move |cx| {
                FileTreeState::with_async_loader(
                    (0..5)
                        .map(|index| entry(&format!("/root-{index}"), FileNodeKind::Directory))
                        .collect(),
                    move |request| {
                        let pending = pending.clone();
                        Box::pin(async move {
                            let (reply, result) = futures::channel::oneshot::channel();
                            pending.lock().unwrap().push((request, reply));
                            result.await.map_err(|_| "cancelled source".to_string())?
                        })
                    },
                    cx,
                )
                .unwrap()
            }
        });
        state.update(&mut cx, |state, cx| {
            state.set_load_limits(2, 2);
            for index in 0..5 {
                state.set_expanded(Path::new(&format!("/root-{index}")), true, cx);
            }
            assert_eq!(state.pipeline_count(), 2);
            assert_eq!(state.queued.len(), 2);
            assert_eq!(state.loading_count(), 4);
            assert!(matches!(
                state.load_state(Path::new("/root-4")),
                Some(FileTreeLoadState::Failed(_))
            ));
        });
        cx.run_until_parked();
        let mut requests = std::mem::take(&mut *pending.lock().unwrap());
        assert_eq!(requests.len(), 2);
        let original = requests
            .iter()
            .find(|(request, _)| request.path == Path::new("/root-0"))
            .unwrap()
            .0
            .clone();
        state.update(&mut cx, |state, cx| state.reload(Path::new("/root-0"), cx));
        cx.run_until_parked();
        assert!(original.is_cancelled());
        let (new_request, reply) = pending.lock().unwrap().pop().unwrap();
        assert_eq!(new_request.path, Path::new("/root-0"));
        reply
            .send(Ok(vec![entry("/root-0/latest", FileNodeKind::File)]))
            .unwrap();
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert!(state.entry(Path::new("/root-0/latest")).is_some());
            assert_eq!(state.cached_entry_count(), 6);
            assert!(state.pipeline_count() <= 2);
            assert!(state.directories.len() <= 5);
        });
        requests.extend(std::mem::take(&mut *pending.lock().unwrap()));
        let weak = state.downgrade();
        drop(state);
        // Entity destruction flushes at the next foreground App update.
        cx.update(|_| {});
        cx.run_until_parked();
        assert!(weak.upgrade().is_none());
        assert!(requests.iter().all(|(request, _)| request.is_cancelled()));
    }

    #[::core::prelude::v1::test]
    fn view_changes_coalesce_and_cancel_superseded_snapshot_rows() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/project"), true, cx)
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            for index in 0..101 {
                state.set_show_hidden(index % 2 == 0, cx);
            }
            assert!(
                state
                    .prepare_task
                    .as_ref()
                    .unwrap()
                    .cancel_rows
                    .load(Ordering::Relaxed)
            );
            assert!(state.snapshot_requested);
            assert!(state.mutations.is_empty());
            assert_eq!(
                state.snapshot.len(),
                4,
                "keep the prior coherent snapshot while preparing"
            );
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert_eq!(state.snapshot.len(), 5);
            assert!(!state.is_preparing());
            assert!(
                state.directories.is_empty(),
                "loaded state comes from the immutable catalog"
            );
        });
    }

    #[::core::prelude::v1::test]
    fn directory_heavy_expansion_keeps_foreground_baseline_shared_and_bounds_deltas() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/project"), true, cx)
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            let mut ids = (0..100_000)
                .map(|i| PathBuf::from(format!("/project/cached-{i}")))
                .collect::<HashSet<_>>();
            ids.insert(PathBuf::from("/project"));
            state.expanded = Arc::new(FileExpansion {
                ids,
                executor: cx.background_executor().clone(),
            });
            let prior = state.expanded.clone();
            let started = std::time::Instant::now();
            state
                .try_set_expanded(Path::new("/project"), false, cx)
                .unwrap();
            eprintln!(
                "100001 expanded directories: foreground branch change {:?}",
                started.elapsed()
            );
            assert!(state.prepare_task.is_some());
            assert!(Arc::ptr_eq(&prior, &state.expanded));
            assert_eq!(prior.ids.len(), 100_001);
            assert!(!state.is_expanded(Path::new("/project")));
            state
                .try_set_expanded(Path::new("/project"), true, cx)
                .unwrap();
            state
                .try_set_expanded(Path::new("/project"), false, cx)
                .unwrap();
            assert!(Arc::ptr_eq(&prior, &state.expanded));
            assert!(!state.is_expanded(Path::new("/project")));
            for i in 0..FILE_TREE_MAX_PENDING_EXPANSIONS - 1 {
                state
                    .stage_expansion(Path::new(&format!("/project/new-{i}")), true)
                    .unwrap();
            }
            assert_eq!(
                state.expansion_changes.len(),
                FILE_TREE_MAX_PENDING_EXPANSIONS
            );
            assert!(
                state
                    .stage_expansion(Path::new("/project/overflow"), true)
                    .is_err()
            );
            assert!(!state.is_expanded(Path::new("/project/overflow")));
            // Repeated edits coalesce without growing the bounded batch.
            state
                .stage_expansion(Path::new("/project/new-0"), false)
                .unwrap();
            assert_eq!(
                state.expansion_changes.len(),
                FILE_TREE_MAX_PENDING_EXPANSIONS
            );
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert!(!state.is_expanded(Path::new("/project")));
            assert!(!state.is_preparing());
            assert!(state.expansion_changes.is_empty());
            assert!(state.preparing_expansion_changes.is_none());
            assert!(
                state.expanded.ids.is_empty(),
                "worker prunes unavailable synthetic branches"
            );
        });
    }

    #[::core::prelude::v1::test]
    fn stale_listing_preserves_requested_expansion_and_newer_deltas() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/project"), true, cx)
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            let executor = cx.background_executor().clone();
            let requested = Arc::new(FileExpansion {
                ids: HashSet::from([PathBuf::from("/project"), PathBuf::from("/project/src")]),
                executor: executor.clone(),
            });
            state.preparing_expansion_changes = Some(Arc::new(FileExpansionChanges {
                values: HashMap::from([(PathBuf::from("/project/src"), true)]),
                executor: executor.clone(),
            }));
            state
                .stage_expansion(Path::new("/project/archive"), true)
                .unwrap();
            state
                .stage_expansion(Path::new("/project/src"), false)
                .unwrap();
            let generation = state.allocate_generation();
            state.directories.insert(
                PathBuf::from("/project"),
                FileDirectory {
                    status: FileTreeLoadState::Loading,
                    generation,
                },
            );
            let prepared = PreparedFiles {
                catalog: state.catalog.clone(),
                snapshot: None,
                expanded: Some(Arc::new(FileExpansion {
                    ids: HashSet::new(),
                    executor: executor.clone(),
                })),
                requested_expanded: requested.clone(),
                mutation: FileMutation::Listing {
                    path: PathBuf::from("/project"),
                    generation: generation - 1,
                    listing: Arc::new(FileListing {
                        entries: Vec::new(),
                        executor,
                    }),
                },
                base_revision: state.catalog_revision,
                view_generation: state.view_generation,
                label: state.accessibility_label.clone(),
                result: Ok(()),
                elapsed: Default::default(),
            };
            state.commit_prepared(prepared, cx);
            assert!(
                Arc::ptr_eq(&state.expanded, &requested),
                "a rejected listing cannot adopt pruned expansion"
            );
            assert!(state.is_expanded(Path::new("/project/archive")));
            assert!(!state.is_expanded(Path::new("/project/src")));
            assert!(state.entry(Path::new("/project/src")).is_some());
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert!(state.is_expanded(Path::new("/project/archive")));
            assert!(!state.is_expanded(Path::new("/project/src")));
        });
    }

    #[::core::prelude::v1::test]
    fn lazy_loads_cache_collapsed_branches_and_release_evicted_memory() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            assert_eq!(state.cached_entry_count(), 1);
            state.set_expanded(Path::new("/project"), true, cx);
            assert_eq!(
                state.load_state(Path::new("/project")),
                Some(&FileTreeLoadState::Loading)
            );
            assert_eq!(state.snapshot.len(), 1);
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            assert_eq!(state.cached_entry_count(), 5);
            assert_eq!(state.snapshot.len(), 4);
            assert_eq!(
                state.load_state(Path::new("/project")),
                Some(&FileTreeLoadState::Loaded)
            );
            state.set_expanded(Path::new("/project/src"), true, cx);
        });
        cx.run_until_parked();
        let cached = state.update(&mut cx, |state, cx| {
            assert_eq!(state.cached_entry_count(), 6);
            assert_eq!(state.snapshot.len(), 5);
            let cached = Arc::downgrade(
                state
                    .catalog
                    .entries
                    .get(Path::new("/project/src/main.rs"))
                    .unwrap(),
            );
            state.set_expanded(Path::new("/project/src"), false, cx);
            assert_eq!(state.cached_entry_count(), 6);
            state.set_show_hidden(true, cx);
            state.evict_directory(Path::new("/project/src"), cx);
            assert_eq!(state.cached_entry_count(), 6);
            assert_eq!(
                state.load_state(Path::new("/project/src")),
                Some(&FileTreeLoadState::Unloaded)
            );
            cached
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert_eq!(state.cached_entry_count(), 5);
            assert_eq!(state.snapshot.len(), 5);
            assert!(
                cached.upgrade().is_none(),
                "eviction must release stale entries"
            );
        });
    }

    #[::core::prelude::v1::test]
    fn stale_responses_invalid_paths_limits_and_cancellation_do_not_corrupt_model() {
        let mut cx = TestAppContext::single();
        let calls = Arc::new(AtomicUsize::new(0));
        let state = cx.new({
            let calls = calls.clone();
            move |cx| {
                FileTreeState::new(
                    vec![entry("/root", FileNodeKind::Directory)],
                    move |_| {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Ok(vec![entry("/outside", FileNodeKind::File)])
                    },
                    cx,
                )
                .unwrap()
            }
        });
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/root"), true, cx);
            state.evict_directory(Path::new("/root"), cx);
        });
        cx.run_until_parked();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            0,
            "cancelled queued load must not run"
        );
        state.update(&mut cx, |state, cx| {
            state.apply_listing(
                Path::new("/root"),
                1,
                Ok(vec![entry("/root/stale", FileNodeKind::File)]),
                cx,
            );
            assert_eq!(state.cached_entry_count(), 1);
            state.set_expanded(Path::new("/root"), true, cx);
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert!(matches!(state.load_state(Path::new("/root")), Some(FileTreeLoadState::Failed(message)) if message.contains("non-child")));
            assert_eq!(state.cached_entry_count(), 1);
        });
        let limited = cx.new(|cx| {
            FileTreeState::new(
                vec![entry("/root", FileNodeKind::Directory)],
                |_| {
                    Ok(vec![
                        entry("/root/a", FileNodeKind::File),
                        entry("/root/b", FileNodeKind::File),
                    ])
                },
                cx,
            )
            .unwrap()
        });
        limited.update(&mut cx, |state, cx| {
            state.set_max_entries(1);
            state.set_expanded(Path::new("/root"), true, cx);
        });
        cx.run_until_parked();
        limited.update(&mut cx, |state, _| assert!(matches!(state.load_state(Path::new("/root")), Some(FileTreeLoadState::Failed(message)) if message.contains("entry limit"))));
    }

    #[::core::prelude::v1::test]
    fn move_validation_rejects_cycles_roots_non_directories_and_supports_keyboard_pickup() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/project"), true, cx)
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/project/src"), true, cx)
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, cx| {
            assert!(
                state
                    .validate_drop(Path::new("/project"), Path::new("/project/archive"))
                    .is_err()
            );
            assert!(
                state
                    .validate_drop(Path::new("/project/src"), Path::new("/project/src"))
                    .is_err()
            );
            assert!(
                state
                    .validate_drop(Path::new("/project/src"), Path::new("/project/readme.md"))
                    .is_err()
            );
            assert!(
                state
                    .validate_drop(Path::new("/project/readme.md"), Path::new("/project"))
                    .is_err()
            );
            assert!(state.cut(Path::new("/project/src/main.rs"), cx));
            assert_eq!(
                state.paste_into(Path::new("/project/archive"), cx).unwrap(),
                FileTreeDrop {
                    source: "/project/src/main.rs".into(),
                    target_directory: "/project/archive".into()
                }
            );
            assert!(state.cut.is_none());
        });
    }

    struct FileHost {
        state: Entity<FileTreeState>,
    }
    impl Render for FileHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            VirtualFileTree::new("file-test", self.state.clone())
                .context_actions(vec![FileTreeContextAction::new("inspect", "Inspect file")])
                .w(px(480.0))
                .h(px(200.0))
        }
    }
    #[::core::prelude::v1::test]
    fn rendered_explorer_expands_and_opens_context_actions_through_real_input() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(state);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| FileHost { state }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&state.focus_handle(cx));
        });
        window.simulate_keystrokes("right");
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(state.read(cx).snapshot.len(), 4);
            assert_eq!(window.accessibility_tree().focused_node_count(), 1);
        });
        window.simulate_keystrokes("end shift-f10");
        window.run_until_parked();
        let inspect = window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(
                state.read(cx).menu.as_ref().unwrap().0,
                PathBuf::from("/project/readme.md")
            );
            window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.label.as_deref() == Some("Inspect file"))
                .expect("actual context menu is mounted")
                .id
        });
        window.update(|window, _| {
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                inspect,
                AccessibilityAction::Click,
            ))
        });
        window.run_until_parked();
        window.update(|_, cx| assert!(state.read(cx).menu.is_none()));
    }

    #[::core::prelude::v1::test]
    fn pointer_drag_emits_validated_file_move_request() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/project"), true, cx)
        });
        cx.run_until_parked();
        let drops = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let _events = cx.update({
            let drops = drops.clone();
            let state = state.clone();
            move |cx| {
                cx.subscribe(&state, move |_, event: &FileTreeEvent, _| {
                    if let FileTreeEvent::DropRequested(request) = event {
                        drops.borrow_mut().push(request.clone());
                    }
                })
            }
        });
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| FileHost { state }
        });
        let (source, target) = window.update(|window, cx| {
            window.draw(cx).clear();
            let center = |label: &str| {
                let rect = window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .find(|node| {
                        node.label.as_deref() == Some(label)
                            && node.role == AccessibilityRole::TreeItem
                    })
                    .unwrap()
                    .bounds
                    .unwrap();
                point(
                    px((rect.x + rect.width / 2.0) as f32),
                    px((rect.y + rect.height / 2.0) as f32),
                )
            };
            (center("readme.md"), center("archive"))
        });
        window.simulate_mouse_down(source, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_move(
            source + point(px(12.0), px(8.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(cx.has_active_drag());
        });
        window.simulate_mouse_move(target, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(target, MouseButton::Left, Modifiers::default());
        window.run_until_parked();
        assert_eq!(
            &*drops.borrow(),
            &[FileTreeDrop {
                source: "/project/readme.md".into(),
                target_directory: "/project/archive".into()
            }]
        );
    }

    #[::core::prelude::v1::test]
    fn hundred_thousand_files_mount_only_viewport_and_bulk_refresh_releases_entries() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| crate::theme::install_theme(cx, Theme::dark()));
        let calls = Arc::new(AtomicUsize::new(0));
        let state = cx.new(move |cx| {
            FileTreeState::new(
                vec![entry("/root", FileNodeKind::Directory)],
                move |_| {
                    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        Ok((0..100_000)
                            .rev()
                            .map(|index| {
                                entry(&format!("/root/file-{index:06}"), FileNodeKind::File)
                            })
                            .collect())
                    } else {
                        Ok(Vec::new())
                    }
                },
                cx,
            )
            .unwrap()
        });
        let started = std::time::Instant::now();
        state.update(&mut cx, |state, cx| {
            state.set_expanded(Path::new("/root"), true, cx)
        });
        cx.run_until_parked();
        let loaded = started.elapsed();
        let (apply, prepare) = state.update(&mut cx, |state, _| {
            (
                state.last_model_apply_duration(),
                state.last_model_prepare_duration(),
            )
        });
        let mount_started = std::time::Instant::now();
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| FileHost { state }
        });
        let first_mount = mount_started.elapsed();
        let retained = window.update(|window, cx| {
            window.draw(cx).clear();
            let state = state.read(cx);
            assert_eq!(state.snapshot.len(), 100_001);
            assert_eq!(state.cached_entry_count(), 100_001);
            assert_eq!(
                window
                    .accessibility_tree()
                    .nodes
                    .values()
                    .filter(|node| node.role == AccessibilityRole::TreeItem)
                    .count(),
                100_001,
                "offscreen files retain logical accessibility without mounted geometry"
            );
            assert!(state.tree_state.read(cx).last_rendered_range().len() <= 7);
            assert!(
                    window
                        .accessibility_tree()
                        .nodes
                        .values()
                        .filter(|node| node.role == AccessibilityRole::TreeItem
                            && node.bounds.is_some())
                        .count()
                        <= 7
                );
            window.focus(&state.focus_handle(cx));
            Arc::downgrade(
                state
                    .catalog
                    .entries
                    .get(Path::new("/root/file-099999"))
                    .unwrap(),
            )
        });
        let mut redraws = Vec::with_capacity(20);
        for _ in 0..20 {
            let started = std::time::Instant::now();
            window.update(|window, cx| window.draw(cx).clear());
            redraws.push(started.elapsed());
        }
        redraws.sort_unstable();
        let navigation_started = std::time::Instant::now();
        window.simulate_keystrokes("end");
        let navigation = window.update(|window, cx| {
            window.draw(cx).clear();
            let navigation = navigation_started.elapsed();
            assert_eq!(
                state
                    .read(cx)
                    .tree_state
                    .read(cx)
                    .active_id()
                    .map(PathBuf::as_path),
                Some(Path::new("/root/file-099999"))
            );
            assert_eq!(window.accessibility_tree().focused_node_count(), 1);
            let focused = &window.accessibility_tree().nodes
                [&window.accessibility_tree().focused_node().unwrap()];
            let active = &window.accessibility_tree().nodes[&focused.active_descendant.unwrap()];
            assert_eq!(active.label.as_deref(), Some("file-099999"));
            navigation
        });
        let refresh = std::time::Instant::now();
        window.update(|_, cx| state.update(cx, |state, cx| state.reload(Path::new("/root"), cx)));
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(state.read(cx).cached_entry_count(), 1);
            assert_eq!(state.read(cx).snapshot.len(), 1);
        });
        // Replacing the mounted logical snapshot drops its old metadata into
        // the worker reclaimer rather than blocking this foreground draw.
        window.run_until_parked();
        assert!(retained.upgrade().is_none());
        eprintln!(
            "100k file model total load={loaded:?}, worker preparation={prepare:?}, foreground Arc commit={apply:?}, window creation+first draw={first_mount:?}, redraw p50={:?} p95={:?}, End dispatch+draw={navigation:?}, total refresh={:?}, foreground refresh commit={:?}",
            redraws[10],
            redraws[18],
            refresh.elapsed(),
            state.update(&mut cx, |state, _| state.last_model_apply_duration())
        );
    }

    #[::core::prelude::v1::test]
    fn global_entry_budget_rejects_listing_without_partial_cache_changes() {
        let mut cx = TestAppContext::single();
        let state = cx.new(state);
        state.update(&mut cx, |state, cx| {
            state.set_cached_entry_budget(2).unwrap();
            state.set_expanded(Path::new("/project"), true, cx);
        });
        cx.run_until_parked();
        state.update(&mut cx, |state, _| {
            assert_eq!(state.cached_entry_count(), 1);
            assert_eq!(state.snapshot.len(), 1);
            assert!(matches!(state.load_state(Path::new("/project")), Some(FileTreeLoadState::Failed(message)) if message.contains("cached entry budget")));
            assert!(state.set_cached_entry_budget(0).is_err());
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[::core::prelude::v1::test]
    fn native_directory_loader_detects_symlinks_without_following_them() {
        let root = std::env::temp_dir().join(format!(
            "kael-file-loader-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("note.txt"), b"abc").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
        let entries = read_directory(FileTreeLoadRequest {
            path: root.clone(),
            cancelled: Arc::new(AtomicBool::new(false)),
            max_entries: 10,
        })
        .unwrap();
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.name() == "note.txt")
                .unwrap()
                .size(),
            Some(3)
        );
        #[cfg(unix)]
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.name() == "loop")
                .unwrap()
                .kind(),
            FileNodeKind::Symlink
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
