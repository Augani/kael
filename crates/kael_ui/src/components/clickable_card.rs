//! ClickableCard component - ASTRYX card action surface.

use crate::{astryx, display::card::CardVariant, styled_ext::StyledExt, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};
use std::cell::RefCell;
use std::panic::Location;
use std::rc::Rc;

#[derive(IntoElement)]
pub struct ClickableCard {
    id: ElementId,
    children: Vec<AnyElement>,
    label: Option<SharedString>,
    variant: CardVariant,
    padding: Option<Pixels>,
    width: Option<Pixels>,
    height: Option<Pixels>,
    max_width: Option<Pixels>,
    min_height: Option<Pixels>,
    href: Option<SharedString>,
    target: Option<SharedString>,
    selected: bool,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl ClickableCard {
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "clickable-card:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            children: Vec::new(),
            label: None,
            variant: CardVariant::Default,
            padding: None,
            width: None,
            height: None,
            max_width: None,
            min_height: None,
            href: None,
            target: None,
            selected: false,
            disabled: false,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn max_width(mut self, max_width: Pixels) -> Self {
        self.max_width = Some(max_width);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxWidth(self, max_width: Pixels) -> Self {
        self.max_width(max_width)
    }

    pub fn min_height(mut self, min_height: Pixels) -> Self {
        self.min_height = Some(min_height);
        self
    }

    #[allow(non_snake_case)]
    pub fn minHeight(self, min_height: Pixels) -> Self {
        self.min_height(min_height)
    }

    pub fn href(mut self, href: impl Into<SharedString>) -> Self {
        self.href = Some(href.into());
        self
    }

    pub fn target(mut self, target: impl Into<SharedString>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }
}

impl Default for ClickableCard {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for ClickableCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ClickableCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let selected = self.selected;
        let disabled = self.disabled;
        let user_style = self.style;
        let dark = theme.tokens.background.l < 0.5;
        let bg = self.variant.background(&theme);
        let border = if selected {
            theme.tokens.primary
        } else if self.variant == CardVariant::Default {
            theme.tokens.input
        } else {
            transparent_black()
        };
        let padding = self.padding.unwrap_or(px(16.0));
        let label = self.label.unwrap_or_else(|| "Open card".into());
        let handler = self.on_click;
        let href = self.href;
        let announces_external_target = self
            .target
            .as_ref()
            .is_some_and(|target| target.as_ref() == "_blank");
        let accessible_label: SharedString = if announces_external_target {
            format!("{label}, opens externally").into()
        } else {
            label
        };
        let has_action = handler.is_some() || href.is_some();
        let children = Rc::new(RefCell::new(Some(self.children)));

        button(self.id)
            .role(if href.is_some() {
                AccessibilityRole::Link
            } else {
                AccessibilityRole::Button
            })
            .label(accessible_label)
            .when(disabled || !has_action, |this| this.disabled())
            .when(has_action && !disabled, |this| {
                this.on_click(move |_, window, cx| {
                    if let Some(href) = href.as_ref() {
                        let _ = cx.open_url(href.as_ref());
                    }
                    if let Some(handler) = handler.as_ref() {
                        handler(window, cx);
                    }
                })
            })
            .render_with(move |state, _, _| {
                let children = children.borrow_mut().take().unwrap_or_default();
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .p(padding)
                    .when_some(self.width, |this, width| this.w(width))
                    .when_some(self.height, |this, height| this.h(height).overflow_hidden())
                    .when_some(self.max_width, |this, max_width| this.max_w(max_width))
                    .when_some(self.min_height, |this, min_height| this.min_h(min_height))
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .rounded(theme.tokens.radius_lg)
                    .transition(theme.tokens.transition_fast)
                    .when(selected, |this| {
                        this.inset_ring(theme.tokens.primary, px(2.0))
                    })
                    .when(state.focused && !state.disabled, |this| {
                        this.shadow(smallvec::smallvec![astryx::focus_ring_outer(
                            theme.tokens.ring
                        )])
                    })
                    .when(state.disabled, |this| this.opacity(0.55))
                    .when(!state.disabled, |this| {
                        this.cursor_pointer()
                            .hover(move |style| style.bg(astryx::overlay_hover(dark)))
                    })
                    .children(children)
                    .map(|this| {
                        let mut div = this;
                        div.style().refine(&user_style);
                        div.into_any_element()
                    })
            })
    }
}
