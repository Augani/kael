# Component Library (kael_ui)

Kael ships a complete, shadcn-inspired component library: **`kael_ui`**. It provides 100+ polished, accessible components so you can build rich desktop applications with Kael alone — no external component library required.

`kael_ui` is the continuation of [adabraka-ui](https://github.com/Augani/adabraka-ui), now developed inside the Kael repository at [`crates/kael_ui`](https://github.com/Augani/kael/tree/main/crates/kael_ui).

## Installation

```toml
[dependencies]
kael = "0.3"
kael_ui = "0.3"
```

## Setup

One import gives you everything — the components plus the Kael essentials
(`div`, `px`, `Render`, `Application`, …). You do not need a separate
`use kael::*;`, and mixing the two globs is discouraged because the names
collide:

```rust,ignore
use kael_ui::prelude::*;

fn main() {
    Application::new().run(|cx: &mut App| {
        kael_ui::init(cx);
        install_theme(cx, Theme::dark());

        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| MyApp)
        })
        .unwrap();
    });
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
adjustments. The [`custom_theme_demo`](https://github.com/Augani/kael/blob/main/crates/kael_ui/examples/custom_theme_demo.rs) example shows all three layers together.

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
cargo run -p kael_ui --example custom_theme_demo
cargo run -p kael_ui --example components_showcase
cargo run -p kael_ui --example data_table_styled_demo
cargo run -p kael_ui --example command_palette_styled_demo
cargo run -p kael_ui --example sidebar_styled_demo
cargo run -p kael_ui --example date_picker_demo
```

## Template apps

Three complete starter applications live in [`templates/`](https://github.com/Augani/kael/tree/main/templates) — copy one as the skeleton of your own app:

```bash
cargo run -p dashboard-app    # analytics: sidebar, stat cards, charts, data table
cargo run -p messaging-app    # chat: conversation list, message bubbles, composer
cargo run -p workspace-app    # IDE shell: file tree, syntax-highlighted editor, status bar
```
