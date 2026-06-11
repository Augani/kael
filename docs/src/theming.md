# Theming

Kael has **one** theming pipeline with two cooperating types, each with a clear
role:

- **`kael::Theme`** — the *serializable, file-facing* theme. It is what a
  JSON/TOML theme file deserializes into (`colors`, `typography`, `spacing`,
  `radii`, `shadows`), and it is what the hot-reload file watcher reloads. Think
  of it as the theme *on disk*.
- **`kael_ui::Theme`** (a `variant` plus a rich [`ThemeTokens`]) — the *runtime
  token system* that components actually render from. Every kael_ui component
  reads `Theme::of(cx).tokens.*`. Think of it as the theme *in memory*.

A bridge connects them so that editing a theme file restyles live components.

## The pipeline

```text
  theme.toml / theme.json          (you edit this)
        │  file watcher (App::observe_theme_file)
        ▼
  kael::Theme                      (parsed, stored as a Global)
        │  App::observe_theme_files subscriber
        ▼
  kael_ui::ThemeTokens             (core fields mapped onto current tokens)
        │  install_theme  →  set_global + refresh_windows
        ▼
  components                       (re-render with Theme::of(cx).tokens.*)
```

Each stage is one observable hop: a file edit walks all the way down to a
visible restyle, with no restart.

## Reading the theme in components

Components read tokens through the zero-clone borrow:

```rust
impl Render for MyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);

        div()
            .bg(theme.tokens.background)
            .text_color(theme.tokens.foreground)
            .border_color(theme.tokens.border)
            .child(
                div()
                    .bg(theme.tokens.primary)
                    .text_color(theme.tokens.primary_foreground)
                    .px(px(16.0))
                    .py(px(8.0))
                    .rounded(theme.tokens.radius_md)
                    .child("Primary button"),
            )
    }
}
```

`Theme::of(cx)` (alias `Theme::get(cx)`) borrows the theme from the app's global
state without cloning. `use_theme()` is a legacy clone-per-call shim retained
for call sites where the borrow checker cannot take a `&Theme` cleanly; prefer
`Theme::of(cx)` everywhere else.

## Presets, custom themes, and live switching

`kael_ui` ships 18 presets and lets you brand your app from any of them:

```rust
kael_ui::init(cx);
install_theme(cx, Theme::dark());

// Brand it: start from a preset's tokens and override what you need.
let brand = Theme::custom(ThemeTokens {
    primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
    radius_md: px(10.0),
    ..ThemeTokens::dark()
});
install_theme(cx, brand);
```

`install_theme` stores the active `Theme` as a `Global` and refreshes every open
window, so re-installing at runtime switches themes live. See
[Component Library](component-library.md#custom-themes-and-live-switching).

## Theme files

A theme file deserializes into a `kael::Theme`. Fields are grouped; omit any
section to keep its defaults.

```toml
[colors]
background = "#0b1020"
surface    = "#161c2e"
primary    = "#6366f1"
accent     = "#22d3ee"
muted      = "#3b4252"
foreground = "#e5e7eb"
border     = "#2a3350"
error      = "#ef4444"

[radii]
sm = 4.0
md = 8.0
lg = 12.0
xl = 16.0

[typography]
ui_font_family   = "Inter"
code_font_family = "JetBrains Mono"
```

The same shape works as JSON. Load one directly with:

```rust
let theme = kael::Theme::from_path("themes/active.toml")?;
cx.set_global(theme);
```

## Hot-reload end-to-end

Wire the file watcher and the bridge once during startup. After that, every save
to the watched file restyles the live UI:

```rust
Application::new().run(move |cx| {
    kael_ui::init(cx);
    install_theme(cx, Theme::dark());

    // Register the bridge: maps reloaded kael::Theme -> ThemeTokens, then
    // install_theme (refreshing all windows).
    install_theme_file_bridge(cx);

    // Watch the file; on_change updates the core kael::Theme global, which
    // fires the bridge subscriber registered above.
    cx.observe_theme_file("themes/active.toml", |theme, cx| cx.set_global(theme))
        .expect("failed to watch theme file");

    // ... open your window; components read Theme::of(cx).tokens.*
});
```

Order matters only in that the bridge must be registered before (or alongside)
the watcher; `observe_theme_file` applies the initial file once, and every
later save flows through the same path. A runnable example lives at
`crates/kael_ui/examples/theme_hot_reload_demo.rs` — run it, edit the printed
TOML path, and watch the buttons, badges, card, and accent bars recolor.

## Core → token mapping

When a theme file reloads, the bridge maps the loaded `kael::Theme` onto the
**currently installed** `ThemeTokens`. Token fields without a core source are
preserved, so a partial file changes only what it names.

| core `kael::Theme` field       | `ThemeTokens` field(s)   |
|--------------------------------|--------------------------|
| `colors.background`            | `background`             |
| `colors.foreground`            | `foreground`             |
| `colors.surface`               | `card`, `popover`        |
| `colors.primary`               | `primary`, `ring`        |
| `colors.accent`                | `accent`                 |
| `colors.muted`                 | `muted`                  |
| `colors.border`                | `border`, `input`        |
| `colors.error`                 | `destructive`            |
| `radii.sm` / `md` / `lg` / `xl`| `radius_sm/md/lg/xl`     |
| `shadows.sm` / `md` / `lg`     | `shadow_sm/md/lg`        |
| `typography.ui_font_family`    | `font_family`            |
| `typography.code_font_family`  | `font_mono`              |

Fields with no core source keep their existing token values: the `*_foreground`
colors, `secondary`, `muted_foreground`, `accent_foreground`, `shadow_xs`,
`shadow_xl`, `ring_offset`, and the spacing / duration / z-index scales. Core
fields with no token target (`separator`, `selected_text`, `warning`,
`success`, `radii.pill`, and the typographic sizes/weights) are intentionally
not mapped.

If you need a different mapping, call `tokens_from_core_theme(core, base)`
yourself inside a custom `cx.observe_theme_files` subscriber.
