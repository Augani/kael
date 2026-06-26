//! ButtonGroup - a row of related actions joined into one segmented control.

use crate::astryx::ControlSize;
use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

pub struct ButtonGroupItem {
    label: SharedString,
    icon: Option<IconSource>,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

impl ButtonGroupItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

#[derive(IntoElement)]
pub struct ButtonGroup {
    id: ElementId,
    items: Vec<ButtonGroupItem>,
    size: ControlSize,
    style: StyleRefinement,
}

impl ButtonGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            size: ControlSize::Md,
            style: StyleRefinement::default(),
        }
    }

    pub fn child(mut self, item: ButtonGroupItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for ButtonGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ButtonGroup {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let height = self.size.height();
        let padding_x = self.size.padding_x();
        let font_size = self.size.font_size();
        let icon_size = self.size.icon_size();
        let id = self.id.clone();

        let mut row = div()
            .id(id)
            .flex()
            .items_center()
            .h(height)
            .bg(theme.tokens.secondary)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_md)
            .overflow_hidden()
            .font_family(theme.tokens.font_family.clone());

        for (index, item) in self.items.into_iter().enumerate() {
            let handler = item.on_click.clone();
            let icon = item.icon.clone();
            let cell = div()
                .id(ElementId::Name(format!("bg-item-{index}").into()))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .h_full()
                .px(padding_x)
                .text_size(font_size)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.tokens.secondary_foreground)
                .cursor(CursorStyle::PointingHand)
                .when(index > 0, |this| {
                    this.border_l_1().border_color(theme.tokens.border)
                })
                .hover(|style| style.bg(theme.tokens.accent))
                .when_some(icon, |this, icon_src| {
                    this.child(
                        Icon::new(icon_src)
                            .size(icon_size)
                            .color(theme.tokens.secondary_foreground),
                    )
                })
                .child(item.label.clone())
                .when_some(handler, |this, handler| {
                    this.on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        (handler)(window, cx);
                    })
                });
            row = row.child(cell);
        }

        row.map(|this| {
            let mut div = this;
            div.style().refine(&user_style);
            div
        })
    }
}
