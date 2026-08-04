# kael_ui

An optional, themeable component system for
[Kael](https://github.com/Augani/kael). It provides production-oriented inputs,
data surfaces, charts, editors, navigation, overlays, feedback, media controls,
and layout helpers while preserving the normal Kael styling API.

Applications can use `kael` without this crate. Choose `kael_ui` when you want
ready-made components that can be reshaped around a product's own design tokens
and brand.

```toml
[dependencies]
kael = "0.3"
kael_ui = "0.3"
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

## Features

| Feature | Default | Purpose |
| --- | --- | --- |
| `http` | yes | Remote images and HTTP-backed assets |
| `markdown` | no | Markdown rendering |
| `html-render` | no | Native HTML document rendering |
| `audio` | no | Audio-player integration |
| `media` | no | Kael media integration used by the Astryx showcase |
| `editor-languages` | no | Additional tree-sitter grammars |

## Astryx

The repository keeps one consolidated component showcase:

```bash
cargo run -p kael_ui --example astryx_showcase \
  --features "media kael/runtime_shaders"
```

Astryx and its assets are repository-only and are excluded from this published
crate. Package consumers receive the library and its required font assets, not
the example application or its media.

Licensed under Apache-2.0.
