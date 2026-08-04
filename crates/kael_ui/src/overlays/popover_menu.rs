//! Popover menu component with positioned menu items.

use crate::components::icon::Icon;
use crate::theme::Theme;
use kael::prelude::FluentBuilder;
use kael::*;
use std::{panic::Location, rc::Rc};

pub struct PopoverMenuItem {
    pub id: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub icon: Option<SharedString>,
    pub end_content: Option<SharedString>,
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    pub disabled: bool,
}

impl PopoverMenuItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            icon: None,
            end_content: None,
            on_click: None,
            disabled: false,
        }
    }

    pub fn icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn end_content(mut self, content: impl Into<SharedString>) -> Self {
        self.end_content = Some(content.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn endContent(self, content: impl Into<SharedString>) -> Self {
        self.end_content(content)
    }

    pub fn shortcut(self, shortcut: impl Into<SharedString>) -> Self {
        self.end_content(shortcut)
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(IntoElement)]
pub struct PopoverMenu {
    id: ElementId,
    position: Point<Pixels>,
    items: Vec<PopoverMenuItem>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl PopoverMenu {
    #[track_caller]
    pub fn new(position: Point<Pixels>, items: Vec<PopoverMenuItem>) -> Self {
        let caller = Location::caller();
        Self {
            id: ElementId::Name(
                format!(
                    "popover-menu:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            position,
            items,
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    pub fn on_close<F>(mut self, handler: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl Styled for PopoverMenu {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PopoverMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let on_close_backdrop = self.on_close.clone();
        let on_close_menu = self.on_close.clone();
        let user_style = self.style;
        let overlay_hover = crate::astryx::overlay_hover(theme.tokens.background.l < 0.5);
        let menu_id = self.id.clone();
        let position = sanitize_position(self.position);
        let items = self.items;
        let interactive_indices: Rc<Vec<usize>> = Rc::new(
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    (!item.disabled && item.on_click.is_some()).then_some(index)
                })
                .collect(),
        );
        let focus_handles: Rc<Vec<FocusHandle>> = Rc::new(
            (0..items.len())
                .map(|index| {
                    window
                        .use_keyed_state(
                            ElementId::NamedChild(
                                Box::new(menu_id.clone()),
                                format!("item-{index}").into(),
                            ),
                            cx,
                            |_, cx| cx.focus_handle(),
                        )
                        .read(cx)
                        .clone()
                })
                .collect(),
        );
        let menu_focus_handle = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(menu_id.clone()), "focus".into()),
                cx,
                |_, cx| cx.focus_handle(),
            )
            .read(cx)
            .clone();
        let tracked_menu_focus = menu_focus_handle
            .clone()
            .tab_index(0)
            .tab_stop(interactive_indices.is_empty());
        if !focus_handles.iter().any(|handle| handle.is_focused(window)) {
            if let Some(index) = interactive_indices.first() {
                window.focus(&focus_handles[*index]);
            } else {
                window.focus(&menu_focus_handle);
            }
        }

        div()
            .id(ElementId::NamedChild(
                Box::new(menu_id.clone()),
                "backdrop".into(),
            ))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                if let Some(ref handler) = on_close_backdrop {
                    handler(window, cx);
                }
            })
            .child(
                deferred(
                    anchored().snap_to_window().position(position).child(
                        div().occlude().child(
                            div()
                                .id(menu_id.clone())
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Menu)
                                        .label("Menu"),
                                )
                                .track_focus(&tracked_menu_focus)
                                .min_w(px(200.0))
                                .max_w(px(300.0))
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .bg(theme.tokens.popover)
                                .text_color(theme.tokens.popover_foreground)
                                .rounded(theme.tokens.radius_lg)
                                .shadow(theme.tokens.shadow_md.to_vec())
                                .p(px(4.0))
                                .map(|this| {
                                    let mut div = this;
                                    div.style().refine(&user_style);
                                    div
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation();
                                })
                                .when(on_close_menu.is_some(), |this| {
                                    let on_close = on_close_menu.clone();
                                    this.on_key_down(move |event: &KeyDownEvent, window, cx| {
                                        if event.keystroke.key.as_str() == "escape" {
                                            if let Some(handler) = on_close.as_ref() {
                                                handler(window, cx);
                                            }
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }
                                    })
                                })
                                .children(items.into_iter().enumerate().map(|(index, item)| {
                                    render_popover_menu_item(
                                        index,
                                        item,
                                        menu_id.clone(),
                                        focus_handles.clone(),
                                        interactive_indices.clone(),
                                        on_close_menu.clone(),
                                        overlay_hover,
                                        &theme,
                                        window,
                                    )
                                })),
                        ),
                    ),
                )
                .with_priority(1),
            )
    }
}

fn sanitize_position(position: Point<Pixels>) -> Point<Pixels> {
    fn coordinate(value: Pixels) -> Pixels {
        let value = f32::from(value);
        if value.is_finite() {
            px(value.max(0.0))
        } else {
            px(0.0)
        }
    }

    point(coordinate(position.x), coordinate(position.y))
}

fn render_popover_menu_item(
    index: usize,
    item: PopoverMenuItem,
    menu_id: ElementId,
    focus_handles: Rc<Vec<FocusHandle>>,
    interactive_indices: Rc<Vec<usize>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    overlay_hover: Hsla,
    theme: &Theme,
    window: &mut Window,
) -> AnyElement {
    let on_click = item.on_click;
    let interactive = !item.disabled && on_click.is_some();
    let focus_handle = focus_handles[index].clone();
    let focus_on_mouse = focus_handle.clone();
    let handles_for_key = focus_handles.clone();
    let indices_for_key = interactive_indices.clone();
    let on_click_mouse = on_click.clone();
    let on_click_key = on_click.clone();
    let close_mouse = on_close.clone();
    let close_key = on_close;
    let label = item.label.clone();
    let mut state = AccessibilityState::NONE;
    if !interactive {
        state |= AccessibilityState::DISABLED;
    }
    if focus_handle.is_focused(window) {
        state |= AccessibilityState::FOCUSED;
    }
    let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::MenuItem)
        .label(label.to_string())
        .states(state);
    if let Some(description) = item.description.as_ref() {
        accessibility = accessibility.description(description.to_string());
    }
    if interactive {
        accessibility =
            accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
    }

    div()
        .id(ElementId::NamedChild(
            Box::new(menu_id),
            format!("item-{index}").into(),
        ))
        .accessibility(accessibility)
        .when(interactive, |row| {
            row.track_focus(&focus_handle.tab_index(0).tab_stop(true))
        })
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(theme.tokens.radius_md)
        .cursor(if interactive {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .when(interactive, |this| {
            this.hover(move |style| style.bg(overlay_hover))
        })
        .when(!interactive, |this| this.opacity(0.5))
        .when_some(item.icon, |this, icon_name| {
            this.child(
                div()
                    .size(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .child(
                        Icon::new(icon_name)
                            .size(px(16.0))
                            .color(theme.tokens.foreground),
                    ),
            )
        })
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(1.0))
                .overflow_hidden()
                .child(
                    div()
                        .text_size(px(14.0))
                        .line_height(px(20.0))
                        .child(StyledText::new(label).accessibility_hidden(true)),
                )
                .when_some(item.description, |this, description| {
                    this.child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(16.0))
                            .text_color(theme.tokens.muted_foreground)
                            .child(StyledText::new(description).accessibility_hidden(true)),
                    )
                }),
        )
        .when_some(item.end_content, |this, content| {
            this.child(
                div()
                    .ml_auto()
                    .text_size(px(12.0))
                    .line_height(px(16.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child(content),
            )
        })
        .when(interactive, |this| {
            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.focus(&focus_on_mouse);
                if let Some(ref handler) = on_click_mouse {
                    handler(window, cx);
                }
                if let Some(ref handler) = close_mouse {
                    handler(window, cx);
                }
                cx.stop_propagation();
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let current = indices_for_key
                    .iter()
                    .position(|candidate| *candidate == index)
                    .unwrap_or(0);
                let target = match key {
                    "down" if !indices_for_key.is_empty() => {
                        Some((current + 1) % indices_for_key.len())
                    }
                    "up" if !indices_for_key.is_empty() => {
                        Some((current + indices_for_key.len() - 1) % indices_for_key.len())
                    }
                    "home" if !indices_for_key.is_empty() => Some(0),
                    "end" if !indices_for_key.is_empty() => Some(indices_for_key.len() - 1),
                    _ => None,
                };
                if let Some(target) = target {
                    window.focus(&handles_for_key[indices_for_key[target]]);
                    cx.stop_propagation();
                } else if matches!(key, "enter" | "space") {
                    if let Some(ref handler) = on_click_key {
                        handler(window, cx);
                    }
                    if let Some(ref handler) = close_key {
                        handler(window, cx);
                    }
                    cx.stop_propagation();
                } else if key == "escape" {
                    if let Some(ref handler) = close_key {
                        handler(window, cx);
                    }
                    cx.stop_propagation();
                }
            })
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn menu_positions_are_finite_and_inside_the_viewport_origin() {
        assert_eq!(
            sanitize_position(point(px(f32::NAN), px(f32::INFINITY))),
            point(px(0.0), px(0.0))
        );
        assert_eq!(
            sanitize_position(point(px(-20.0), px(30.0))),
            point(px(0.0), px(30.0))
        );
    }
}
