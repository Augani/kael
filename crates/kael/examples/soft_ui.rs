use kael::{
    div, hsla, point, prelude::*, px, rgb, App, Application, Context, Render, Window, WindowOptions,
};
use std::time::Duration;

struct SoftUi {
    reordered: bool,
}

impl Render for SoftUi {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reordered = self.reordered;
        let mut tiles = vec![
            ("tile-a", rgb(0xef4444)),
            ("tile-b", rgb(0x22c55e)),
            ("tile-c", rgb(0x3b82f6)),
        ];
        if reordered {
            tiles.reverse();
        }

        div()
            .font_family(".SystemUIFont")
            .bg(rgb(0xf4f4f5))
            .size_full()
            .p_8()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .gap_6()
                    .items_center()
                    .child(
                        div()
                            .id("rotated-card")
                            .w(px(220.))
                            .h(px(120.))
                            .bg(kael::white())
                            .rounded(px(16.))
                            .border_2()
                            .border_color(rgb(0x6366f1))
                            .rotate(8.0)
                            .p_4()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child("Rotated subtree")
                            .child(div().w_full().h(px(8.)).rounded_full().bg(rgb(0x6366f1))),
                    )
                    .child(
                        div()
                            .id("skewed")
                            .w(px(160.))
                            .h(px(60.))
                            .bg(rgb(0x0ea5e9))
                            .rounded(px(8.))
                            .skew_x(-12.0)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(kael::white())
                            .child("Skewed"),
                    )
                    .child(
                        div()
                            .id("translated")
                            .w(px(160.))
                            .h(px(60.))
                            .bg(rgb(0xf59e0b))
                            .rounded(px(8.))
                            .translate_y(px(14.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child("Translated"),
                    ),
            )
            .child(
                div()
                    .id("rounded-clip")
                    .w(px(280.))
                    .h(px(110.))
                    .rounded(px(28.))
                    .overflow_hidden()
                    .bg(kael::white())
                    .border_2()
                    .border_color(rgb(0x111827))
                    .child(
                        div()
                            .w(px(400.))
                            .h(px(40.))
                            .bg(rgb(0xdc2626))
                            .child("clipped to rounded corners"),
                    )
                    .child(div().w(px(400.)).h(px(40.)).bg(rgb(0x16a34a)))
                    .child(div().w(px(400.)).h(px(40.)).bg(rgb(0x2563eb))),
            )
            .child(
                div()
                    .id("hover-me")
                    .w(px(220.))
                    .h(px(52.))
                    .rounded(px(10.))
                    .bg(rgb(0x111827))
                    .text_color(kael::white())
                    .flex()
                    .items_center()
                    .justify_center()
                    .transition(Duration::from_millis(200))
                    .hover(|style| {
                        style
                            .bg(rgb(0x6366f1))
                            .rounded(px(26.))
                            .text_color(hsla(0., 0., 1., 1.))
                    })
                    .active(|style| style.scale(0.96))
                    .child("Hover and press me"),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .children(tiles.into_iter().map(|(id, color)| {
                        div()
                            .id(id)
                            .animate_layout(Duration::from_millis(350))
                            .w(px(90.))
                            .h(px(90.))
                            .rounded(px(12.))
                            .bg(color)
                    }))
                    .child(
                        div()
                            .id("reorder")
                            .px_4()
                            .h(px(90.))
                            .rounded(px(12.))
                            .border_2()
                            .border_color(rgb(0x111827))
                            .flex()
                            .items_center()
                            .child("Reorder (FLIP)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reordered = !this.reordered;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .w(px(300.))
                    .rounded(px(12.))
                    .p_3()
                    .bg(kael::white())
                    .shadow(vec![
                        kael::BoxShadow {
                            color: hsla(0., 0., 0., 0.10),
                            offset: point(px(0.), px(10.)),
                            blur_radius: px(15.),
                            spread_radius: px(-3.),
                            inset: false,
                        },
                        kael::BoxShadow {
                            color: hsla(0., 0., 0., 0.05),
                            offset: point(px(0.), px(4.)),
                            blur_radius: px(6.),
                            spread_radius: px(-4.),
                            inset: false,
                        },
                    ])
                    .child("Layered soft shadow"),
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
            |_, cx| cx.new(|_| SoftUi { reordered: false }),
        )
        .unwrap();
        cx.activate(true);
    });
}
