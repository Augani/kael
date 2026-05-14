use kael::{
    App, Application, Bounds, ColorSpace, Context, Half, Render, Window, WindowOptions, canvas,
    div, linear_color_stop, linear_gradient, point, prelude::*, px, size,
};

struct GradientViewer {
    color_space: ColorSpace,
}

impl GradientViewer {
    fn new() -> Self {
        Self {
            color_space: ColorSpace::default(),
        }
    }
}

impl Render for GradientViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let color_space = self.color_space;

        div()
            .font_family(".SystemUIFont")
            .bg(kael::white())
            .size_full()
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .justify_between()
                    .items_center()
                    .child("Gradient Examples")
                    .child(
                        div().flex().gap_2().items_center().child(
                            div()
                                .id("method")
                                .flex()
                                .px_3()
                                .py_1()
                                .text_sm()
                                .bg(kael::black())
                                .text_color(kael::white())
                                .child(format!("{}", color_space))
                                .active(|this| this.opacity(0.8))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.color_space = match this.color_space {
                                        ColorSpace::Oklab => ColorSpace::Srgb,
                                        ColorSpace::Srgb => ColorSpace::Oklab,
                                    };
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_3()
                    .child(
                        div()
                            .size_full()
                            .rounded_xl()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(kael::red())
                            .text_color(kael::white())
                            .child("Solid Color"),
                    )
                    .child(
                        div()
                            .size_full()
                            .rounded_xl()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(kael::blue())
                            .text_color(kael::white())
                            .child("Solid Color"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_3()
                    .h_24()
                    .text_color(kael::white())
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            45.,
                            linear_color_stop(kael::red(), 0.),
                            linear_color_stop(kael::blue(), 1.),
                        )
                        .color_space(color_space)),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            135.,
                            linear_color_stop(kael::red(), 0.),
                            linear_color_stop(kael::green(), 1.),
                        )
                        .color_space(color_space)),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            225.,
                            linear_color_stop(kael::green(), 0.),
                            linear_color_stop(kael::blue(), 1.),
                        )
                        .color_space(color_space)),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            315.,
                            linear_color_stop(kael::green(), 0.),
                            linear_color_stop(kael::yellow(), 1.),
                        )
                        .color_space(color_space)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_3()
                    .h_24()
                    .text_color(kael::white())
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            0.,
                            linear_color_stop(kael::red(), 0.),
                            linear_color_stop(kael::white(), 1.),
                        )
                        .color_space(color_space)),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            90.,
                            linear_color_stop(kael::blue(), 0.),
                            linear_color_stop(kael::white(), 1.),
                        )
                        .color_space(color_space)),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            180.,
                            linear_color_stop(kael::green(), 0.),
                            linear_color_stop(kael::white(), 1.),
                        )
                        .color_space(color_space)),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            360.,
                            linear_color_stop(kael::yellow(), 0.),
                            linear_color_stop(kael::white(), 1.),
                        )
                        .color_space(color_space)),
                    ),
            )
            .child(
                div().flex_1().rounded_xl().bg(linear_gradient(
                    0.,
                    linear_color_stop(kael::green(), 0.05),
                    linear_color_stop(kael::yellow(), 0.95),
                )
                .color_space(color_space)),
            )
            .child(
                div().flex_1().rounded_xl().bg(linear_gradient(
                    90.,
                    linear_color_stop(kael::blue(), 0.05),
                    linear_color_stop(kael::red(), 0.95),
                )
                .color_space(color_space)),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .gap_3()
                    .child(
                        div().flex().flex_1().gap_3().child(
                            div().flex_1().rounded_xl().bg(linear_gradient(
                                90.,
                                linear_color_stop(kael::blue(), 0.5),
                                linear_color_stop(kael::red(), 0.5),
                            )
                            .color_space(color_space)),
                        ),
                    )
                    .child(
                        div().flex_1().rounded_xl().bg(linear_gradient(
                            180.,
                            linear_color_stop(kael::green(), 0.),
                            linear_color_stop(kael::blue(), 0.5),
                        )
                        .color_space(color_space)),
                    ),
            )
            .child(
                div()
                    .h_24()
                    .child(canvas(size(px(0.), px(96.)), move |dc, _, _| {
                        let bounds = dc.bounds();
                        let canvas_size = size(bounds.size.width * 0.8, px(80.));
                        let square_bounds = Bounds {
                            origin: point(
                                bounds.size.width.half() - canvas_size.width.half(),
                                px(0.),
                            ),
                            size: canvas_size,
                        };
                        let height = square_bounds.size.height;
                        let horizontal_offset = height;
                        let vertical_offset = px(30.);
                        let mut builder = kael::PathBuilder::fill();
                        builder.move_to(square_bounds.bottom_left());
                        builder.line_to(
                            square_bounds.origin + point(horizontal_offset, vertical_offset),
                        );
                        builder.line_to(
                            square_bounds.top_right() + point(-horizontal_offset, vertical_offset),
                        );
                        builder.line_to(square_bounds.bottom_right());
                        builder.line_to(square_bounds.bottom_left());
                        let path = builder.build().unwrap();
                        dc.fill_path(
                            &path,
                            linear_gradient(
                                180.,
                                linear_color_stop(kael::red(), 0.),
                                linear_color_stop(kael::blue(), 1.),
                            )
                            .color_space(color_space),
                        );
                        dc.stroke_path(&path, kael::stroke(px(1.5), kael::black()));
                    })),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions {
                focus: true,
                ..Default::default()
            },
            |_, cx| cx.new(|_| GradientViewer::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}
