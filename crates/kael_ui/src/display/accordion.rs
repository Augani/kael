//! Accordion - Collapsible content sections with smooth animations.

use crate::{
    components::icon::Icon,
    components::icon_source::IconSource,
    styled_ext::StyledExt,
    theme::{Theme, use_theme},
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(IntoElement)]
pub struct Accordion {
    id: ElementId,
    accessibility_label: SharedString,
    items: Vec<AccordionItem>,
    multiple: bool,
    bordered: bool,
    disabled: bool,
    open_indices: Vec<usize>,
    on_change: Option<Rc<dyn Fn(&[usize], &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Accordion {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            accessibility_label: "Accordion".into(),
            items: Vec::new(),
            multiple: false,
            bordered: true,
            disabled: false,
            open_indices: Vec::new(),
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the label announced for the accordion group.
    ///
    /// Use a unique, contextual label when a view contains multiple accordions.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.accessibility_label = if label.is_empty() {
            "Accordion".into()
        } else {
            label
        };
        self
    }

    pub fn item<F>(mut self, builder: F) -> Self
    where
        F: FnOnce(AccordionItem) -> AccordionItem,
    {
        let item = builder(AccordionItem::new(self.items.len()));
        if item.is_open {
            self.open_indices.push(self.items.len());
        }
        self.items.push(item);
        self
    }

    pub fn on_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&[usize], &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(callback));
        self
    }
}

impl Styled for Accordion {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Accordion {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let _theme = use_theme();
        let user_style = self.style;
        let multiple = self.multiple;
        let on_change = self.on_change;
        let accordion_id = self.id.clone();
        let item_count = self.items.len();
        let initial_open_indices = normalize_open_indices(self.open_indices, item_count, multiple);
        let open_indices = window.use_keyed_state(
            ElementId::NamedChild(Box::new(accordion_id.clone()), "open-items".into()),
            cx,
            move |_, _| initial_open_indices,
        );
        open_indices.update(cx, |indices, _| {
            *indices = normalize_open_indices(std::mem::take(indices), item_count, multiple);
        });

        div()
            .id(self.id)
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(self.accessibility_label.to_string()),
            )
            .flex()
            .flex_col()
            .w_full()
            .gap(if self.bordered { px(8.0) } else { px(0.0) })
            .children(self.items.into_iter().map(|item| {
                let item_index = item.index;
                let is_open = open_indices.read(cx).contains(&item_index);
                let open_indices_clone = open_indices.clone();
                let on_change_clone = on_change.clone();

                item.id(ElementId::NamedChild(
                    Box::new(accordion_id.clone()),
                    format!("item-{item_index}").into(),
                ))
                .bordered(self.bordered)
                .disabled(self.disabled)
                .open(is_open)
                .on_toggle(move |is_opening, window, cx| {
                    let open_vec = open_indices_clone.update(cx, |indices, cx| {
                        if is_opening {
                            if !multiple {
                                indices.clear();
                            }
                            if !indices.contains(&item_index) {
                                indices.push(item_index);
                            }
                        } else {
                            indices.retain(|&i| i != item_index);
                        }
                        indices.sort_unstable();
                        cx.notify();
                        indices.clone()
                    });

                    if let Some(ref callback) = on_change_clone {
                        callback(&open_vec, window, cx);
                    }
                })
            }))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn normalize_open_indices(
    mut indices: Vec<usize>,
    item_count: usize,
    multiple: bool,
) -> Vec<usize> {
    indices.retain(|index| *index < item_count);
    indices.sort_unstable();
    indices.dedup();
    if !multiple && indices.len() > 1 {
        indices.drain(..indices.len() - 1);
    }
    indices
}

#[derive(IntoElement)]
pub struct AccordionItem {
    id: Option<ElementId>,
    index: usize,
    title: SharedString,
    content: Option<AnyElement>,
    icon: Option<IconSource>,
    is_open: bool,
    bordered: bool,
    disabled: bool,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
}

impl AccordionItem {
    fn new(index: usize) -> Self {
        Self {
            id: None,
            index,
            title: SharedString::from(""),
            content: None,
            icon: None,
            is_open: false,
            bordered: true,
            disabled: false,
            on_toggle: None,
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn open(mut self, open: bool) -> Self {
        self.is_open = open;
        self
    }

    fn id(mut self, id: ElementId) -> Self {
        self.id = Some(id);
        self
    }

    fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    fn on_toggle<F>(mut self, callback: F) -> Self
    where
        F: Fn(bool, &mut Window, &mut App) + 'static,
    {
        self.on_toggle = Some(Rc::new(callback));
        self
    }
}

impl RenderOnce for AccordionItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let item_id = self
            .id
            .clone()
            .unwrap_or_else(|| ElementId::Name(format!("accordion-item-{}", self.index).into()));
        let header_id = ElementId::NamedChild(Box::new(item_id.clone()), "header".into());
        let focus_handle = window
            .use_keyed_state(header_id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);
        let theme = Theme::of(cx);
        let is_open = self.is_open;
        let panel_id = ElementId::NamedChild(Box::new(item_id.clone()), "panel".into());
        let title = self.title.clone();
        let disabled = self.disabled;
        let callback = self.on_toggle.filter(|_| !disabled);
        let mut accessibility_state = AccessibilityState::NONE;
        accessibility_state |= if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        let mut header_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(title.to_string())
            .states(accessibility_state)
            .focused(is_focused);
        if callback.is_some() {
            header_accessibility = header_accessibility
                .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        div()
            .id(item_id)
            .flex()
            .flex_col()
            .w_full()
            .overflow_hidden()
            .bg(theme.tokens.card)
            .when(self.bordered, |div| {
                div.border_1()
                    .border_color(theme.tokens.border)
                    .rounded(theme.tokens.radius_lg)
            })
            .child(
                div()
                    .id(header_id)
                    .accessibility(header_accessibility)
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.0))
                    .py(px(12.0))
                    .cursor(if self.disabled {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .when(!self.disabled, |div| {
                        div.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                            .transition(theme.tokens.transition_fast)
                            .hover(|style| style.bg(theme.tokens.muted.opacity(0.5)))
                            .focus_visible(|style| style.inset_ring(theme.tokens.ring, px(2.0)))
                    })
                    .when(self.is_open && self.bordered, |div| {
                        div.border_b_1().border_color(theme.tokens.border)
                    })
                    .when_some(callback, |div, callback| {
                        let on_key = callback.clone();
                        div.on_click(move |_, window, cx| {
                            callback(!is_open, window, cx);
                        })
                        .on_key_down(move |event, window, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                on_key(!is_open, window, cx);
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                        })
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .when_some(self.icon, |div, icon| {
                                div.child(
                                    Icon::new(icon)
                                        .size(px(18.0))
                                        .color(theme.tokens.muted_foreground),
                                )
                            })
                            .child(
                                div()
                                    .text_size(px(15.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.tokens.foreground)
                                    .child(StyledText::new(self.title).accessibility_hidden(true)),
                            ),
                    )
                    .child(
                        Icon::new(if is_open {
                            "chevron-up"
                        } else {
                            "chevron-down"
                        })
                        .size(px(16.0))
                        .color(theme.tokens.muted_foreground),
                    ),
            )
            .when(is_open, |parent| {
                parent.child(
                    div()
                        .id(panel_id)
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Group)
                                .label(title.to_string()),
                        )
                        .px(px(16.0))
                        .py(px(12.0))
                        .text_size(px(14.0))
                        .text_color(theme.tokens.muted_foreground)
                        .when_some(self.content, |content_div, content| {
                            content_div.child(content)
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn open_indices_are_bounded_deduplicated_and_respect_single_mode() {
        assert_eq!(
            normalize_open_indices(vec![4, 1, 1, 2, 9], 5, true),
            vec![1, 2, 4]
        );
        assert_eq!(
            normalize_open_indices(vec![4, 1, 1, 2, 9], 5, false),
            vec![4]
        );
        assert!(normalize_open_indices(vec![2], 2, true).is_empty());
    }

    #[::core::prelude::v1::test]
    fn empty_accessibility_labels_fall_back_to_a_useful_name() {
        let accordion = Accordion::new("settings").accessibility_label("");
        assert_eq!(accordion.accessibility_label.as_ref(), "Accordion");
    }
}
