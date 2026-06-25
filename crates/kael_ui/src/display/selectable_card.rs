//! SelectableCard - a card that behaves like a large radio/checkbox option.

use crate::components::icon::Icon;
use crate::styled_ext::StyledExt;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(IntoElement)]
pub struct SelectableCard {
    base: Stateful<Div>,
    selected: bool,
    disabled: bool,
    show_check: bool,
    content: Option<AnyElement>,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl SelectableCard {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id.into()),
            selected: false,
            disabled: false,
            show_check: true,
            content: None,
            on_click: None,
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

    pub fn show_check(mut self, show: bool) -> Self {
        self.show_check = show;
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for SelectableCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for SelectableCard {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for SelectableCard {}

impl RenderOnce for SelectableCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let selected = self.selected;
        let disabled = self.disabled;
        let on_click = self.on_click.clone();

        let border = if selected {
            theme.tokens.ring
        } else {
            theme.tokens.border
        };

        self.base
            .relative()
            .p(px(16.0))
            .bg(theme.tokens.card)
            .border_1()
            .border_color(border)
            .rounded(theme.tokens.radius_lg)
            .shadow(theme.tokens.shadow_xs.to_vec())
            .transition(theme.tokens.transition_fast)
            .when(selected, |this| {
                this.inset_ring(theme.tokens.ring.opacity(0.5), px(2.0))
            })
            .when(disabled, |this| this.opacity(0.55))
            .when(!disabled, |this| {
                this.cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.shadow(theme.tokens.shadow_sm.to_vec()))
            })
            .when_some(self.content, |this, content| this.child(content))
            .when(selected && self.show_check, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(10.0))
                        .right(px(10.0))
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(theme.tokens.primary)
                        .child(
                            Icon::new("check")
                                .size(px(12.0))
                                .color(theme.tokens.primary_foreground),
                        ),
                )
            })
            .when(!disabled, |this| {
                this.when_some(on_click, |this, handler| {
                    this.on_click(move |_, window, cx| {
                        let next = !selected;
                        (handler)(&next, window, cx);
                    })
                })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}
