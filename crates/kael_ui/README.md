# kael_ui

An optional, themeable component system for
[Kael](https://github.com/Augani/kael). It provides production-oriented inputs,
data surfaces, charts, editors, navigation, overlays, feedback, media controls,
and layout helpers while preserving the normal Kael styling API.

Applications can use `kael` without this crate. Choose `kael_ui` when you want
ready-made components that can be reshaped around a product's own design tokens
and brand.

Start with the
[component guide](https://augani.github.io/kael/component-library.html), then
use this crate's module and type documentation while implementing a view.

```toml
[dependencies]
kael = "0.4"
kael_ui = "0.4"
```

```rust,ignore
use kael_ui::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::tokyo_night());

        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Welcome)
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}

struct Welcome;

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(Button::new("welcome", "Build with Kael"))
    }
}
```

## Branding

Install a preset or construct `ThemeTokens` for your product. Individual
components also accept Kael's `Styled` methods, so a component can be adjusted
without forking the library.

```rust,ignore
install_theme(cx, Theme::custom(ThemeTokens {
    primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
    radius_md: px(10.0),
    ..ThemeTokens::dark()
}));
```

The component icons work without copying assets into an application. `kael_ui`
bundles the compact Lucide subset its components use and resolves it through
virtual `kael-icons/<name>.svg` paths. To replace the set with branded SVGs:

```rust,ignore
kael_ui::set_icon_base_path("assets/icons");
```

## Features

| Feature | Default | Purpose |
| --- | --- | --- |
| `native` | yes | Desktop font and window backends |
| `browser` | no | Lean WebAssembly/WebGL2 component surface |
| `editor` | yes | Rope and tree-sitter editor core |
| `http` | yes | Reqwest transport for remote images and HTTP-backed assets |
| `markdown` | no | Markdown rendering |
| `html-render` | no | Native HTML document rendering |
| `audio` | no | Audio-player integration |
| `media` | no | Kael media integration used by the Astryx showcase |
| `image-avif`, `image-exr` | no | Opt-in AVIF (libdav1d) and OpenEXR image decoding |
| `editor-languages` | no | Additional tree-sitter grammars |

## Astryx

The repository keeps one consolidated component showcase:

```bash
cargo run -p kael_ui --example astryx_showcase \
  --features "media kael/runtime_shaders"
```

Astryx and its assets are repository-only and are not part of the crate package.
Crate consumers receive the library and its required font assets, not the
example application or its media.

## Large tree explorers

`VirtualTreeList` mounts only the visible rows of large expanded hierarchies.
Build a reusable `VirtualTreeModel` when data, expansion, or filtering changes,
and retain `VirtualTreeState` for one keyboard focus handle, stable active node
IDs, and scroll-to-item navigation. Selection and expansion stay controlled by
the application. Displayed IDs must be unique. Mounted rows expose hierarchy
levels and working accessibility actions; offscreen accessibility nodes are not
synthesized.

```bash
cargo run -p kael_ui --example virtual_tree
```

The example displays 100,000 files across 25 expandable projects. Use arrows to
navigate, Left/Right to collapse or expand, Home/End to jump, and Enter to select.
For smaller trees, `TreeList::shared_nodes(Arc<[TreeNode<T>]>)` reuses immutable
source models across redraws without cloning descendants.

## Suite-scale release workload

For a spreadsheet surface, use `VirtualSheetGrid` instead of constructing one
column definition or one row entity per logical coordinate. It supports the
Excel-scale 1,000,000 × 16,384 address space with two-axis virtual mounting,
generation-scoped tile requests, frozen panes, an LRU tile cache, sparse edits,
IME-backed cell editing, and bounded TSV/HTML clipboard interchange:

```rust,ignore
let model = model.clone();
let sheet = cx.new(|cx| {
    VirtualSheetGrid::new(1_000_000, 16_384, cx)
        .expect("document dimensions are validated")
        .with_frozen_panes(1, 2)
        .expect("frozen panes are bounded")
        .on_fetch_tile(move |request, entity, _window, cx| {
            let values = model.load_row_major(&request.rows, &request.columns);
            cx.defer(move |cx| {
                entity.update(cx, |sheet, cx| {
                    if sheet.provide_tile(request, values).is_ok() {
                        cx.notify();
                    }
                });
            });
        })
        .on_commit_edit(|edit, _window, _cx| {
            // Persist edit.position and edit.value in the application model.
        })
});
```

Tile responses must exactly match a live request and its generation. Cache,
pending-request, tile-cell, tile-byte, cell-byte, edit, undo, and clipboard
limits are public constants or builders so an application can budget them
explicitly. Clipboard export returns an error for unloaded cells instead of
silently exporting incomplete data.

The repository also keeps a same-source desktop/WebAssembly workload for the
framework paths used by document, spreadsheet, presentation, and whiteboard
applications:

```bash
cargo run -p kael_ui --example suite_scale_smoke
bash scripts/verify-browser-suite-smoke.sh
```

It verifies a real million-row × 16,384-column virtual sheet grid and compressed selection,
virtual document pages and slide thumbnails, and 100,000 retained whiteboard
shapes with spatial culling, tiled damage, bounded tile payloads, rich pointer
input, and a fixed frame clock.

Desktop workflows also include a reusable asynchronous `FileTreeState` /
`VirtualFileTree`, persistent `DockWorkspace` tab/split/floating layouts, and a
typed `PropertyInspector` with atomic edits and bounded undo/redo. Filesystem
catalog preparation and reclamation run on workers; viewport mounting and
prepared Arc swaps keep large loads out of the foreground render path.

```bash
cargo run -p kael_ui --example filesystem_explorer
cargo run -p kael_ui --release --example filesystem_explorer -- --stress
cargo run -p kael_ui --example desktop_workspace
```

The explorer performs real moves inside its demo directory; passing an existing
directory makes it read only. The workspace demonstrates nested docking,
pointer/keyboard floating panes, background JSON save/restore, live typed editing
and pane state retained across tab changes.

The document/data workbench combines the existing Markdown source editor and
worker-parsed rich preview with a real loopback HTTP record source. Its 100,000
logical records support frozen panes, query cancellation, editing, clipboard,
undo/redo and bounded accessibility metadata; writes keep the query identity that
was active when each edit occurred.

```bash
cargo run -p kael_ui --features markdown --example document_data_workbench
```

Licensed under Apache-2.0.
