# Theming

Kael's theme system provides JSON/TOML-based theming with hot-reload support. The `Theme` type implements `Global`, making it available anywhere in your app.

## Built-in themes

```rust
// Initialize theme system
Theme::init(cx);

// Switch themes
cx.set_global(Theme::dark());
cx.set_global(Theme::light());

// Match system appearance
cx.set_global(Theme::for_appearance(window));
```

## Theme from JSON

```json
{
  "name": "Ocean",
  "appearance": "dark",
  "background": "#0a1628",
  "foreground": "#e2e8f0",
  "primary": "#3b82f6",
  "secondary": "#64748b",
  "accent": "#06b6d4",
  "error": "#ef4444",
  "warning": "#f59e0b",
  "success": "#22c55e",
  "border": "#1e293b",
  "surface": "#0f172a",
  "muted": "#334155"
}
```

Load it:

```rust
let theme = Theme::from_json_str(json_str)?;
cx.set_global(theme);
```

## Theme from TOML

```toml
name = "Forest"
appearance = "dark"
background = "#1a2e1a"
foreground = "#d4e6d4"
primary = "#22c55e"
```

```rust
let theme = Theme::from_toml_str(toml_str)?;
cx.set_global(theme);
```

## Loading from file

```rust
let theme = Theme::from_path("themes/custom.json")?;
cx.set_global(theme);
```

## Hot-reload

Automatically reload theme when the file changes:

```rust
use kael::ThemeRuntime;

ThemeRuntime::watch("themes/active.json", cx);
// Theme reloads automatically when the file is saved
```

## Using theme colors in views

```rust
impl Render for MyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();

        div()
            .bg(theme.background)
            .text_color(theme.foreground)
            .border_color(theme.border)
            .child(
                div()
                    .bg(theme.primary)
                    .text_color(rgb(0xffffff))
                    .px_4().py_2()
                    .rounded(px(6.0))
                    .child("Primary button")
            )
    }
}
```

## Theme properties

| Property | Description |
|----------|-------------|
| `name` | Theme name |
| `appearance` | `"light"` or `"dark"` |
| `background` | Main background color |
| `foreground` | Main text color |
| `primary` | Primary action color |
| `secondary` | Secondary/muted action color |
| `accent` | Highlight/accent color |
| `error` | Error state color |
| `warning` | Warning state color |
| `success` | Success state color |
| `border` | Default border color |
| `surface` | Elevated surface color |
| `muted` | Subdued text/element color |
