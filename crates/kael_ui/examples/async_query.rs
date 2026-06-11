use kael_ui::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

fn main() {
    Application::new().run(|cx| {
        kael_ui::init(cx);
        install_theme(cx, Theme::dark());

        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Async Query".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(560.0), px(520.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |_window, cx| cx.new(AsyncQueryDemo::new),
        )
        .unwrap();
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Profile {
    name: SharedString,
    role: SharedString,
}

struct AsyncQueryDemo {
    query: Entity<QueryState<Profile>>,
    fail_next: Rc<Cell<bool>>,
    attempt: Rc<Cell<u32>>,
}

impl AsyncQueryDemo {
    fn new(cx: &mut Context<Self>) -> Self {
        let query = cx.new(|_| QueryState::new());
        let demo = Self {
            query: query.clone(),
            fail_next: Rc::new(Cell::new(false)),
            attempt: Rc::new(Cell::new(0)),
        };

        cx.observe(&query, |_, _, cx| cx.notify()).detach();
        demo.start_fetch(cx);
        demo
    }

    fn start_fetch(&self, cx: &mut Context<Self>) {
        let fail_next = self.fail_next.clone();
        let attempt = self.attempt.clone();
        self.query.update(cx, |query, cx| {
            query.run(cx, move || {
                let fail_next = fail_next.clone();
                let attempt = attempt.clone();
                async move {
                    Timer::after(Duration::from_millis(900)).await;
                    let count = attempt.get() + 1;
                    attempt.set(count);
                    if fail_next.get() {
                        Err(SharedString::from("network unreachable"))
                    } else {
                        Ok(Profile {
                            name: SharedString::from("Ada Lovelace"),
                            role: SharedString::from(format!("Engineer (load #{count})")),
                        })
                    }
                }
            });
        });
    }
}

impl Render for AsyncQueryDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let tokens = theme.tokens.clone();
        let state = self.query.read(cx).state().clone();

        let body = match state {
            Loadable::Idle | Loadable::Loading => div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(skeleton_line(&tokens, px(180.0)))
                .child(skeleton_line(&tokens, px(120.0)))
                .into_any_element(),
            Loadable::Loaded(profile) => div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(tokens.foreground)
                        .child(profile.name.clone()),
                )
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(tokens.muted_foreground)
                        .child(profile.role.clone()),
                )
                .into_any_element(),
            Loadable::Error(message) => div()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .text_size(px(16.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(tokens.destructive)
                        .child("Failed to load"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(tokens.muted_foreground)
                        .child(message.clone()),
                )
                .into_any_element(),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap(px(20.0))
            .p(px(28.0))
            .bg(tokens.background)
            .child(
                div()
                    .text_size(px(24.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(tokens.foreground)
                    .child("Async Query"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .p(px(20.0))
                    .min_h(px(120.0))
                    .rounded(tokens.radius_lg)
                    .border_1()
                    .border_color(tokens.border)
                    .bg(tokens.card)
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(12.0))
                    .child(Button::new("refetch", "Refetch").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.fail_next.set(false);
                            this.query.update(cx, |query, cx| query.refetch(cx));
                        },
                    )))
                    .child(
                        Button::new("fail", "Trigger error")
                            .variant(ButtonVariant::Destructive)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.fail_next.set(true);
                                this.query.update(cx, |query, cx| query.refetch(cx));
                            })),
                    ),
            )
    }
}

fn skeleton_line(tokens: &ThemeTokens, width: Pixels) -> Div {
    div()
        .w(width)
        .h(px(16.0))
        .rounded(tokens.radius_sm)
        .bg(tokens.muted)
}
