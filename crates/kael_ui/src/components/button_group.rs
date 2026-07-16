//! ButtonGroup - a row of related actions joined into one segmented control.

use crate::astryx::ControlSize;
use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ButtonGroupOrientation {
    #[default]
    Horizontal,
    Vertical,
}

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
    orientation: ButtonGroupOrientation,
    style: StyleRefinement,
}

impl ButtonGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            size: ControlSize::Md,
            orientation: ButtonGroupOrientation::Horizontal,
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

    pub fn orientation(mut self, orientation: ButtonGroupOrientation) -> Self {
        self.orientation = orientation;
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
        let id_key = id.to_string();
        let orientation = self.orientation;

        let mut row = div()
            .id(id)
            .flex()
            .when(orientation == ButtonGroupOrientation::Vertical, |this| {
                this.flex_col().min_w(px(140.0))
            })
            .when(orientation == ButtonGroupOrientation::Horizontal, |this| {
                this.flex_row().items_center().h(height)
            })
            .bg(theme.tokens.secondary)
            .border_1()
            .border_color(theme.tokens.border)
            .rounded(theme.tokens.radius_md)
            .overflow_hidden()
            .font_family(theme.tokens.font_family.clone());

        for (index, item) in self.items.into_iter().enumerate() {
            let handler = item.on_click.clone();
            let icon = item.icon.clone();
            let label = item.label.clone();
            let cell_theme = theme.clone();
            let cell = button(ElementId::Name(format!("{id_key}-item-{index}").into()))
                .label(label.clone())
                .when_some(handler, |this, handler| {
                    this.on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        (handler)(window, cx);
                    })
                })
                .render_with(move |state, _, _| {
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(6.0))
                        .when(orientation == ButtonGroupOrientation::Horizontal, |this| {
                            this.h(height)
                        })
                        .when(orientation == ButtonGroupOrientation::Vertical, |this| {
                            this.h(height).w_full()
                        })
                        .px(padding_x)
                        .text_size(font_size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cell_theme.tokens.secondary_foreground)
                        .when(
                            index > 0 && orientation == ButtonGroupOrientation::Horizontal,
                            |this| this.border_l_1().border_color(cell_theme.tokens.border),
                        )
                        .when(
                            index > 0 && orientation == ButtonGroupOrientation::Vertical,
                            |this| this.border_t_1().border_color(cell_theme.tokens.border),
                        )
                        .when(state.focused, |this| {
                            this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                                cell_theme.tokens.ring
                            ),])
                        })
                        .hover(|style| style.bg(cell_theme.tokens.accent))
                        .when_some(icon.clone(), |this, icon_src| {
                            this.child(
                                Icon::new(icon_src)
                                    .size(icon_size)
                                    .color(cell_theme.tokens.secondary_foreground),
                            )
                        })
                        .child(label.clone())
                        .into_any_element()
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
