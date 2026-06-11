use std::{fs, path::PathBuf};

use anyhow::Result;
use kael::{
    App, Application, AssetSource, Bounds, Context, SharedString, Window, WindowBounds,
    WindowOptions, conic_gradient, div, img, linear_color_stop, linear_gradient, prelude::*, px,
    rgb, size,
};

struct Assets {
    base: PathBuf,
}

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        fs::read(self.base.join(path))
            .map(|data| Some(std::borrow::Cow::Owned(data)))
            .map_err(|e| e.into())
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        fs::read_dir(self.base.join(path))
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
            .map_err(|e| e.into())
    }
}

struct FiltersGradients;

impl FiltersGradients {
    fn new(_window: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }

    fn swatch(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .items_center()
            .justify_center()
            .size(px(110.))
            .rounded_lg()
            .bg(linear_gradient(
                45.0,
                linear_color_stop(rgb(0xff5f6d), 0.),
                linear_color_stop(rgb(0x2563eb), 1.),
            ))
            .text_color(kael::white())
            .child("🌈")
            .child(img("image/app-icon.png").size_8())
    }

    fn labeled(&self, label: &'static str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .items_center()
            .child(content)
            .child(div().text_xs().text_color(kael::black()).child(label))
    }
}

impl Render for FiltersGradients {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_6()
            .size_full()
            .bg(rgb(0xf4f4f5))
            .p_6()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .items_start()
                    .child(self.labeled("normal", self.swatch()))
                    .child(self.labeled("saturate(0)", div().saturate(0.0).child(self.swatch())))
                    .child(self.labeled("grayscale(1)", div().grayscale(1.0).child(self.swatch())))
                    .child(self.labeled(
                        "brightness(1.6)",
                        div().brightness(1.6).child(self.swatch()),
                    ))
                    .child(self.labeled("contrast(1.8)", div().contrast(1.8).child(self.swatch()))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_8()
                    .items_center()
                    .child(
                        self.labeled(
                            "conic gradient border",
                            div()
                                .size(px(150.))
                                .rounded_xl()
                                .bg(kael::white())
                                .border_4()
                                .border_gradient(conic_gradient(
                                    0.5,
                                    0.5,
                                    0.0,
                                    &[
                                        linear_color_stop(rgb(0xff0080), 0.0),
                                        linear_color_stop(rgb(0x7928ca), 0.5),
                                        linear_color_stop(rgb(0xff0080), 1.0),
                                    ],
                                ))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(div().text_color(kael::black()).text_sm().child("gradient")),
                        ),
                    )
                    .child(
                        self.labeled(
                            "linear gradient border",
                            div()
                                .size(px(150.))
                                .rounded_xl()
                                .bg(rgb(0x111827))
                                .border_4()
                                .border_gradient(linear_gradient(
                                    90.0,
                                    linear_color_stop(rgb(0x22d3ee), 0.0),
                                    linear_color_stop(rgb(0xa3e635), 1.0),
                                ))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(div().text_color(kael::white()).text_sm().child("border")),
                        ),
                    )
                    .child(
                        self.labeled(
                            "rotated img + emoji",
                            div()
                                .size(px(160.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    div()
                                        .rotate(0.5)
                                        .flex()
                                        .flex_col()
                                        .gap_2()
                                        .items_center()
                                        .justify_center()
                                        .size(px(120.))
                                        .rounded_lg()
                                        .bg(rgb(0xfde68a))
                                        .child(div().text_2xl().child("🚀"))
                                        .child(img("image/app-icon.png").size_12()),
                                ),
                        ),
                    ),
            )
    }
}

fn main() {
    Application::new()
        .with_assets(Assets {
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples"),
        })
        .run(|cx: &mut App| {
            let bounds = Bounds::centered(None, size(px(760.0), px(420.0)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| FiltersGradients::new(window, cx)),
            )
            .unwrap();
            cx.activate(true);
        });
}
