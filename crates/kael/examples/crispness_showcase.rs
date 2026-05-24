use kael::{
    App, Application, AssetSource, Bounds, BoxShadow, Context, Div, Result, ScrollHandle,
    SharedString, Window, WindowBounds, WindowOptions, div, hsla, point, prelude::*, px, rgb, size,
    svg,
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
                        card("2px radius")
                            .child(div().size_16().bg(rgb(MACOS_SYSTEM_BLUE)).rounded(px(2.))),
                    )
                    .child(
                        card("4px radius")
                            .child(div().size_16().bg(rgb(MACOS_SYSTEM_GREEN)).rounded(px(4.))),
                    )
                    .child(
                        card("8px radius").child(
                            div()
                                .size_16()
                                .bg(rgb(MACOS_SYSTEM_ORANGE))
                                .rounded(px(8.)),
                        ),
                    )
                    .child(
                        card("12px radius")
                            .child(div().size_16().bg(rgb(MACOS_SYSTEM_RED)).rounded(px(12.))),
                    )
                    .child(
                        card("Full circle")
                            .child(div().size_16().bg(rgb(MACOS_SYSTEM_PURPLE)).rounded_full()),
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
                                .border_color(rgb(MACOS_SYSTEM_BLUE)),
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
                                .border_color(rgb(MACOS_SYSTEM_GRAY)),
                        ),
                    )
                    .child(
                        card("Colored border").child(
                            div()
                                .size_16()
                                .bg(rgb(0xffffff))
                                .rounded(px(10.))
                                .border_2()
                                .border_color(rgb(MACOS_SYSTEM_RED)),
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
                                .bg(rgb(MACOS_SYSTEM_BLUE))
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
                                .text_color(rgb(MACOS_SYSTEM_GREEN)),
                        ),
                    )
                    .child(
                        card("20px").child(
                            svg()
                                .path("icons/check.svg")
                                .size_5()
                                .text_color(rgb(MACOS_SYSTEM_GREEN)),
                        ),
                    )
                    .child(
                        card("24px").child(
                            svg()
                                .path("icons/check.svg")
                                .size_6()
                                .text_color(rgb(MACOS_SYSTEM_GREEN)),
                        ),
                    )
                    .child(
                        card("Close 16px").child(
                            svg()
                                .path("icons/close.svg")
                                .size_4()
                                .text_color(rgb(MACOS_SYSTEM_RED)),
                        ),
                    )
                    .child(
                        card("Close 24px").child(
                            svg()
                                .path("icons/close.svg")
                                .size_6()
                                .text_color(rgb(MACOS_SYSTEM_RED)),
                        ),
                    )
                    .child(
                        card("Chevron 24px").child(
                            svg()
                                .path("icons/chevron_right.svg")
                                .size_6()
                                .text_color(rgb(MACOS_SYSTEM_GRAY)),
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
                                            .bg(rgb(MACOS_SYSTEM_ORANGE))
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
            .child(section_label("Hairline Grid — 1 device-pixel strokes"))
            .child(
                div()
                    .mx_6()
                    .mb_4()
                    .bg(rgb(0xffffff))
                    .rounded(px(8.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(0., 0., 0.5, 1.0))
                            .child("Each line below should be exactly 1 device pixel, never blurry"),
                    )
                    .child(div().h(px(0.5)).bg(rgb(0x000000)).w_full())
                    .child(div().h(px(0.5)).bg(rgb(MACOS_SYSTEM_BLUE)).w_full())
                    .child(div().h(px(0.5)).bg(rgb(MACOS_SYSTEM_RED)).w_full())
                    .child(div().h(px(0.5)).bg(rgb(MACOS_SYSTEM_GREEN)).w_full())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .h(px(60.))
                            .child(div().w(px(0.5)).bg(rgb(0x000000)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(MACOS_SYSTEM_BLUE)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(MACOS_SYSTEM_RED)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(MACOS_SYSTEM_GREEN)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(MACOS_SYSTEM_ORANGE)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(MACOS_SYSTEM_PURPLE)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(MACOS_SYSTEM_GRAY)).h_full())
                            .child(div().w(px(0.5)).bg(rgb(0x000000)).h_full()),
                    ),
            )
            .child(section_label("Text Baselines — fractional origin positions"))
            .child(
                div()
                    .mx_6()
                    .mb_4()
                    .bg(rgb(0xffffff))
                    .rounded(px(8.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(0., 0., 0.5, 1.0))
                            .child("Text at sub-pixel y offsets — should not shimmer when scrolling"),
                    )
                    .child(div().pt(px(0.0)).text_sm().text_color(hsla(0., 0., 0.1, 1.0)).child("Baseline at +0.0px offset"))
                    .child(div().pt(px(0.25)).text_sm().text_color(hsla(0., 0., 0.1, 1.0)).child("Baseline at +0.25px offset"))
                    .child(div().pt(px(0.5)).text_sm().text_color(hsla(0., 0., 0.1, 1.0)).child("Baseline at +0.5px offset"))
                    .child(div().pt(px(0.75)).text_sm().text_color(hsla(0., 0., 0.1, 1.0)).child("Baseline at +0.75px offset"))
                    .child(div().pt(px(0.33)).text_sm().text_color(hsla(0., 0., 0.1, 1.0)).child("Baseline at +0.33px offset"))
                    .child(div().pt(px(0.67)).text_sm().text_color(hsla(0., 0., 0.1, 1.0)).child("Baseline at +0.67px offset")),
            )
            .child(section_label("sRGB Gradients — linear blending correctness"))
            .child(
                div()
                    .mx_6()
                    .mb_4()
                    .bg(rgb(0xffffff))
                    .rounded(px(8.))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(0., 0., 0.5, 1.0))
                            .child("Gradients should be smooth without banding or muddy midtones"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .h(px(32.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .child(div().flex_1().bg(rgb(0x000000)))
                            .child(div().flex_1().bg(rgb(0x1a1a1a)))
                            .child(div().flex_1().bg(rgb(0x333333)))
                            .child(div().flex_1().bg(rgb(0x4d4d4d)))
                            .child(div().flex_1().bg(rgb(0x666666)))
                            .child(div().flex_1().bg(rgb(0x808080)))
                            .child(div().flex_1().bg(rgb(0x999999)))
                            .child(div().flex_1().bg(rgb(0xb3b3b3)))
                            .child(div().flex_1().bg(rgb(0xcccccc)))
                            .child(div().flex_1().bg(rgb(0xe6e6e6)))
                            .child(div().flex_1().bg(rgb(0xffffff))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .h(px(32.))
                            .rounded(px(4.))
                            .overflow_hidden()
                            .child(div().flex_1().bg(rgb(MACOS_SYSTEM_BLUE)))
                            .child(div().flex_1().bg(rgb(0x1a8aff)))
                            .child(div().flex_1().bg(rgb(0x3399ff)))
                            .child(div().flex_1().bg(rgb(0x4da9ff)))
                            .child(div().flex_1().bg(rgb(0x66b8ff)))
                            .child(div().flex_1().bg(rgb(0x80c8ff)))
                            .child(div().flex_1().bg(rgb(0x99d7ff)))
                            .child(div().flex_1().bg(rgb(0xb3e6ff)))
                            .child(div().flex_1().bg(rgb(0xccf0ff)))
                            .child(div().flex_1().bg(rgb(0xe6f8ff)))
                            .child(div().flex_1().bg(rgb(0xffffff))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(hsla(0., 0., 0.5, 1.0))
                            .child("Alpha-blended edges over background"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(div().size_12().bg(hsla(0., 0., 0., 0.1)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(0., 0., 0., 0.2)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(0., 0., 0., 0.3)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(0., 0., 0., 0.5)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(0., 0., 0., 0.7)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(0., 0., 0., 0.9)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(211. / 360., 1.0, 0.5, 0.3)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(211. / 360., 1.0, 0.5, 0.6)).rounded(px(6.)))
                            .child(div().size_12().bg(hsla(211. / 360., 1.0, 0.5, 0.9)).rounded(px(6.))),
                    ),
            )
            .child(section_label("Resize Stress — shadows, text, and borders"))
            .child(
                div()
                    .mx_6()
                    .mb_6()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_3()
                    .child(
                        div()
                            .size_20()
                            .bg(rgb(0xffffff))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(hsla(0., 0., 0., 0.1))
                            .shadow(vec![BoxShadow {
                                color: hsla(0., 0., 0., 0.15),
                                offset: point(px(0.), px(2.)),
                                blur_radius: px(6.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().text_xs().text_color(hsla(0., 0., 0.4, 1.0)).child("A")),
                    )
                    .child(
                        div()
                            .size_20()
                            .bg(rgb(MACOS_SYSTEM_BLUE))
                            .rounded(px(12.))
                            .shadow(vec![BoxShadow {
                                color: hsla(211. / 360., 1.0, 0.5, 0.4),
                                offset: point(px(0.), px(4.)),
                                blur_radius: px(12.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().text_sm().font_weight(kael::FontWeight::BOLD).text_color(rgb(0xffffff)).child("B")),
                    )
                    .child(
                        div()
                            .size_20()
                            .bg(rgb(0xffffff))
                            .rounded(px(6.))
                            .border_2()
                            .border_color(rgb(MACOS_SYSTEM_RED))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().text_xs().text_color(rgb(MACOS_SYSTEM_RED)).child("C")),
                    )
                    .child(
                        div()
                            .size_20()
                            .bg(rgb(MACOS_SYSTEM_GREEN))
                            .rounded_full()
                            .shadow(vec![BoxShadow {
                                color: hsla(140. / 360., 0.68, 0.5, 0.3),
                                offset: point(px(0.), px(3.)),
                                blur_radius: px(8.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().text_sm().font_weight(kael::FontWeight::BOLD).text_color(rgb(0xffffff)).child("D")),
                    )
                    .child(
                        div()
                            .size_20()
                            .bg(rgb(0xffffff))
                            .rounded(px(8.))
                            .shadow(vec![
                                BoxShadow {
                                    color: hsla(0., 0., 0., 0.12),
                                    offset: point(px(0.), px(1.)),
                                    blur_radius: px(3.),
                                    spread_radius: px(0.),
                                    inset: false,
                                },
                                BoxShadow {
                                    color: hsla(0., 0., 0., 0.08),
                                    offset: point(px(0.), px(8.)),
                                    blur_radius: px(24.),
                                    spread_radius: px(0.),
                                    inset: false,
                                },
                            ])
                            .border_1()
                            .border_color(hsla(0., 0., 0., 0.06))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().text_xs().text_color(hsla(0., 0., 0.4, 1.0)).child("E")),
                    )
                    .child(
                        div()
                            .size_20()
                            .bg(rgb(MACOS_SYSTEM_ORANGE))
                            .rounded(px(10.))
                            .shadow(vec![BoxShadow {
                                color: hsla(38. / 360., 1.0, 0.5, 0.35),
                                offset: point(px(0.), px(4.)),
                                blur_radius: px(10.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(div().text_sm().font_weight(kael::FontWeight::BOLD).text_color(rgb(0xffffff)).child("F")),
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
