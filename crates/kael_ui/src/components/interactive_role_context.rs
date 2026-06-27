//! InteractiveRoleContext - ASTRYX-style interactive region wrapper.

use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InteractiveRole {
    #[default]
    Group,
    Button,
    MenuItem,
    Option,
}

#[derive(IntoElement)]
pub struct InteractiveRoleContext {
    role: InteractiveRole,
    selected: bool,
    disabled: bool,
    children: Vec<AnyElement>,
    on_activate: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl InteractiveRoleContext {
    pub fn new(role: InteractiveRole) -> Self {
        Self {
            role,
            selected: false,
            disabled: false,
            children: Vec::new(),
            on_activate: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn on_activate(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_activate = Some(Rc::new(handler));
        self
    }
}

impl Styled for InteractiveRoleContext {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for InteractiveRoleContext {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let interactive = self.on_activate.is_some();
        let user_style = self.style;
        let padding_x = match self.role {
            InteractiveRole::Group => px(0.0),
            InteractiveRole::Button | InteractiveRole::MenuItem | InteractiveRole::Option => {
                px(8.0)
            }
        };
        let height = match self.role {
            InteractiveRole::Group => None,
            InteractiveRole::Button | InteractiveRole::MenuItem | InteractiveRole::Option => {
                Some(px(32.0))
            }
        };

        div()
            .flex()
            .items_center()
            .px(padding_x)
            .when_some(height, |this, height| this.min_h(height))
            .rounded(theme.tokens.radius_md)
            .bg(if self.selected {
                theme.tokens.accent
            } else {
                transparent_black()
            })
            .when(self.disabled, |this| this.opacity(0.5))
            .when(interactive && !self.disabled, |this| {
                this.cursor_pointer()
                    .hover(|style| style.bg(theme.tokens.accent))
            })
            .when_some(
                self.on_activate.filter(|_| !self.disabled),
                |this, handler| {
                    this.on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                        handler(window, cx);
                    })
                },
            )
            .children(self.children)
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
