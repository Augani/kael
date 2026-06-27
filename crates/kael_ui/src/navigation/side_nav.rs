//! SideNav component - ASTRYX-named sidebar navigation.

use crate::components::icon::Icon;
use crate::navigation::sidebar::{Sidebar, SidebarItem, SidebarVariant};
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::sync::Arc;

pub type SideNavItem = SidebarItem<SharedString>;
pub type SideNavSection = crate::navigation::nav_menu::NavMenu;

#[derive(IntoElement)]
pub struct SideNavCollapseButton {
    collapsed: bool,
    label: Option<SharedString>,
    on_toggle: Option<Arc<dyn Fn(bool, &mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
}

impl SideNavCollapseButton {
    pub fn new() -> Self {
        Self {
            collapsed: false,
            label: None,
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn on_toggle(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_toggle = Some(Arc::new(handler));
        self
    }
}

impl Default for SideNavCollapseButton {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for SideNavCollapseButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SideNavCollapseButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let collapsed = self.collapsed;
        let label = self.label.unwrap_or_else(|| {
            if collapsed {
                "Expand sidebar".into()
            } else {
                "Collapse sidebar".into()
            }
        });
        let user_style = self.style;

        div()
            .relative()
            .size(px(32.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme.tokens.radius_md)
            .cursor_pointer()
            .hover(|style| style.bg(theme.tokens.accent))
            .when_some(self.on_toggle, |this, handler| {
                this.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                    handler(!collapsed, window, cx);
                })
            })
            .child(
                div()
                    .absolute()
                    .left(px(-10000.0))
                    .top(px(0.0))
                    .size(px(1.0))
                    .overflow_hidden()
                    .child(label),
            )
            .child(
                Icon::new("chevron-left")
                    .size(px(18.0))
                    .color(theme.tokens.foreground)
                    .when(collapsed, |icon| icon.rotate(Radians(std::f32::consts::PI))),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct SideNavHeading {
    label: SharedString,
    style: StyleRefinement,
}

impl SideNavHeading {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for SideNavHeading {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SideNavHeading {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;

        div()
            .px(px(8.0))
            .pt(px(10.0))
            .pb(px(4.0))
            .text_size(px(11.0))
            .line_height(px(14.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.tokens.muted_foreground)
            .child(self.label)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(IntoElement)]
pub struct SideNav {
    items: Vec<SidebarItem<SharedString>>,
    selected_id: Option<SharedString>,
    collapsed: bool,
    on_select: Option<Arc<dyn Fn(&SharedString, &mut Window, &mut App) + Send + Sync + 'static>>,
    style: StyleRefinement,
}

impl SideNav {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_id: None,
            collapsed: false,
            on_select: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn item(mut self, item: SidebarItem<SharedString>) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: Vec<SidebarItem<SharedString>>) -> Self {
        self.items = items;
        self
    }

    pub fn selected_id(mut self, id: impl Into<SharedString>) -> Self {
        self.selected_id = Some(id.into());
        self
    }

    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    pub fn on_select(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + Send + Sync + 'static,
    ) -> Self {
        self.on_select = Some(Arc::new(handler));
        self
    }
}

impl Default for SideNav {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for SideNav {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SideNav {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut sidebar = Sidebar::new(cx)
            .items(self.items)
            .variant(SidebarVariant::Collapsible)
            .expanded(!self.collapsed);

        if let Some(selected_id) = self.selected_id {
            sidebar = sidebar.selected_id(selected_id);
        }

        if let Some(handler) = self.on_select {
            sidebar = sidebar.on_select(move |id, window, cx| {
                handler(id, window, cx);
            });
        }

        sidebar.map(|this| {
            let mut sidebar = this;
            sidebar.style().refine(&self.style);
            sidebar
        })
    }
}
