use kael::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, black, checkbox,
    date_picker, div, prelude::*, px, radio_group, rgb, select, size, slider, toggle, white,
};
use time::{Date, Month};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemeMode {
    System,
    Light,
    Dark,
}

impl ThemeMode {
    fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AccentColor {
    Blue,
    Green,
    Orange,
}

impl AccentColor {
    fn label(self) -> &'static str {
        match self {
            Self::Blue => "Atlantic",
            Self::Green => "Forest",
            Self::Orange => "Ember",
        }
    }
}

struct FormControlsExample {
    notifications: bool,
    compact_mode: bool,
    volume: f64,
    theme: ThemeMode,
    accent: AccentColor,
    delivery_date: Date,
}

impl Render for FormControlsExample {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let example = cx.entity();

        div()
            .bg(rgb(0xe2e8f0))
            .size_full()
            .id("form-controls-scroll-root")
            .overflow_y_auto()
            .child(
                div()
                    .bg(white())
                    .border_1()
                    .border_color(black())
                    .m_4()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child("Tier 1 form controls")
                    .child("Focus any control and use Cmd-Z / Cmd-Shift-Z to verify local undo and redo.")
                    .child(
                        checkbox("notifications", self.notifications)
                            .label("Enable notifications")
                            .on_change({
                                let example = example.clone();
                                move |checked, _, cx| {
                                    example.update(cx, |this, cx| {
                                        this.notifications = *checked;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        toggle("compact_mode", self.compact_mode)
                            .label("Compact mode")
                            .on_change({
                                let example = example.clone();
                                move |on, _, cx| {
                                    example.update(cx, |this, cx| {
                                        this.compact_mode = *on;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(format!("Volume: {:.0}%", self.volume))
                            .child(
                                slider("volume", self.volume)
                                    .min(0.0)
                                    .max(100.0)
                                    .step(5.0)
                                    .on_change({
                                        let example = example.clone();
                                        move |value, _, cx| {
                                            example.update(cx, |this, cx| {
                                                this.volume = *value;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child("Theme")
                            .child(
                                radio_group(
                                    "theme",
                                    self.theme,
                                    [
                                        (ThemeMode::System, "System"),
                                        (ThemeMode::Light, "Light"),
                                        (ThemeMode::Dark, "Dark"),
                                    ],
                                )
                                .on_change({
                                    let example = example.clone();
                                    move |value, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.theme = *value;
                                            cx.notify();
                                        });
                                    }
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child("Accent color")
                            .child(
                                select(
                                    "accent",
                                    self.accent,
                                    [
                                        (AccentColor::Blue, "Atlantic"),
                                        (AccentColor::Green, "Forest"),
                                        (AccentColor::Orange, "Ember"),
                                    ],
                                )
                                .searchable()
                                .on_change({
                                    let example = example.clone();
                                    move |value, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.accent = *value;
                                            cx.notify();
                                        });
                                    }
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child("Delivery date")
                            .child(
                                date_picker("delivery_date", self.delivery_date).on_change({
                                    move |date, _, cx| {
                                        example.update(cx, |this, cx| {
                                            this.delivery_date = *date;
                                            cx.notify();
                                        });
                                    }
                                }),
                            ),
                    )
                    .child(format!(
                        "Summary: {}, {}, {}%, {}, {}",
                        if self.notifications {
                            "notifications on"
                        } else {
                            "notifications off"
                        },
                        if self.compact_mode {
                            "compact"
                        } else {
                            "comfortable"
                        },
                        self.volume,
                        self.theme.label(),
                        self.accent.label()
                    ))
                    .child(format!("Scheduled date: {}", self.delivery_date)),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(460.0), px(540.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| FormControlsExample {
                    notifications: true,
                    compact_mode: false,
                    volume: 65.0,
                    theme: ThemeMode::System,
                    accent: AccentColor::Blue,
                    delivery_date: Date::from_calendar_date(2025, Month::June, 15)
                        .expect("valid example date"),
                })
            },
        )
        .unwrap();
    });
}
