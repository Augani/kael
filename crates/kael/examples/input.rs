use kael::{
    App, Application, Bounds, Context, Keystroke, MouseButton, MouseUpEvent, SharedString, Window,
    WindowBounds, WindowOptions, black, div, opaque_grey, prelude::*, px, rgb, size, text_input,
    white, yellow,
};

struct InputExample {
    text: SharedString,
    notes: SharedString,
    password: SharedString,
    submitted_text: Option<SharedString>,
    recent_keystrokes: Vec<Keystroke>,
}

impl InputExample {
    fn on_reset_click(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.text = SharedString::default();
        self.notes = SharedString::default();
        self.password = SharedString::default();
        self.submitted_text = None;
        self.recent_keystrokes.clear();
        cx.notify();
    }
}

impl Render for InputExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let example = cx.entity();
        let submitted_text = self
            .submitted_text
            .clone()
            .unwrap_or_else(|| "Press Enter to submit the first field".into());

        div()
            .bg(rgb(0xaaaaaa))
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .bg(white())
                    .border_b_1()
                    .border_color(black())
                    .flex()
                    .flex_row()
                    .justify_between()
                    .child(format!("Keyboard {}", cx.keyboard_layout().name()))
                    .child(
                        div()
                            .border_1()
                            .border_color(black())
                            .px_2()
                            .bg(yellow())
                            .child("Reset")
                            .hover(|style| {
                                style
                                    .bg(yellow().blend(opaque_grey(0.5, 0.5)))
                                    .cursor_pointer()
                            })
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_reset_click)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_4()
                    .child("Text input")
                    .child(
                        div().w_full().child(
                            text_input("message", self.text.clone())
                                .placeholder("Type here...")
                                .on_change({
                                    let example = example.clone();
                                    move |text, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.text = text;
                                            cx.notify();
                                        });
                                    }
                                })
                                .on_submit({
                                    let example = example.clone();
                                    move |text, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.submitted_text = Some(text);
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                    )
                    .child("Multi-line input")
                    .child(
                        div().w_full().child(
                            text_input("notes", self.notes.clone())
                                .multi_line()
                                .max_lines(4)
                                .placeholder("Add a longer note...")
                                .on_change({
                                    let example = example.clone();
                                    move |text, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.notes = text;
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                    )
                    .child("Password input")
                    .child(
                        div().w_full().child(
                            text_input("password", self.password.clone())
                                .password()
                                .placeholder("Enter a password")
                                .on_change({
                                    move |text, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.password = text;
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                    )
                    .child(format!("Submitted: {}", submitted_text))
                    .children(
                        self.recent_keystrokes
                            .iter()
                            .rev()
                            .take(12)
                            .map(|keystroke| {
                                format!(
                                    "{:} {}",
                                    keystroke.unparse(),
                                    keystroke
                                        .key_char
                                        .as_ref()
                                        .map(|key_char| format!("-> {:?}", key_char))
                                        .unwrap_or_default()
                                )
                            }),
                    ),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(360.0), px(420.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| InputExample {
                        text: SharedString::default(),
                        notes: SharedString::default(),
                        password: SharedString::default(),
                        submitted_text: None,
                        recent_keystrokes: Vec::new(),
                    })
                },
            )
            .unwrap();

        let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        cx.observe_keystrokes(move |event, _, cx| {
            view.update(cx, |view, cx| {
                view.recent_keystrokes.push(event.keystroke.clone());
                if view.recent_keystrokes.len() > 12 {
                    let remove_count = view.recent_keystrokes.len() - 12;
                    view.recent_keystrokes.drain(0..remove_count);
                }
                cx.notify();
            });
        })
        .detach();

        cx.on_keyboard_layout_change({
            move |cx| {
                window.update(cx, |_, _, cx| cx.notify()).ok();
            }
        })
        .detach();
    });
}
