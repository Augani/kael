use kael::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, black,
    button, checkbox, date_picker, div, fill, modal, prelude::*, progress, px, radio_group, rgb,
    select, size, slider, text_input, toggle, white,
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

fn default_delivery_date() -> Date {
    Date::from_calendar_date(2025, Month::June, 15).expect("valid example date")
}

struct FormControlsExample {
    project_name: SharedString,
    notifications: bool,
    compact_mode: bool,
    volume: f64,
    theme: ThemeMode,
    accent: AccentColor,
    delivery_date: Date,
    reset_modal_open: bool,
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
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child("Project name")
                            .child(
                                text_input("project_name", self.project_name.clone())
                                    .placeholder("Studio console")
                                    .render_with(|state, window, cx| {
                                        let border: kael::Hsla = if state.focused {
                                            rgb(0x1d4ed8).into()
                                        } else if state.hovered {
                                            rgb(0x60a5fa).into()
                                        } else {
                                            rgb(0xcbd5e1).into()
                                        };

                                        window.paint_quad(fill(state.outer_bounds, border));
                                        window.paint_quad(fill(state.field_bounds, white()));
                                        state.paint_selection(rgb(0xbfdbfe).into(), window);
                                        state.paint_text(window, cx);
                                        state.paint_cursor(rgb(0x1d4ed8).into(), window);
                                    })
                                    .on_change({
                                        let example = example.clone();
                                        move |value, _, cx| {
                                            example.update(cx, |this, cx| {
                                                this.project_name = value;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(
                        checkbox("notifications", self.notifications)
                            .label("Enable notifications")
                            .render_with(|state, _, _| {
                                let mut body = div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(18.0))
                                            .h(px(18.0))
                                            .rounded(px(6.0))
                                            .border_1()
                                            .border_color(if state.focused {
                                                rgb(0x1d4ed8)
                                            } else {
                                                rgb(0x94a3b8)
                                            })
                                            .bg(if state.checked || state.indeterminate {
                                                rgb(0x1d4ed8)
                                            } else {
                                                rgb(0xffffff)
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .w(px(8.0))
                                                    .h(px(if state.indeterminate { 2.0 } else { 8.0 }))
                                                    .rounded(px(2.0))
                                                    .bg(if state.checked || state.indeterminate {
                                                        rgb(0xffffff)
                                                    } else {
                                                        kael::rgba(0x00000000)
                                                    }),
                                            ),
                                    );

                                if let Some(label) = state.label {
                                    body = body.child(div().text_color(rgb(0x0f172a)).child(label));
                                }

                                body.into_any_element()
                            })
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
                            .render_with(|state, _, _| {
                                let mut body = div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(38.0))
                                            .h(px(22.0))
                                            .rounded(px(999.0))
                                            .px(px(3.0))
                                            .flex()
                                            .items_center()
                                            .when(state.on, |this| this.justify_end())
                                            .when(!state.on, |this| this.justify_start())
                                            .border_1()
                                            .border_color(if state.focused {
                                                rgb(0x1d4ed8)
                                            } else {
                                                rgb(0x94a3b8)
                                            })
                                            .bg(if state.on {
                                                rgb(0x1d4ed8)
                                            } else {
                                                rgb(0xe2e8f0)
                                            })
                                            .child(
                                                div()
                                                    .w(px(14.0))
                                                    .h(px(14.0))
                                                    .rounded(px(999.0))
                                                    .bg(rgb(0xffffff)),
                                            ),
                                    );

                                if let Some(label) = state.label {
                                    body = body.child(div().text_color(rgb(0x0f172a)).child(label));
                                }

                                body.into_any_element()
                            })
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
                            .child("Export progress")
                            .child(
                                progress("export_progress", self.volume / 100.0).render_with(
                                    |state, bounds, window, _| {
                                        window.paint_quad(
                                            fill(bounds, rgb(0xe2e8f0)).corner_radii(px(999.0)),
                                        );

                                        let width = if state.indeterminate {
                                            (bounds.size.width * 0.35).max(px(12.0))
                                        } else {
                                            bounds.size.width
                                                * state.percentage.unwrap_or(0.0) as f32
                                        };

                                        if width > px(0.0) {
                                            window.paint_quad(
                                                fill(
                                                    Bounds::new(
                                                        bounds.origin,
                                                        size(width, bounds.size.height),
                                                    ),
                                                    rgb(0x1d4ed8),
                                                )
                                                .corner_radii(px(999.0)),
                                            );
                                        }
                                    },
                                ),
                            )
                            .child(format!("{:.0}% complete", self.volume)),
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
                                .render_with(|state, _, _| {
                                    div()
                                        .mb_1()
                                        .px_3()
                                        .py_2()
                                        .rounded(px(8.0))
                                        .border_1()
                                        .border_color(if state.focused {
                                            rgb(0x1d4ed8)
                                        } else if state.selected {
                                            rgb(0x60a5fa)
                                        } else {
                                            rgb(0xcbd5e1)
                                        })
                                        .bg(if state.selected {
                                            rgb(0xdbeafe)
                                        } else {
                                            rgb(0xffffff)
                                        })
                                        .text_color(rgb(0x0f172a))
                                        .child(state.label)
                                        .into_any_element()
                                })
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
                                .placeholder("Choose an accent")
                                .searchable()
                                .render_with(|state, _, _| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_3()
                                        .px(px(12.0))
                                        .py(px(8.0))
                                        .rounded(px(10.0))
                                        .border_1()
                                        .border_color(if state.focused || state.open {
                                            rgb(0x1d4ed8)
                                        } else {
                                            rgb(0x94a3b8)
                                        })
                                        .bg(if state.open {
                                            rgb(0xdbeafe)
                                        } else {
                                            rgb(0xffffff)
                                        })
                                        .child(
                                            div()
                                                .text_color(if state.showing_placeholder {
                                                    rgb(0x64748b)
                                                } else {
                                                    rgb(0x0f172a)
                                                })
                                                .child(state.display_text),
                                        )
                                        .child(
                                            div()
                                                .text_color(rgb(0x475569))
                                                .child(if state.open { "^" } else { "v" }),
                                        )
                                        .into_any_element()
                                })
                                .render_options_with(|state, _, _| {
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .gap_3()
                                        .px(px(10.0))
                                        .py(px(8.0))
                                        .rounded(px(8.0))
                                        .border_1()
                                        .border_color(if state.highlighted {
                                            rgb(0x2563eb)
                                        } else if state.selected {
                                            rgb(0x60a5fa)
                                        } else {
                                            rgb(0xe2e8f0)
                                        })
                                        .bg(if state.highlighted {
                                            rgb(0xdbeafe)
                                        } else if state.selected {
                                            rgb(0xf8fafc)
                                        } else {
                                            rgb(0xffffff)
                                        })
                                        .child(div().child(state.label))
                                        .child(
                                            div()
                                                .text_color(if state.selected {
                                                    rgb(0x1d4ed8)
                                                } else {
                                                    kael::rgba(0x00000000)
                                                })
                                                .child("*"),
                                        )
                                        .into_any_element()
                                })
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
                                date_picker("delivery_date", self.delivery_date)
                                    .render_with(|state, _, _| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_3()
                                            .px(px(12.0))
                                            .py(px(8.0))
                                            .rounded(px(10.0))
                                            .border_1()
                                            .border_color(if state.focused || state.open {
                                                rgb(0x1d4ed8)
                                            } else {
                                                rgb(0x94a3b8)
                                            })
                                            .bg(if state.open {
                                                rgb(0xdbeafe)
                                            } else {
                                                rgb(0xffffff)
                                            })
                                            .child(
                                                div()
                                                    .text_color(rgb(0x0f172a))
                                                    .child(state.display_label),
                                            )
                                            .child(
                                                div()
                                                    .text_color(rgb(0x475569))
                                                    .child(if state.open { "^" } else { "v" }),
                                            )
                                            .into_any_element()
                                    })
                                    .render_days_with(|state, _, _| {
                                        div()
                                            .w(px(36.0))
                                            .h(px(36.0))
                                            .rounded(px(8.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_color(if state.disabled {
                                                rgb(0x94a3b8)
                                            } else if state.selected {
                                                rgb(0xffffff)
                                            } else {
                                                rgb(0x0f172a)
                                            })
                                            .bg(if state.disabled {
                                                kael::rgba(0x00000000)
                                            } else if state.selected {
                                                rgb(0x1d4ed8)
                                            } else if state.highlighted {
                                                rgb(0xdbeafe)
                                            } else {
                                                rgb(0xffffff)
                                            })
                                            .border_1()
                                            .border_color(if state.highlighted && !state.selected {
                                                rgb(0x60a5fa)
                                            } else {
                                                kael::rgba(0x00000000)
                                            })
                                            .child(state.day.to_string())
                                            .into_any_element()
                                    })
                                    .on_change({
                                        let example = example.clone();
                                        move |date, _, cx| {
                                            example.update(cx, |this, cx| {
                                                this.delivery_date = *date;
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
                            .child("Overlay primitives")
                            .child("The modal keeps layering, focus, escape, and click-outside dismissal inside the framework while the dialog body stays caller-owned.")
                            .child(
                                button("reset-delivery-date")
                                    .label("Review delivery reset")
                                    .on_click({
                                        let example = example.clone();
                                        move |_, _, cx| {
                                            example.update(cx, |this, cx| {
                                                this.reset_modal_open = true;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                modal("delivery-reset-modal", self.reset_modal_open)
                                    .label("Reset delivery date")
                                    .render_with({
                                        let example = example.clone();
                                        move |state, _, _| {
                                            div()
                                                .w(px(340.0))
                                                .flex()
                                                .flex_col()
                                                .gap_3()
                                                .p_4()
                                                .rounded(px(16.0))
                                                .border_1()
                                                .border_color(if state.focused {
                                                    rgb(0x1d4ed8)
                                                } else {
                                                    rgb(0xcbd5e1)
                                                })
                                                .bg(rgb(0xffffff))
                                                .shadow_lg()
                                                .child(
                                                    div()
                                                        .text_color(rgb(0x0f172a))
                                                        .font_weight(kael::FontWeight::SEMIBOLD)
                                                        .child("Reset delivery date?"),
                                                )
                                                .child(
                                                    div()
                                                        .text_color(rgb(0x475569))
                                                        .child("This restores the example back to its original June 15 schedule."),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .justify_end()
                                                        .gap_2()
                                                        .child(
                                                            button("cancel-delivery-reset")
                                                                .label("Cancel")
                                                                .on_click({
                                                                    let example = example.clone();
                                                                    move |_, _, cx| {
                                                                        example.update(cx, |this, cx| {
                                                                            this.reset_modal_open = false;
                                                                            cx.notify();
                                                                        });
                                                                    }
                                                                }),
                                                        )
                                                        .child(
                                                            button("confirm-delivery-reset")
                                                                .label("Reset date")
                                                                .on_click({
                                                                    let example = example.clone();
                                                                    move |_, _, cx| {
                                                                        example.update(cx, |this, cx| {
                                                                            this.delivery_date = default_delivery_date();
                                                                            this.reset_modal_open = false;
                                                                            cx.notify();
                                                                        });
                                                                    }
                                                                }),
                                                        ),
                                                )
                                                .into_any_element()
                                        }
                                    })
                                    .on_change({
                                        let example = example.clone();
                                        move |open, _, cx| {
                                            example.update(cx, |this, cx| {
                                                this.reset_modal_open = *open;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    )
                    .child(format!(
                        "Summary: {}, {}, {}, {}%, {}, {}",
                        if self.project_name.is_empty() {
                            "untitled".to_string()
                        } else {
                            self.project_name.to_string()
                        },
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
                    project_name: SharedString::from("Control room"),
                    notifications: true,
                    compact_mode: false,
                    volume: 65.0,
                    theme: ThemeMode::System,
                    accent: AccentColor::Blue,
                    delivery_date: default_delivery_date(),
                    reset_modal_open: false,
                })
            },
        )
        .unwrap();
    });
}
