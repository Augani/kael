//! Bottom sheet component for slide-up panels.

use kael::{prelude::FluentBuilder as _, *};
use std::{panic::Location, rc::Rc};

use crate::animations::presets;
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::text::{Text, TextVariant};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BottomSheetSize {
    Sm,
    #[default]
    Md,
    Lg,
    Xl,
    Custom,
}

impl BottomSheetSize {
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

    fn height(&self) -> Pixels {
        match self {
            Self::Sm => px(300.0),
            Self::Md => px(400.0),
            Self::Lg => px(500.0),
            Self::Xl => px(600.0),
            Self::Custom => px(400.0),
        }
    }
}

#[derive(IntoElement)]
pub struct BottomSheet {
    id: ElementId,
    size: BottomSheetSize,
    custom_height: Option<Pixels>,
    title: Option<SharedString>,
    description: Option<SharedString>,
    content: Option<AnyElement>,
    actions: Option<AnyElement>,
    show_drag_handle: bool,
    show_close_button: bool,
    close_on_backdrop_click: bool,
    close_on_escape: bool,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl BottomSheet {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "bottom-sheet:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            size: BottomSheetSize::default(),
            custom_height: None,
            title: None,
            description: None,
            content: None,
            actions: None,
            show_drag_handle: true,
            show_close_button: true,
            close_on_backdrop_click: true,
            close_on_escape: true,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn size(mut self, size: BottomSheetSize) -> Self {
        self.size = size;
        self
    }

    pub fn height(mut self, height: impl Into<Pixels>) -> Self {
        let height = height.into();
        let value = f32::from(height);
        if value.is_finite() && value > 0.0 {
            self.custom_height = Some(height);
            self.size = BottomSheetSize::Custom;
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

    pub fn actions(mut self, actions: impl IntoElement) -> Self {
        self.actions = Some(actions.into_any_element());
        self
    }

    pub fn show_drag_handle(mut self, show: bool) -> Self {
        self.show_drag_handle = show;
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

    pub fn close_on_escape(mut self, close: bool) -> Self {
        self.close_on_escape = close;
        self
    }

    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }

    /// Stable size key for content-safe diagnostics.
    pub fn size_key(&self) -> &'static str {
        self.size.to_text()
    }

    /// Returns true when a custom height is configured.
    pub fn has_custom_height(&self) -> bool {
        self.custom_height.is_some()
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
        self.content.is_some()
    }

    /// Returns true when action content is configured.
    pub fn has_actions(&self) -> bool {
        self.actions.is_some()
    }

    /// Returns true when a close handler is configured.
    pub fn has_close_handler(&self) -> bool {
        self.on_close.is_some()
    }

    /// Stable dismissal policy key.
    pub fn dismissal_mode(&self) -> &'static str {
        match (self.close_on_backdrop_click, self.close_on_escape) {
            (true, true) => "backdrop_escape",
            (true, false) => "backdrop",
            (false, true) => "escape",
            (false, false) => "manual",
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "bottom_sheet(size={}, custom_height={}, has_title={}, has_description={}, has_content={}, actions={}, drag_handle={}, close_button={}, dismissal={}, close_handler={})",
            self.size_key(),
            self.has_custom_height(),
            self.has_title(),
            self.has_description(),
            self.has_content(),
            self.has_actions(),
            self.show_drag_handle,
            self.show_close_button,
            self.dismissal_mode(),
            self.has_close_handler()
        )
    }

    fn get_sheet_height(&self) -> Pixels {
        if let Some(height) = self.custom_height {
            return height;
        }
        self.size.height()
    }
}

impl Default for BottomSheet {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for BottomSheet {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for BottomSheet {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let has_header = self.title.is_some()
            || self.description.is_some()
            || self.actions.is_some()
            || (self.show_close_button && self.on_close.is_some());
        let sheet_height = self.get_sheet_height();
        let on_close = self.on_close.clone();
        let user_style = self.style;
        let sheet_id = self.id.clone();
        let accessibility_label = self
            .title
            .clone()
            .unwrap_or_else(|| "Bottom sheet".into())
            .to_string();
        let mut sheet_accessibility =
            AccessibilityAttributes::new(AccessibilityRole::Dialog).label(accessibility_label);
        if let Some(description) = self.description.as_ref() {
            sheet_accessibility = sheet_accessibility.description(description.to_string());
        }
        let focus_handle = window
            .use_keyed_state(sheet_id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        if !focus_handle.contains_focused(window, cx) {
            window.focus(&focus_handle);
        }

        deferred(
            div()
                .id(ElementId::NamedChild(
                    Box::new(sheet_id.clone()),
                    "overlay".into(),
                ))
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .bg(hsla(0.0, 0.0, 0.0, 0.6))
                .when(self.close_on_backdrop_click, |this| {
                    let on_close = on_close.clone();
                    this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        if let Some(handler) = on_close.as_ref() {
                            handler(window, cx);
                        }
                    })
                })
                .child(
                    div()
                        .id(sheet_id.clone())
                        .accessibility(sheet_accessibility)
                        .track_focus(&focus_handle.tab_index(0).tab_stop(true))
                        .when(self.close_on_escape && on_close.is_some(), |panel| {
                            let on_close = on_close.clone();
                            panel.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key.as_str() == "escape" {
                                    if let Some(handler) = on_close.as_ref() {
                                        handler(window, cx);
                                    }
                                    cx.stop_propagation();
                                }
                            })
                        })
                        .occlude()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(sheet_height)
                        .max_h(relative(0.92))
                        .flex()
                        .flex_col()
                        .bg(theme.tokens.card)
                        .border_t_1()
                        .border_color(theme.tokens.border)
                        .rounded_tl(theme.tokens.radius_xl)
                        .rounded_tr(theme.tokens.radius_xl)
                        .shadow(theme.tokens.shadow_lg.to_vec())
                        .map(|this| {
                            let mut div = this;
                            div.style().refine(&user_style);
                            div
                        })
                        .when(self.show_drag_handle, |this| {
                            this.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .pt(px(12.0))
                                    .pb(px(8.0))
                                    .child(
                                        div()
                                            .w(px(40.0))
                                            .h(px(4.0))
                                            .bg(theme.tokens.muted.opacity(0.5))
                                            .rounded(px(2.0)),
                                    ),
                            )
                        })
                        .when(has_header, |this| {
                            this.child(
                                div()
                                    .flex()
                                    .items_start()
                                    .justify_between()
                                    .px(px(24.0))
                                    .pt(px(16.0))
                                    .pb(px(16.0))
                                    .border_b_1()
                                    .border_color(theme.tokens.border)
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.0))
                                            .when_some(self.title, |this: Div, title| {
                                                this.child(
                                                    Text::new(title)
                                                        .variant(TextVariant::H4)
                                                        .accessibility_hidden(true),
                                                )
                                            })
                                            .when_some(self.description, |this: Div, desc| {
                                                this.child(
                                                    Text::new(desc)
                                                        .variant(TextVariant::Caption)
                                                        .accessibility_hidden(true)
                                                        .color(theme.tokens.muted_foreground),
                                                )
                                            }),
                                    )
                                    .when_some(self.actions, |this: Div, actions| {
                                        this.child(
                                            div().flex().items_center().gap(px(8.0)).child(actions),
                                        )
                                    })
                                    .when(self.show_close_button && on_close.is_some(), |this| {
                                        let on_close = on_close.clone();
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
                                            .icon("x")
                                            .tooltip("Close bottom sheet")
                                            .on_click(
                                                move |_, window, cx| {
                                                    if let Some(handler) = on_close.as_ref() {
                                                        handler(window, cx);
                                                    }
                                                },
                                            ),
                                        )
                                    }),
                            )
                        })
                        .when_some(self.content, |this, content| {
                            this.child(
                                div()
                                    .id(ElementId::NamedChild(
                                        Box::new(sheet_id.clone()),
                                        "content".into(),
                                    ))
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .overflow_y_scroll()
                                    .child(content),
                            )
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                        })
                        .with_animation(
                            "bottom-sheet-slide",
                            presets::slide_in_bottom(),
                            |div, delta| div.mb(px(-600.0 * (1.0 - delta))),
                        ),
                ),
        )
        .with_priority(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kael::{div, px};

    #[::core::prelude::v1::test]
    fn bottom_sheet_summary_is_content_safe() {
        let sheet = BottomSheet::new()
            .height(px(560.0))
            .title("Private Mobile Panel")
            .description("Secret account options")
            .content(div().child("private bottom sheet content"))
            .actions(div().child("private action"))
            .show_drag_handle(false)
            .close_on_backdrop_click(false)
            .on_close(|_, _| {});

        assert_eq!(BottomSheetSize::Custom.to_text(), "custom");
        assert_eq!(sheet.size_key(), "custom");
        assert!(sheet.has_custom_height());
        assert!(sheet.has_title());
        assert!(sheet.has_description());
        assert!(sheet.has_content());
        assert!(sheet.has_actions());
        assert!(!sheet.show_drag_handle);
        assert_eq!(sheet.dismissal_mode(), "escape");
        assert!(sheet.has_close_handler());

        let summary = sheet.to_text();
        assert!(summary.contains("size=custom"));
        assert!(summary.contains("drag_handle=false"));
        assert!(summary.contains("dismissal=escape"));
        assert!(!summary.contains("Private Mobile Panel"));
        assert!(!summary.contains("Secret account"));
        assert!(!summary.contains("private bottom"));
        assert!(!summary.contains("private action"));
        assert!(!summary.contains("560"));
    }

    #[::core::prelude::v1::test]
    fn invalid_custom_heights_keep_the_default_size() {
        for height in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            let sheet = BottomSheet::new().height(px(height));
            assert_eq!(sheet.size, BottomSheetSize::Md);
            assert_eq!(sheet.get_sheet_height(), BottomSheetSize::Md.height());
            assert!(!sheet.has_custom_height());
        }
    }
}
