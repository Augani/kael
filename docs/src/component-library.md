# Component Library (kael_ui)

Kael ships a complete, shadcn-inspired component library: **`kael_ui`**. It provides 100+ polished, accessible components so you can build rich desktop applications with Kael alone — no external component library required.

`kael_ui` is the continuation of [adabraka-ui](https://github.com/Augani/adabraka-ui), now developed inside the Kael repository at [`crates/kael_ui`](https://github.com/Augani/kael/tree/main/crates/kael_ui).

## Installation

```toml
[dependencies]
kael = "0.4"
kael_ui = "0.4"
```

## Setup

One import gives you everything — the components plus the Kael essentials
(`div`, `px`, `Render`, `Application`, …). You do not need a separate
`use kael::*;`, and mixing the two globs is discouraged because the names
collide:

```rust,ignore
use kael_ui::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx: &mut App| {
        kael_ui::init(cx);
        install_theme(cx, Theme::dark());

        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| MyApp)
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}
```

`kael_ui::init(cx)` registers the bundled Inter and JetBrains Mono fonts, sets up keybindings for interactive components (inputs, selects, the editor, sidebars, popovers, sheets, dialogs), and initializes the HTTP client used for remote image loading.

## Using the theme

`install_theme` stores the active `Theme` in the app's global state, so the
recommended way to read it is `Theme::get(cx)` (or the alias `Theme::of(cx)`),
which borrows the theme out of `cx` with no per-render clone:

```rust,ignore
impl Render for MyApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .child(
                Button::new("save", "Save")
                    .variant(ButtonVariant::Default)
                    .on_click(|_, _, _| println!("Saved!")),
            )
    }
}
```

`use_theme()` is still available and returns an owned `Theme`; it is the legacy
path (it clones the whole theme on every call and does not take a `cx`). Prefer
`Theme::get(cx)` / `Theme::of(cx)` in new code.

Tokens follow shadcn/ui naming: `background`/`foreground`, `primary`, `secondary`, `muted`, `accent`, `destructive`, `border`, `card`, and so on, each with light and dark variants.

## Custom themes and live switching

Eighteen presets ship in-tree (`Theme::dark()`, `Theme::light()`,
`Theme::tokyo_night()`, `Theme::catppuccin_mocha()`, `Theme::nord()`, …), and
you can brand your app with `Theme::custom`: start from any preset's tokens and
override only what you need with struct-update syntax.

```rust,ignore
let brand = Theme::custom(ThemeTokens {
    primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
    primary_foreground: hsla(0.0, 0.0, 1.0, 1.0),
    radius_md: px(10.0),
    ..ThemeTokens::dark()
});
install_theme(cx, brand);
```

`install_theme` can be called again at any time — it refreshes every open
window, so components re-read the new tokens immediately. Wiring a theme picker
is just a button:

```rust,ignore
Button::new("theme-light", "Light").on_click(cx.listener(|_, _, _, cx| {
    install_theme(cx, Theme::light());
    cx.notify();
}))
```

## Customizing individual components

Every component implements Kael's `Styled` trait, so the entire Tailwind-like
styling API works directly on it — this is the `className` of kael_ui. User
styles are applied last and override the component's defaults:

```rust,ignore
Button::new("cta", "Get started")
    .rounded(px(999.0))          // pill shape
    .px(px(28.0))                // wider padding
    .bg(rgb(0x8b5cf6))           // one-off brand color
    .shadow_lg()

Card::new()
    .content(body("Hello"))
    .w(px(360.0))
    .border_2()
    .border_color(rgb(0x10b981))
```

Use the theme for app-wide identity and `Styled` overrides for one-off
adjustments. The repository-only Astryx showcase composes both approaches in
one application.

## What's included

| Module       | Components                                                                  |
| ------------ | --------------------------------------------------------------------------- |
| `components` | Button, IconButton, Input, Textarea, SearchInput, NumberInput, OtpInput, TagInput, MentionInput, HotkeyInput, Checkbox, Radio, Toggle, Switch, Slider, RangeSlider, Select, Combobox, Dropdown, DatePicker, TimePicker, Calendar, ColorPicker, Rating, FileUpload, Avatar, AvatarGroup, Progress, Spinner, Skeleton, Stepper, Pagination, Carousel, Timeline, QrCode, CopyButton, InlineEdit, code Editor with tree-sitter syntax highlighting, audio/video players, and many more |
| `display`    | Table, DataTable, DataGrid, Card, Badge, Accordion, RichText, Markdown and HTML rendering (feature-gated) |
| `navigation` | Sidebar, Menu, AppMenu, Tabs, Breadcrumbs, Toolbar, StatusBar, Tree, VirtualTreeList, FileTree, VirtualFileTree, VirtualList |
| `overlays`   | Dialog, AlertDialog, ConfirmDialog, Sheet, BottomSheet, Popover, PopoverMenu, HoverCard, ContextMenu, Toast, Tooltip, CommandPalette |
| `charts`     | LineChart, AreaChart, BarChart, PieChart, DonutChart, RadarChart, Gauge, Heatmap, Treemap, Sparkline |
| `layout`     | VStack, HStack, Grid, ScrollContainer, responsive breakpoint helpers        |
| `animations` | Easing presets, springs, transitions, animated presence/state, shimmer, confetti, and other motion effects |

For native desktop code editors, markdown editors, log viewers, SQL consoles,
and prompt builders, prefer the native `Editor` before embedding Monaco,
CodeMirror, or a WebView textarea. `Position::to_text()`,
`Selection::to_text()`, `FoldRange::to_text()`,
`EditorDiagnostic::to_text()`, `EditorState::to_text()`, and
`Editor::to_text()` expose language, line/content byte counts, cursor and
selection geometry, modified/file-path presence, undo/redo depth, syntax and
highlight readiness, search counts/options, fold state, readonly mode,
diagnostic counts, and visual override coverage without logging document text,
file paths, selected text, search terms, diagnostic messages, or style callback
internals.

For native desktop dashboards, admin tools, file managers, and data-heavy
workspaces, prefer native `Table`, `DataTable`, and `DataGrid` components before
reaching for browser tables. `ColumnDef::to_text()`,
`DataTableState::to_text()`, `DataTable::to_text()`, `RowAction::to_text()`,
`GridColumnDef::to_text()`, `DataGridState::to_text()`, and
`DataGrid::to_text()` expose column/row counts, virtual backing, cached rows,
sort and selection state, editable columns, active edit buffers, search
presence, edit/row-action handlers, and load-more/fetch-page wiring without
logging headers, row values, ids, labels, queries, edit text, dimensions, or
callback internals.

For native desktop media players, timelines, podcast tools, video review
surfaces, and lightweight editors, prefer native `VideoPlayer`, `AudioPlayer`,
and `Waveform` before embedding a browser player. `VideoPlayer::to_text()`,
`VideoPlayerState::to_text()`, `VideoCaptionStyle::to_text()`,
`AudioPlayer::to_text()`, `AudioPlayerState::to_text()`, and
`Waveform::to_text()` expose source kind, route, player size,
controls/captions/poster/source/title presence, progress and volume buckets,
handler counts, and waveform sample shape without logging media URLs, file
paths, titles, caption text, exact seek times, volume or rate values, waveform
amplitudes, or colors.

For native desktop dialogs, sheets, custom menus, context menus, command
palettes, and omniboxes, prefer the native `Dialog`, `Sheet`, `BottomSheet`,
`Menu`, `ContextMenu`, `MenuBar`, and `CommandPalette` stacks instead of hosted
browser overlays. Their `to_text()` helpers expose size/purpose/dismissal
policy, header/content/footer presence, item/result counts, nesting, disabled
state, shortcut coverage, query presence/length, selection state, and handler
coverage without logging ids, labels, titles, descriptions, categories,
shortcut strings, user queries, coordinates, dimensions, child contents, or
callback internals.

## Tree models and rendering cost

For file explorers, navigation trees, and project outlines, `TreeList` builds
shallow row payloads and visits only expanded branches when no filter is active.
It computes keyboard parent navigation in one pass. Filtering borrows the model,
retains matching ancestors, and maps highlights back to the original label even
when Unicode lowercasing expands a character.

Keep large immutable models in an `Arc` and pass them with `shared_nodes` to
avoid cloning the entire hierarchy on unrelated redraws:

```rust,ignore
use std::sync::Arc;
use kael_ui::prelude::*;

// Store this model in your view state and replace it when the hierarchy changes.
let nodes: Arc<[TreeNode<u64>]> = vec![
    TreeNode::new(1, "Project").with_children(vec![
        TreeNode::new(2, "src").with_lazy_children(true),
        TreeNode::new(3, "Cargo.toml"),
    ]),
].into();

TreeList::new()
    .id("project-tree")
    .shared_nodes(nodes.clone())
    .expanded_ids(vec![1]);
```

`nodes(Vec<TreeNode<T>>)` remains available for owned models. Selection and
expansion remain controlled by the application; lazy child markers allow it to
load a branch on demand. `TreeList` mounts all expanded rows, so keep large
hierarchies collapsed or lazily populated, or use `VirtualTreeList` for large
expanded hierarchies. For large flat datasets, use `uniform_virtual_list` or
`variable_virtual_list` to mount only viewport rows.

`VirtualTreeList` pairs an immutable `VirtualTreeModel` with explicit
`VirtualTreeState`. Build the snapshot when data, expansion, or filtering changes;
clone it cheaply on redraws. The state owns one keyboard focus handle and keeps
an active logical ID as rows move. Up/Down skip disabled rows, Home/End jump,
Left/Right request expansion changes, and navigation scrolls the active row into
view. Selection and expansion callbacks remain controlled by your application.

```rust,ignore
// During view construction or when the hierarchy changes:
let expanded = std::collections::HashSet::from([1]);
let model = VirtualTreeModel::new(&nodes, &expanded)?;
let state = cx.new(|cx| VirtualTreeState::new(cx));

// During render; give the tree a bounded viewport height:
VirtualTreeList::new("project-tree", model.clone(), state.clone())
    .label("Project files")
    .h(px(480.0))
    .on_toggle(move |id, expanded, _window, cx| {
        // Update expanded IDs, rebuild the snapshot, and notify your view.
    });
```

Displayed IDs must be unique; model construction returns an error for duplicates.
`VirtualTreeModel::filtered` supports highlighted case-insensitive matching and
automatic ancestor expansion. A retained logical accessibility snapshot covers
offscreen items while only viewport rows receive mounted geometry. Focus remains
on the tree with an active descendant; Focus, Click, Expand, Collapse and reveal
actions resolve stable current model IDs. For large Send/Sync models, capture
`state.accessibility_preparation_context(label, has_toggle)` and call
`model.prepare_accessibility(context, &background_executor)` on a worker before
sharing the model. This prepares semantic labels/IDs and reclamation outside the
frame budget; the generic fallback prepares them synchronously. Model metadata
remains linear in expanded rows and element construction remains bounded by the
viewport. Native assistive-technology behavior still requires platform QA.

A runnable explorer with 100,000 files is included:

```bash
cargo run -p kael_ui --example virtual_tree
```

For reusable filesystem explorers, keep a `FileTreeState` entity and mount
`VirtualFileTree`. The native source loads only expanded directories and does
not follow symlinks; custom blocking and asynchronous loaders use the same
generation, cancellation and entry-budget checks.

```rust,ignore
let files = cx.new(|cx| FileTreeState::filesystem(project_root, cx).unwrap());
files.update(cx, |files, cx| files.set_expanded(project_root, true, cx));

VirtualFileTree::new("files", files.clone())
    .h(px(480.0))
    .show_file_size(true)
    .context_actions(vec![FileTreeContextAction::new("reveal", "Reveal")]);
```

Listings retain normalized sort keys and shallow entries in an immutable catalog.
Workers prepare catalog changes, row snapshots, logical accessibility and large-value reclamation.
Foreground commits swap prepared Arcs and bounded request metadata; redraws mount
only viewport rows. Expansion and cache eviction are asynchronous, so the previous
coherent snapshot remains visible until its replacement is ready. Expanded membership
uses shared immutable sets and bounded coalesced deltas; `try_set_expanded` rejects
a new distinct delta beyond `FILE_TREE_MAX_PENDING_EXPANSIONS` (4,096) without
changing requested intent, so callers may retry after preparation. Configure
`set_cached_entry_budget` and `set_load_limits` for application-specific storage.
`FileTreeEvent` reports selection, opening, context actions, failures and validated
drop requests. Applications perform file operations, then reload affected parents.
Pointer drag/drop and keyboard Pick up / Move here use the same cycle/root/target
validation. `filesystem_explorer` demonstrates actual moves inside a demo directory;
an explicit directory argument is read only, and `--stress` loads 100,000 synthetic
files on workers with preparation/commit diagnostics.

## Desktop workspaces and property editing

`DockWorkspaceState` combines an application-owned pane registry with a versioned
`DockLayout`. Stable pane IDs retain interactive entities when inactive tabs are
unmounted. The workspace renders nested resizable splits, movable tabs and groups,
edge docking, zoom, close/reopen, and floating groups with pointer/keyboard movement
and resizing. Floating groups occupy the workspace window. Runtime mutations and
restore validate node IDs, pane coverage, geometry and maximum split depth.

```rust,ignore
let workspace = cx.new(|cx| DockWorkspaceState::new(
    vec![
        DockPane::new("files", "Files", move |_, _| VirtualFileTree::new("pane-files", files.clone()).into_any_element()),
        DockPane::new("editor", "Editor", move |_, _| Editor::new(&editor).into_any_element()),
    ],
    DockLayout::group(["files", "editor"]),
    cx,
).unwrap());
workspace.update(cx, |state, cx| {
    state.move_pane("files", 1, DockPlacement::Left, cx);
});
DockWorkspace::new("workspace", workspace.clone());
```

Persist `layout_json()` on `DockWorkspaceEvent::LayoutChanged` and restore with
`restore_json()`. Render callbacks and runtime entities remain in the application;
invalid snapshots and impossible splits leave the current layout intact.

`PropertyInspectorState` declares groups of typed text, number, boolean and choice
fields. `PropertyInspector` renders the existing themed controls, descriptions,
read-only states, inline errors and bounded transactional Undo/Redo. `set_values`
validates an entire batch before changing any value; editor contents synchronize
after programmatic changes while ordinary typing preserves the cursor. Subscribe
to `PropertyInspectorEvent::Changed` to update application models or previews.
The runnable `desktop_workspace` example combines a lazy project explorer, live
typed properties, preview and retained notes with background layout save/restore.

## Document and remote-data workflows

The editor accepts native text/IME input, preserves Unicode grapheme boundaries
when moving/deleting, and treats selection replacement or an entire composition
as one undoable operation. `EditorState::set_selection_bytes` accepts checked UTF-8
anchor/focus offsets without editing history. Complete native accessibility text
metadata prepares once per content revision (on workers for large documents),
reuses its Arc during caret movement, and exposes native text selection after
preparation. Unfolded line coordinates map directly; collapsed folds retain compact
interval metadata, and paint/hit testing enumerate only viewport lines. Large
document syntax/fold extraction prepares on workers with revision checks. The
compatibility `display_lines()` method explicitly enumerates the complete document.
Caret blinking cancels on blur/deactivation and stops when a retained
editor no longer paints; reduced motion keeps a steady caret.
`EditorState::replace_selection` supports application
formatting commands. A Markdown editor plus worker-parsed rich preview, formatting
and Undo/Redo appears in `document_data_workbench`.

`RemoteSheetState<Q>` adds bounded asynchronous data sources to the existing
`VirtualSheetGrid`. A source receives a captured query, exact tile request and
cooperative cancellation token; stale replies never commit after a query change.
Invalid tile responses pause automatic fetching until explicit `refresh`.
Remote edits and paste require loaded previous values so Undo cannot overwrite
an unknown original value with an invented empty string; mixed loaded/unloaded
batches reject atomically.

```rust,ignore
let remote = cx.new(|cx| RemoteSheetState::new(
    row_count, column_count, query,
    move |request| Box::pin(async move {
        // Fetch exactly request.tile.rows × request.tile.columns in row-major
        // order. Check request.is_cancelled() during long operations.
        storage.fetch_tile(request.query.clone(), request.tile.clone()).await
    }),
    cx,
).unwrap());
RemoteSheet::new("records", remote.clone()).h(px(480.0));
```

Use `RemoteSheetOptions` to configure tile dimensions and cache/pending limits.
`state.grid()` exposes the existing selection, frozen panes, cell editing,
clipboard, undo/redo and named-column APIs. Handle `RemoteSheetEvent::Edited` to
write application records: its captured query/generation belongs to the edit,
even if a newer query is active when your write completes. `set_query` retires
positional edits/history because rows now represent different records;
`refresh` preserves local edits for the same records.

Grid accessibility keeps full column headers and logical dimensions, stable
coordinate IDs, the active descendant and at most 1,024 cached cell semantics.
Viewport cells add bounded geometry; independent scrolling preserves the logical
focused cell. Offscreen active/cached actions reveal and fetch current coordinates.
Unknown remote values stay marked as loading; values over 4 KiB expose a preview
and the complete value remains available through the cell editor. Accessibility
metadata and retired snapshots prepare/reclaim on workers. Reserving coordinate
IDs allocates no cell nodes, even for a million-by-XFD sheet.

```bash
cargo run -p kael_ui --features markdown --example document_data_workbench
```

The example uses a real loopback HTTP fixture with 100,000 records, sparse remote
writes, query reversal and frozen panes. Its source/preview document includes
tables, tasks and multilingual text. Native IME, clipboard and assistive-technology
QA should accompany the automated workflow tests for your supported platforms.

## Icons

Components render [Lucide](https://lucide.dev/) icons by name. `kael_ui`
bundles the compact set used by its built-in components, so published-crate
consumers do not need to copy framework assets into their application. An
application asset source is checked first, preserving branded overrides.

The complete 1,600+ SVG catalog remains repository-only under
`crates/kael_ui/assets/icons` for discovery and the Astryx showcase. Point the
resolver at your own icon directory to replace the bundled set:

```rust,ignore
kael_ui::set_icon_base_path("assets/icons");
```

## Feature flags

| Feature            | Default | Enables                                             |
| ------------------ | ------- | ---------------------------------------------------- |
| `http`             | yes     | Remote image loading (`Avatar`, image components)    |
| `markdown`         | no      | `display::markdown` rendering                        |
| `html-render`      | no      | `display::html` rendering                            |
| `audio`            | no      | `AudioPlayer` playback via rodio                     |
| `image-avif`, `image-exr` | no | Opt-in AVIF (libdav1d) and OpenEXR decoding      |
| `editor-languages` | no      | Tree-sitter grammars for 20+ languages in the editor |

## Showcase

The repository keeps one comprehensive, sectioned showcase instead of a large
collection of small examples:

```bash
cargo run -p kael_ui --example astryx_showcase \
  --features "markdown html-render audio media editor-languages"
```

The showcase is not part of the `kael_ui` crate package.

## Template apps

Three complete starter applications live in [`templates/`](https://github.com/Augani/kael/tree/main/templates) — copy one as the skeleton of your own app:

```bash
cargo run -p dashboard-app    # analytics: sidebar, stat cards, charts, data table
cargo run -p messaging-app    # chat: conversation list, message bubbles, composer
cargo run -p workspace-app    # IDE shell: file tree, syntax-highlighted editor, status bar
```
