use kael_ui::prelude::*;
use std::path::PathBuf;

const DEFAULT_THEME_TOML: &str = r##"
[colors]
background = "#0b1020"
surface = "#161c2e"
primary = "#6366f1"
accent = "#22d3ee"
muted = "#3b4252"
foreground = "#e5e7eb"
border = "#2a3350"
error = "#ef4444"

[radii]
sm = 4.0
md = 8.0
lg = 12.0
xl = 16.0
"##;

fn theme_file_path() -> PathBuf {
    if let Ok(path) = std::env::var("KAEL_THEME_FILE") {
        return PathBuf::from(path);
    }
    std::env::temp_dir().join("kael_theme_hot_reload_demo.toml")
}

fn main() {
    let theme_path = theme_file_path();
    if !theme_path.exists() {
        std::fs::write(&theme_path, DEFAULT_THEME_TOML.trim_start())
            .expect("failed to seed theme file");
    }
    println!("watching theme file: {}", theme_path.display());
    println!("edit it while the app runs to restyle the components");

    Application::new().run(move |cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::dark());
        install_theme_file_bridge(cx);
        cx.observe_theme_file(&theme_path, |theme, cx| cx.set_global(theme))
            .expect("failed to watch theme file");

        cx.open_window(
            WindowOptions {
                titlebar: Some(kael::TitlebarOptions {
                    title: Some("Theme Hot Reload".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(720.0), px(560.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ThemeHotReloadApp::new(window, cx)),
        )
        .unwrap();
    });
}

struct ThemeHotReloadApp;

impl ThemeHotReloadApp {
    fn new(_window: &mut Window, _cx: &mut App) -> Self {
        Self
    }
}

impl Render for ThemeHotReloadApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let tokens = theme.tokens.clone();

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(24.0))
            .bg(tokens.background)
            .p(px(32.0))
            .child(
                div()
                    .text_size(px(28.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens.foreground)
                    .child("Theme Hot Reload"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(Button::new("primary", "Primary").variant(ButtonVariant::Default))
                    .child(Button::new("secondary", "Secondary").variant(ButtonVariant::Secondary))
                    .child(Button::new("outline", "Outline").variant(ButtonVariant::Outline)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .child(Badge::new("Default"))
                    .child(Badge::new("Secondary").variant(BadgeVariant::Secondary))
                    .child(Badge::new("Destructive").variant(BadgeVariant::Destructive)),
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .rounded(tokens.radius_lg)
                    .border_1()
                    .border_color(tokens.border)
                    .bg(tokens.card)
                    .p(px(24.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tokens.foreground)
                            .child("Surface card"),
                    )
                    .child(
                        div()
                            .text_color(tokens.muted_foreground)
                            .child("Edit the watched TOML to recolor everything live."),
                    )
                    .child(
                        div()
                            .mt(px(8.0))
                            .h(px(48.0))
                            .w_full()
                            .rounded(tokens.radius_md)
                            .bg(tokens.primary),
                    )
                    .child(
                        div()
                            .h(px(48.0))
                            .w_full()
                            .rounded(tokens.radius_md)
                            .bg(tokens.accent),
                    ),
            )
    }
}
