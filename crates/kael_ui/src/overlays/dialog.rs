//! Dialog component with focus trap and backdrop.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;
use std::{panic::Location, rc::Rc};

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
        self.top = finite_edge(top);
        self
    }

    pub fn right(mut self, right: Pixels) -> Self {
        self.right = finite_edge(right);
        self
    }

    pub fn bottom(mut self, bottom: Pixels) -> Self {
        self.bottom = finite_edge(bottom);
        self
    }

    pub fn left(mut self, left: Pixels) -> Self {
        self.left = finite_edge(left);
        self
    }

    /// Number of positioned edges supplied by the caller.
    pub fn edge_count(&self) -> usize {
        usize::from(self.top.is_some())
            + usize::from(self.right.is_some())
            + usize::from(self.bottom.is_some())
            + usize::from(self.left.is_some())
    }

    /// Returns true when any edge is configured.
    pub fn has_edges(&self) -> bool {
        self.edge_count() > 0
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "dialog_position(edges={}, top={}, right={}, bottom={}, left={})",
            self.edge_count(),
            self.top.is_some(),
            self.right.is_some(),
            self.bottom.is_some(),
            self.left.is_some()
        )
    }
}

fn finite_edge(edge: Pixels) -> Option<Pixels> {
    let edge = f32::from(edge);
    edge.is_finite().then(|| px(edge.max(0.0)))
}

impl DialogSize {
    /// Stable size key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Sm => "sm",
            Self::Md => "md",
            Self::Lg => "lg",
            Self::Xl => "xl",
            Self::Full => "full",
        }
    }

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

impl DialogVariant {
    /// Stable variant key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fullscreen => "fullscreen",
        }
    }
}

impl DialogPurpose {
    /// Stable purpose key for content-safe diagnostics.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Form => "form",
            Self::Info => "info",
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

    /// Returns the title length without exposing title text.
    pub fn title_len_bytes(&self) -> usize {
        self.title.len()
    }

    /// Returns true when a subtitle is configured.
    pub fn has_subtitle(&self) -> bool {
        self.subtitle.is_some()
    }

    /// Returns true when start content is configured.
    pub fn has_start_content(&self) -> bool {
        self.start_content.is_some()
    }

    /// Returns true when end content is configured.
    pub fn has_end_content(&self) -> bool {
        self.end_content.is_some()
    }

    /// Returns true when a close/open-change handler is configured.
    pub fn has_open_change_handler(&self) -> bool {
        self.on_open_change.is_some()
    }

    /// Returns true when the header divider is enabled.
    pub fn has_divider_enabled(&self) -> bool {
        self.has_divider
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "dialog_header(title_len_bytes={}, has_subtitle={}, start_content={}, end_content={}, divider={}, open_change_handler={})",
            self.title_len_bytes(),
            self.has_subtitle(),
            self.has_start_content(),
            self.has_end_content(),
            self.has_divider_enabled(),
            self.has_open_change_handler()
        )
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
    id: ElementId,
    focus_handle: FocusHandle,
    header: Option<AnyElement>,
    title: Option<SharedString>,
    description: Option<SharedString>,
    size: DialogSize,
    variant: DialogVariant,
    purpose: DialogPurpose,
    position: Option<DialogPosition>,
    children: Vec<AnyElement>,
    content_builder: Option<Rc<dyn Fn() -> AnyElement>>,
    footer: Option<AnyElement>,
    footer_builder: Option<Rc<dyn Fn() -> AnyElement>>,
    show_close_button: bool,
    close_on_backdrop_click: bool,
    close_on_escape: bool,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    previous_focus_handle: Option<FocusHandle>,
    focused: bool,
    dismissing: bool,
    dismiss_complete: bool,
    style: StyleRefinement,
}

impl Dialog {
    #[track_caller]
    pub fn new(cx: &mut Context<Self>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "dialog:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            focus_handle: cx.focus_handle(),
            header: None,
            title: None,
            description: None,
            size: DialogSize::Md,
            variant: DialogVariant::Standard,
            purpose: DialogPurpose::Info,
            position: None,
            children: vec![],
            content_builder: None,
            footer: None,
            footer_builder: None,
            show_close_button: true,
            close_on_backdrop_click: true,
            close_on_escape: true,
            on_close: None,
            previous_focus_handle: None,
            focused: false,
            dismissing: false,
            dismiss_complete: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
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

    /// Build persistent content again for every render of a reusable dialog.
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

    /// Build a persistent footer again for every render of a reusable dialog.
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

    /// Stable size key for content-safe diagnostics.
    pub fn size_key(&self) -> &'static str {
        self.size.to_text()
    }

    /// Stable variant key for content-safe diagnostics.
    pub fn variant_key(&self) -> &'static str {
        self.variant.to_text()
    }

    /// Stable purpose key for content-safe diagnostics.
    pub fn purpose_key(&self) -> &'static str {
        self.purpose.to_text()
    }

    /// Returns true when a custom header element is configured.
    pub fn has_header_slot(&self) -> bool {
        self.header.is_some()
    }

    /// Returns true when title text is configured.
    pub fn has_title(&self) -> bool {
        self.title.is_some()
    }

    /// Returns true when description text is configured.
    pub fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// Returns true when a custom position is configured.
    pub fn has_position(&self) -> bool {
        self.position.is_some()
    }

    /// Returns the configured position edge count.
    pub fn position_edge_count(&self) -> usize {
        self.position.map_or(0, |position| position.edge_count())
    }

    /// Returns the number of child content elements.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns true when body content is configured through either API.
    pub fn has_content(&self) -> bool {
        !self.children.is_empty() || self.content_builder.is_some()
    }

    /// Returns true when a footer element is configured.
    pub fn has_footer(&self) -> bool {
        self.footer.is_some() || self.footer_builder.is_some()
    }

    /// Returns true when a close handler is configured.
    pub fn has_close_handler(&self) -> bool {
        self.on_close.is_some()
    }

    /// Stable dismissal policy key.
    pub fn dismissal_mode(&self) -> &'static str {
        match (self.close_on_backdrop_click, self.close_on_escape) {
            (false, false) => "none",
            (true, false) => "backdrop",
            (false, true) => "escape",
            (true, true) => "backdrop_escape",
        }
    }

    /// Content-safe summary for logs, tests, and AI-agent diagnostics.
    pub fn to_text(&self) -> String {
        format!(
            "dialog(size={}, variant={}, purpose={}, header_slot={}, has_title={}, has_description={}, content={}, children={}, footer={}, close_button={}, dismissal={}, close_handler={}, positioned={}, position_edges={}, focused={}, dismissing={}, dismiss_complete={})",
            self.size_key(),
            self.variant_key(),
            self.purpose_key(),
            self.has_header_slot(),
            self.has_title(),
            self.has_description(),
            self.has_content(),
            self.child_count(),
            self.has_footer(),
            self.show_close_button,
            self.dismissal_mode(),
            self.has_close_handler(),
            self.has_position(),
            self.position_edge_count(),
            self.focused,
            self.dismissing,
            self.dismiss_complete
        )
    }

    fn handle_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dismissing {
            return;
        }
        self.dismissing = true;
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            Timer::after(Duration::from_millis(200)).await;
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
            let handler = self.on_close.clone();
            if let Some(previous_focus_handle) = self.previous_focus_handle.take() {
                window.focus(&previous_focus_handle);
            }
            self.dismissing = false;
            self.dismiss_complete = false;
            self.focused = false;
            if let Some(handler) = handler {
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
        let dialog_id = self.id.clone();
        let accessibility_label = self
            .title
            .clone()
            .unwrap_or_else(|| "Dialog".into())
            .to_string();
        let mut dialog_accessibility =
            AccessibilityAttributes::new(AccessibilityRole::Dialog).label(accessibility_label);
        if let Some(description) = self.description.as_ref() {
            dialog_accessibility = dialog_accessibility.description(description.to_string());
        }
        let content = self
            .content_builder
            .as_ref()
            .map(|builder| builder())
            .or_else(|| {
                (!self.children.is_empty()).then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(16.0))
                        .children(std::mem::take(&mut self.children))
                        .into_any_element()
                })
            });
        let footer = self
            .footer_builder
            .as_ref()
            .map(|builder| builder())
            .or_else(|| self.footer.take());

        if !self.focused {
            self.previous_focus_handle = window.focused(cx);
            window.focus(&self.focus_handle);
            self.focused = true;
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(dialog_id.clone()),
                "overlay".into(),
            ))
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .when(position.is_none(), |this| {
                this.items_center().justify_center()
            })
            .bg(kael::black().opacity(0.5))
            .child(
                div()
                    .id(dialog_id.clone())
                    .accessibility(dialog_accessibility)
                    .occlude()
                    .key_context("Dialog")
                    .track_focus(&self.focus_handle)
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
                                    .when(footer.is_none() && content.is_none(), |this| {
                                        this.pb(px(24.0))
                                    })
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
                                                    .min_w(px(0.0))
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
                                                                .child(
                                                                    StyledText::new(title)
                                                                        .accessibility_hidden(true),
                                                                ),
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
                                                                    .whitespace_normal()
                                                                    .line_height(px(20.0))
                                                                    .child(
                                                                        StyledText::new(desc)
                                                                            .accessibility_hidden(
                                                                                true,
                                                                            ),
                                                                    ),
                                                            )
                                                        },
                                                    ),
                                            )
                                            .when(self.show_close_button, |this| {
                                                let dialog_entity = dialog_entity.clone();
                                                this.child(
                                                    Button::new(
                                                        ElementId::NamedChild(
                                                            Box::new(dialog_id.clone()),
                                                            "close".into(),
                                                        ),
                                                        "Close",
                                                    )
                                                    .variant(ButtonVariant::Ghost)
                                                    .size(ButtonSize::Icon)
                                                    .flex_shrink_0()
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
                    .when_some(content, |this, content| {
                        this.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .px(px(24.0))
                                .py(px(16.0))
                                .flex_1()
                                .child(content),
                        )
                    })
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div
                    })
                    .when_some(footer, |this, footer| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use kael::{TestAppContext, div, px};

    #[::core::prelude::v1::test]
    fn dialog_position_and_header_summary_is_content_safe() {
        let position = DialogPosition::new().top(px(48.0)).left(px(96.0));
        assert_eq!(position.edge_count(), 2);
        assert!(position.has_edges());

        let position_summary = position.to_text();
        assert!(position_summary.contains("edges=2"));
        assert!(position_summary.contains("top=true"));
        assert!(position_summary.contains("left=true"));
        assert!(!position_summary.contains("48"));
        assert!(!position_summary.contains("96"));

        let header = DialogHeader::new("Private Account")
            .subtitle("Secret workspace settings")
            .start_content(div().child("private start"))
            .end_content(div().child("private end"))
            .has_divider(true)
            .on_open_change(|_, _, _| {});

        assert_eq!(header.title_len_bytes(), "Private Account".len());
        assert!(header.has_subtitle());
        assert!(header.has_start_content());
        assert!(header.has_end_content());
        assert!(header.has_divider_enabled());
        assert!(header.has_open_change_handler());

        let header_summary = header.to_text();
        assert!(header_summary.contains("has_subtitle=true"));
        assert!(header_summary.contains("divider=true"));
        assert!(!header_summary.contains("Private Account"));
        assert!(!header_summary.contains("Secret workspace"));
        assert!(!header_summary.contains("private start"));
    }

    #[::core::prelude::v1::test]
    fn dialog_summary_is_content_safe() {
        let cx = TestAppContext::single();
        let dialog = cx.update(|cx| {
            cx.new(|cx| {
                Dialog::new(cx)
                    .title("Delete Private Workspace")
                    .description("This removes secret project data")
                    .purpose(DialogPurpose::Required)
                    .position(DialogPosition::new().right(px(24.0)).bottom(px(32.0)))
                    .child(div().child("private child content"))
                    .footer(div().child("private footer"))
                    .on_close(|_, _| {})
            })
        });

        cx.update(|cx| {
            let dialog = dialog.read(cx);
            assert_eq!(dialog.size_key(), "md");
            assert_eq!(dialog.variant_key(), "standard");
            assert_eq!(dialog.purpose_key(), "required");
            assert!(dialog.has_title());
            assert!(dialog.has_description());
            assert_eq!(dialog.child_count(), 1);
            assert!(dialog.has_footer());
            assert!(!dialog.show_close_button);
            assert_eq!(dialog.dismissal_mode(), "none");
            assert!(dialog.has_close_handler());
            assert!(dialog.has_position());
            assert_eq!(dialog.position_edge_count(), 2);

            let summary = dialog.to_text();
            assert!(summary.contains("purpose=required"));
            assert!(summary.contains("dismissal=none"));
            assert!(summary.contains("children=1"));
            assert!(!summary.contains("Delete Private Workspace"));
            assert!(!summary.contains("secret project"));
            assert!(!summary.contains("private child"));
            assert!(!summary.contains("private footer"));
            assert!(!summary.contains("24"));
            assert!(!summary.contains("32"));
        });
    }

    #[::core::prelude::v1::test]
    fn invalid_position_edges_are_ignored_and_negative_edges_are_clamped() {
        let position = DialogPosition::new()
            .top(px(f32::NAN))
            .right(px(f32::INFINITY))
            .bottom(px(-12.0))
            .left(px(24.0));
        assert!(position.top.is_none());
        assert!(position.right.is_none());
        assert_eq!(position.bottom, Some(px(0.0)));
        assert_eq!(position.left, Some(px(24.0)));
        assert_eq!(position.edge_count(), 2);
    }

    #[::core::prelude::v1::test]
    fn persistent_builders_are_reported_as_content_and_footer() {
        let cx = TestAppContext::single();
        let dialog = cx.update(|cx| {
            cx.new(|cx| {
                Dialog::new(cx)
                    .content_builder(|| div().child("rebuilt body"))
                    .footer_builder(|| div().child("rebuilt footer"))
            })
        });

        cx.update(|cx| {
            let dialog = dialog.read(cx);
            assert!(dialog.has_content());
            assert!(dialog.has_footer());
            assert_eq!(dialog.child_count(), 0);
            assert!(dialog.to_text().contains("content=true"));
            assert!(!dialog.to_text().contains("rebuilt body"));
            assert!(!dialog.to_text().contains("rebuilt footer"));
        });
    }
}

// Real mounted Tab-routing baselines. The fixture does not install a focus trap.
#[cfg(test)]
mod dialog_modal_focus_mounted_baseline {
    use super::*;
    use crate::components::button::Button;
    use kael::{TestAppContext, VisualTestContext};
    use std::{cell::Cell, rc::Rc};

    struct Body {
        prefix: &'static str,
        middle_disabled: bool,
        omit_last: bool,
        nested: Option<Entity<Dialog>>,
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
        modal: Option<Entity<Dialog>>,
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
        _modal: Entity<Dialog>,
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
                Dialog::new(cx)
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
                Dialog::new(cx)
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
    fn dialog_tab_wraps_inside_modal_with_outside_controls() {
        let mut f = mounted();
        focus_button(&mut f.window, "Modal last");
        key(&mut f.window, "tab", "Modal first");
        key(&mut f.window, "tab", "Modal last");
    }

    #[::core::prelude::v1::test]
    fn dialog_shift_tab_wraps_inside_modal_with_outside_controls() {
        let mut f = mounted();
        focus_button(&mut f.window, "Modal first");
        key(&mut f.window, "shift-tab", "Modal last");
        key(&mut f.window, "shift-tab", "Modal first");
    }

    #[::core::prelude::v1::test]
    fn dialog_tab_uses_current_disabled_and_removed_children() {
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
    fn dialog_nested_modal_has_the_innermost_tab_scope() {
        let mut f = mounted();
        let _inner_closes = add_nested(&mut f);
        focus_button(&mut f.window, "Inner last");
        key(&mut f.window, "tab", "Inner first");
        key(&mut f.window, "shift-tab", "Inner last");
        assert_eq!(f._closes.get(), 0);
    }
}

#[cfg(test)]
mod empty_dialog_focus_mounted {
    use super::*;
    use crate::components::button::Button;
    use kael::{TestAppContext, VisualTestContext};

    struct EmptyHost {
        modal: Option<Entity<Dialog>>,
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
        modal: &Entity<Dialog>,
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
                        && node.label.as_deref() == Some("Empty Dialog")
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
    fn empty_dialog_tab_and_shift_tab_keep_one_exported_dialog_focus_owner() {
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
                Dialog::new(cx)
                    .id("empty-dialog-root")
                    .title("Empty Dialog")
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

// Investigate render-time autofocus after an independently retained opener is cached.
#[cfg(test)]
mod cached_opener_dialog_focus {
    use super::*;
    use crate::components::button::Button;
    use kael::{AnyView, StyleRefinement, TestAppContext};
    use std::{cell::Cell, rc::Rc};

    struct CachedOpener {
        renders: Rc<Cell<usize>>,
    }

    impl Render for CachedOpener {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get().checked_add(1).unwrap());
            div().size_full().child(
                Button::new("cached-modal-opener", "Cached modal opener").on_click(|_, _, _| {}),
            )
        }
    }

    struct Host {
        opener: Entity<CachedOpener>,
        modal: Option<Entity<Dialog>>,
        control_epoch: usize,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut style = StyleRefinement::default();
            style.size.width = Some(px(200.0).into());
            style.size.height = Some(px(40.0).into());
            div()
                .relative()
                .size_full()
                .child(AnyView::from(self.opener.clone()).cached(style))
                .children(self.modal.clone())
        }
    }

    #[::core::prelude::v1::test]
    fn cached_opener_replay_after_dialog_render_autofocus_has_current_semantic_focus() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let renders = Rc::new(Cell::new(0));
        let opener = cx.new({
            let renders = renders.clone();
            move |_| CachedOpener { renders }
        });
        let (host, borrowed) = cx.add_window_view({
            let opener = opener.clone();
            move |_, _| Host {
                opener,
                modal: None,
                control_epoch: 0,
            }
        });
        let mut window = borrowed.clone();
        window.simulate_resize(size(px(640.0), px(480.0)));
        window.deactivate_window();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        let opener_id = window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window.is_window_active());
            let matches = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Cached modal opener")
                })
                .map(|node| node.id)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1);
            let id = matches[0];
            assert!(window.has_accessibility_action_handler(id, AccessibilityAction::Focus));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Focus,
            ));
            id
        });
        window.run_until_parked();
        let opener_bounds = window.update(|window, cx| {
            // Native Focus refreshes caches. Establish the settled, focused
            // cache before either reuse assertion; this is setup, not a rescue
            // draw after the modal has mounted.
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert_eq!(tree.focused_node_count(), 1);
            assert_eq!(tree.focused_node(), Some(opener_id));
            let bounds = tree.get(opener_id).unwrap().bounds;
            assert!(bounds.is_some());
            bounds
        });
        let settled_renders = renders.get();
        assert!(settled_renders > 0);
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.control_epoch += 1;
                cx.notify();
            });
            window.draw(cx).clear();
            assert_eq!(
                renders.get(),
                settled_renders,
                "fixture eligibility: Host-only control must reuse the cached opener",
            );
            let tree = window.accessibility_tree();
            assert_eq!(tree.get(opener_id).unwrap().bounds, opener_bounds);
            assert_eq!(tree.focused_node_count(), 1);
            assert_eq!(tree.focused_node(), Some(opener_id));
        });
        let modal = window.update(|_, cx| {
            cx.new(|cx| {
                Dialog::new(cx)
                    .id("cached-opener-dialog")
                    .title("Cached opener Dialog")
                    .show_close_button(false)
                    .close_on_backdrop_click(false)
            })
        });
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.modal = Some(modal.clone());
                cx.notify();
            });
            // Exactly one mount draw. Dialog autofocus happens in its Render;
            // no direct focus call, defer, timer, trap or follow-up draw here.
            window.draw(cx).clear();
            assert_eq!(
                renders.get(),
                settled_renders,
                "fixture eligibility: mount must preserve actual cached-opener reuse",
            );
            assert!(modal.read(cx).focus_handle.is_focused(window));
            let tree = window.accessibility_tree();
            assert_eq!(tree.get(opener_id).unwrap().bounds, opener_bounds);
            let dialogs = tree
                .nodes
                .values()
                .filter(|node| {
                    node.role == AccessibilityRole::Dialog
                        && node.label.as_deref() == Some("Cached opener Dialog")
                })
                .map(|node| node.id)
                .collect::<Vec<_>>();
            assert_eq!(dialogs.len(), 1);
            let dialog = dialogs[0];
            assert_eq!(tree.focused_node_count(), 1, "one exported focus owner");
            assert_eq!(tree.focused_node(), Some(dialog));
            let export = tree.to_accesskit_tree_update(None, None);
            assert_eq!(export.focus.0, dialog.0);
            assert!(export.nodes.iter().any(|(id, _)| id.0 == dialog.0));
        });
        window.update(|window, _| window.remove_window());
        drop(modal);
        drop(host);
        drop(opener);
        drop(window);
        cx.quit();
    }
}

// Investigate render-time autofocus after an independently retained opener is cached.
#[cfg(test)]
mod cached_opener_paint_gap_policy {
    use super::*;
    use crate::components::button::Button;
    use kael::{AnyView, StyleRefinement, TestAppContext};
    use std::{cell::Cell, rc::Rc};

    struct CachedOpener {
        renders: Rc<Cell<usize>>,
    }

    impl Render for CachedOpener {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get().checked_add(1).unwrap());
            div().size_full().child(
                Button::new("cached-modal-opener", "Cached modal opener").on_click(|_, _, _| {}),
            )
        }
    }

    struct Host {
        opener: Entity<CachedOpener>,
        modal: Option<Entity<Dialog>>,
        control_epoch: usize,
        hidden: bool,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let mut style = StyleRefinement::default();
            style.size.width = Some(px(200.0).into());
            style.size.height = Some(px(40.0).into());
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("cached-opener-visibility")
                        .w(px(200.0))
                        .h(px(40.0))
                        .when(self.hidden, |this| this.invisible())
                        .child(AnyView::from(self.opener.clone()).cached(style)),
                )
                .children(self.modal.clone())
        }
    }

    #[::core::prelude::v1::test]
    fn cached_opener_rebuilds_after_hidden_paint_gap_then_reuses_for_dialog_autofocus() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let renders = Rc::new(Cell::new(0));
        let opener = cx.new({
            let renders = renders.clone();
            move |_| CachedOpener { renders }
        });
        let (host, borrowed) = cx.add_window_view({
            let opener = opener.clone();
            move |_, _| Host {
                opener,
                modal: None,
                control_epoch: 0,
                hidden: false,
            }
        });
        let mut window = borrowed.clone();
        window.simulate_resize(size(px(640.0), px(480.0)));
        window.deactivate_window();
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
        let opener_id = window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(window.is_window_active());
            let matches = window
                .accessibility_tree()
                .nodes
                .values()
                .filter(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Cached modal opener")
                })
                .map(|node| node.id)
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1);
            let id = matches[0];
            assert!(window.has_accessibility_action_handler(id, AccessibilityAction::Focus));
            window.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Focus,
            ));
            id
        });
        window.run_until_parked();
        let opener_bounds = window.update(|window, cx| {
            // Native Focus refreshes caches. Establish the settled, focused
            // cache before either reuse assertion; this is setup, not a rescue
            // draw after the modal has mounted.
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            assert_eq!(tree.focused_node_count(), 1);
            assert_eq!(tree.focused_node(), Some(opener_id));
            let bounds = tree.get(opener_id).unwrap().bounds;
            assert!(bounds.is_some());
            bounds
        });
        let settled_renders = renders.get();
        assert!(settled_renders > 0);
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.control_epoch += 1;
                cx.notify();
            });
            window.draw(cx).clear();
            assert_eq!(
                renders.get(),
                settled_renders,
                "fixture eligibility: Host-only control must reuse the cached opener",
            );
            let tree = window.accessibility_tree();
            assert_eq!(tree.get(opener_id).unwrap().bounds, opener_bounds);
            assert_eq!(tree.focused_node_count(), 1);
            assert_eq!(tree.focused_node(), Some(opener_id));
        });
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.hidden = true;
                cx.notify();
            });
            // One hidden draw retains the child Entity and its layout slot.
            window.draw(cx).clear();
            assert_eq!(
                renders.get(),
                settled_renders,
                "fixture eligibility: hiding must retain the already cached child",
            );
            let tree = window.accessibility_tree();
            assert!(
                tree.get(opener_id).is_none(),
                "fixture eligibility: the hidden cached opener must not be painted",
            );
            assert!(
                tree.nodes.values().all(|node| {
                    node.role != AccessibilityRole::Button
                        || node.label.as_deref() != Some("Cached modal opener")
                }),
                "fixture eligibility: no replacement hidden opener semantic is allowed",
            );
        });
        let shown_renders = window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.hidden = false;
                cx.notify();
            });
            // A skipped paint invalidates its old frame ranges. One show draw
            // must rebuild exactly once without notifying/replacing the child.
            window.draw(cx).clear();
            assert_eq!(
                renders.get(),
                settled_renders.checked_add(1).unwrap(),
                "paint-gap policy: one show draw must rebuild the cached child once",
            );
            let tree = window.accessibility_tree();
            let shown = tree
                .get(opener_id)
                .expect("fixture eligibility: cached semantic identity must be restored");
            assert_eq!(shown.bounds, opener_bounds);
            assert_eq!(shown.label.as_deref(), Some("Cached modal opener"));
            assert_eq!(tree.focused_node_count(), 1);
            assert_eq!(tree.focused_node(), Some(opener_id));
            renders.get()
        });
        let modal = window.update(|_, cx| {
            cx.new(|cx| {
                Dialog::new(cx)
                    .id("cached-opener-dialog")
                    .title("Cached opener Dialog")
                    .show_close_button(false)
                    .close_on_backdrop_click(false)
            })
        });
        window.update(|window, cx| {
            host.update(cx, |host, cx| {
                host.modal = Some(modal.clone());
                cx.notify();
            });
            // Exactly one mount draw. Dialog autofocus happens in its Render;
            // no direct focus call, defer, timer, trap or follow-up draw here.
            window.draw(cx).clear();
            assert_eq!(
                renders.get(),
                shown_renders,
                "fixture eligibility: mount must reuse the freshly rebuilt opener cache",
            );
            assert!(modal.read(cx).focus_handle.is_focused(window));
            let tree = window.accessibility_tree();
            assert_eq!(tree.get(opener_id).unwrap().bounds, opener_bounds);
            let dialogs = tree
                .nodes
                .values()
                .filter(|node| {
                    node.role == AccessibilityRole::Dialog
                        && node.label.as_deref() == Some("Cached opener Dialog")
                })
                .map(|node| node.id)
                .collect::<Vec<_>>();
            assert_eq!(dialogs.len(), 1);
            let dialog = dialogs[0];
            assert_eq!(tree.focused_node_count(), 1, "one exported focus owner");
            assert_eq!(tree.focused_node(), Some(dialog));
            let export = tree.to_accesskit_tree_update(None, None);
            assert_eq!(export.focus.0, dialog.0);
            assert!(export.nodes.iter().any(|(id, _)| id.0 == dialog.0));
        });
        window.update(|window, _| window.remove_window());
        drop(modal);
        drop(host);
        drop(opener);
        drop(window);
        cx.quit();
    }
}
