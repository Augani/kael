//! MetadataList component - read-only label/value metadata.

use crate::{
    components::{
        button::{Button, ButtonSize, ButtonVariant},
        icon::Icon,
        icon_source::IconSource,
    },
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::panic::Location;
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
    id: ElementId,
    accessible_label: SharedString,
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
    #[track_caller]
    pub fn new() -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "metadata-list-{}-{}-{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            accessible_label: "Metadata".into(),
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

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Set the name announced for the metadata collection.
    pub fn accessible_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessible_label = label.into();
        self
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let explicit_column_count = match self.columns {
            MetadataListColumns::Count(count) if count > 1 => {
                Some(count.min(u16::MAX as usize) as u16)
            }
            _ => None,
        };
        let is_fluid_multi = matches!(self.columns, MetadataListColumns::Multi);
        let is_multi = is_fluid_multi || explicit_column_count.is_some();
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
        let is_controlled = self.on_toggle.is_some();
        let local_expanded = window.use_keyed_state(
            ElementId::NamedChild(Box::new(self.id.clone()), "expanded".into()),
            cx,
            |_, _| self.expanded,
        );
        let expanded = if is_controlled {
            self.expanded
        } else {
            *local_expanded.read(cx)
        };
        let theme = Theme::of(cx);
        let is_collapsed = self.max_items.is_some() && !expanded && total_items > max_items;
        let show_toggle = self.max_items.is_some() && total_items > max_items;
        let expanded_next = !expanded;
        let on_toggle = self.on_toggle;
        let list_id = self.id.clone();
        let list_id_for_items = list_id.clone();
        let list_id_for_toggle = list_id.clone();

        div()
            .id(list_id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::List)
                    .label(self.accessible_label.to_string()),
            )
            .flex()
            .flex_col()
            .font_family(theme.tokens.font_family.clone())
            .when_some(self.title, |this, title| {
                this.child(div().mb(px(12.0)).child(title))
            })
            .child(
                div()
                    .when_some(explicit_column_count, |this, column_count| {
                        this.grid().grid_cols(column_count).gap(px(16.0))
                    })
                    .when(
                        explicit_column_count.is_none()
                            && self.orientation == MetadataListOrientation::Horizontal,
                        |this| this.flex_row().flex_wrap().gap(px(16.0)),
                    )
                    .when(
                        explicit_column_count.is_none()
                            && self.orientation == MetadataListOrientation::Vertical,
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
                                Some({
                                    let item_id = ElementId::NamedChild(
                                        Box::new(list_id_for_items.clone()),
                                        format!("item-{idx}").into(),
                                    );
                                    render_metadata_item(
                                        item,
                                        item_id,
                                        is_stacked,
                                        is_fluid_multi,
                                        theme,
                                    )
                                    .into_any_element()
                                })
                            }),
                    ),
            )
            .when(show_toggle, |this| {
                this.child(
                    Button::new(
                        ElementId::NamedChild(Box::new(list_id_for_toggle), "toggle".into()),
                        if is_collapsed {
                            format!("Show {} more", total_items - max_items)
                        } else {
                            "Show less".to_owned()
                        },
                    )
                    .size(ButtonSize::Sm)
                    .variant(ButtonVariant::Ghost)
                    .mt(px(8.0))
                    .on_click(move |_, window, cx| {
                        if let Some(handler) = on_toggle.as_ref() {
                            handler(expanded_next, window, cx);
                        } else {
                            local_expanded.update(cx, |expanded, cx| {
                                *expanded = expanded_next;
                                cx.notify();
                            });
                        }
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
    id: ElementId,
    is_stacked: bool,
    is_fluid_multi: bool,
    theme: &Theme,
) -> impl IntoElement {
    let label_text = item.label.clone();
    let accessible_label = item.label.clone();
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
            .id(id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::ListItem)
                    .label(accessible_label.to_string()),
            )
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(label())
            .child(value)
    } else {
        div()
            .id(id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::ListItem)
                    .label(accessible_label.to_string()),
            )
            .flex()
            .items_start()
            .gap(px(16.0))
            .child(div().w(px(128.0)).child(label()))
            .child(div().flex_1().child(value))
    };

    item.when(is_fluid_multi, |this| this.min_w(px(220.0)).flex_1())
}
