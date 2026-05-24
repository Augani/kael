use kael::{
    App, Application, AssetSource, Bounds, BoxShadow, Context, Div, Result, ScrollHandle,
    SharedString, Window, WindowBounds, WindowOptions, div, hsla, linear_color_stop,
    linear_gradient, point, prelude::*, px, rgb, size, svg,
};
use std::borrow::Cow;
use std::path::PathBuf;

const MACOS_SYSTEM_BLUE: u32 = 0x0088FF;
const MACOS_SYSTEM_RED: u32 = 0xFF383C;
const MACOS_SYSTEM_GREEN: u32 = 0x34C759;
const MACOS_SYSTEM_ORANGE: u32 = 0xFF8D28;
const MACOS_SYSTEM_PURPLE: u32 = 0xCB30E0;
const MACOS_SYSTEM_GRAY: u32 = 0x8E8E93;

struct FsAssets {
    base: PathBuf,
}

impl AssetSource for FsAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        let full = self.base.join(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(Some(Cow::Owned(bytes))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let full = self.base.join(path);
        let mut entries = Vec::new();
        if let Ok(dir) = std::fs::read_dir(&full) {
            for entry in dir.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    entries.push(name.to_string().into());
                }
            }
        }
        Ok(entries)
    }
}

struct NativeComparison {
    scroll_handle: ScrollHandle,
}

fn section_title(title: &str) -> Div {
    div()
        .text_size(px(11.))
        .line_height(px(13.))
        .font_weight(kael::FontWeight::SEMIBOLD)
        .text_color(hsla(0., 0., 0.46, 1.0))
        .child(title.to_string().to_uppercase())
}

fn color_swatch(color: u32, name: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .child(div().size(px(48.)).rounded(px(8.)).bg(rgb(color)))
        .child(
            div()
                .text_size(px(10.))
                .line_height(px(12.))
                .text_color(hsla(0., 0., 0.46, 1.0))
                .child(name.to_string()),
        )
}

fn text(content: &str, font_size: f32) -> Div {
    div()
        .text_size(px(font_size))
        .line_height(px(font_size * 1.2))
        .text_color(hsla(0., 0., 0.1, 1.0))
        .child(content.to_string())
}

fn text_weighted(content: &str, font_size: f32, weight: kael::FontWeight) -> Div {
    div()
        .text_size(px(font_size))
        .line_height(px(font_size * 1.2))
        .font_weight(weight)
        .text_color(hsla(0., 0., 0.1, 1.0))
        .child(content.to_string())
}

impl Render for NativeComparison {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xffffff)).child(
            div()
                .id("native-comparison")
                .overflow_y_scroll()
                .size_full()
                .track_scroll(&self.scroll_handle)
                .child(
                    div()
                        .p_6()
                        .flex()
                        .flex_col()
                        .gap_6()
                        .child(header())
                        .child(text_section())
                        .child(colors_section())
                        .child(gradients_section())
                        .child(borders_section())
                        .child(card_section()),
                ),
        )
    }
}

fn header() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(24.))
                .line_height(px(29.))
                .font_weight(kael::FontWeight::BOLD)
                .text_color(hsla(0., 0., 0.1, 1.0))
                .child("Native macOS Rendering"),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(16.))
                .text_color(hsla(0., 0., 0.46, 1.0))
                .child("System font, native colors, standard controls"),
        )
}

fn text_section() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(section_title("Text Rendering"))
        .child(
            div()
                .bg(rgb(0xffffff))
                .rounded(px(8.))
                .p_4()
                .shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.08),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(2.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(text(
                    "11px \u{2014} The quick brown fox jumps over the lazy dog. 0123456789",
                    11.,
                ))
                .child(text(
                    "13px \u{2014} The quick brown fox jumps over the lazy dog. 0123456789",
                    13.,
                ))
                .child(text(
                    "15px \u{2014} The quick brown fox jumps over the lazy dog",
                    15.,
                ))
                .child(text(
                    "17px \u{2014} The quick brown fox jumps over the lazy dog",
                    17.,
                ))
                .child(text_weighted(
                    "20px semibold \u{2014} Native macOS Text",
                    20.,
                    kael::FontWeight::SEMIBOLD,
                ))
                .child(text_weighted(
                    "24px bold \u{2014} Kael vs Native",
                    24.,
                    kael::FontWeight::BOLD,
                )),
        )
}

fn colors_section() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(section_title("System Colors"))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .child(color_swatch(MACOS_SYSTEM_BLUE, "Blue"))
                .child(color_swatch(MACOS_SYSTEM_RED, "Red"))
                .child(color_swatch(MACOS_SYSTEM_GREEN, "Green"))
                .child(color_swatch(MACOS_SYSTEM_ORANGE, "Orange"))
                .child(color_swatch(MACOS_SYSTEM_PURPLE, "Purple"))
                .child(color_swatch(MACOS_SYSTEM_GRAY, "Gray")),
        )
        .child(section_title("Alpha Blending").mt_2())
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(6.))
                        .bg(hsla(0., 0., 0., 0.1)),
                )
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(6.))
                        .bg(hsla(0., 0., 0., 0.2)),
                )
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(6.))
                        .bg(hsla(0., 0., 0., 0.3)),
                )
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(6.))
                        .bg(hsla(0., 0., 0., 0.5)),
                )
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(6.))
                        .bg(hsla(0., 0., 0., 0.7)),
                )
                .child(
                    div()
                        .size(px(48.))
                        .rounded(px(6.))
                        .bg(hsla(0., 0., 0., 0.9)),
                ),
        )
}

fn gradients_section() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(section_title("Gradients"))
        .child(div().h(px(32.)).rounded(px(6.)).bg(linear_gradient(
            90.,
            linear_color_stop(hsla(0., 0., 0., 1.0), 0.0),
            linear_color_stop(hsla(0., 0., 1., 1.0), 1.0),
        )))
        .child(div().h(px(32.)).rounded(px(6.)).bg(linear_gradient(
            90.,
            linear_color_stop(hsla(211. / 360., 1.0, 0.5, 1.0), 0.0),
            linear_color_stop(hsla(0., 0., 1., 1.0), 1.0),
        )))
}

fn borders_section() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(section_title("Borders & Hairlines"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().h(px(0.5)).bg(rgb(0x000000)).w_full())
                .child(div().h(px(0.5)).bg(rgb(MACOS_SYSTEM_BLUE)).w_full())
                .child(div().h(px(0.5)).bg(rgb(MACOS_SYSTEM_RED)).w_full()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_3()
                .mt_2()
                .child(
                    div()
                        .size(px(64.))
                        .bg(rgb(0xffffff))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(hsla(0., 0., 0., 0.15)),
                )
                .child(
                    div()
                        .size(px(64.))
                        .bg(rgb(0xffffff))
                        .rounded(px(8.))
                        .border_2()
                        .border_color(rgb(MACOS_SYSTEM_BLUE)),
                )
                .child(
                    div()
                        .size(px(64.))
                        .bg(rgb(0xffffff))
                        .rounded(px(8.))
                        .border_2()
                        .border_color(rgb(MACOS_SYSTEM_RED)),
                ),
        )
}

fn card_section() -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(section_title("Card with Shadow"))
        .child(
            div()
                .bg(rgb(0xffffff))
                .rounded(px(10.))
                .shadow(vec![
                    BoxShadow {
                        color: hsla(0., 0., 0., 0.1),
                        offset: point(px(0.), px(2.)),
                        blur_radius: px(4.),
                        spread_radius: px(0.),
                        inset: false,
                    },
                    BoxShadow {
                        color: hsla(0., 0., 0., 0.06),
                        offset: point(px(0.), px(4.)),
                        blur_radius: px(12.),
                        spread_radius: px(0.),
                        inset: false,
                    },
                ])
                .border_1()
                .border_color(hsla(0., 0., 0., 0.06))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_4()
                        .py_3()
                        .border_b_1()
                        .border_color(hsla(0., 0., 0., 0.08))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(div().size(px(12.)).rounded_full().bg(rgb(MACOS_SYSTEM_RED)))
                                .child(div().size(px(12.)).rounded_full().bg(rgb(0xFFCC00)))
                                .child(
                                    div()
                                        .size(px(12.))
                                        .rounded_full()
                                        .bg(rgb(MACOS_SYSTEM_GREEN)),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.))
                                .line_height(px(16.))
                                .text_color(hsla(0., 0., 0.46, 1.0))
                                .text_center()
                                .child("Window Title"),
                        ),
                )
                .child(
                    div()
                        .p_4()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .size_10()
                                        .bg(rgb(MACOS_SYSTEM_BLUE))
                                        .rounded(px(8.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            svg()
                                                .path("icons/check.svg")
                                                .size_5()
                                                .text_color(rgb(0xffffff)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(16.))
                                                .font_weight(kael::FontWeight::MEDIUM)
                                                .text_color(hsla(0., 0., 0.1, 1.0))
                                                .child("General"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .line_height(px(13.))
                                                .text_color(hsla(0., 0., 0.46, 1.0))
                                                .child("Appearance, language, and startup"),
                                        ),
                                )
                                .child(div().flex_1())
                                .child(
                                    svg()
                                        .path("icons/chevron_right.svg")
                                        .size_4()
                                        .text_color(hsla(0., 0., 0.76, 1.0)),
                                ),
                        )
                        .child(div().h(px(1.)).bg(hsla(0., 0., 0., 0.08)).ml(px(52.)))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .size_10()
                                        .bg(rgb(MACOS_SYSTEM_ORANGE))
                                        .rounded(px(8.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            svg()
                                                .path("icons/chevron_right.svg")
                                                .size_5()
                                                .text_color(rgb(0xffffff)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .line_height(px(16.))
                                                .font_weight(kael::FontWeight::MEDIUM)
                                                .text_color(hsla(0., 0., 0.1, 1.0))
                                                .child("Keyboard"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .line_height(px(13.))
                                                .text_color(hsla(0., 0., 0.46, 1.0))
                                                .child("Shortcuts, input sources, and dictation"),
                                        ),
                                )
                                .child(div().flex_1())
                                .child(
                                    svg()
                                        .path("icons/chevron_right.svg")
                                        .size_4()
                                        .text_color(hsla(0., 0., 0.76, 1.0)),
                                ),
                        ),
                ),
        )
}

fn main() {
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    Application::new()
        .with_assets(FsAssets { base: assets_dir })
        .run(|cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(500.0), px(700.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(kael::TitlebarOptions {
                        title: Some("Native macOS Rendering".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| NativeComparison {
                        scroll_handle: ScrollHandle::new().always_show_scrollbars(),
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
