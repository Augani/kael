//! MetadataList component - read-only label/value metadata.

use crate::{
    components::{icon::Icon, icon_source::IconSource},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MetadataListColumns {
    #[default]
    Single,
    Multi,
    Count(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MetadataLabelPosition {
    #[default]
    Start,
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MetadataListOrientation {
    #[default]
    Vertical,
    Horizontal,
}

pub struct MetadataListItem {
    label: SharedString,
    value: AnyElement,
    icon: Option<IconSource>,
}

impl MetadataListItem {
    pub fn new(label: impl Into<SharedString>, value: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            value: value.into_any_element(),
            icon: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }
}

#[derive(IntoElement)]
pub struct MetadataList {
    title: Option<AnyElement>,
    items: Vec<MetadataListItem>,
    columns: MetadataListColumns,
    label_position: Option<MetadataLabelPosition>,
    orientation: MetadataListOrientation,
    max_items: Option<usize>,
    expanded: bool,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl MetadataList {
    pub fn new() -> Self {
        Self {
            title: None,
            items: Vec::new(),
            columns: MetadataListColumns::Single,
            label_position: None,
            orientation: MetadataListOrientation::Vertical,
            max_items: None,
            expanded: false,
            on_toggle: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn title(mut self, title: impl IntoElement) -> Self {
        self.title = Some(title.into_any_element());
        self
    }

    pub fn item(mut self, item: MetadataListItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = MetadataListItem>) -> Self {
        self.items.extend(items);
        self
    }

    pub fn columns(mut self, columns: MetadataListColumns) -> Self {
        self.columns = columns;
        self
    }

    pub fn label_position(mut self, position: MetadataLabelPosition) -> Self {
        self.label_position = Some(position);
        self
    }

    pub fn label(mut self, position: MetadataLabelPosition) -> Self {
        self.label_position = Some(position);
        self
    }

    pub fn orientation(mut self, orientation: MetadataListOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn max_items(mut self, max_items: usize) -> Self {
        self.max_items = Some(max_items);
        self
    }

    pub fn max_num_of_items(self, max_items: usize) -> Self {
        self.max_items(max_items)
    }

    #[allow(non_snake_case)]
    pub fn maxNumOfItems(self, max_items: usize) -> Self {
        self.max_num_of_items(max_items)
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn on_toggle(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl Default for MetadataList {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for MetadataList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MetadataList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let is_multi = matches!(self.columns, MetadataListColumns::Multi)
            || matches!(self.columns, MetadataListColumns::Count(count) if count > 1);
        let label_position = self.label_position.unwrap_or(
            if is_multi || self.orientation == MetadataListOrientation::Horizontal {
                MetadataLabelPosition::Top
            } else {
                MetadataLabelPosition::Start
            },
        );
        let is_stacked = label_position == MetadataLabelPosition::Top
            || self.orientation == MetadataListOrientation::Horizontal;
        let total_items = self.items.len();
        let max_items = self.max_items.unwrap_or(total_items);
        let is_collapsed = self.max_items.is_some() && !self.expanded && total_items > max_items;
        let show_toggle = self.max_items.is_some() && total_items > max_items;
        let expanded_next = !self.expanded;
        let on_toggle = self.on_toggle;

        div()
            .flex()
            .flex_col()
            .font_family(theme.tokens.font_family.clone())
            .when_some(self.title, |this, title| {
                this.child(div().mb(px(12.0)).child(title))
            })
            .child(
                div()
                    .flex()
                    .when(
                        self.orientation == MetadataListOrientation::Horizontal,
                        |this| this.flex_row().flex_wrap().gap(px(16.0)),
                    )
                    .when(
                        self.orientation == MetadataListOrientation::Vertical,
                        |this| {
                            if is_multi {
                                this.flex_row().flex_wrap().gap(px(16.0))
                            } else {
                                this.flex_col().gap(px(if is_stacked { 12.0 } else { 8.0 }))
                            }
                        },
                    )
                    .children(
                        self.items
                            .into_iter()
                            .enumerate()
                            .filter_map(move |(idx, item)| {
                                if is_collapsed && idx >= max_items {
                                    return None;
                                }
                                Some(
                                    render_metadata_item(item, is_stacked, is_multi, &theme)
                                        .into_any_element(),
                                )
                            }),
                    ),
            )
            .when(show_toggle, |this| {
                this.child(
                    div()
                        .mt(px(8.0))
                        .py(px(8.0))
                        .text_size(px(14.0))
                        .line_height(relative(1.4))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.tokens.primary)
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            if let Some(handler) = on_toggle.as_ref() {
                                handler(expanded_next, window, cx);
                            }
                        })
                        .child(if is_collapsed {
                            "Show more"
                        } else {
                            "Show less"
                        }),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn render_metadata_item(
    item: MetadataListItem,
    is_stacked: bool,
    is_multi: bool,
    theme: &Theme,
) -> impl IntoElement {
    let label_text = item.label.clone();
    let icon = item.icon.clone();
    let label = || {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .min_h(px(24.0))
            .text_size(px(14.0))
            .line_height(px(20.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.tokens.muted_foreground)
            .when_some(icon.clone(), |this, icon| {
                this.child(
                    Icon::new(icon)
                        .size(px(16.0))
                        .color(theme.tokens.muted_foreground),
                )
            })
            .child(label_text.clone())
    };

    let value = div()
        .min_h(px(24.0))
        .text_size(px(14.0))
        .line_height(px(20.0))
        .text_color(theme.tokens.foreground)
        .child(item.value);

    let item = if is_stacked {
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(label())
            .child(value)
    } else {
        div()
            .flex()
            .items_start()
            .gap(px(16.0))
            .child(div().w(px(128.0)).child(label()))
            .child(div().flex_1().child(value))
    };

    item.when(is_multi, |this| this.min_w(px(280.0)).flex_1())
}
