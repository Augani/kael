//! MoreMenu component - compact overflow menu trigger.

use crate::{
    components::icon::Icon,
    components::icon_button::IconButton,
    navigation::menu::{MenuItem, MenuItemKind},
    theme::Theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(IntoElement)]
pub struct MoreMenu {
    id: ElementId,
    items: Vec<MenuItem>,
    label: SharedString,
    disabled: bool,
    style: StyleRefinement,
}

impl MoreMenu {
    pub fn new(items: Vec<MenuItem>) -> Self {
        let id_suffix = items
            .iter()
            .find(|item| !matches!(item.kind, MenuItemKind::Separator))
            .map(|item| item.id.to_string())
            .unwrap_or_else(|| "empty".to_string());
        Self {
            id: ElementId::Name(format!("more-menu-{id_suffix}").into()),
            items,
            label: "More actions".into(),
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Override the stable element id when more than one overflow menu shares
    /// the same first item.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }
}

impl Styled for MoreMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for MoreMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        if self.disabled {
            return div()
                .child(
                    IconButton::new("ellipsis")
                        .label(self.label)
                        .size(px(32.0))
                        .icon_size(px(16.0))
                        .disabled(true),
                )
                .map(|this| {
                    let mut div = this;
                    div.style().refine(&user_style);
                    div
                })
                .into_any_element();
        }

        let menu_items = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let item_value = MenuButtonItem::new(index, item.label.clone());
                if matches!(item.kind, MenuItemKind::Separator) {
                    item_value.separator()
                } else if item.disabled || matches!(item.kind, MenuItemKind::Submenu) {
                    item_value.disabled()
                } else {
                    item_value
                }
            })
            .collect::<Vec<_>>();
        let action_items = Rc::new(self.items);
        let render_items = action_items.clone();
        let trigger_theme = theme.clone();
        let item_theme = theme.clone();

        div()
            .child(
                menu_button(self.id, menu_items)
                    .label(self.label)
                    .on_select(move |index, window, cx| {
                        if let Some(handler) = action_items
                            .get(*index)
                            .and_then(|item| item.on_click.clone())
                        {
                            handler(window, cx);
                        }
                    })
                    .render_trigger_with(move |state, _, _| {
                        div()
                            .size(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(trigger_theme.tokens.radius_md)
                            .border_1()
                            .border_color(if state.open {
                                trigger_theme.tokens.ring
                            } else {
                                trigger_theme.tokens.border
                            })
                            .bg(if state.open {
                                trigger_theme.tokens.accent
                            } else {
                                trigger_theme.tokens.card
                            })
                            .when(state.focused, |this| {
                                this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                                    trigger_theme.tokens.ring
                                ),])
                            })
                            .hover(|style| style.bg(trigger_theme.tokens.accent))
                            .child(
                                Icon::new("ellipsis")
                                    .size(px(16.0))
                                    .color(trigger_theme.tokens.foreground),
                            )
                            .into_any_element()
                    })
                    .render_items_with(move |state, _, _| {
                        let source = render_items.get(state.value);
                        if state.separator {
                            return div()
                                .h(px(1.0))
                                .my(px(4.0))
                                .bg(item_theme.tokens.border)
                                .into_any_element();
                        }
                        let is_checked = source.is_some_and(MenuItem::is_checked);
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .min_w(px(176.0))
                            .px(px(10.0))
                            .py(px(8.0))
                            .rounded(item_theme.tokens.radius_md)
                            .bg(if state.highlighted {
                                item_theme.tokens.accent
                            } else {
                                transparent_black()
                            })
                            .text_size(px(13.0))
                            .text_color(if state.disabled {
                                item_theme.tokens.muted_foreground
                            } else {
                                item_theme.tokens.foreground
                            })
                            .when_some(source.and_then(|item| item.icon.clone()), |this, icon| {
                                this.child(
                                    Icon::new(icon)
                                        .size(px(16.0))
                                        .color(item_theme.tokens.muted_foreground),
                                )
                            })
                            .when(is_checked, |this| {
                                this.child(
                                    Icon::new("check")
                                        .size(px(14.0))
                                        .color(item_theme.tokens.foreground),
                                )
                            })
                            .child(state.label)
                            .when_some(
                                source.and_then(|item| item.shortcut.clone()),
                                |this, shortcut| {
                                    this.child(
                                        div()
                                            .ml_auto()
                                            .text_size(px(12.0))
                                            .text_color(item_theme.tokens.muted_foreground)
                                            .child(shortcut),
                                    )
                                },
                            )
                            .when(
                                source
                                    .is_some_and(|item| matches!(item.kind, MenuItemKind::Submenu)),
                                |this| {
                                    this.child(
                                        Icon::new("chevron-right")
                                            .size(px(14.0))
                                            .color(item_theme.tokens.muted_foreground),
                                    )
                                },
                            )
                            .into_any_element()
                    }),
            )
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div.into_any_element()
            })
    }
}
