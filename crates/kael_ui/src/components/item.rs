//! Item component - ASTRYX-style row primitive.

use crate::{astryx, components::icon::Icon, theme::Theme};
use kael::{prelude::FluentBuilder as _, *};
use std::{cell::RefCell, panic::Location, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ItemAlign {
    #[default]
    Center,
    Start,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ItemDensity {
    Compact,
    #[default]
    Balanced,
    Spacious,
}

impl ItemDensity {
    fn min_height(self) -> Pixels {
        match self {
            Self::Compact => px(32.0),
            Self::Balanced => px(40.0),
            Self::Spacious => px(48.0),
        }
    }

    fn padding_y(self) -> Pixels {
        match self {
            Self::Compact => px(4.0),
            Self::Balanced => px(8.0),
            Self::Spacious => px(12.0),
        }
    }

    fn padding_x(self) -> Pixels {
        match self {
            Self::Compact | Self::Balanced => px(8.0),
            Self::Spacious => px(12.0),
        }
    }
}

#[derive(IntoElement)]
pub struct Item {
    id: ElementId,
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<SharedString>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    align: ItemAlign,
    density: ItemDensity,
    highlighted: bool,
    selected: bool,
    disabled: bool,
    href: Option<SharedString>,
    target: Option<SharedString>,
    rel: Option<SharedString>,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Item {
    #[track_caller]
    pub fn new(title: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "item:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            title: title.into(),
            description: None,
            icon: None,
            leading: None,
            trailing: None,
            align: ItemAlign::default(),
            density: ItemDensity::default(),
            highlighted: false,
            selected: false,
            disabled: false,
            href: None,
            target: None,
            rel: None,
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn start_content(mut self, content: impl IntoElement) -> Self {
        self.leading = Some(content.into_any_element());
        self
    }

    #[allow(non_snake_case)]
    pub fn startContent(self, content: impl IntoElement) -> Self {
        self.start_content(content)
    }

    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }

    pub fn end_content(self, content: impl IntoElement) -> Self {
        self.trailing(content)
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl IntoElement) -> Self {
        self.end_content(content)
    }

    pub fn align(mut self, align: ItemAlign) -> Self {
        self.align = align;
        self
    }

    pub fn density(mut self, density: ItemDensity) -> Self {
        self.density = density;
        self
    }

    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    #[allow(non_snake_case)]
    pub fn isSelected(self, selected: bool) -> Self {
        self.selected(selected)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn href(mut self, href: impl Into<SharedString>) -> Self {
        self.href = Some(href.into());
        self
    }

    pub fn target(mut self, target: impl Into<SharedString>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Preserve web-style relationship metadata for API compatibility.
    /// Native URL launches do not send a referrer or expose an opener window.
    pub fn rel(mut self, rel: impl Into<SharedString>) -> Self {
        self.rel = Some(rel.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }
}

impl Styled for Item {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Item {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let user_style = self.style;
        let disabled = self.disabled;
        let interactive = self.on_click.is_some() || self.href.is_some();
        let dark = theme.tokens.background.l < 0.5;
        let announces_external_target = self
            .target
            .as_ref()
            .is_some_and(|target| target.as_ref() == "_blank");
        let href = self.href;
        let on_click = self.on_click;
        let title = self.title;
        let accessible_label: SharedString = if announces_external_target {
            format!("{title}, opens externally").into()
        } else {
            title.clone()
        };
        let icon = self.icon;
        let leading = Rc::new(RefCell::new(self.leading));
        let trailing = Rc::new(RefCell::new(self.trailing));
        let description = self.description;
        let align = self.align;
        let density = self.density;
        let selected = self.selected;
        let highlighted = self.highlighted;

        let render_body = move |focused: bool| {
            let leading = leading.borrow_mut().take();
            let trailing = trailing.borrow_mut().take();

            div()
                .flex()
                .when(align == ItemAlign::Center, |this| this.items_center())
                .when(align == ItemAlign::Start, |this| this.items_start())
                .gap(px(8.0))
                .w_full()
                .min_h(density.min_height())
                .px(density.padding_x())
                .py(density.padding_y())
                .rounded(theme.tokens.radius_md)
                .bg(if selected {
                    theme.tokens.accent
                } else if highlighted {
                    astryx::overlay_hover(dark)
                } else {
                    transparent_black()
                })
                .when(disabled, |this| this.opacity(0.5))
                .when(focused && !disabled, |this| {
                    this.shadow(smallvec::smallvec![astryx::focus_ring_outer(
                        theme.tokens.ring
                    )])
                })
                .when(!disabled && interactive, |this| {
                    this.cursor_pointer()
                        .hover(move |style| style.bg(astryx::overlay_hover(dark)))
                })
                .when_some(icon.clone(), |this, icon| {
                    this.child(
                        div()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(theme.tokens.muted_foreground)
                            .child(Icon::new(icon).size(px(16.0))),
                    )
                })
                .when_some(leading, |this, leading| {
                    this.child(div().flex().flex_shrink_0().child(leading))
                })
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(1.0))
                        .overflow_hidden()
                        .child(
                            div()
                                .text_size(px(14.0))
                                .line_height(px(20.0))
                                .text_color(theme.tokens.foreground)
                                .child(title.clone()),
                        )
                        .when_some(description.clone(), |this, description| {
                            this.child(
                                div()
                                    .text_size(px(12.0))
                                    .line_height(px(16.0))
                                    .text_color(theme.tokens.muted_foreground)
                                    .child(description),
                            )
                        }),
                )
                .when_some(trailing, |this, trailing| {
                    this.child(div().ml_auto().flex().child(trailing))
                })
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div.into_any_element()
                })
        };

        if interactive {
            button(self.id)
                .role(if href.is_some() {
                    AccessibilityRole::Link
                } else {
                    AccessibilityRole::Button
                })
                .label(accessible_label)
                .when(disabled, |this| this.disabled())
                .when(!disabled, |this| {
                    this.on_click(move |_, window, cx| {
                        if let Some(href) = href.as_ref() {
                            let _ = cx.open_url(href.as_ref());
                        }
                        if let Some(handler) = on_click.as_ref() {
                            handler(window, cx);
                        }
                    })
                })
                .render_with(move |state, _, _| render_body(state.focused))
                .into_any_element()
        } else {
            render_body(false)
        }
    }
}
