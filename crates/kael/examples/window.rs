use kael::{
    App, Application, Bounds, Context, DismissEvent, EventEmitter, FocusHandle, Focusable,
    KeyBinding, LayerAnchor, LayerOptions, LayerStack, PromptButton, PromptLevel, SharedString,
    Timer, Window, WindowBounds, WindowKind, WindowOptions, actions, div, point, prelude::*, px,
    rgb, size,
};

struct SubWindow {
    custom_titlebar: bool,
}

struct ModalDemo {
    focus: FocusHandle,
}

struct PopoverDemo {
    focus: FocusHandle,
}

fn button(text: &str, on_click: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div()
        .id(SharedString::from(text.to_string()))
        .implicit_transitions()
        .flex_none()
        .px_2()
        .bg(rgb(0xf7f7f7))
        .hover(|this| this.bg(rgb(0xefefef)).border_color(rgb(0xd8d8d8)))
        .active(|this| this.opacity(0.85).scale(0.985))
        .border_1()
        .border_color(rgb(0xe0e0e0))
        .rounded_sm()
        .cursor_pointer()
        .child(text.to_string())
        .on_click(move |_, window, cx| on_click(window, cx))
}

impl Render for SubWindow {
    fn render(&mut self, _window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .size_full()
            .gap_2()
            .when(self.custom_titlebar, |cx| {
                cx.child(
                    div()
                        .flex()
                        .h(px(32.))
                        .px_4()
                        .bg(kael::blue())
                        .text_color(kael::white())
                        .w_full()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size_full()
                                .child("Custom Titlebar"),
                        ),
                )
            })
            .child(
                div()
                    .p_8()
                    .gap_2()
                    .child("SubWindow")
                    .child(button("Close", |window, _| {
                        window.remove_window();
                    })),
            )
    }
}

impl EventEmitter<DismissEvent> for ModalDemo {}

impl Focusable for ModalDemo {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ModalDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_3()
            .w(px(360.))
            .p_6()
            .rounded_lg()
            .border_1()
            .border_color(rgb(0xdedede))
            .bg(rgb(0xffffff))
            .shadow_xl()
            .child(
                div()
                    .text_lg()
                    .font_weight(kael::FontWeight::SEMIBOLD)
                    .child("LayerStack Modal"),
            )
            .child(div().text_sm().text_color(rgb(0x525252)).child(
                "This modal is rendered inside the window using the new in-window layer stack.",
            ))
            .child(
                div().flex().justify_end().child(
                    div()
                        .id("close-layer-modal")
                        .implicit_transitions()
                        .px_3()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgb(0xe0e0e0))
                        .bg(rgb(0xf7f7f7))
                        .hover(|this| this.bg(rgb(0xefefef)).border_color(rgb(0xd8d8d8)))
                        .active(|this| this.opacity(0.85).scale(0.985))
                        .cursor_pointer()
                        .child("Close")
                        .on_click(cx.listener(|_, _, _, cx| {
                            cx.emit(DismissEvent);
                        })),
                ),
            )
    }
}

impl EventEmitter<DismissEvent> for PopoverDemo {}

impl Focusable for PopoverDemo {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PopoverDemo {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .gap_2()
            .w(px(240.))
            .p_4()
            .rounded_md()
            .border_1()
            .border_color(rgb(0xdedede))
            .bg(rgb(0xffffff))
            .shadow_lg()
            .child(
                div()
                    .font_weight(kael::FontWeight::SEMIBOLD)
                    .child("Anchored Popover"),
            )
            .child(div().text_sm().text_color(rgb(0x525252)).child(
                "This layer uses window-space anchoring plus outside-click and escape dismissal.",
            ))
    }
}

struct WindowDemo {
    layer_stack: kael::Entity<LayerStack>,
}

impl Render for WindowDemo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_bounds =
            WindowBounds::Windowed(Bounds::centered(None, size(px(300.0), px(300.0)), cx));
        let modal_layer_stack = self.layer_stack.clone();
        let popover_layer_stack = self.layer_stack.clone();

        div()
            .relative()
            .bg(rgb(0xffffff))
            .size_full()
            .child(
                div()
                    .p_4()
                    .flex()
                    .flex_wrap()
                    .size_full()
                    .justify_center()
                    .content_center()
                    .gap_2()
                    .child(button("Normal", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                window_bounds: Some(window_bounds),
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: false,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("Popup", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                window_bounds: Some(window_bounds),
                                kind: WindowKind::PopUp,
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: false,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("Custom Titlebar", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                titlebar: None,
                                window_bounds: Some(window_bounds),
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: true,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("Invisible", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                show: false,
                                window_bounds: Some(window_bounds),
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: false,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("Unmovable", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                is_movable: false,
                                titlebar: None,
                                window_bounds: Some(window_bounds),
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: false,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("Unresizable", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                is_resizable: false,
                                window_bounds: Some(window_bounds),
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: false,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("Unminimizable", move |_, cx| {
                        cx.open_window(
                            WindowOptions {
                                is_minimizable: false,
                                window_bounds: Some(window_bounds),
                                ..Default::default()
                            },
                            |_, cx| {
                                cx.new(|_| SubWindow {
                                    custom_titlebar: false,
                                })
                            },
                        )
                        .unwrap();
                    }))
                    .child(button("In-Window Modal", move |window, cx| {
                        let modal = cx.new(|cx| ModalDemo {
                            focus: cx.focus_handle(),
                        });
                        modal_layer_stack.update(cx, |stack, cx| {
                            stack.push(modal, LayerOptions::modal(), window, cx);
                        });
                    }))
                    .child(button("Anchored Popover", move |window, cx| {
                        let popover = cx.new(|cx| PopoverDemo {
                            focus: cx.focus_handle(),
                        });
                        popover_layer_stack.update(cx, |stack, cx| {
                            stack.push(
                                popover,
                                LayerOptions::default()
                                    .anchored(
                                        LayerAnchor::at(point(px(620.), px(96.)))
                                            .offset(point(px(0.), px(12.)))
                                            .snap_to_window(),
                                    )
                                    .dismiss_on_click_outside()
                                    .dismiss_on_escape()
                                    .priority(50),
                                window,
                                cx,
                            );
                        });
                    }))
                    .child(button("Hide Application", |window, cx| {
                        cx.hide();

                        window
                            .spawn(cx, async move |cx| {
                                Timer::after(std::time::Duration::from_secs(3)).await;
                                cx.update(|_, cx| {
                                    cx.activate(false);
                                })
                            })
                            .detach();
                    }))
                    .child(button("Resize", |window, _| {
                        let content_size = window.bounds().size;
                        window.resize(size(content_size.height, content_size.width));
                    }))
                    .child(button("Prompt", |window, cx| {
                        let answer = window.prompt(
                            PromptLevel::Info,
                            "Are you sure?",
                            None,
                            &["Ok", "Cancel"],
                            cx,
                        );

                        cx.spawn(async move |_| {
                            if answer.await.unwrap() == 0 {
                                println!("You have clicked Ok");
                            } else {
                                println!("You have clicked Cancel");
                            }
                        })
                        .detach();
                    }))
                    .child(button("Prompt (non-English)", |window, cx| {
                        let answer = window.prompt(
                            PromptLevel::Info,
                            "Are you sure?",
                            None,
                            &[PromptButton::ok("确定"), PromptButton::cancel("取消")],
                            cx,
                        );

                        cx.spawn(async move |_| {
                            if answer.await.unwrap() == 0 {
                                println!("You have clicked Ok");
                            } else {
                                println!("You have clicked Cancel");
                            }
                        })
                        .detach();
                    })),
            )
            .child(self.layer_stack.clone())
    }
}

actions!(window, [Quit]);

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    cx.observe_window_bounds(window, move |_, window, _| {
                        println!("Window bounds changed: {:?}", window.bounds());
                    })
                    .detach();

                    WindowDemo {
                        layer_stack: cx.new(|_| LayerStack::new()),
                    }
                })
            },
        )
        .unwrap();

        cx.activate(true);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    });
}
