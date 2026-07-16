//! Segmented navigation with animated sliding highlight indicator.

use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

use crate::{astryx, components::icon::Icon, theme::Theme};

#[derive(Clone)]
struct SegmentedNavItem {
    id: SharedString,
    label: SharedString,
    icon: Option<SharedString>,
    label_hidden: bool,
    disabled: bool,
}

#[derive(Clone)]
pub struct SegmentedControlItem {
    id: SharedString,
    label: SharedString,
    icon: Option<SharedString>,
    label_hidden: bool,
    disabled: bool,
}

impl SegmentedControlItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: value.into(),
            label: label.into(),
            icon: None,
            label_hidden: false,
            disabled: false,
        }
    }

    pub fn value(&self) -> &SharedString {
        &self.id
    }

    pub fn label(&self) -> &SharedString {
        &self.label
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.label_hidden(hidden)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SegmentedNavSize {
    Sm,
    #[default]
    Md,
    Lg,
}

pub type SegmentedControlSize = SegmentedNavSize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SegmentedControlLayout {
    #[default]
    Hug,
    Fill,
}

impl SegmentedNavSize {
    fn height(&self) -> Pixels {
        match self {
            Self::Sm => px(28.0),
            Self::Md => px(32.0),
            Self::Lg => px(36.0),
        }
    }

    fn item_height(&self) -> Pixels {
        self.height() - px(4.0)
    }

    fn text_size(&self) -> Pixels {
        match self {
            Self::Sm => px(12.0),
            Self::Md | Self::Lg => px(14.0),
        }
    }

    fn padding_x(&self) -> Pixels {
        match self {
            Self::Sm => px(8.0),
            Self::Md => px(12.0),
            Self::Lg => px(16.0),
        }
    }
}

pub struct SegmentedNavState {
    active: SharedString,
    previous_active: Option<SharedString>,
    items: Vec<SegmentedNavItem>,
}

impl SegmentedNavState {
    pub fn new(active: impl Into<SharedString>) -> Self {
        Self {
            active: active.into(),
            previous_active: None,
            items: Vec::new(),
        }
    }

    pub fn set_active(&mut self, id: impl Into<SharedString>, cx: &mut Context<Self>) {
        let new_id = id.into();
        if self.active != new_id {
            self.previous_active = Some(self.active.clone());
            self.active = new_id;
            cx.notify();
        }
    }

    pub fn active(&self) -> &SharedString {
        &self.active
    }

    fn _active_index(&self) -> Option<usize> {
        self.items.iter().position(|item| item.id == self.active)
    }
}

#[derive(IntoElement)]
pub struct SegmentedNav {
    id: ElementId,
    state: Entity<SegmentedNavState>,
    items: Vec<SegmentedNavItem>,
    nav_size: SegmentedNavSize,
    layout: SegmentedControlLayout,
    label: Option<SharedString>,
    disabled: bool,
    on_change: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl SegmentedNav {
    pub fn new(id: impl Into<ElementId>, state: Entity<SegmentedNavState>) -> Self {
        Self {
            id: id.into(),
            state,
            items: Vec::new(),
            nav_size: SegmentedNavSize::default(),
            layout: SegmentedControlLayout::Hug,
            label: None,
            disabled: false,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn item(mut self, id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        self.items.push(SegmentedNavItem {
            id: id.into(),
            label: label.into(),
            icon: None,
            label_hidden: false,
            disabled: false,
        });
        self
    }

    pub fn control_item(mut self, item: SegmentedControlItem) -> Self {
        self.items.push(SegmentedNavItem {
            id: item.id,
            label: item.label,
            icon: item.icon,
            label_hidden: item.label_hidden,
            disabled: item.disabled,
        });
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn size(mut self, size: SegmentedNavSize) -> Self {
        self.nav_size = size;
        self
    }

    pub fn layout(mut self, layout: SegmentedControlLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn on_change<F>(mut self, handler: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for SegmentedNav {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn adjacent_enabled_segment(
    items: &[SegmentedNavItem],
    current: usize,
    direction: isize,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }

    for offset in 1..=items.len() {
        let index = (current as isize + direction * offset as isize)
            .rem_euclid(items.len() as isize) as usize;
        if !items[index].disabled {
            return Some(index);
        }
    }
    None
}

impl RenderOnce for SegmentedNav {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let user_style = self.style;
        let state = self.state.read(cx);
        let active_id = state.active.clone();

        self.state.update(cx, |state, _| {
            state.items = self.items.clone();
        });

        let nav_id = self.id.clone();
        let item_focus_handles: Vec<FocusHandle> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let id = ElementId::NamedChild(
                    Box::new(nav_id.clone()),
                    format!("item-{}", index).into(),
                );
                window
                    .use_keyed_state(id, cx, |_, cx| cx.focus_handle())
                    .read(cx)
                    .clone()
            })
            .collect();

        let tokens = &Theme::of(cx).tokens;
        let card = tokens.card;
        let foreground = tokens.foreground;
        let muted_foreground = tokens.muted_foreground;
        let layout = self.layout;
        let disabled = self.disabled;
        let group_label = self
            .label
            .clone()
            .unwrap_or_else(|| "Segmented navigation".into());

        div()
            .id(nav_id.clone())
            .accessibility(
                AccessibilityAttributes::new(AccessibilityRole::Group)
                    .label(group_label.to_string())
                    .disabled(disabled),
            )
            .flex()
            .items_center()
            .gap(px(2.0))
            .bg(tokens.secondary)
            .rounded(tokens.radius_md)
            .p(px(2.0))
            .h(self.nav_size.height())
            .when(disabled, |this| this.opacity(0.5))
            .children(self.items.iter().enumerate().map(|(idx, item)| {
                let item_id = item.id.clone();
                let is_active = item.id == active_id;
                let item_disabled = disabled || item.disabled;
                let on_change = self.on_change.clone();
                let state = self.state.clone();
                let click_id = item_id.clone();
                let item_element_id =
                    ElementId::NamedChild(Box::new(nav_id.clone()), format!("item-{}", idx).into());
                let focus_handle = item_focus_handles[idx].clone();
                let focus_on_mouse = focus_handle.clone();
                let is_focused = focus_handle.is_focused(window);
                let items_for_key = self.items.clone();
                let handles_for_key = item_focus_handles.clone();
                let state_for_key = self.state.clone();
                let on_change_for_key = self.on_change.clone();

                div()
                    .id(item_element_id)
                    .accessibility(
                        AccessibilityAttributes::new(AccessibilityRole::Tab)
                            .label(item.label.to_string())
                            .selected(is_active)
                            .disabled(item_disabled)
                            .focused(is_focused)
                            .actions(if item_disabled {
                                Vec::new()
                            } else {
                                vec![AccessibilityAction::Focus, AccessibilityAction::Click]
                            }),
                    )
                    .when(!item_disabled, |this| {
                        this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
                    })
                    .when(layout == SegmentedControlLayout::Fill, |this| this.flex_1())
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(self.nav_size.item_height())
                    .px(self.nav_size.padding_x())
                    .gap(px(4.0))
                    .rounded((tokens.radius_md - px(2.0)).max(px(0.0)))
                    .text_size(self.nav_size.text_size())
                    .line_height(px(20.0))
                    .font_weight(if is_active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if is_active {
                        foreground
                    } else if item_disabled {
                        muted_foreground.opacity(0.7)
                    } else {
                        muted_foreground
                    })
                    .when(is_active, |this| {
                        this.bg(card).shadow(smallvec::smallvec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.08),
                            offset: point(px(0.0), px(1.0)),
                            blur_radius: px(2.0),
                            spread_radius: px(0.0),
                            inset: false,
                        }])
                    })
                    .when(!is_active && !item_disabled, |this| {
                        this.hover(|style| style.bg(astryx::overlay_hover(false)))
                    })
                    .when(!item_disabled, |this| {
                        this.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            move |_, window, cx| {
                                window.focus(&focus_on_mouse);
                                state.update(cx, |state, cx| {
                                    state.set_active(click_id.clone(), cx);
                                });
                                if let Some(handler) = on_change.as_ref() {
                                    handler(item_id.clone(), window, cx);
                                }
                            },
                        )
                    })
                    .when(!item_disabled, |this| {
                        this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                            let target = match event.keystroke.key.as_str() {
                                "enter" | "space" => Some(idx),
                                "left" | "up" => adjacent_enabled_segment(&items_for_key, idx, -1),
                                "right" | "down" => {
                                    adjacent_enabled_segment(&items_for_key, idx, 1)
                                }
                                "home" => items_for_key.iter().position(|item| !item.disabled),
                                "end" => items_for_key.iter().rposition(|item| !item.disabled),
                                _ => None,
                            };
                            if let Some(target) = target {
                                let target_id = items_for_key[target].id.clone();
                                state_for_key.update(cx, |state, cx| {
                                    state.set_active(target_id.clone(), cx);
                                });
                                window.focus(&handles_for_key[target]);
                                if let Some(handler) = on_change_for_key.as_ref() {
                                    handler(target_id, window, cx);
                                }
                                cx.stop_propagation();
                            }
                        })
                    })
                    .when(item_disabled, |this| this.cursor(CursorStyle::Arrow))
                    .when_some(item.icon.clone(), |this, icon| {
                        this.child(
                            div()
                                .size(match self.nav_size {
                                    SegmentedNavSize::Sm => px(14.0),
                                    SegmentedNavSize::Md => px(16.0),
                                    SegmentedNavSize::Lg => px(18.0),
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .flex_shrink_0()
                                .child(Icon::new(icon).size(match self.nav_size {
                                    SegmentedNavSize::Sm => px(14.0),
                                    SegmentedNavSize::Md => px(16.0),
                                    SegmentedNavSize::Lg => px(18.0),
                                })),
                        )
                    })
                    .when(!item.label_hidden, |this| this.child(item.label.clone()))
            }))
            .map(|this| {
                let mut el = this;
                el.style().refine(&user_style);
                el
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, disabled: bool) -> SegmentedNavItem {
        SegmentedNavItem {
            id: id.to_string().into(),
            label: id.to_string().into(),
            icon: None,
            label_hidden: false,
            disabled,
        }
    }

    #[::core::prelude::v1::test]
    fn keyboard_navigation_wraps_and_skips_disabled_segments() {
        let items = vec![item("one", false), item("two", true), item("three", false)];

        assert_eq!(adjacent_enabled_segment(&items, 0, 1), Some(2));
        assert_eq!(adjacent_enabled_segment(&items, 2, 1), Some(0));
        assert_eq!(adjacent_enabled_segment(&items, 0, -1), Some(2));
    }
}
