use kael::{
    App, Application, Bounds, Context, ListAlignment, ListState, ScrollHandle, ScrollWheelEvent,
    Window, WindowBounds, WindowOptions, div, list, point, prelude::*, px, rgb, size, uniform_list,
};
use std::ops::Range;

const VARIABLE_LIST_ITEMS: usize = 180;
const UNIFORM_LIST_ITEMS: usize = 240;
const DIV_SCROLL_CARDS: usize = 22;

struct ElasticScrollingDemo {
    variable_list: ListState,
    horizontal_div_scroll: ScrollHandle,
    last_horizontal_wheel: String,
}

impl ElasticScrollingDemo {
    fn new() -> Self {
        let horizontal_div_scroll = ScrollHandle::new();
        horizontal_div_scroll.set_offset(point(px(-360.0), px(0.0)));

        Self {
            variable_list: ListState::new(VARIABLE_LIST_ITEMS, ListAlignment::Top, px(220.0)),
            horizontal_div_scroll,
            last_horizontal_wheel: "no wheel events yet".to_string(),
        }
    }
}

impl Render for ElasticScrollingDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let horizontal_status = format!(
            "offset.x {} / max.x {} | last wheel {}",
            self.horizontal_div_scroll.offset().x,
            self.horizontal_div_scroll.max_offset().width,
            self.last_horizontal_wheel,
        );

        div()
            .size_full()
            .p_4()
            .gap_4()
            .flex()
            .flex_col()
            .bg(rgb(0xf3ede2))
            .text_color(rgb(0x2f241f))
            .child(
                div()
                    .w_full()
                    .p_4()
                    .gap_2()
                    .flex()
                    .flex_col()
                    .bg(rgb(0xfffbf4))
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0xd9cfc2))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x8a6f5b))
                            .child("GitHub issue #1 · Feature Request: Support for macOS-native Rubber Banding"),
                    )
                    .child(div().text_xl().child("Elastic Scrolling Verification"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x5d4a3f))
                            .child("Use a trackpad on macOS and pull past each boundary. The content should stretch beyond its limit and spring back on release. The first pane also lets you verify horizontal elasticity."),
                    ),
            )
            .child(
                div()
                    .size_full()
                    .flex()
                    .gap_4()
                    .child(pane(
                        "Scrollable Div",
                        "Vertical and horizontal overflow using the standard Div scroll path.",
                        div_scroll_demo(
                            &self.horizontal_div_scroll,
                            horizontal_status,
                            window,
                            cx,
                        ),
                    ))
                    .child(pane(
                        "Variable Height List",
                        "The List element with stable but uneven row heights.",
                        variable_list_demo(self.variable_list.clone()),
                    ))
                    .child(pane(
                        "Uniform List",
                        "The uniform_list element used by dense feeds and tables.",
                        uniform_list_demo(cx),
                    )),
            )
    }
}

fn pane(
    title: &'static str,
    description: &'static str,
    content: impl IntoElement,
) -> impl IntoElement {
    div()
        .flex_1()
        .h_full()
        .p_3()
        .gap_3()
        .flex()
        .flex_col()
        .bg(rgb(0xfffbf6))
        .rounded_lg()
        .border_1()
        .border_color(rgb(0xd9cfc2))
        .child(
            div()
                .gap_1()
                .flex()
                .flex_col()
                .child(div().child(title))
                .child(div().text_sm().text_color(rgb(0x7b675a)).child(description)),
        )
        .child(
            div()
                .flex_1()
                .overflow_hidden()
                .bg(rgb(0xfff7ee))
                .rounded_lg()
                .border_1()
                .border_color(rgb(0xe4d8ca))
                .child(content),
        )
}

fn div_scroll_demo(
    horizontal_div_scroll: &ScrollHandle,
    horizontal_status: String,
    _window: &mut Window,
    cx: &mut Context<ElasticScrollingDemo>,
) -> impl IntoElement {
    div()
        .size_full()
        .p_3()
        .gap_3()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .p_3()
                .gap_2()
                .bg(rgb(0x203249))
                .rounded_lg()
                .border_1()
                .border_color(rgb(0x36516f))
                .text_color(rgb(0xf2f7ff))
                .child("Horizontal overscroll check")
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xb7d2ef))
                        .child("This strip is independent from the vertical stack below. Drag left or right here to verify horizontal scrolling and bounce without vertical interference."),
                ),
        )
        .child(
            div()
                .text_sm()
                .text_color(rgb(0x6a5a4d))
                .child(horizontal_status),
        )
        .child(
            div()
                .h(px(156.0))
                .w_full()
                .id("elastic-horizontal-demo")
                .overflow_x_scroll()
                .track_scroll(horizontal_div_scroll)
                .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                    let delta = event.delta.pixel_delta(window.line_height());
                    this.last_horizontal_wheel = format!(
                        "dx {} dy {} phase {:?} momentum {}",
                        delta.x,
                        delta.y,
                        event.touch_phase,
                        event.is_momentum,
                    );
                    cx.notify();
                }))
                .border_1()
                .border_color(rgb(0x36516f))
                .rounded_lg()
                .child(
                    div()
                        .w(px(2200.0))
                        .h_full()
                        .p_2()
                        .gap_2()
                        .flex()
                        .items_center()
                        .bg(palette(4).opacity(0.08))
                        .children((0..8).map(|ix| {
                            let accent = palette(ix + 1);
                            div()
                                .flex_none()
                                .w(px(240.0))
                                .h(px(120.0))
                                .p_3()
                                .gap_2()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .bg(accent.opacity(0.22))
                                .border_1()
                                .rounded_lg()
                                .border_color(accent.opacity(0.58))
                                .child(format!("COLUMN {:02}", ix + 1))
                                .child(
                                    div()
                                        .text_xl()
                                        .text_color(rgb(0x2d3742))
                                        .child(format!("{}00 px", ix + 1)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x4f5f70))
                                        .child("Swipe left/right here. These labeled bands should visibly slide under the viewport."),
                                )
                        })),
                ),
        )
        .child(
            div()
                .flex_1()
                .id("elastic-vertical-demo")
                .overflow_y_scroll()
                .child(
                    div()
                        .gap_3()
                        .flex()
                        .flex_col()
                        .children((0..DIV_SCROLL_CARDS).map(|ix| {
                            let accent = palette(ix);
                            div()
                                .w_full()
                                .h(px(72.0 + (ix % 3) as f32 * 20.0))
                                .p_3()
                                .gap_1()
                                .flex()
                                .flex_col()
                                .bg(accent.opacity(0.14))
                                .rounded_lg()
                                .border_1()
                                .border_color(accent.opacity(0.55))
                                .child(format!("Card {:02}", ix + 1))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x5f5b56))
                                        .child("Pull past the boundary here and the card stack should rebound instead of stopping rigidly."),
                                )
                        })),
                ),
        )
}

fn variable_list_demo(state: ListState) -> impl IntoElement {
    list(state, |ix, _window, _cx| {
        variable_list_row(ix).into_any_element()
    })
    .size_full()
}

fn variable_list_row(ix: usize) -> impl IntoElement {
    let accent = palette(ix + 2);
    let density = ["compact", "regular", "tall", "expanded"][ix % 4];

    div()
        .w_full()
        .h(px(58.0 + (ix % 4) as f32 * 18.0))
        .p_3()
        .gap_1()
        .flex()
        .flex_col()
        .bg(accent.opacity(0.14))
        .border_1()
        .border_color(accent.opacity(0.52))
        .child(format!("Message batch {:03} · {density}", ix + 1))
        .child(
            div()
                .text_sm()
                .text_color(rgb(0x5f5b56))
                .child("This pane exercises the variable-height List path that now carries elastic boundary scrolling."),
        )
}

fn uniform_list_demo(cx: &mut Context<ElasticScrollingDemo>) -> impl IntoElement {
    uniform_list(
        "elastic-uniform-list",
        UNIFORM_LIST_ITEMS,
        cx.processor(|_this, range: Range<usize>, _window, _cx| {
            range.map(uniform_list_row).collect::<Vec<_>>()
        }),
    )
    .size_full()
}

fn uniform_list_row(ix: usize) -> impl IntoElement {
    let accent = palette(ix + 4);

    div()
        .w_full()
        .h(px(56.0))
        .px_3()
        .flex()
        .items_center()
        .bg(accent.opacity(0.12))
        .border_b_1()
        .border_color(accent.opacity(0.45))
        .child(format!(
            "Uniform row {:03} · consistent height, elastic edge behavior",
            ix + 1
        ))
}

fn palette(ix: usize) -> kael::Hsla {
    match ix % 6 {
        0 => rgb(0xcb5d4d).into(),
        1 => rgb(0xde8b4a).into(),
        2 => rgb(0xa8793e).into(),
        3 => rgb(0x4f7a65).into(),
        4 => rgb(0x4f6d8c).into(),
        _ => rgb(0x7b5c99).into(),
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1180.0), px(860.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| ElasticScrollingDemo::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}
