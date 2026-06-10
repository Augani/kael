# kael_ui

A comprehensive, professional UI component library for [Kael](https://github.com/Augani/kael), the GPU-accelerated native UI framework for Rust. Inspired by [shadcn/ui](https://ui.shadcn.com/), `kael_ui` provides 100+ polished, accessible components for building beautiful, performant desktop applications — no extra component library needed.

`kael_ui` is the continuation of [adabraka-ui](https://github.com/Augani/adabraka-ui), now developed in-tree so that Kael ships everything you need to build rich desktop apps out of the box.

## Features

- **Complete theme system** — built-in light/dark themes with shadcn-style semantic color tokens
- **100+ components** — buttons, inputs, dialogs, data tables, charts, command palettes, and more
- **Responsive layout** — flexible layout utilities (`VStack`, `HStack`, `Grid`, `ScrollContainer`)
- **Professional animations** — smooth transitions with cubic-bezier easing and spring physics
- **Typography system** — built-in `Text` component with semantic variants and bundled Inter / JetBrains Mono fonts
- **Code editor** — multi-line editor with tree-sitter syntax highlighting and full keyboard support
- **Accessibility** — keyboard navigation and focus management across components
- **High performance** — optimized for Kael's GPU-accelerated rendering with virtual scrolling for large datasets

## Installation

```toml
[dependencies]
kael = "0.1"
kael_ui = "0.1"
```

## Quick Start

```rust,ignore
use kael::*;
use kael_ui::{prelude::*, theme};

fn main() {
    Application::new().run(|cx: &mut App| {
        // Install a theme and initialize the component library
        theme::install_theme(cx, theme::Theme::dark());
        kael_ui::init(cx);

        cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| MyApp)
        })
        .unwrap();
    });
}

struct MyApp;

impl Render for MyApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_8()
            .bg(theme.tokens.background)
            .child(Button::new("hello", "Click me").on_click(|_, _, _| {
                println!("Clicked!");
            }))
    }
}
```

## Module Overview

| Module       | What's inside                                                                 |
| ------------ | ----------------------------------------------------------------------------- |
| `components` | Core interactive elements: buttons, inputs, selects, sliders, date pickers, file upload, OTP input, editor, and dozens more |
| `display`    | Presentation components: tables, data grids, cards, badges, accordions, markdown/HTML rendering |
| `navigation` | Sidebars, menus, tabs, breadcrumbs, toolbars, file trees, status bars         |
| `overlays`   | Dialogs, sheets, popovers, toasts, tooltips, context menus, command palettes  |
| `charts`     | Line, area, bar, pie, donut, radar charts, gauges, heatmaps, treemaps        |
| `layout`     | `VStack`, `HStack`, `Grid`, `ScrollContainer`, responsive utilities          |
| `theme`      | Design tokens and theming with light/dark variants                            |
| `animations` | Easing presets, springs, transitions, animated state helpers                 |

## Icons

Components use [Lucide](https://lucide.dev/) icons resolved from a configurable asset path. The SVGs ship in this repository under `crates/kael_ui/assets/icons`. Point the loader at your app's icon directory at startup:

```rust,ignore
kael_ui::set_icon_base_path("assets/icons");
```

## Feature Flags

| Feature            | Default | Description                                          |
| ------------------ | ------- | ---------------------------------------------------- |
| `http`             | yes     | Remote image loading for `Avatar`, `Image`, etc.     |
| `markdown`         | no      | Markdown rendering via `pulldown-cmark`              |
| `html-render`      | no      | HTML rendering via `html5ever`                       |
| `audio`            | no      | Audio playback for `AudioPlayer` via `rodio`         |
| `editor-languages` | no      | Tree-sitter grammars for editor syntax highlighting  |

## Examples

Over 140 runnable examples live in [`examples/`](examples/):

```bash
cargo run -p kael_ui --example button_demo
cargo run -p kael_ui --example data_table_styled_demo
cargo run -p kael_ui --example command_palette_styled_demo
cargo run -p kael_ui --example pie_chart_demo
```

## License

Apache-2.0, like the rest of the Kael workspace.
