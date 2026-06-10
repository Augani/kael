# Component Library (kael_ui)

Kael ships a complete, shadcn-inspired component library: **`kael_ui`**. It provides 100+ polished, accessible components so you can build rich desktop applications with Kael alone — no external component library required.

`kael_ui` is the continuation of [adabraka-ui](https://github.com/Augani/adabraka-ui), now developed inside the Kael repository at [`crates/kael_ui`](https://github.com/Augani/kael/tree/main/crates/kael_ui).

## Installation

```toml
[dependencies]
kael = "0.1"
kael_ui = "0.1"
```

## Setup

Install a theme and initialize the library before opening windows:

```rust,ignore
use kael::*;
use kael_ui::{prelude::*, theme};

fn main() {
    Application::new().run(|cx: &mut App| {
        theme::install_theme(cx, theme::Theme::dark());
        kael_ui::init(cx);

        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| MyApp)
        })
        .unwrap();
    });
}
```

`kael_ui::init(cx)` registers the bundled Inter and JetBrains Mono fonts, sets up keybindings for interactive components (inputs, selects, the editor, sidebars, popovers, sheets, dialogs), and initializes the HTTP client used for remote image loading.

## Using the theme

Every component reads from the active theme. Access it in your own render code with `use_theme()`:

```rust,ignore
impl Render for MyApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
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

Tokens follow shadcn/ui naming: `background`/`foreground`, `primary`, `secondary`, `muted`, `accent`, `destructive`, `border`, `card`, and so on, each with light and dark variants. Switch themes at runtime with `install_theme(cx, Theme::light())`.

## What's included

| Module       | Components                                                                  |
| ------------ | --------------------------------------------------------------------------- |
| `components` | Button, IconButton, Input, Textarea, SearchInput, NumberInput, OtpInput, TagInput, MentionInput, HotkeyInput, Checkbox, Radio, Toggle, Switch, Slider, RangeSlider, Select, Combobox, Dropdown, DatePicker, TimePicker, Calendar, ColorPicker, Rating, FileUpload, Avatar, AvatarGroup, Progress, Spinner, Skeleton, Stepper, Pagination, Carousel, Timeline, QrCode, CopyButton, InlineEdit, code Editor with tree-sitter syntax highlighting, audio/video players, and many more |
| `display`    | Table, DataTable, DataGrid, Card, Badge, Accordion, RichText, Markdown and HTML rendering (feature-gated) |
| `navigation` | Sidebar, Menu, AppMenu, Tabs, Breadcrumbs, Toolbar, StatusBar, Tree, FileTree, VirtualList |
| `overlays`   | Dialog, AlertDialog, ConfirmDialog, Sheet, BottomSheet, Popover, PopoverMenu, HoverCard, ContextMenu, Toast, Tooltip, CommandPalette |
| `charts`     | LineChart, AreaChart, BarChart, PieChart, DonutChart, RadarChart, Gauge, Heatmap, Treemap, Sparkline |
| `layout`     | VStack, HStack, Grid, ScrollContainer, responsive breakpoint helpers        |
| `animations` | Easing presets, springs, transitions, animated presence/state, shimmer, confetti, and other motion effects |

## Icons

Components render [Lucide](https://lucide.dev/) icons by name, resolved against a configurable base path. The 1,600+ SVGs ship in the repository under `crates/kael_ui/assets/icons`. Point the resolver at your app's asset directory at startup:

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
| `editor-languages` | no      | Tree-sitter grammars for 20+ languages in the editor |

## Examples

More than 140 runnable demos live in [`crates/kael_ui/examples`](https://github.com/Augani/kael/tree/main/crates/kael_ui/examples):

```bash
cargo run -p kael_ui --example button_demo
cargo run -p kael_ui --example data_table_styled_demo
cargo run -p kael_ui --example command_palette_styled_demo
cargo run -p kael_ui --example sidebar_styled_demo
cargo run -p kael_ui --example pie_chart_demo
cargo run -p kael_ui --example date_picker_demo
```
