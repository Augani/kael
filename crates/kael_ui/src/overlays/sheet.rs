//! Sheet component for slide-in side panels.

use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::theme::Theme;

actions!(sheet, [SheetClose]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SheetSide {
    Left,
    #[default]
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SheetSize {
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
    Custom,
}

impl SheetSide {
    /// Stable side key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

impl SheetSize {
    /// Stable size key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
            Self::Xl => "xl",
            Self::Custom => "custom",
        }
    }

    fn size(&self) -> Pixels {
        match self {
            Self::Sm => px(320.0),
            Self::Md => px(400.0),
            Self::Lg => px(500.0),
            Self::Xl => px(640.0),
            Self::Custom => px(400.0),
        }
    }
}

pub struct Sheet {
    id: ElementId,
    focus_handle: FocusHandle,
    side: SheetSide,
    size: SheetSize,
    custom_width: Option<Pixels>,
    custom_height: Option<Pixels>,
    title: Option<SharedString>,
    description: Option<SharedString>,
    content: Option<AnyElement>,
    content_builder: Option<Rc<dyn Fn() -> AnyElement>>,
    footer: Option<AnyElement>,
    footer_builder: Option<Rc<dyn Fn() -> AnyElement>>,
    show_close_button: bool,
    close_on_backdrop_click: bool,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    previous_focus_handle: Option<FocusHandle>,
    focus_initialized: bool,
    style: StyleRefinement,
}

impl Sheet {
    #[track_caller]
    pub fn new(cx: &mut Context<Self>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "sheet:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            focus_handle: cx.focus_handle(),
            side: SheetSide::default(),
            size: SheetSize::default(),
            custom_width: None,
            custom_height: None,
            title: None,
            description: None,
            content: None,
            content_builder: None,
            footer: None,
            footer_builder: None,
            show_close_button: true,
            close_on_backdrop_click: true,
            on_close: None,
            previous_focus_handle: None,
            focus_initialized: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn side(mut self, side: SheetSide) -> Self {
        self.side = side;
        self
    }

    pub fn size(mut self, size: SheetSize) -> Self {
        self.size = size;
        self
    }

    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        let width = width.into();
        let value = f32::from(width);
        if value.is_finite() && value > 0.0 {
            self.custom_width = Some(width);
            self.size = SheetSize::Custom;
        }
        self
    }

    pub fn height(mut self, height: impl Into<Pixels>) -> Self {
        let height = height.into();
        let value = f32::from(height);
        if value.is_finite() && value > 0.0 {
            self.custom_height = Some(height);
            self.size = SheetSize::Custom;
        }
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// Build persistent content again for every render of a reusable sheet.
    pub fn content_builder<E>(mut self, builder: impl Fn() -> E + 'static) -> Self
    where
        E: IntoElement,
    {
        self.content_builder = Some(Rc::new(move || builder().into_any_element()));
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Build a persistent footer again for every render of a reusable sheet.
    pub fn footer_builder<E>(mut self, builder: impl Fn() -> E + 'static) -> Self
    where
        E: IntoElement,
    {
        self.footer_builder = Some(Rc::new(move || builder().into_any_element()));
        self
    }

    pub fn show_close_button(mut self, show: bool) -> Self {
        self.show_close_button = show;
        self
    }

    pub fn close_on_backdrop_click(mut self, close: bool) -> Self {
        self.close_on_backdrop_click = close;
        self
    }

    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Stable side key for content-safe diagnostics.
    pub fn side_key(&self) -> &'static str {
        self.side.to_text()
    }

    /// Stable size key for content-safe diagnostics.
    pub fn size_key(&self) -> &'static str {
        self.size.to_text()
    }

    /// Returns true when a custom width or height is configured.
    pub fn has_custom_size(&self) -> bool {
        self.custom_width.is_some() || self.custom_height.is_some()
    }

    /// Returns true when a title is configured.
    pub fn has_title(&self) -> bool {
        self.title.is_some()
    }

    /// Returns true when a description is configured.
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// Returns true when content is configured.
    pub fn has_content(&self) -> bool {
        self.content.is_some() || self.content_builder.is_some()
    }

    /// Returns true when footer content is configured.
    pub fn has_footer(&self) -> bool {
        self.footer.is_some() || self.footer_builder.is_some()
    }

    /// Returns true when a close handler is configured.
    pub fn has_close_handler(&self) -> bool {
        self.on_close.is_some()
    }

    /// Stable dismissal policy key.
    pub fn dismissal_mode(&self) -> &'static str {
        if self.close_on_backdrop_click {
            "backdrop_escape"
        } else {
            "escape"
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "sheet(side={}, size={}, custom_size={}, has_title={}, has_description={}, has_content={}, footer={}, close_button={}, dismissal={}, close_handler={})",
            self.side_key(),
            self.size_key(),
            self.has_custom_size(),
            self.has_title(),
            self.has_description(),
            self.has_content(),
            self.has_footer(),
            self.show_close_button,
            self.dismissal_mode(),
            self.has_close_handler()
        )
    }

    fn handle_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(previous_focus_handle) = self.previous_focus_handle.take() {
            window.focus(&previous_focus_handle);
        }
        self.focus_initialized = false;
        if let Some(handler) = &self.on_close {
            handler(window, cx);
        }
    }

    fn handle_escape(&mut self, _: &SheetClose, window: &mut Window, cx: &mut Context<Self>) {
        self.handle_close(window, cx);
    }

    fn get_sheet_size(&self) -> Pixels {
        if let Some(width) = self.custom_width {
            return width;
        }
        if let Some(height) = self.custom_height {
            return height;
        }
        self.size.size()
    }
}

pub fn init_sheet(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", SheetClose, Some("Sheet"))]);
}

impl Styled for Sheet {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for Sheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let has_header =
            self.title.is_some() || self.description.is_some() || self.show_close_button;
        let sheet_size = self.get_sheet_size();
        let user_style = self.style.clone();
        let sheet_id = self.id.clone();
        let accessibility_label = self
            .title
            .clone()
            .unwrap_or_else(|| "Sheet".into())
            .to_string();
        let mut sheet_accessibility =
            AccessibilityAttributes::new(AccessibilityRole::Dialog).label(accessibility_label);
        if let Some(description) = self.description.as_ref() {
            sheet_accessibility = sheet_accessibility.description(description.to_string());
        }
        let content = self
            .content_builder
            .as_ref()
            .map(|builder| builder())
            .or_else(|| self.content.take());
        let footer = self
            .footer_builder
            .as_ref()
            .map(|builder| builder())
            .or_else(|| self.footer.take());

        if !self.focus_initialized {
            self.previous_focus_handle = window.focused(cx);
            self.focus_initialized = true;
        }
        if !self.focus_handle.contains_focused(window, cx) {
            window.focus(&self.focus_handle);
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(sheet_id.clone()),
                "overlay".into(),
            ))
            .key_context("Sheet")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let modifiers = event.keystroke.modifiers;
                if event.keystroke.key != "tab"
                    || modifiers.number_of_modifiers() != u8::from(modifiers.shift)
                {
                    return;
                }
                if modifiers.shift {
                    window.focus_prev_within(&this.focus_handle);
                } else {
                    window.focus_next_within(&this.focus_handle);
                }
                window.prevent_default();
                cx.stop_propagation();
            }))
            .on_action(cx.listener(Self::handle_escape))
            .absolute()
            .inset_0()
            .flex()
            .bg(hsla(0.0, 0.0, 0.0, 0.5))
            .when(self.close_on_backdrop_click, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.handle_close(window, cx);
                    }),
                )
            })
            .child(
                div()
                    .id(sheet_id.clone())
                    .accessibility(sheet_accessibility)
                    .track_focus(&self.focus_handle)
                    .occlude()
                    .flex()
                    .flex_col()
                    .bg(theme.tokens.card)
                    .border_color(theme.tokens.border)
                    .shadow(theme.tokens.shadow_lg.to_vec())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .when(self.side == SheetSide::Right, |this| {
                        this.absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(sheet_size)
                            .border_l_1()
                    })
                    .when(self.side == SheetSide::Left, |this| {
                        this.absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(sheet_size)
                            .border_r_1()
                    })
                    .when(self.side == SheetSide::Top, |this| {
                        this.absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(sheet_size)
                            .border_b_1()
                    })
                    .when(self.side == SheetSide::Bottom, |this| {
                        this.absolute()
                            .bottom_0()
                            .left_0()
                            .right_0()
                            .h(sheet_size)
                            .border_t_1()
                    })
                    .when(has_header, |this| {
                        this.child(
                            div()
                                .flex()
                                .items_start()
                                .justify_between()
                                .px(px(24.0))
                                .pt(px(24.0))
                                .pb(px(20.0))
                                .border_b_1()
                                .border_color(theme.tokens.border)
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .gap(px(4.0))
                                        .when_some(self.title.clone(), |this: Div, title| {
                                            this.child(
                                                div()
                                                    .text_size(px(18.0))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.tokens.foreground)
                                                    .child(
                                                        StyledText::new(title)
                                                            .accessibility_hidden(true),
                                                    ),
                                            )
                                        })
                                        .when_some(self.description.clone(), |this: Div, desc| {
                                            this.child(
                                                div()
                                                    .text_size(px(14.0))
                                                    .text_color(theme.tokens.muted_foreground)
                                                    .whitespace_normal()
                                                    .child(
                                                        StyledText::new(desc)
                                                            .accessibility_hidden(true),
                                                    ),
                                            )
                                        }),
                                )
                                .when(self.show_close_button, |this: Div| {
                                    this.child(
                                        Button::new(
                                            ElementId::NamedChild(
                                                Box::new(sheet_id.clone()),
                                                "close".into(),
                                            ),
                                            "Close",
                                        )
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Icon)
                                        .flex_shrink_0()
                                        .icon("x")
                                        .tooltip("Close sheet")
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.handle_close(window, cx);
                                            }),
                                        ),
                                    )
                                }),
                        )
                    })
                    .when_some(content, |this, content| {
                        this.child(div().flex_1().overflow_hidden().child(content))
                    })
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div
                    })
                    .when_some(footer, |this, footer| {
                        this.child(
                            div()
                                .px(px(24.0))
                                .py(px(16.0))
                                .border_t_1()
                                .border_color(theme.tokens.border)
                                .child(footer),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::{TestAppContext, div, px};

    #[::core::prelude::v1::test]
    fn sheet_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let sheet = cx.update(|cx| {
            cx.new(|cx| {
                Sheet::new(cx)
                    .side(SheetSide::Left)
                    .width(px(512.0))
                    .title("Private Inspector")
                    .description("Secret object metadata")
                    .content(div().child("private sheet content"))
                    .footer(div().child("private sheet footer"))
                    .show_close_button(false)
                    .close_on_backdrop_click(false)
                    .on_close(|_, _| {})
            })
        });

        cx.update(|cx| {
            let sheet = sheet.read(cx);
            assert_eq!(SheetSide::Left.to_text(), "left");
            assert_eq!(SheetSize::Custom.to_text(), "custom");
            assert_eq!(sheet.side_key(), "left");
            assert_eq!(sheet.size_key(), "custom");
            assert!(sheet.has_custom_size());
            assert!(sheet.has_title());
            assert!(sheet.has_description());
            assert!(sheet.has_content());
            assert!(sheet.has_footer());
            assert!(!sheet.show_close_button);
            assert_eq!(sheet.dismissal_mode(), "escape");
            assert!(sheet.has_close_handler());

            let summary = sheet.to_text();
            assert!(summary.contains("side=left"));
            assert!(summary.contains("size=custom"));
            assert!(summary.contains("dismissal=escape"));
            assert!(!summary.contains("Private Inspector"));
            assert!(!summary.contains("Secret object"));
            assert!(!summary.contains("private sheet"));
            assert!(!summary.contains("512"));
        });
    }

    #[::core::prelude::v1::test]
    fn invalid_custom_sizes_keep_the_default_size() {
        let cx = TestAppContext::single();
        for value in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            let sheet =
                cx.update(|cx| cx.new(|cx| Sheet::new(cx).width(px(value)).height(px(value))));
            cx.update(|cx| {
                let sheet = sheet.read(cx);
                assert_eq!(sheet.size, SheetSize::Md);
                assert!(!sheet.has_custom_size());
            });
        }
    }
}

// Real mounted Tab-routing baselines. The fixture does not install a focus trap.
#[cfg(test)]
mod sheet_modal_focus_mounted_baseline {
    use super::*;
    use crate::components::button::Button;
    use kael::{TestAppContext, VisualTestContext};
    use std::{cell::Cell, rc::Rc};

    struct Body {
        prefix: &'static str,
        middle_disabled: bool,
        omit_last: bool,
        nested: Option<Entity<Sheet>>,
    }

    impl Render for Body {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let button = |part: &str| {
                Button::new(
                    ElementId::Name(format!("{}-{part}", self.prefix).into()),
                    format!("{} {part}", self.prefix),
                )
                .on_click(|_, _, _| {})
            };
            div()
                .relative()
                .w_full()
                .h(px(220.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(button("first"))
                .child(button("middle").disabled(self.middle_disabled))
                .when(!self.omit_last, |this| this.child(button("last")))
                .children(self.nested.clone())
        }
    }

    struct Host {
        modal: Option<Entity<Sheet>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .p(px(16.0))
                        .flex()
                        .gap(px(8.0))
                        .child(
                            Button::new("outside-before", "Outside before").on_click(|_, _, _| {}),
                        )
                        .child(
                            Button::new("outside-after", "Outside after").on_click(|_, _, _| {}),
                        ),
                )
                .children(self.modal.clone())
        }
    }

    struct Fixture {
        _host: Entity<Host>,
        body: Entity<Body>,
        _modal: Entity<Sheet>,
        window: VisualTestContext,
        _closes: Rc<Cell<usize>>,
    }

    fn button_id(window: &Window, label: &str) -> AccessibilityId {
        let matches = window
            .accessibility_tree()
            .nodes
            .values()
            .filter(|node| {
                node.role == AccessibilityRole::Button && node.label.as_deref() == Some(label)
            })
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1, "expected one real button {label}");
        matches[0].id
    }

    fn button_labels(window: &Window) -> Vec<String> {
        let mut labels = window
            .accessibility_tree()
            .nodes
            .values()
            .filter(|node| node.role == AccessibilityRole::Button)
            .map(|node| node.label.clone().expect("every fixture button is named"))
            .collect::<Vec<_>>();
        labels.sort();
        labels
    }

    fn assert_button_inventory(window: &Window, expected: &[&str]) {
        let mut expected = expected
            .iter()
            .map(|label| (*label).to_owned())
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            button_labels(window),
            expected,
            "complete mounted button inventory"
        );
    }

    fn assert_focused(window: &mut VisualTestContext, label: &str) {
        window.update(|window, cx| {
            assert!(
                window.focused(cx).is_some(),
                "actual window focus must exist"
            );
            let tree = window.accessibility_tree();
            assert_eq!(tree.focused_node_count(), 1, "one exported focus owner");
            assert_eq!(
                tree.focused_node(),
                Some(button_id(window, label)),
                "actual focused button must be {label}"
            );
        });
    }

    fn focus_button(window: &mut VisualTestContext, label: &str) {
        window.update(|window, _| {
            let id = button_id(window, label);
            assert!(window.has_accessibility_action_handler(id, AccessibilityAction::Focus));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Focus,
            ));
        });
        window.run_until_parked();
        window.update(|window, cx| window.draw(cx).clear());
        assert_focused(window, label);
    }

    fn key(window: &mut VisualTestContext, keystroke: &str, expected: &str) {
        let before = window.update(|window, _| button_labels(window));
        window.simulate_keystrokes(keystroke);
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(
                button_labels(window),
                before,
                "Tab must not remove fixture controls"
            );
        });
        assert_focused(window, expected);
    }

    fn mounted() -> Fixture {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let body = cx.new(|_| Body {
            prefix: "Modal",
            middle_disabled: true,
            omit_last: false,
            nested: None,
        });
        let closes = Rc::new(Cell::new(0));
        let (host, borrowed_window) = cx.add_window_view(|_, _| Host { modal: None });
        let mut window = borrowed_window.clone();
        window.simulate_resize(size(px(640.0), px(480.0)));
        window.deactivate_window();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        window.update(|window, cx| {
            assert!(
                window.is_window_active(),
                "focus lifecycle needs an active test window"
            );
            window.draw(cx).clear();
            assert_button_inventory(window, &["Outside before", "Outside after"]);
        });
        // Establish ordinary global Tab behavior before mounting any modal.
        focus_button(&mut window, "Outside before");
        key(&mut window, "tab", "Outside after");
        focus_button(&mut window, "Outside before");
        let weak_host = host.downgrade();
        let content = body.clone();
        let close_count = closes.clone();
        let modal = window.update(|_, cx| {
            cx.new(|cx| {
                Sheet::new(cx)
                    .id("outer-modal")
                    .title("Outer modal")
                    .show_close_button(false)
                    .close_on_backdrop_click(false)
                    .content_builder(move || content.clone())
                    .on_close(move |_, cx| {
                        close_count.set(close_count.get() + 1);
                        let _ = weak_host.update(cx, |host, cx| {
                            host.modal = None;
                            cx.notify();
                        });
                    })
            })
        });
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.modal = Some(modal.clone());
                cx.notify();
            });
            window.draw(cx).clear();
            assert!(window.accessibility_tree().nodes.values().any(|node| {
                node.role == AccessibilityRole::Dialog
                    && node.label.as_deref() == Some("Outer modal")
            }));
            assert_button_inventory(
                window,
                &[
                    "Outside before",
                    "Outside after",
                    "Modal first",
                    "Modal middle",
                    "Modal last",
                ],
            );
            let disabled = window
                .accessibility_tree()
                .get(button_id(window, "Modal middle"))
                .unwrap();
            assert!(disabled.states.contains(AccessibilityState::DISABLED));
        });
        Fixture {
            _host: host,
            body,
            _modal: modal,
            window,
            _closes: closes,
        }
    }

    fn add_nested(f: &mut Fixture) -> Rc<Cell<usize>> {
        focus_button(&mut f.window, "Modal first");
        let inner_body = f.window.update(|_, cx| {
            cx.new(|_| Body {
                prefix: "Inner",
                middle_disabled: true,
                omit_last: false,
                nested: None,
            })
        });
        let closes = Rc::new(Cell::new(0));
        let close_count = closes.clone();
        let weak_body = f.body.downgrade();
        let inner = f.window.update(|_, cx| {
            cx.new(|cx| {
                Sheet::new(cx)
                    .id("inner-modal")
                    .title("Inner modal")
                    .show_close_button(false)
                    .close_on_backdrop_click(false)
                    .content_builder(move || inner_body.clone())
                    .on_close(move |_, cx| {
                        close_count.set(close_count.get() + 1);
                        let _ = weak_body.update(cx, |body, cx| {
                            body.nested = None;
                            cx.notify();
                        });
                    })
            })
        });
        f.window.update(|window, cx| {
            f.body.update(cx, |body, cx| {
                body.nested = Some(inner);
                cx.notify();
            });
            window.draw(cx).clear();
            assert!(window.accessibility_tree().nodes.values().any(|node| {
                node.role == AccessibilityRole::Dialog
                    && node.label.as_deref() == Some("Inner modal")
            }));
            assert_button_inventory(
                window,
                &[
                    "Outside before",
                    "Outside after",
                    "Modal first",
                    "Modal middle",
                    "Modal last",
                    "Inner first",
                    "Inner middle",
                    "Inner last",
                ],
            );
        });
        closes
    }

    #[::core::prelude::v1::test]
    fn sheet_tab_wraps_inside_modal_with_outside_controls() {
        let mut f = mounted();
        focus_button(&mut f.window, "Modal last");
        key(&mut f.window, "tab", "Modal first");
        key(&mut f.window, "tab", "Modal last");
    }

    #[::core::prelude::v1::test]
    fn sheet_shift_tab_wraps_inside_modal_with_outside_controls() {
        let mut f = mounted();
        focus_button(&mut f.window, "Modal first");
        key(&mut f.window, "shift-tab", "Modal last");
        key(&mut f.window, "shift-tab", "Modal first");
    }

    #[::core::prelude::v1::test]
    fn sheet_tab_uses_current_disabled_and_removed_children() {
        let mut f = mounted();
        let removed = f.window.update(|window, _| button_id(window, "Modal last"));
        focus_button(&mut f.window, "Modal first");
        f.window.update(|window, cx| {
            f.body.update(cx, |body, cx| {
                body.middle_disabled = false;
                body.omit_last = true;
                cx.notify();
            });
            window.draw(cx).clear();
            assert!(window.accessibility_tree().get(removed).is_none());
            assert!(!window.has_accessibility_action_handler(removed, AccessibilityAction::Focus));
            let middle = window
                .accessibility_tree()
                .get(button_id(window, "Modal middle"))
                .unwrap();
            assert!(!middle.states.contains(AccessibilityState::DISABLED));
            assert_button_inventory(
                window,
                &[
                    "Outside before",
                    "Outside after",
                    "Modal first",
                    "Modal middle",
                ],
            );
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                removed,
                AccessibilityAction::Focus,
            ));
        });
        f.window.run_until_parked();
        f.window.update(|window, cx| window.draw(cx).clear());
        assert_focused(&mut f.window, "Modal first");
        key(&mut f.window, "tab", "Modal middle");
        key(&mut f.window, "tab", "Modal first");
    }

    #[::core::prelude::v1::test]
    fn sheet_nested_modal_has_the_innermost_tab_scope() {
        let mut f = mounted();
        let _inner_closes = add_nested(&mut f);
        focus_button(&mut f.window, "Inner last");
        key(&mut f.window, "tab", "Inner first");
        key(&mut f.window, "shift-tab", "Inner last");
        assert_eq!(f._closes.get(), 0);
    }

    #[::core::prelude::v1::test]
    fn sheet_escape_closes_only_innermost_and_restores_each_opener() {
        let mut f = mounted();
        let inner_closes = add_nested(&mut f);
        focus_button(&mut f.window, "Inner last");
        f.window.simulate_keystrokes("escape");
        f.window.run_until_parked();
        for _ in 0..2 {
            f.window.simulate_platform_frame();
            f.window.run_until_parked();
        }
        assert_eq!(inner_closes.get(), 1);
        assert_eq!(f._closes.get(), 0);
        f.window.update(|window, _| {
            let tree = window.accessibility_tree();
            assert!(!tree.nodes.values().any(|node| {
                node.role == AccessibilityRole::Dialog
                    && node.label.as_deref() == Some("Inner modal")
            }));
            assert!(tree.nodes.values().any(|node| {
                node.role == AccessibilityRole::Dialog
                    && node.label.as_deref() == Some("Outer modal")
            }));
            assert_button_inventory(
                window,
                &[
                    "Outside before",
                    "Outside after",
                    "Modal first",
                    "Modal middle",
                    "Modal last",
                ],
            );
        });
        assert_focused(&mut f.window, "Modal first");
        f.window.simulate_keystrokes("escape");
        f.window.run_until_parked();
        for _ in 0..2 {
            f.window.simulate_platform_frame();
            f.window.run_until_parked();
        }
        assert_eq!(inner_closes.get(), 1);
        assert_eq!(f._closes.get(), 1);
        f.window.update(|window, _| {
            assert_button_inventory(window, &["Outside before", "Outside after"]);
        });
        assert_focused(&mut f.window, "Outside before");
        key(&mut f.window, "tab", "Outside after");
    }
}

#[cfg(test)]
mod empty_sheet_focus_mounted {
    use super::*;
    use crate::components::button::Button;
    use kael::{TestAppContext, VisualTestContext};

    struct EmptyHost {
        modal: Option<Entity<Sheet>>,
    }
    impl Render for EmptyHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .relative()
                .size_full()
                .child(
                    Button::new("empty-modal-opener", "Empty modal opener").on_click(|_, _, _| {}),
                )
                .child(Button::new("empty-modal-after", "Empty modal after").on_click(|_, _, _| {}))
                .children(self.modal.clone())
        }
    }

    fn assert_empty_root(
        window: &mut VisualTestContext,
        modal: &Entity<Sheet>,
        expected: Option<AccessibilityId>,
    ) -> AccessibilityId {
        window.update(|window, cx| {
            assert!(window.is_window_active());
            assert!(window.focused(cx).is_some());
            assert!(
                modal.read(cx).focus_handle.is_focused(window),
                "the actual empty-modal handle must own focus"
            );
            let tree = window.accessibility_tree();
            let roots = tree
                .nodes
                .values()
                .filter(|node| {
                    node.role == AccessibilityRole::Dialog
                        && node.label.as_deref() == Some("Empty Sheet")
                })
                .collect::<Vec<_>>();
            assert_eq!(roots.len(), 1);
            let root = roots[0].id;
            if let Some(expected) = expected {
                assert_eq!(root, expected, "semantic identity must survive traversal");
            }
            assert_eq!(tree.focused_node_count(), 1, "one semantic focus owner");
            assert_eq!(tree.focused_node(), Some(root));
            let mut buttons = tree
                .nodes
                .values()
                .filter(|node| node.role == AccessibilityRole::Button)
                .map(|node| node.label.as_deref().unwrap())
                .collect::<Vec<_>>();
            buttons.sort_unstable();
            assert_eq!(
                buttons,
                ["Empty modal after", "Empty modal opener"],
                "no hidden close/content tab stops"
            );
            let export = tree.to_accesskit_tree_update(None, None);
            assert_eq!(
                export.focus.0, root.0,
                "actual AccessKit focus must name the semantic Dialog"
            );
            assert!(export.nodes.iter().any(|(id, _)| id.0 == root.0));
            root
        })
    }

    #[::core::prelude::v1::test]
    fn empty_sheet_tab_and_shift_tab_keep_one_exported_dialog_focus_owner() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (host, borrowed) = cx.add_window_view(|_, _| EmptyHost { modal: None });
        let mut window = borrowed.clone();
        window.simulate_resize(size(px(640.0), px(480.0)));
        window.deactivate_window();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window.is_window_active());
            let opener = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Empty modal opener")
                })
                .unwrap()
                .id;
            assert!(window.has_accessibility_action_handler(opener, AccessibilityAction::Focus));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                opener,
                AccessibilityAction::Focus,
            ));
        });
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert_eq!(tree.focused_node_count(), 1);
            assert_eq!(
                tree.get(tree.focused_node().unwrap())
                    .unwrap()
                    .label
                    .as_deref(),
                Some("Empty modal opener")
            );
            assert!(window.focused(cx).is_some());
        });
        let modal = window.update(|_, cx| {
            cx.new(|cx| {
                Sheet::new(cx)
                    .id("empty-sheet-root")
                    .title("Empty Sheet")
                    .show_close_button(false)
                    .close_on_backdrop_click(false)
            })
        });
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.modal = Some(modal.clone());
                cx.notify();
            });
            window.draw(cx).clear();
        });
        let root = assert_empty_root(&mut window, &modal, None);
        for key in ["tab", "shift-tab", "tab", "tab", "shift-tab", "shift-tab"] {
            window.simulate_keystrokes(key);
            window.run_until_parked();
            window.update(|window, cx| window.draw(cx).clear());
            assert_empty_root(&mut window, &modal, Some(root));
        }
        window.update(|window, _| window.remove_window());
        drop(modal);
        drop(host);
        drop(window);
        cx.quit();
    }
}
