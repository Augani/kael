use std::time::Duration;

use kael::{
    Animation, AnimationExt as _, App, Application, Bounds, Context, Render, Window, WindowBounds,
    WindowOptions, conic_gradient, div, ease_in_out, effect_layer, hsla, linear_color_stop, point,
    prelude::*, px, rgb, size,
};

const PHASE_COUNT: usize = 4;
const PHASE_MS: u64 = 1500;

struct Showcase {
    phase: usize,
}

impl Showcase {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(PHASE_MS))
                    .await;
                let result = this.update(cx, |state, cx| {
                    state.phase = (state.phase + 1) % PHASE_COUNT;
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self { phase: 0 }
    }

    fn morph_card(&self) -> impl IntoElement {
        let (bg, radius, shadow_alpha) = match self.phase {
            0 => (rgb(0x6366f1), px(12.), 0.12),
            1 => (rgb(0xec4899), px(40.), 0.28),
            2 => (rgb(0x22c55e), px(20.), 0.18),
            _ => (rgb(0xf59e0b), px(60.), 0.35),
        };

        div()
            .id("morph-card")
            .w(px(180.))
            .h(px(120.))
            .rounded(radius)
            .bg(bg)
            .text_color(kael::white())
            .flex()
            .items_center()
            .justify_center()
            .transition(Duration::from_millis(PHASE_MS - 200))
            .shadow(vec![kael::BoxShadow {
                color: hsla(0., 0., 0., shadow_alpha),
                offset: point(px(0.), px(14.)),
                blur_radius: px(24.),
                spread_radius: px(-6.),
                inset: false,
            }])
            .child("transition")
    }

    fn flip_grid(&self) -> impl IntoElement {
        let palette = [
            ("flip-a", rgb(0xef4444)),
            ("flip-b", rgb(0x22c55e)),
            ("flip-c", rgb(0x3b82f6)),
            ("flip-d", rgb(0xa855f7)),
        ];
        let mut order: Vec<usize> = (0..palette.len()).collect();
        order.rotate_left(self.phase % palette.len());

        div()
            .flex()
            .flex_row()
            .gap_2()
            .children(order.into_iter().map(|idx| {
                let (id, color) = palette[idx];
                div()
                    .id(id)
                    .animate_layout(Duration::from_millis(PHASE_MS - 400))
                    .w(px(56.))
                    .h(px(56.))
                    .rounded(px(12.))
                    .bg(color)
            }))
    }

    fn oscillating_card(&self) -> impl IntoElement {
        div()
            .size(px(150.))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(110.))
                    .h(px(70.))
                    .rounded(px(14.))
                    .bg(rgb(0x0ea5e9))
                    .text_color(kael::white())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child("rotate")
                    .with_animation(
                        "oscillating-rotation",
                        Animation::new(Duration::from_secs(3))
                            .repeat_forever()
                            .with_easing(ease_in_out),
                        |element, delta| {
                            let angle = (delta * std::f32::consts::TAU).sin() * 10.0;
                            element.rotate(angle)
                        },
                    ),
            )
    }

    fn gradient_swatch(&self) -> impl IntoElement {
        let swatch = div()
            .size(px(120.))
            .rounded_xl()
            .bg(rgb(0x111827))
            .border_4()
            .border_gradient(conic_gradient(
                0.5,
                0.5,
                0.0,
                &[
                    linear_color_stop(rgb(0xff0080), 0.0),
                    linear_color_stop(rgb(0x7928ca), 0.5),
                    linear_color_stop(rgb(0x22d3ee), 1.0),
                ],
            ))
            .flex()
            .items_center()
            .justify_center()
            .child(div().text_color(kael::white()).text_sm().child("gradient"));

        let desaturated = self.phase % 2 == 1;
        div()
            .grayscale(if desaturated { 1.0 } else { 0.0 })
            .child(swatch)
    }

    fn blur_pattern(&self) -> impl IntoElement {
        let mut grid = div().flex().flex_col().size(px(120.)).overflow_hidden();
        for r in 0..6 {
            let mut row = div().flex().flex_row().w_full().flex_1();
            for c in 0..6 {
                let color = if (r + c) % 2 == 0 {
                    rgb(0x1e293b)
                } else {
                    rgb(0xf97316)
                };
                row = row.child(div().flex_1().h_full().bg(color));
            }
            grid = grid.child(row);
        }

        let blur = match self.phase {
            0 => 0.0,
            1 => 4.0,
            2 => 8.0,
            _ => 2.0,
        };

        div()
            .size(px(120.))
            .rounded_lg()
            .overflow_hidden()
            .child(effect_layer(grid).content_blur(px(blur)))
    }

    fn labeled(&self, label: &'static str, content: impl IntoElement) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .items_center()
            .child(content)
            .child(div().text_xs().text_color(rgb(0x52525b)).child(label))
    }
}

impl Render for Showcase {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .font_family(".SystemUIFont")
            .size_full()
            .bg(rgb(0xf4f4f5))
            .text_color(rgb(0x111827))
            .p_8()
            .flex()
            .flex_col()
            .gap_6()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xl().child("Kael 0.3 — smooth UI"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x52525b))
                            .child("self-animating: transitions, FLIP, rotation, filters, blur"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_10()
                    .items_start()
                    .child(self.labeled("implicit transition", self.morph_card()))
                    .child(self.labeled("FLIP reorder", self.flip_grid()))
                    .child(self.labeled("explicit rotation", self.oscillating_card())),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_10()
                    .items_start()
                    .child(self.labeled("gradient border + grayscale", self.gradient_swatch()))
                    .child(self.labeled("effect_layer content blur", self.blur_pattern())),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(720.0), px(520.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|cx| Showcase::new(cx)),
        )
        .unwrap();
        cx.activate(true);
    });
}
