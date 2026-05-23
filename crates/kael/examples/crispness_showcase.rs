use kael::{
    div, hsla, point, prelude::*, px, rgb, size, svg, App, Application, AssetSource, Bounds,
    BoxShadow, Context, Div, Result, ScrollHandle, SharedString, Window, WindowBounds,
    WindowOptions,
};
use std::borrow::Cow;
use std::path::PathBuf;

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

struct CrispnessShowcase {
    scroll_handle: ScrollHandle,
}

fn card(title: &str) -> Div {
    div().flex().flex_col().gap_2().p_4().child(
        div()
            .text_xs()
            .text_color(hsla(0., 0., 0.5, 1.0))
            .child(title.to_string()),
    )
}

fn section_label(label: &str) -> Div {
    div().px_6().py_2().child(
        div()
            .text_sm()
            .font_weight(kael::FontWeight::SEMIBOLD)
            .text_color(hsla(0., 0., 0.3, 1.0))
            .child(label.to_string()),
    )
}

impl Render for CrispnessShowcase {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xf0f0f0))
            .flex()
            .flex_row()
            .child(
                div()
                    .id("crispness-showcase")
                    .overflow_y_scroll()
                    .flex_1()
                    .h_full()
                    .track_scroll(&self.scroll_handle)
                    .flex()
                    .flex_col()
            .child(
                div()
                    .px_6()
                    .pt_6()
                    .pb_2()
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(kael::FontWeight::BOLD)
                            .text_color(hsla(0., 0., 0.1, 1.0))
                            .child("Kael Rendering Crispness"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(hsla(0., 0., 0.5, 1.0))
                            .child("Pixel-snapped corners, premultiplied shadows, crisp clipping"),
                    ),
            )
            .child(section_label("Rounded Corners — pixel-snapped radii"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .px_6()
                    .pb_4()
                    .child(
                        card("2px radius").child(div().size_16().bg(rgb(0x007AFF)).rounded(px(2.))),
                    )
                    .child(
                        card("4px radius").child(div().size_16().bg(rgb(0x34C759)).rounded(px(4.))),
                    )
                    .child(
                        card("8px radius").child(div().size_16().bg(rgb(0xFF9500)).rounded(px(8.))),
                    )
                    .child(
                        card("12px radius")
                            .child(div().size_16().bg(rgb(0xFF3B30)).rounded(px(12.))),
                    )
                    .child(
                        card("Full circle").child(div().size_16().bg(rgb(0xAF52DE)).rounded_full()),
                    ),
            )
            .child(section_label("Borders + Corners — edge crispness"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .px_6()
                    .pb_4()
                    .child(
                        card("1px border").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(rgb(0xd1d1d6)),
                        ),
                    )
                    .child(
                        card("2px border").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(8.))
                                .border_2()
                                .border_color(rgb(0x007AFF)),
                        ),
                    )
                    .child(
                        card("Dashed").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(6.))
                                .border_1()
                                .border_dashed()
                                .border_color(rgb(0x8E8E93)),
                        ),
                    )
                    .child(
                        card("Colored border").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(10.))
                                .border_2()
                                .border_color(rgb(0xFF3B30)),
                        ),
                    ),
            )
            .child(section_label("Shadows — premultiplied alpha, no fringing"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_6()
                    .px_6()
                    .pb_6()
                    .child(
                        card("Soft shadow").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(8.))
                                .shadow(vec![BoxShadow {
                                    color: hsla(0., 0., 0., 0.15),
                                    offset: point(px(0.), px(2.)),
                                    blur_radius: px(8.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }]),
                        ),
                    )
                    .child(
                        card("Medium shadow").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(8.))
                                .shadow(vec![BoxShadow {
                                    color: hsla(0., 0., 0., 0.25),
                                    offset: point(px(0.), px(4.)),
                                    blur_radius: px(12.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }]),
                        ),
                    )
                    .child(
                        card("Heavy shadow").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(12.))
                                .shadow(vec![BoxShadow {
                                    color: hsla(0., 0., 0., 0.35),
                                    offset: point(px(0.), px(8.)),
                                    blur_radius: px(24.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }]),
                        ),
                    )
                    .child(
                        card("Colored shadow").child(
                            div()
                                .size_16()
                                .bg(rgb(0x007AFF))
                                .rounded(px(8.))
                                .shadow(vec![BoxShadow {
                                    color: hsla(211. / 360., 1.0, 0.5, 0.4),
                                    offset: point(px(0.), px(6.)),
                                    blur_radius: px(16.),
                                    spread_radius: px(0.),
                                    inset: false,
                                }]),
                        ),
                    )
                    .child(
                        card("Inset shadow").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(8.))
                                .shadow(vec![BoxShadow {
                                    color: hsla(0., 0., 0., 0.2),
                                    offset: point(px(0.), px(2.)),
                                    blur_radius: px(6.),
                                    spread_radius: px(0.),
                                    inset: true,
                                }]),
                        ),
                    ),
            )
            .child(section_label("SVG Icons — 2x supersampled"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .px_6()
                    .pb_4()
                    .items_center()
                    .child(
                        card("16px").child(
                            svg()
                                .path("icons/check.svg")
                                .size_4()
                                .text_color(rgb(0x34C759)),
                        ),
                    )
                    .child(
                        card("20px").child(
                            svg()
                                .path("icons/check.svg")
                                .size_5()
                                .text_color(rgb(0x34C759)),
                        ),
                    )
                    .child(
                        card("24px").child(
                            svg()
                                .path("icons/check.svg")
                                .size_6()
                                .text_color(rgb(0x34C759)),
                        ),
                    )
                    .child(
                        card("Close 16px").child(
                            svg()
                                .path("icons/close.svg")
                                .size_4()
                                .text_color(rgb(0xFF3B30)),
                        ),
                    )
                    .child(
                        card("Close 24px").child(
                            svg()
                                .path("icons/close.svg")
                                .size_6()
                                .text_color(rgb(0xFF3B30)),
                        ),
                    )
                    .child(
                        card("Chevron 24px").child(
                            svg()
                                .path("icons/chevron_right.svg")
                                .size_6()
                                .text_color(rgb(0x8E8E93)),
                        ),
                    ),
            )
            .child(section_label("Text Rendering — subpixel antialiased"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_6()
                    .pb_4()
                    .bg(rgb(0xffffff))
                    .mx_6()
                    .rounded(px(8.))
                    .p_4()
                    .shadow(vec![BoxShadow {
                        color: hsla(0., 0., 0., 0.08),
                        offset: point(px(0.), px(1.)),
                        blur_radius: px(3.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .child(
                        div().text_xs().text_color(hsla(0., 0., 0.2, 1.0)).child(
                            "12px — The quick brown fox jumps over the lazy dog. 0123456789",
                        ),
                    )
                    .child(
                        div().text_sm().text_color(hsla(0., 0., 0.2, 1.0)).child(
                            "14px — The quick brown fox jumps over the lazy dog. 0123456789",
                        ),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(hsla(0., 0., 0.2, 1.0))
                            .child("16px — The quick brown fox jumps over the lazy dog"),
                    )
                    .child(
                        div()
                            .text_lg()
                            .text_color(hsla(0., 0., 0.2, 1.0))
                            .child("18px — The quick brown fox jumps over the lazy dog"),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(kael::FontWeight::SEMIBOLD)
                            .text_color(hsla(0., 0., 0.1, 1.0))
                            .child("20px semibold — Kael GPU-accelerated UI"),
                    )
                    .child(
                        div()
                            .text_2xl()
                            .font_weight(kael::FontWeight::BOLD)
                            .text_color(hsla(0., 0., 0.1, 1.0))
                            .child("24px bold — Kael"),
                    ),
            )
            .child(section_label("macOS-style UI card"))
            .child(
                div()
                    .mx_6()
                    .mb_6()
                    .bg(rgb(0xffffff))
                    .rounded(px(10.))
                    .shadow(vec![
                        BoxShadow {
                            color: hsla(0., 0., 0., 0.1),
                            offset: point(px(0.), px(1.)),
                            blur_radius: px(3.),
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
                            .gap_3()
                            .px_4()
                            .py_3()
                            .border_b_1()
                            .border_color(hsla(0., 0., 0., 0.08))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(div().size(px(12.)).rounded_full().bg(rgb(0xFF5F57)))
                                    .child(div().size(px(12.)).rounded_full().bg(rgb(0xFEBC2E)))
                                    .child(div().size(px(12.)).rounded_full().bg(rgb(0x28C840))),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .text_color(hsla(0., 0., 0.4, 1.0))
                                    .text_center()
                                    .child("Kael — Settings"),
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
                                            .bg(rgb(0x007AFF))
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
                                                    .text_sm()
                                                    .font_weight(kael::FontWeight::MEDIUM)
                                                    .text_color(hsla(0., 0., 0.1, 1.0))
                                                    .child("General"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(hsla(0., 0., 0.5, 1.0))
                                                    .child("Appearance, language, and startup"),
                                            ),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        svg()
                                            .path("icons/chevron_right.svg")
                                            .size_4()
                                            .text_color(hsla(0., 0., 0.7, 1.0)),
                                    ),
                            )
                            .child(div().h(px(1.)).bg(hsla(0., 0., 0., 0.08)).mx_1())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .size_10()
                                            .bg(rgb(0xFF9500))
                                            .rounded(px(8.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                svg()
                                                    .path("icons/close.svg")
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
                                                    .text_sm()
                                                    .font_weight(kael::FontWeight::MEDIUM)
                                                    .text_color(hsla(0., 0., 0.1, 1.0))
                                                    .child("Keyboard"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(hsla(0., 0., 0.5, 1.0))
                                                    .child(
                                                        "Shortcuts, input sources, and dictation",
                                                    ),
                                            ),
                                    )
                                    .child(div().flex_1())
                                    .child(
                                        svg()
                                            .path("icons/chevron_right.svg")
                                            .size_4()
                                            .text_color(hsla(0., 0., 0.7, 1.0)),
                                    ),
                            ),
                    ),
            )
            )
    }
}

fn main() {
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    Application::new()
        .with_assets(FsAssets { base: assets_dir })
        .run(|cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(800.0), px(900.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| CrispnessShowcase {
                        scroll_handle: ScrollHandle::new(),
                    })
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
