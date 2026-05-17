use kael::{
    App, Application, Bounds, BoxShadow, Context, FontWeight, Window, WindowBounds,
    WindowOptions, div, hsla, point, prelude::*, px, rgb, size,
};

struct SharpRenderingTest;

fn border_sample(label: &'static str, detail: &'static str) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .w(px(220.))
        .p_4()
        .rounded(px(10.))
        .border_1()
        .border_color(rgb(0xd4d4d8))
        .bg(rgb(0xffffff))
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(0x18181b))
                .child(label),
        )
        .child(div().text_xs().text_color(rgb(0x52525b)).child(detail))
        .child(
            div()
                .h(px(36.))
                .w_full()
                .rounded(px(8.))
                .border_1()
                .border_color(rgb(0xa1a1aa))
                .bg(rgb(0xf8fafc)),
        )
}

impl Render for SharpRenderingTest {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("sharp-rendering-test")
            .flex()
            .flex_col()
            .gap_6()
            .size_full()
            .bg(rgb(0xf5f5f4))
            .p_8()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0xd6d3d1))
                    .bg(rgb(0xffffff))
                    .shadow(vec![BoxShadow {
                        color: hsla(0.0, 0.0, 0.0, 0.16),
                        offset: point(px(0.), px(12.)),
                        blur_radius: px(24.),
                        spread_radius: px(-8.),
                        inset: false,
                    }])
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0x111827))
                            .child("Sharp text rendering test"),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(rgb(0x1f2937))
                            .child("The quick brown fox jumps over the lazy dog 0123456789"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x4b5563))
                            .child("Small text should stay crisp, borders should stay exactly one device pixel, and shadows should not clip at the card edge."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .child(border_sample(
                        "Hairline border",
                        "1px logical border rendered against a light fill.",
                    ))
                    .child(border_sample(
                        "Adjacent edges",
                        "Check that neighboring edges meet without gaps or double-thick seams.",
                    ))
                    .child(border_sample(
                        "Rounded shadow",
                        "Inspect the outer blur for clipping and banding.",
                    )),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.), px(620.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| SharpRenderingTest),
        )
        .unwrap();

        cx.activate(true);
    });
}