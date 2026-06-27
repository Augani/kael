//! Dialog component with focus trap and backdrop.

use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;
use std::time::Duration;

use crate::animations::easings;
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::theme::Theme;

actions!(dialog, [DialogCancel]);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DialogSize {
    Sm,
    Md,
    Lg,
    Xl,
    Full,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DialogVariant {
    #[default]
    Standard,
    Fullscreen,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum DialogPurpose {
    Required,
    Form,
    #[default]
    Info,
}

#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct DialogPosition {
    pub top: Option<Pixels>,
    pub right: Option<Pixels>,
    pub bottom: Option<Pixels>,
    pub left: Option<Pixels>,
}

impl DialogPosition {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn top(mut self, top: Pixels) -> Self {
        self.top = Some(top);
        self
    }

    pub fn right(mut self, right: Pixels) -> Self {
        self.right = Some(right);
        self
    }

    pub fn bottom(mut self, bottom: Pixels) -> Self {
        self.bottom = Some(bottom);
        self
    }

    pub fn left(mut self, left: Pixels) -> Self {
        self.left = Some(left);
        self
    }
}

impl DialogSize {
    fn width(&self) -> Length {
        match self {
            Self::Sm => px(320.0).into(),
            Self::Md => px(400.0).into(),
            Self::Lg => px(600.0).into(),
            Self::Xl => px(800.0).into(),
            Self::Full => relative(1.0).into(),
        }
    }

    fn max_height(&self) -> Length {
        match self {
            Self::Full => relative(1.0).into(),
            _ => relative(0.75).into(),
        }
    }
}

#[derive(IntoElement)]
pub struct DialogHeader {
    title: SharedString,
    subtitle: Option<SharedString>,
    start_content: Option<AnyElement>,
    end_content: Option<AnyElement>,
    has_divider: bool,
    on_open_change: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl DialogHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            start_content: None,
            end_content: None,
            has_divider: false,
            on_open_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn start_content(mut self, content: impl IntoElement) -> Self {
        self.start_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn startContent(self, content: impl IntoElement) -> Self {
        self.start_content(content)
    }

    pub fn end_content(mut self, content: impl IntoElement) -> Self {
        self.end_content = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl IntoElement) -> Self {
        self.end_content(content)
    }

    pub fn has_divider(mut self, has_divider: bool) -> Self {
        self.has_divider = has_divider;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasDivider(self, has_divider: bool) -> Self {
        self.has_divider(has_divider)
    }

    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onOpenChange(self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change(handler)
    }
}

impl Styled for DialogHeader {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for DialogHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let on_open_change = self.on_open_change;

        div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(12.0))
            .px(px(24.0))
            .py(px(16.0))
            .when(self.has_divider, |this| {
                this.border_b_1().border_color(theme.tokens.border)
            })
            .when_some(self.start_content, |this, content| this.child(content))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .min_w(px(0.0))
                    .mt(px(1.0))
                    .child(
                        div()
                            .text_size(px(18.0))
                            .line_height(px(22.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.tokens.foreground)
                            .text_ellipsis()
                            .child(self.title),
                    )
                    .when_some(self.subtitle, |this, subtitle| {
                        this.child(
                            div()
                                .text_size(px(14.0))
                                .line_height(px(20.0))
                                .text_color(theme.tokens.muted_foreground)
                                .text_ellipsis()
                                .child(subtitle),
                        )
                    }),
            )
            .when_some(self.end_content, |this, content| this.child(content))
            .when_some(on_open_change, |this, handler| {
                this.child(
                    Button::new("dialog-header-close", "Close")
                        .variant(ButtonVariant::Ghost)
                        .size(ButtonSize::Icon)
                        .icon("x")
                        .tooltip("Close")
                        .on_click(move |_, window, cx| handler(false, window, cx)),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

pub struct Dialog {
    focus_handle: FocusHandle,
    header: Option<AnyElement>,
    title: Option<SharedString>,
    description: Option<SharedString>,
    size: DialogSize,
    variant: DialogVariant,
    purpose: DialogPurpose,
    position: Option<DialogPosition>,
    children: Vec<AnyElement>,
    footer: Option<AnyElement>,
    show_close_button: bool,
    close_on_backdrop_click: bool,
    close_on_escape: bool,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    focused: bool,
    dismissing: bool,
    dismiss_complete: bool,
    style: StyleRefinement,
}

impl Dialog {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            header: None,
            title: None,
            description: None,
            size: DialogSize::Md,
            variant: DialogVariant::Standard,
            purpose: DialogPurpose::Info,
            position: None,
            children: vec![],
            footer: None,
            show_close_button: true,
            close_on_backdrop_click: true,
            close_on_escape: true,
            on_close: None,
            focused: false,
            dismissing: false,
            dismiss_complete: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
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

    pub fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    pub fn variant(mut self, variant: DialogVariant) -> Self {
        self.variant = variant;
        if variant == DialogVariant::Fullscreen {
            self.size = DialogSize::Full;
        }
        self
    }

    pub fn purpose(mut self, purpose: DialogPurpose) -> Self {
        self.purpose = purpose;
        match purpose {
            DialogPurpose::Required => {
                self.close_on_backdrop_click = false;
                self.close_on_escape = false;
                self.show_close_button = false;
            }
            DialogPurpose::Form => {
                self.close_on_backdrop_click = false;
                self.close_on_escape = true;
            }
            DialogPurpose::Info => {}
        }
        self
    }

    pub fn position(mut self, position: DialogPosition) -> Self {
        self.position = Some(position);
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children<I>(mut self, children: impl IntoIterator<Item = I>) -> Self
    where
        I: IntoElement,
    {
        for child in children {
            self.children.push(child.into_any_element());
        }
        self
    }

    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    pub fn show_close_button(mut self, show: bool) -> Self {
        self.show_close_button = show;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasCloseButton(self, show: bool) -> Self {
        self.show_close_button(show)
    }

    pub fn close_on_backdrop_click(mut self, close: bool) -> Self {
        self.close_on_backdrop_click = close;
        self
    }

    #[allow(non_snake_case)]
    pub fn closeOnBackdropClick(self, close: bool) -> Self {
        self.close_on_backdrop_click(close)
    }

    pub fn close_on_escape(mut self, close: bool) -> Self {
        self.close_on_escape = close;
        self
    }

    #[allow(non_snake_case)]
    pub fn closeOnEscape(self, close: bool) -> Self {
        self.close_on_escape(close)
    }

    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    pub fn on_open_change(self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_close(move |window, cx| handler(false, window, cx))
    }

    #[allow(non_snake_case)]
    pub fn onOpenChange(self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change(handler)
    }

    fn handle_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dismissing {
            return;
        }
        self.dismissing = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            smol::Timer::after(Duration::from_millis(200)).await;
            let _ = this.update(cx, |dialog, cx| {
                dialog.dismiss_complete = true;
                cx.notify();
            });
        })
        .detach();
    }
}

impl Styled for Dialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for Dialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dismiss_complete {
            if let Some(handler) = &self.on_close {
                (handler)(window, cx);
            }
            return div().into_any_element();
        }

        let theme = Theme::of(cx);
        let has_slot_header = self.header.is_some();
        let has_header = has_slot_header
            || self.title.is_some()
            || self.description.is_some()
            || self.show_close_button;

        let dialog_entity = cx.entity().clone();
        let user_style = self.style.clone();
        let dismissing = self.dismissing;
        let position = self.position;

        if !self.focused {
            window.focus(&self.focus_handle);
            self.focused = true;
        }

        div()
            .id("dialog-overlay")
            .absolute()
            .inset_0()
            .flex()
            .when(position.is_none(), |this| {
                this.items_center().justify_center()
            })
            .bg(kael::black().opacity(0.5))
            .child(
                div()
                    .id("dialog-content")
                    .occlude()
                    .key_context("Dialog")
                    .track_focus(&self.focus_handle)
                    .when(self.close_on_backdrop_click, |this| {
                        this.on_mouse_down_out(cx.listener(|this, _, window, cx| {
                            this.handle_close(window, cx);
                        }))
                    })
                    .on_action(cx.listener(|this, _: &DialogCancel, window, cx| {
                        if this.close_on_escape {
                            this.handle_close(window, cx);
                        }
                    }))
                    .w(self.size.width())
                    .max_h(self.size.max_height())
                    .when_some(position, |this, position| {
                        this.absolute()
                            .when_some(position.top, |this, top| this.top(top))
                            .when_some(position.right, |this, right| this.right(right))
                            .when_some(position.bottom, |this, bottom| this.bottom(bottom))
                            .when_some(position.left, |this, left| this.left(left))
                    })
                    .flex()
                    .flex_col()
                    .bg(theme.tokens.popover)
                    .rounded(theme.tokens.radius_lg)
                    .shadow(theme.tokens.shadow_lg.to_vec())
                    .overflow_hidden()
                    .when(has_header, |this| {
                        if has_slot_header {
                            let header = self.header.take().unwrap();
                            this.child(header)
                        } else {
                            this.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.0))
                                    .px(px(24.0))
                                    .pt(px(24.0))
                                    .pb(px(16.0))
                                    .when(
                                        self.footer.is_none() && self.children.is_empty(),
                                        |this| this.pb(px(24.0)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_start()
                                            .justify_between()
                                            .gap(px(16.0))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.0))
                                                    .flex_1()
                                                    .when_some(self.title.clone(), |this, title| {
                                                        this.child(
                                                            div()
                                                                .text_size(px(18.0))
                                                                .font_family(
                                                                    theme
                                                                        .tokens
                                                                        .font_family
                                                                        .clone(),
                                                                )
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .text_color(theme.tokens.foreground)
                                                                .line_height(px(22.0))
                                                                .child(title),
                                                        )
                                                    })
                                                    .when_some(
                                                        self.description.clone(),
                                                        |this, desc| {
                                                            this.child(
                                                                div()
                                                                    .text_size(px(14.0))
                                                                    .font_family(
                                                                        theme
                                                                            .tokens
                                                                            .font_family
                                                                            .clone(),
                                                                    )
                                                                    .text_color(
                                                                        theme
                                                                            .tokens
                                                                            .muted_foreground,
                                                                    )
                                                                    .line_height(px(20.0))
                                                                    .child(desc),
                                                            )
                                                        },
                                                    ),
                                            )
                                            .when(self.show_close_button, |this| {
                                                let dialog_entity = dialog_entity.clone();
                                                this.child(
                                                    Button::new("dialog-close-btn", "Close")
                                                        .variant(ButtonVariant::Ghost)
                                                        .size(ButtonSize::Icon)
                                                        .icon("x")
                                                        .tooltip("Close")
                                                        .on_click(move |_, window, cx| {
                                                            cx.update_entity(
                                                                &dialog_entity,
                                                                |dialog, cx| {
                                                                    dialog.handle_close(window, cx);
                                                                },
                                                            );
                                                        }),
                                                )
                                            }),
                                    ),
                            )
                        }
                    })
                    .when(!self.children.is_empty(), |this| {
                        let children = std::mem::take(&mut self.children);
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .px(px(24.0))
                                .py(px(16.0))
                                .flex_1()
                                .children(children),
                        )
                    })
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div
                    })
                    .when_some(self.footer.take(), |this, footer| {
                        this.child(
                            div()
                                .flex()
                                .items_center()
                                .justify_end()
                                .gap(px(8.0))
                                .px(px(24.0))
                                .py(px(16.0))
                                .border_t_1()
                                .border_color(theme.tokens.border)
                                .child(footer),
                        )
                    })
                    .with_animation(
                        if dismissing {
                            "dialog-content-exit"
                        } else {
                            "dialog-content-enter"
                        },
                        Animation::new(Duration::from_millis(if dismissing { 200 } else { 250 }))
                            .with_easing(if dismissing {
                                easings::ease_in_cubic as fn(f32) -> f32
                            } else {
                                easings::ease_out_cubic as fn(f32) -> f32
                            }),
                        move |el, delta| {
                            if dismissing {
                                el.opacity(1.0 - delta).scale(1.0 - 0.04 * delta)
                            } else {
                                el.opacity(delta).scale(0.96 + 0.04 * delta)
                            }
                        },
                    ),
            )
            .with_animation(
                if dismissing {
                    "dialog-backdrop-exit"
                } else {
                    "dialog-backdrop-fade"
                },
                Animation::new(Duration::from_millis(200)).with_easing(if dismissing {
                    easings::ease_in_cubic as fn(f32) -> f32
                } else {
                    easings::ease_out_cubic as fn(f32) -> f32
                }),
                move |el, delta| {
                    if dismissing {
                        el.opacity(1.0 - delta)
                    } else {
                        el.opacity(delta)
                    }
                },
            )
            .into_any_element()
    }
}

impl Focusable for Dialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<()> for Dialog {}

pub fn init_dialog(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", DialogCancel, Some("Dialog"))]);
}
