#![allow(clippy::redundant_clone)]

use std::{cell::RefCell, rc::Rc, sync::OnceLock, time::Instant};

use kael::{
    actions, div, prelude::*, px, rgb, size, text_input, App, Application, Bounds, Context, Entity,
    FontWeight, Keystroke, Menu, MenuItem, OsAction, Pixels, SharedString, Window, WindowBounds,
    WindowOptions,
};

actions!(
    objc2_smoke,
    [
        Quit,
        Cut,
        Copy,
        Paste,
        SelectAll,
        Undo,
        Redo,
        EmitTestEvent,
        ToggleFullscreen,
    ]
);

const HOTKEY_ID: u32 = 0xC0FFEE;

#[derive(Clone)]
struct Logger {
    lines: Rc<RefCell<Vec<SharedString>>>,
    entity: Rc<RefCell<Option<Entity<MigrationCheck>>>>,
}

impl Logger {
    fn new() -> Self {
        Self {
            lines: Rc::new(RefCell::new(Vec::new())),
            entity: Rc::new(RefCell::new(None)),
        }
    }

    fn bind(&self, entity: Entity<MigrationCheck>) {
        *self.entity.borrow_mut() = Some(entity);
    }

    fn log(&self, message: impl Into<SharedString>, cx: &mut App) {
        let message: SharedString = message.into();
        let stamped = format!(
            "{:>6.2}s  {}",
            start_time().elapsed().as_secs_f64(),
            message.as_ref()
        );
        let mut lines = self.lines.borrow_mut();
        lines.push(stamped.into());
        let overflow = lines.len().saturating_sub(MAX_LINES);
        if overflow > 0 {
            lines.drain(0..overflow);
        }
        drop(lines);
        if let Some(entity) = self.entity.borrow().as_ref() {
            entity.update(cx, |_, cx| cx.notify());
        }
    }
}

const MAX_LINES: usize = 80;

static START_TIME: OnceLock<Instant> = OnceLock::new();

fn start_time() -> Instant {
    *START_TIME.get_or_init(Instant::now)
}

struct MigrationCheck {
    logger: Logger,
    input_text: SharedString,
}

impl Render for MigrationCheck {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let logger = self.logger.clone();
        let lines = logger.lines.borrow().clone();
        let entity = cx.entity();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x0d1117))
            .text_color(rgb(0xc9d1d9))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(0x30363d))
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child("objc2 migration smoke check"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x8b949e))
                            .child(checklist_summary()),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(rgb(0x30363d))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child("Type to exercise IME / insertText / setMarkedText:")
                    .child(
                        text_input("ime-probe", self.input_text.clone())
                            .placeholder("Try emoji input (Ctrl-Cmd-Space), IME, dead keys…")
                            .on_change({
                                let entity = entity.clone();
                                move |text, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.logger.log(
                                            format!("text_input change: {:?}", text.as_ref()),
                                            cx,
                                        );
                                        this.input_text = text;
                                        cx.notify();
                                    });
                                }
                            })
                            .on_submit({
                                let entity = entity.clone();
                                move |text, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.logger.log(
                                            format!("text_input submit: {:?}", text.as_ref()),
                                            cx,
                                        );
                                    });
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .id("event-log")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .children(lines.into_iter().map(|line| {
                        div()
                            .py_0p5()
                            .border_b_1()
                            .border_color(rgb(0x21262d))
                            .child(line)
                    })),
            )
    }
}

fn checklist_summary() -> &'static str {
    "Resize, focus/blur, fullscreen, Cmd+W close, Cmd+C/V/Z menu items, scroll, tray click, sleep/wake, Ctrl+Cmd+K hotkey, kael://hi URL scheme — every wired AppKit selector should print here."
}

fn main() {
    let logger = Logger::new();
    let app = Application::new();
    app.on_open_urls({
        let logger = logger.clone();
        move |urls| {
            eprintln!("on_open_urls: {urls:?}");
            // Cannot push to logger from outside the App turn; surface via stderr too.
            let _ = logger;
        }
    });

    app.run(move |cx: &mut App| {
        cx.activate(true);
        cx.set_keep_alive_without_windows(false);

        register_actions_and_menus(cx, logger.clone());
        register_tray(cx, logger.clone());
        register_global_hotkey(cx, logger.clone());
        register_platform_observers(cx, logger.clone());

        let bounds = Bounds::centered(None, size(px(720.0), px(540.0)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                {
                    let logger = logger.clone();
                    move |window, cx| {
                        let logger_for_should_close = logger.clone();
                        window.on_window_should_close(cx, move |_window, cx| {
                            logger_for_should_close.log("window_should_close fired", cx);
                            true
                        });

                        let view = cx.new({
                            let logger = logger.clone();
                            move |cx| {
                                let logger_for_bounds = logger.clone();
                                cx.observe_window_bounds(window, move |_, window, cx| {
                                    let bounds = window.bounds();
                                    logger_for_bounds.log(
                                        format!(
                                            "observe_window_bounds: {}",
                                            describe_bounds(bounds)
                                        ),
                                        cx,
                                    );
                                })
                                .detach();

                                let logger_for_act = logger.clone();
                                cx.observe_window_activation(window, move |_, window, cx| {
                                    logger_for_act.log(
                                        format!(
                                            "observe_window_activation: active={}",
                                            window.is_window_active()
                                        ),
                                        cx,
                                    );
                                })
                                .detach();

                                let logger_for_appearance = logger.clone();
                                cx.observe_window_appearance(window, move |_, window, cx| {
                                    logger_for_appearance.log(
                                        format!(
                                            "observe_window_appearance: {:?}",
                                            window.appearance()
                                        ),
                                        cx,
                                    );
                                })
                                .detach();

                                MigrationCheck {
                                    logger: logger.clone(),
                                    input_text: SharedString::default(),
                                }
                            }
                        });
                        view
                    }
                },
            )
            .expect("failed to open window");

        let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
        logger.bind(view.clone());

        let logger_for_ks = logger.clone();
        cx.observe_keystrokes(move |event, _, cx| {
            logger_for_ks.log(
                format!("observe_keystrokes: {}", event.keystroke.unparse()),
                cx,
            );
        })
        .detach();

        let logger_for_layout = logger.clone();
        cx.on_keyboard_layout_change(move |cx| {
            logger_for_layout.log(
                format!("on_keyboard_layout_change: {}", cx.keyboard_layout().name()),
                cx,
            );
        })
        .detach();

        logger.log("App started — exercise each path; lines appear here.", cx);
        logger.log(format!("OS: {:?}", cx.os_info()), cx);

        let _ = cx.register_url_scheme("kael");
    });
}

fn describe_bounds(bounds: Bounds<Pixels>) -> String {
    format!(
        "origin=({:.0},{:.0}) size=({:.0},{:.0})",
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    )
}

fn register_actions_and_menus(cx: &mut App, logger: Logger) {
    let logger_q = logger.clone();
    cx.on_action(move |_: &Quit, cx: &mut App| {
        logger_q.log("Quit action", cx);
        cx.quit();
    });
    let logger_t = logger.clone();
    cx.on_action(move |_: &EmitTestEvent, cx: &mut App| {
        logger_t.log("EmitTestEvent action fired", cx);
    });
    let l = logger.clone();
    cx.on_action(move |_: &Cut, cx: &mut App| l.log("menu: cut:", cx));
    let l = logger.clone();
    cx.on_action(move |_: &Copy, cx: &mut App| l.log("menu: copy:", cx));
    let l = logger.clone();
    cx.on_action(move |_: &Paste, cx: &mut App| l.log("menu: paste:", cx));
    let l = logger.clone();
    cx.on_action(move |_: &SelectAll, cx: &mut App| l.log("menu: selectAll:", cx));
    let l = logger.clone();
    cx.on_action(move |_: &Undo, cx: &mut App| l.log("menu: undo:", cx));
    let l = logger.clone();
    cx.on_action(move |_: &Redo, cx: &mut App| l.log("menu: redo:", cx));

    cx.set_menus(vec![
        Menu {
            name: "Kael".into(),
            icon: None,
            items: vec![
                MenuItem::action("Quit Kael", Quit),
                MenuItem::separator(),
                MenuItem::action("Emit test event", EmitTestEvent),
            ],
        },
        Menu {
            name: "Edit".into(),
            icon: None,
            items: vec![
                MenuItem::os_action("Cut", Cut, OsAction::Cut),
                MenuItem::os_action("Copy", Copy, OsAction::Copy),
                MenuItem::os_action("Paste", Paste, OsAction::Paste),
                MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
                MenuItem::separator(),
                MenuItem::os_action("Undo", Undo, OsAction::Undo),
                MenuItem::os_action("Redo", Redo, OsAction::Redo),
            ],
        },
    ]);
}

fn register_tray(cx: &mut App, logger: Logger) {
    cx.set_tray_tooltip("kael objc2 smoke check");
    cx.set_tray_menu(vec![
        kael::TrayMenuItem::Action {
            label: "Ping".into(),
            id: "ping".into(),
        },
        kael::TrayMenuItem::Separator,
        kael::TrayMenuItem::Action {
            label: "Quit".into(),
            id: "quit".into(),
        },
    ]);

    let l = logger.clone();
    cx.on_tray_icon_event(move |event, cx| {
        l.log(format!("on_tray_icon_event: {event:?}"), cx);
    });
    let l = logger.clone();
    cx.on_tray_menu_action(move |id, cx| {
        l.log(format!("on_tray_menu_action: {id}"), cx);
        if id.as_ref() == "quit" {
            cx.quit();
        }
    });
}

fn register_global_hotkey(cx: &mut App, logger: Logger) {
    match Keystroke::parse("ctrl-cmd-k") {
        Ok(stroke) => match cx.register_global_hotkey(HOTKEY_ID, &stroke) {
            Ok(()) => {
                let l = logger.clone();
                cx.on_global_hotkey(move |id| {
                    eprintln!("global hotkey fired: {id}");
                    let _ = &l;
                });
                let l = logger.clone();
                cx.on_global_hotkey_up(move |id| {
                    eprintln!("global hotkey released: {id}");
                    let _ = &l;
                });
            }
            Err(error) => {
                eprintln!("register_global_hotkey failed: {error:#}");
            }
        },
        Err(error) => eprintln!("parse hotkey failed: {error:?}"),
    }
}

fn register_platform_observers(cx: &mut App, logger: Logger) {
    let l = logger.clone();
    cx.on_system_power_event(move |event, cx| {
        l.log(format!("on_system_power_event: {event:?}"), cx);
    });
    let l = logger.clone();
    cx.on_network_status_change(move |status, cx| {
        l.log(format!("on_network_status_change: {status:?}"), cx);
    });
    let l = logger.clone();
    cx.on_media_key_event(move |event, cx| {
        l.log(format!("on_media_key_event: {event:?}"), cx);
    });
}
