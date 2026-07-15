//! Alert dialog component for confirmations.

use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::theme::Theme;

fn sanitize_dialog_width(width: Pixels) -> Pixels {
    let width = f32::from(width);
    if width.is_finite() && width > 0.0 {
        px(width.clamp(280.0, 960.0))
    } else {
        px(400.0)
    }
}

actions!(alert_dialog, [AlertDialogCancel]);

pub struct AlertDialog {
    id: ElementId,
    focus_handle: FocusHandle,
    title: SharedString,
    description: SharedString,
    cancel_text: SharedString,
    action_text: SharedString,
    action_variant: ButtonVariant,
    action_loading: bool,
    width: Pixels,
    on_cancel: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    on_action: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    previous_focus_handle: Option<FocusHandle>,
    focus_initialized: bool,
    style: StyleRefinement,
}

impl AlertDialog {
    #[track_caller]
    pub fn new(cx: &mut Context<Self>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "alert-dialog:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            focus_handle: cx.focus_handle(),
            title: "Are you sure?".into(),
            description: "This action cannot be undone.".into(),
            cancel_text: "Cancel".into(),
            action_text: "Continue".into(),
            action_variant: ButtonVariant::Default,
            action_loading: false,
            width: px(400.0),
            on_cancel: None,
            on_action: None,
            previous_focus_handle: None,
            focus_initialized: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = description.into();
        self
    }

    pub fn cancel_text(mut self, text: impl Into<SharedString>) -> Self {
        self.cancel_text = text.into();
        self
    }

    #[allow(non_snake_case)]
    pub fn cancelLabel(self, text: impl Into<SharedString>) -> Self {
        self.cancel_text(text)
    }

    pub fn action_text(mut self, text: impl Into<SharedString>) -> Self {
        self.action_text = text.into();
        self
    }

    #[allow(non_snake_case)]
    pub fn actionLabel(self, text: impl Into<SharedString>) -> Self {
        self.action_text(text)
    }

    pub fn action_variant(mut self, variant: ButtonVariant) -> Self {
        self.action_variant = variant;
        self
    }

    #[allow(non_snake_case)]
    pub fn actionVariant(self, variant: ButtonVariant) -> Self {
        self.action_variant(variant)
    }

    pub fn action_loading(mut self, loading: bool) -> Self {
        self.action_loading = loading;
        self
    }

    #[allow(non_snake_case)]
    pub fn isActionLoading(self, loading: bool) -> Self {
        self.action_loading(loading)
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = sanitize_dialog_width(width);
        self
    }

    pub fn destructive(mut self, destructive: bool) -> Self {
        self.action_variant = if destructive {
            ButtonVariant::Destructive
        } else {
            ButtonVariant::Default
        };
        self
    }

    pub fn on_cancel<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_cancel = Some(Rc::new(handler));
        self
    }

    pub fn on_action<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_action = Some(Rc::new(handler));
        self
    }

    fn handle_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_previous_focus(window);
        if let Some(handler) = &self.on_cancel {
            handler(window, cx);
        }
    }

    fn handle_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_previous_focus(window);
        if let Some(handler) = &self.on_action {
            handler(window, cx);
        }
    }

    fn restore_previous_focus(&mut self, window: &mut Window) {
        if let Some(previous_focus_handle) = self.previous_focus_handle.take() {
            window.focus(&previous_focus_handle);
        }
        self.focus_initialized = false;
    }

    fn handle_escape(
        &mut self,
        _: &AlertDialogCancel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_cancel(window, cx);
    }
}

pub fn init_alert_dialog(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        AlertDialogCancel,
        Some("AlertDialog"),
    )]);
}

impl Styled for AlertDialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Render for AlertDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style.clone();
        let title = self.title.clone();
        let description = self.description.clone();
        let cancel_text = self.cancel_text.clone();
        let action_text = self.action_text.clone();
        let action_variant = self.action_variant;
        let action_loading = self.action_loading;
        let width = self.width;
        let dialog_id = self.id.clone();
        let dialog_accessibility = AccessibilityAttributes::new(AccessibilityRole::Dialog)
            .label(title.to_string())
            .description(description.to_string());

        if !self.focus_initialized {
            self.previous_focus_handle = window.focused(cx);
            self.focus_initialized = true;
        }
        if !self.focus_handle.contains_focused(window, cx) {
            window.focus(&self.focus_handle);
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(dialog_id.clone()),
                "overlay".into(),
            ))
            .key_context("AlertDialog")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::handle_escape))
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0.0, 0.0, 0.0, 0.5))
            .child(
                div()
                    .id(dialog_id.clone())
                    .accessibility(dialog_accessibility)
                    .key_context("AlertDialog")
                    .w(width)
                    .max_w(relative(0.9))
                    .bg(theme.tokens.popover)
                    .rounded(theme.tokens.radius_lg)
                    .shadow(theme.tokens.shadow_lg.to_vec())
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.0))
                            .p(px(24.0))
                            .child(
                                div()
                                    .text_size(px(18.0))
                                    .line_height(px(22.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.tokens.foreground)
                                    .child(StyledText::new(title).accessibility_hidden(true)),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .line_height(px(20.0))
                                    .child(StyledText::new(description).accessibility_hidden(true)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.0))
                                    .justify_end()
                                    .items_center()
                                    .child(
                                        Button::new(
                                            ElementId::NamedChild(
                                                Box::new(dialog_id.clone()),
                                                "cancel".into(),
                                            ),
                                            cancel_text,
                                        )
                                        .variant(ButtonVariant::Ghost)
                                        .size(ButtonSize::Md)
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.handle_cancel(window, cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        Button::new(
                                            ElementId::NamedChild(
                                                Box::new(dialog_id.clone()),
                                                "action".into(),
                                            ),
                                            action_text,
                                        )
                                        .variant(action_variant)
                                        .size(ButtonSize::Md)
                                        .loading(action_loading)
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.handle_action(window, cx);
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_and_extreme_widths_are_bounded() {
        assert_eq!(sanitize_dialog_width(px(f32::NAN)), px(400.0));
        assert_eq!(sanitize_dialog_width(px(-1.0)), px(400.0));
        assert_eq!(sanitize_dialog_width(px(10.0)), px(280.0));
        assert_eq!(sanitize_dialog_width(px(2_000.0)), px(960.0));
    }
}
