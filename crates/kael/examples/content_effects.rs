use kael::{
    div, effect_layer, hsla, linear_color_stop, linear_gradient, point, prelude::*, px, rgb, size,
    App, Application, Bounds, BoxShadow, Context, Window, WindowBounds, WindowOptions,
};

struct ContentEffects {
    warmup_frames: u32,
}

impl ContentEffects {
    fn new(_window: &mut Window, _: &mut Context<Self>) -> Self {
        Self { warmup_frames: 4 }
    }

    fn shapes_card(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .items_center()
            .justify_center()
            .size(px(150.))
            .rounded_xl()
            .bg(linear_gradient(
                135.0,
                linear_color_stop(rgb(0x6366f1), 0.),
                linear_color_stop(rgb(0xec4899), 1.),
            ))
            .child(div().size_12().rounded_full().bg(kael::white()))
            .child(div().w(px(90.)).h(px(10.)).rounded_full().bg(rgb(0xfde047)))
            .child(div().size_10().rounded_md().bg(rgb(0x22d3ee)))
    }

    fn diamond(&self) -> impl IntoElement {
        div()
            .size(px(190.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .rotate(45.0)
                    .size(px(95.))
                    .rounded_lg()
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(rgb(0x22d3ee), 0.),
                        linear_color_stop(rgb(0x4ade80), 1.),
                    )),
            )
    }

    fn patterned(&self) -> impl IntoElement {
        let mut row = div().flex().flex_col().size_full();
        for r in 0..6 {
            let mut cells = div().flex().flex_row().w_full().flex_1();
            for c in 0..6 {
                let color = if (r + c) % 2 == 0 {
                    rgb(0x1e293b)
                } else {
                    rgb(0xf97316)
                };
                cells = cells.child(div().flex_1().h_full().bg(color));
            }
            row = row.child(cells);
        }
        row
    }

    fn labeled(&self, label: &'static str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .items_center()
            .child(content)
            .child(div().text_xs().text_color(kael::black()).child(label))
    }
}

impl Render for ContentEffects {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if self.warmup_frames > 0 {
            self.warmup_frames -= 1;
            window.request_animation_frame();
        }

        div()
            .flex()
            .flex_row()
            .gap_10()
            .items_start()
            .size_full()
            .bg(rgb(0xf4f4f5))
            .p_8()
            .child(
                self.labeled(
                    "content blur vs original",
                    div()
                        .flex()
                        .flex_row()
                        .gap_4()
                        .child(
                            div()
                                .size(px(150.))
                                .child(effect_layer(self.shapes_card()).content_blur(px(6.))),
                        )
                        .child(self.shapes_card()),
                ),
            )
            .child(
                self.labeled(
                    "drop shadow follows silhouette",
                    div()
                        .size(px(210.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(effect_layer(self.diamond()).drop_shadow(BoxShadow {
                            color: hsla(0., 0., 0., 0.6),
                            offset: point(px(12.), px(16.)),
                            blur_radius: px(8.),
                            spread_radius: px(0.),
                            inset: false,
                        })),
                ),
            )
            .child(
                self.labeled(
                    "frosted glass overlay",
                    div()
                        .relative()
                        .size(px(200.))
                        .rounded_lg()
                        .overflow_hidden()
                        .child(self.patterned())
                        .child(
                            div()
                                .absolute()
                                .top(px(60.))
                                .left(px(20.))
                                .w(px(160.))
                                .h(px(80.))
                                .child(
                                    effect_layer(
                                        div()
                                            .w(px(160.))
                                            .h(px(80.))
                                            .rounded_lg()
                                            .bg(hsla(0., 0., 1., 0.35))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .text_color(kael::white())
                                                    .text_sm()
                                                    .child("Frosted"),
                                            ),
                                    )
                                    .content_blur(px(8.)),
                                ),
                        ),
                ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(820.0), px(440.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| ContentEffects::new(window, cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
