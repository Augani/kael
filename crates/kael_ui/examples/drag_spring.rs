use kael_ui::prelude::*;

fn main() {
    Application::new().run(|cx| {
        cx.open_window(
            WindowOptions {
                titlebar: Some(kael::TitlebarOptions {
                    title: Some("Drag Spring Demo".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(900.0), px(700.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| DragSpringApp::new(window, cx)),
        )
        .unwrap();
    });
}

struct DragSpringApp {
    card: Entity<DraggableSpringState>,
}

impl DragSpringApp {
    fn new(_window: &mut Window, cx: &mut App) -> Self {
        install_theme(cx, Theme::dark());

        let card = cx.new(DraggableSpringState::new);
        card.update(cx, |state, _| {
            state.set_preset(SpringPreset::Snappy);
            state.set_snap_points(vec![point(0.0, 0.0), point(-220.0, 0.0), point(220.0, 0.0)]);
        });

        Self { card }
    }
}

impl Render for DragSpringApp {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = use_theme();

        div()
            .bg(theme.tokens.background)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(32.0))
            .child(
                div()
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.tokens.foreground)
                    .child("Throw the card. It springs to the nearest snap point."),
            )
            .child(
                div()
                    .text_size(px(13.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child("Snap points: center, left, right. Grab mid-flight to take over."),
            )
            .child(
                div()
                    .relative()
                    .w(px(700.0))
                    .h(px(360.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        DraggableSpring::new("throw-card", self.card.clone()).child(
                            div()
                                .w(px(180.0))
                                .h(px(180.0))
                                .rounded(theme.tokens.radius_xl)
                                .bg(theme.tokens.primary)
                                .border_1()
                                .border_color(theme.tokens.border)
                                .shadow(smallvec::smallvec![BoxShadow {
                                    color: hsla(0.0, 0.0, 0.0, 0.35),
                                    offset: point(px(0.0), px(8.0)),
                                    blur_radius: px(24.0),
                                    spread_radius: px(0.0),
                                    inset: false,
                                }])
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(theme.tokens.primary_foreground)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(16.0))
                                .child("Throw me"),
                        ),
                    ),
            )
    }
}
