//! Custom theme demo: brand your app with `Theme::custom`, switch themes live,
//! and override individual components with the `Styled` trait.

use kael_ui::components::scrollable::scrollable_vertical;
use kael_ui::prelude::*;
use kael_ui::theme::{install_theme, use_theme, Theme, ThemeTokens, ThemeVariant};
use std::path::PathBuf;

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        std::fs::read(self.base.join(path))
            .map(|data| Some(std::borrow::Cow::Owned(data)))
            .map_err(|err| err.into())
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        std::fs::read_dir(self.base.join(path))
            .map(|entries| {
                entries
                    .filter_map(|entry| {
                        entry
                            .ok()
                            .and_then(|entry| entry.file_name().into_string().ok())
                            .map(SharedString::from)
                    })
                    .collect()
            })
            .map_err(|err| err.into())
    }
}

fn brand_theme() -> Theme {
    Theme::custom(ThemeTokens {
        primary: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
        primary_foreground: hsla(0.0, 0.0, 1.0, 1.0),
        accent: hsla(199.0 / 360.0, 0.89, 0.48, 1.0),
        accent_foreground: hsla(0.0, 0.0, 1.0, 1.0),
        ring: hsla(262.0 / 360.0, 0.83, 0.58, 1.0),
        radius_sm: px(2.0),
        radius_md: px(10.0),
        radius_lg: px(14.0),
        radius_xl: px(20.0),
        ..ThemeTokens::dark()
    })
}

fn main() {
    Application::new()
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        })
        .run(|cx| {
            kael_ui::init(cx);
            kael_ui::set_icon_base_path("assets/icons");
            install_theme(cx, brand_theme());

            cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("Custom Theme Demo".into()),
                        ..Default::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(960.0), px(820.0)),
                    })),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| CustomThemeDemo::default()),
            )
            .unwrap();
        });
}

#[derive(Default)]
struct CustomThemeDemo {
    notifications: bool,
}

impl CustomThemeDemo {
    fn theme_button(
        &self,
        id: &'static str,
        name: &'static str,
        theme: Theme,
        active: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id, name)
            .variant(if active {
                ButtonVariant::Default
            } else {
                ButtonVariant::Outline
            })
            .size(ButtonSize::Sm)
            .on_click(cx.listener(move |_, _, _, cx| {
                install_theme(cx, theme.clone());
                cx.notify();
            }))
    }
}

impl Render for CustomThemeDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();
        let tokens = &theme.tokens;
        let variant = theme.variant;

        div()
            .size_full()
            .bg(tokens.background)
            .text_color(tokens.foreground)
            .overflow_hidden()
            .child(scrollable_vertical(
                div()
                    .flex()
                    .flex_col()
                    .p(px(32.0))
                    .gap(px(28.0))
                    .child(
                        VStack::new()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .text_size(px(26.0))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Theme Customization"),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(tokens.muted_foreground)
                                    .child(
                                        "Build a branded theme with Theme::custom, switch any \
                                             of the 18 presets live, and override single \
                                             components with the Styled trait.",
                                    ),
                            ),
                    )
                    .child(
                        VStack::new()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(format!(
                                        "1. Live theme switching — active: {}",
                                        theme.variant.display_name()
                                    )),
                            )
                            .child(
                                HStack::new()
                                    .gap(px(8.0))
                                    .flex_wrap()
                                    .child(self.theme_button(
                                        "t-brand",
                                        "Brand (Custom)",
                                        brand_theme(),
                                        variant == ThemeVariant::Custom,
                                        cx,
                                    ))
                                    .child(self.theme_button(
                                        "t-dark",
                                        "Dark",
                                        Theme::dark(),
                                        variant == ThemeVariant::Dark,
                                        cx,
                                    ))
                                    .child(self.theme_button(
                                        "t-light",
                                        "Light",
                                        Theme::light(),
                                        variant == ThemeVariant::Light,
                                        cx,
                                    ))
                                    .child(self.theme_button(
                                        "t-tokyo",
                                        "Tokyo Night",
                                        Theme::tokyo_night(),
                                        variant == ThemeVariant::TokyoNight,
                                        cx,
                                    ))
                                    .child(self.theme_button(
                                        "t-cat",
                                        "Catppuccin",
                                        Theme::catppuccin_mocha(),
                                        variant == ThemeVariant::CatppuccinMocha,
                                        cx,
                                    ))
                                    .child(self.theme_button(
                                        "t-nord",
                                        "Nord",
                                        Theme::nord(),
                                        variant == ThemeVariant::Nord,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        VStack::new()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("2. Components pick the theme up automatically"),
                            )
                            .child(
                                HStack::new()
                                    .gap(px(10.0))
                                    .items_center()
                                    .flex_wrap()
                                    .child(Button::new("b-primary", "Primary"))
                                    .child(
                                        Button::new("b-secondary", "Secondary")
                                            .variant(ButtonVariant::Secondary),
                                    )
                                    .child(
                                        Button::new("b-outline", "Outline")
                                            .variant(ButtonVariant::Outline),
                                    )
                                    .child(
                                        Button::new("b-ghost", "Ghost")
                                            .variant(ButtonVariant::Ghost),
                                    )
                                    .child(
                                        Button::new("b-destructive", "Destructive")
                                            .variant(ButtonVariant::Destructive),
                                    ),
                            )
                            .child(
                                HStack::new()
                                    .gap(px(10.0))
                                    .items_center()
                                    .child(Badge::new("Default"))
                                    .child(Badge::new("Tokens").variant(BadgeVariant::Secondary))
                                    .child(Badge::new("Drive").variant(BadgeVariant::Outline))
                                    .child(
                                        Badge::new("Everything").variant(BadgeVariant::Destructive),
                                    ),
                            )
                            .child(
                                HStack::new()
                                    .gap(px(16.0))
                                    .items_center()
                                    .child(
                                        Checkbox::new("c-updates")
                                            .label("Receive updates")
                                            .checked(self.notifications)
                                            .on_click(cx.listener(|view, checked, _, cx| {
                                                view.notifications = *checked;
                                                cx.notify();
                                            })),
                                    )
                                    .child(ProgressBar::new(0.66).w(px(220.0))),
                            ),
                    )
                    .child(
                        VStack::new()
                            .gap(px(12.0))
                            .child(
                                div()
                                    .text_size(px(17.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("3. Per-component overrides via Styled"),
                            )
                            .child(
                                HStack::new()
                                    .gap(px(10.0))
                                    .items_center()
                                    .flex_wrap()
                                    .child(
                                        Button::new("s-pill", "Pill override")
                                            .rounded(px(999.0))
                                            .px(px(28.0)),
                                    )
                                    .child(
                                        Button::new("s-brand", "One-off color")
                                            .variant(ButtonVariant::Ghost)
                                            .bg(tokens.accent)
                                            .text_color(tokens.accent_foreground)
                                            .shadow_md(),
                                    )
                                    .child(
                                        Button::new("s-wide", "Fixed width")
                                            .variant(ButtonVariant::Outline)
                                            .w(px(220.0)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .p(px(16.0))
                            .rounded(tokens.radius_lg)
                            .bg(tokens.muted.opacity(0.4))
                            .border_1()
                            .border_color(tokens.border)
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .text_color(tokens.muted_foreground)
                                    .child(
                                        "Theme::custom(ThemeTokens { primary, radius_md, .. \
                                             ..ThemeTokens::dark() }) — start from any preset, \
                                             override only what your brand needs. install_theme() \
                                             refreshes every open window.",
                                    ),
                            ),
                    ),
            ))
    }
}
