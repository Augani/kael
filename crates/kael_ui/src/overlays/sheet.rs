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
            .track_focus(&self.focus_handle)
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
