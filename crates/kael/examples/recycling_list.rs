use kael::{
    AnyElement, App, Application, Bounds, Context, ListDelegate, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, recycling_list, rgb, size,
};
use std::rc::Rc;

#[derive(Clone)]
struct Row {
    title: String,
    detail: String,
    estimated_height: kael::Pixels,
}

#[derive(Clone)]
struct ExampleDelegate {
    rows: Rc<Vec<Row>>,
}

impl ListDelegate for ExampleDelegate {
    fn item_count(&self) -> usize {
        self.rows.len()
    }

    fn estimated_item_height(&self, ix: usize) -> kael::Pixels {
        self.rows[ix].estimated_height
    }

    fn render_item(&self, ix: usize, _window: &mut Window, _cx: &mut App) -> AnyElement {
        let row = &self.rows[ix];
        div()
            .p_3()
            .border_b_1()
            .border_color(rgb(0xd6dce5))
            .bg(if ix.is_multiple_of(2) {
                rgb(0xf9fbfd)
            } else {
                rgb(0xffffff)
            })
            .child(
                div()
                    .text_lg()
                    .font_weight(kael::FontWeight::SEMIBOLD)
                    .child(row.title.clone()),
            )
            .child(
                div()
                    .mt_1()
                    .text_sm()
                    .text_color(rgb(0x5d6b7a))
                    .child(row.detail.clone()),
            )
            .into_any()
    }
}

struct RecyclingListExample {
    rows: Rc<Vec<Row>>,
}

impl Render for RecyclingListExample {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(rgb(0xf0f4f8)).child(
            recycling_list(
                "heterogeneous-rows",
                ExampleDelegate {
                    rows: self.rows.clone(),
                },
            )
            .h_full(),
        )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let rows: Rc<Vec<Row>> = Rc::new(
            (0..200)
                .map(|ix| {
                    let detail = if ix % 5 == 0 {
                        "Longer detail rows estimate more height before they are measured, which keeps scrolling stable when mixed with short rows.".to_string()
                    } else {
                        "Short detail row.".to_string()
                    };

                    Row {
                        title: format!("Conversation Row {}", ix + 1),
                        detail,
                        estimated_height: if ix % 5 == 0 { px(88.) } else { px(56.) },
                    }
                })
                .collect(),
        );

        let bounds = Bounds::centered(None, size(px(420.), px(540.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(kael::TitlebarOptions {
                    title: Some("Recycling List Example".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|_| RecyclingListExample { rows: rows.clone() }),
        )
        .unwrap();
    });
}
