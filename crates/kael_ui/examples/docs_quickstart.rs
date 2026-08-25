//! Compiled source for the Getting started and One codebase guides.

use kael_ui::prelude::*;

struct Counter {
    count: i32,
}

impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let counter = cx.entity();

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_4()
            .child(div().text_3xl().child(format!("Count: {}", self.count)))
            .child(
                Button::new("increment", "Increase").on_click(move |_, _, cx| {
                    counter.update(cx, |state, cx| {
                        state.count += 1;
                        cx.notify();
                    });
                }),
            )
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::try_new()?.run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::dark());

        if let Err(error) = cx.open_window(WindowOptions::default(), |_, cx| {
            cx.new(|_| Counter { count: 0 })
        }) {
            eprintln!("failed to open the application window: {error}");
            cx.quit();
        }
    });
    Ok(())
}
