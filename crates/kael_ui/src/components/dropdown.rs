use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

fn next_enabled_index(
    enabled_indices: &[usize],
    current: Option<usize>,
    move_up: bool,
) -> Option<usize> {
    if enabled_indices.is_empty() {
        return None;
    }

    let current_position = current.and_then(|index| {
        enabled_indices
            .iter()
            .position(|candidate| *candidate == index)
    });
    let next_position = if move_up {
        current_position
            .map(|position| {
                if position == 0 {
                    enabled_indices.len() - 1
                } else {
                    position - 1
                }
            })
            .unwrap_or(enabled_indices.len() - 1)
    } else {
        current_position
            .map(|position| (position + 1) % enabled_indices.len())
            .unwrap_or(0)
    };
    Some(enabled_indices[next_position])
}

#[derive(Clone)]
pub struct DropdownItem {
    id: SharedString,
    label: SharedString,
    description: Option<SharedString>,
    icon: Option<IconSource>,
    end_content: Option<SharedString>,
    disabled: bool,
    destructive: bool,
    on_click: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
}

pub type DropdownMenuItem = DropdownItem;
pub type DropdownMenuDivider = DropdownItem;
pub type DropdownMenuOption = DropdownItem;
pub type DropdownMenuSection = Vec<DropdownItem>;
pub type DropdownMenuItemData = DropdownItem;

impl DropdownItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
            icon: None,
            end_content: None,
            disabled: false,
            destructive: false,
            on_click: None,
        }
    }

    pub fn separator() -> Self {
        Self {
            id: "__separator__".into(),
            label: "".into(),
            description: None,
            icon: None,
            end_content: None,
            disabled: true,
            destructive: false,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
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

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn is_separator(&self) -> bool {
        self.id.as_ref() == "__separator__"
    }
}

pub struct DropdownState {
    open: bool,
    highlighted_index: Option<usize>,
    focus_handle: FocusHandle,
    trigger_bounds: Bounds<Pixels>,
    menu_bounds: Bounds<Pixels>,
    scroll_handle: ScrollHandle,
    open_generation: u64,
    keyboard_press: Option<KeyboardButton>,
    focus_subscription: Option<(AnyWindowHandle, Subscription)>,
    item_accessibility_ids: HashMap<SharedString, AccessibilityId>,
}

impl DropdownState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            highlighted_index: None,
            focus_handle: cx.focus_handle(),
            trigger_bounds: Bounds::default(),
            menu_bounds: Bounds::default(),
            scroll_handle: ScrollHandle::new(),
            open_generation: 0,
            keyboard_press: None,
            focus_subscription: None,
            item_accessibility_ids: HashMap::new(),
        }
    }

    /// Keep the highlighted item inside the visible rows of the menu
    /// viewport. Row geometry mirrors the render budget: 28px rows with 4px
    /// top padding and a 360px cap; a negative y offset means the menu is
    /// scrolled down.
    pub fn scroll_highlighted_into_view(&self) {
        const ROW_HEIGHT: f32 = 28.0;
        const TOP_PADDING: f32 = 4.0;
        const MAX_HEIGHT: f32 = 360.0;

        let Some(position) = self.highlighted_index else {
            return;
        };
        let row_top = TOP_PADDING + position as f32 * ROW_HEIGHT;
        let row_bottom = row_top + ROW_HEIGHT;
        let scrolled = -f32::from(self.scroll_handle.offset().y);

        let new_scrolled = if row_top < scrolled {
            (row_top - TOP_PADDING).max(0.0)
        } else if row_bottom > scrolled + MAX_HEIGHT {
            row_bottom - MAX_HEIGHT
        } else {
            return;
        };
        self.scroll_handle
            .set_offset(point(px(0.0), px(-new_scrolled.max(0.0))));
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.close(cx);
        } else {
            self.open(cx);
        }
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            self.open_generation = self.open_generation.wrapping_add(1);
            self.keyboard_press = None;
        }
        self.open = true;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.highlighted_index = None;
        self.keyboard_press = None;
        self.menu_bounds = Bounds::default();
        cx.notify();
    }
}

impl Focusable for DropdownState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for DropdownState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropdownAlign {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropdownPlacement {
    Auto,
    Bottom,
    Top,
}

#[derive(IntoElement)]
pub struct Dropdown {
    state: Entity<DropdownState>,
    trigger: AnyElement,
    items: Vec<DropdownItem>,
    align: DropdownAlign,
    placement: DropdownPlacement,
    min_width: Option<Pixels>,
    accessibility_label: SharedString,
    style: StyleRefinement,
}

impl Dropdown {
    pub fn new(state: Entity<DropdownState>, trigger: impl IntoElement) -> Self {
        Self {
            state,
            trigger: trigger.into_any_element(),
            items: Vec::new(),
            align: DropdownAlign::Start,
            placement: DropdownPlacement::Auto,
            min_width: Some(px(180.0)),
            accessibility_label: "Open menu".into(),
            style: StyleRefinement::default(),
        }
    }

    pub fn items(mut self, items: Vec<DropdownItem>) -> Self {
        self.items = items;
        self
    }

    pub fn align(mut self, align: DropdownAlign) -> Self {
        self.align = align;
        self
    }

    pub fn placement(mut self, placement: DropdownPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn min_width(mut self, width: Pixels) -> Self {
        self.min_width = Some(width);
        self
    }

    /// Set the accessible name announced for the menu trigger.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.accessibility_label = label.into();
        self
    }
}

impl Styled for Dropdown {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Dropdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        const GAP: Pixels = px(4.0);
        let user_style = self.style;
        let state = self.state.clone();
        let bound_window = window.window_handle();
        state.update(cx, |state, cx| {
            if state.focus_subscription.as_ref().map(|(handle, _)| *handle) != Some(bound_window) {
                state.focus_subscription = Some((
                    bound_window,
                    cx.on_blur(&state.focus_handle, window, |state, _, _| {
                        // Private key arming has no painted state to invalidate.
                        state.keyboard_press = None;
                    }),
                ));
            }
        });
        let is_open = state.read(cx).open;
        let highlighted_index = state.read(cx).highlighted_index;
        let trigger_bounds = state.read(cx).trigger_bounds;
        let entity_id = state.entity_id().as_u64();
        let focus_handle = state.read(cx).focus_handle.clone();
        let is_focused = focus_handle.is_focused(window);
        let tracked_focus_handle = focus_handle.clone().tab_index(0).tab_stop(true);
        let align = self.align;
        let min_width = self.min_width;

        let measured = trigger_bounds.size.width > px(0.0);
        let viewport = window.viewport_size();
        let est_height = self.items.iter().fold(px(8.0), |acc, item| {
            acc + if item.is_separator() {
                px(9.0)
            } else if item.description.is_some() {
                px(48.0)
            } else {
                px(32.0)
            }
        });
        let open_up = match self.placement {
            DropdownPlacement::Top => true,
            DropdownPlacement::Bottom => false,
            DropdownPlacement::Auto => {
                let space_below = viewport.height - trigger_bounds.bottom();
                let space_above = trigger_bounds.top();
                space_below < est_height + GAP && space_above > space_below
            }
        };
        let (anchor_corner, anchor_pos) = match (open_up, align) {
            (false, DropdownAlign::Start) => {
                (Corner::TopLeft, trigger_bounds.corner(Corner::BottomLeft))
            }
            (false, DropdownAlign::End) => {
                (Corner::TopRight, trigger_bounds.corner(Corner::BottomRight))
            }
            (true, DropdownAlign::Start) => {
                (Corner::BottomLeft, trigger_bounds.corner(Corner::TopLeft))
            }
            (true, DropdownAlign::End) => {
                (Corner::BottomRight, trigger_bounds.corner(Corner::TopRight))
            }
        };
        let use_mb = measured && open_up;
        let items = self.items;
        let keyboard_items = Rc::new(items.clone());
        // Retain authored identities for the latest open item set, without
        // building an ID worklist while closed. The focused Menu owns them.
        let item_accessibility_ids = if is_open {
            state.update(cx, |state, _| {
                let current: HashSet<_> = items
                    .iter()
                    .filter(|item| !item.is_separator())
                    .map(|item| item.id.clone())
                    .collect();
                state
                    .item_accessibility_ids
                    .retain(|id, _| current.contains(id));
                items
                    .iter()
                    .map(|item| {
                        (!item.is_separator()).then(|| {
                            *state
                                .item_accessibility_ids
                                .entry(item.id.clone())
                                .or_insert_with(AccessibilityId::new)
                        })
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            Vec::new()
        };
        let theme = Theme::of(cx);
        let dark = theme.tokens.background.l < 0.5;
        let overlay_hover = crate::astryx::overlay_hover(dark);
        let open_generation = state.read(cx).open_generation;
        let activation_claim = Rc::new(Cell::new(false));
        let enabled_indices: Rc<Vec<usize>> = Rc::new(
            items
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    (!item.disabled && !item.is_separator()).then_some(index)
                })
                .collect(),
        );
        let first_enabled = enabled_indices.first().copied();
        let highlighted_index = highlighted_index
            .filter(|index| enabled_indices.contains(index))
            .or_else(|| if is_open { first_enabled } else { None });
        let activate_highlighted = {
            let state = state.clone();
            let items = keyboard_items.clone();
            let claim = activation_claim.clone();
            let enabled_indices = enabled_indices.clone();
            Rc::new(move |window: &mut Window, cx: &mut App| {
                let current = state.read(cx);
                if !current.open || current.open_generation != open_generation {
                    return;
                }
                let handler = current
                    .highlighted_index
                    .filter(|index| enabled_indices.contains(index))
                    .or(first_enabled)
                    .and_then(|index| items.get(index))
                    .filter(|item| !item.disabled && !item.is_separator())
                    .and_then(|item| item.on_click.clone());
                if claim.replace(true) {
                    return;
                }
                if let Some(handler) = handler {
                    handler(window, cx);
                }
                state.update(cx, |state, cx| state.close(cx));
            })
        };
        let key_up_handler = {
            let state = state.clone();
            let focus_handle = focus_handle.clone();
            let activate_highlighted = activate_highlighted.clone();
            Rc::new(
                move |event: &KeyUpEvent, window: &mut Window, cx: &mut App| {
                    let button = match event.keystroke.key.as_str() {
                        "enter" => KeyboardButton::Enter,
                        "space" => KeyboardButton::Space,
                        _ => return,
                    };
                    let armed =
                        state.update(cx, |state, _| state.keyboard_press.take() == Some(button));
                    let valid = armed
                        && !event.keystroke.modifiers.modified()
                        && focus_handle.is_focused(window);
                    if is_open {
                        if valid {
                            activate_highlighted(window, cx);
                        }
                        // The Menu commits explicitly, never through a second bridge.
                        cx.stop_propagation();
                        window.prevent_default();
                    } else if !valid {
                        // Preserve ordinary native Click, which is not a KeyUpEvent,
                        // while rejecting unpaired physical release on the trigger.
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                },
            )
        };
        let mut trigger_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if is_focused && !is_open {
            trigger_state |= AccessibilityState::FOCUSED;
        }
        let trigger_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(self.accessibility_label.to_string())
            .states(trigger_state)
            .actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::Click,
                if is_open {
                    AccessibilityAction::Collapse
                } else {
                    AccessibilityAction::Expand
                },
            ]);
        let key_handler = {
            let state = state.clone();
            Rc::new(
                move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                    if event.keystroke.modifiers.modified() {
                        return;
                    }
                    let key = event.keystroke.key.as_str();
                    let handled = match key {
                        "escape" => {
                            state.update(cx, |s, cx| s.close(cx));
                            true
                        }
                        // Arming survives a redraw between the two halves.
                        "space" | "enter" => {
                            state.update(cx, |state, _| {
                                state.keyboard_press = Some(if key == "enter" {
                                    KeyboardButton::Enter
                                } else {
                                    KeyboardButton::Space
                                });
                            });
                            true
                        }
                        "up" | "down" => {
                            if enabled_indices.is_empty() {
                                true
                            } else {
                                state.update(cx, |s, cx| {
                                    if !s.open {
                                        s.open(cx);
                                    }
                                    s.highlighted_index = next_enabled_index(
                                        &enabled_indices,
                                        s.highlighted_index,
                                        key == "up",
                                    );
                                    s.scroll_highlighted_into_view();
                                    cx.notify();
                                });
                                true
                            }
                        }
                        _ => false,
                    };
                    if handled {
                        cx.stop_propagation();
                        window.prevent_default();
                    }
                },
            )
        };

        div()
            .relative()
            .on_mouse_down_out({
                let state = state.clone();
                move |event, _, cx| {
                    state.update(cx, |s, cx| {
                        if s.open && !s.menu_bounds.contains(&event.position) {
                            s.close(cx);
                        }
                    });
                }
            })
            .child(
                div()
                    .id(("dropdown-trigger", entity_id))
                    .relative()
                    .key_context("Dropdown")
                    .when(!is_open, |trigger| {
                        trigger.track_focus(&tracked_focus_handle)
                    })
                    .on_key_down({
                        let key_handler = key_handler.clone();
                        move |event, window, cx| key_handler(event, window, cx)
                    })
                    .capture_key_up({
                        let key_up_handler = key_up_handler.clone();
                        move |event, window, cx| key_up_handler(event, window, cx)
                    })
                    .accessibility(trigger_accessibility)
                    .on_accessibility_action(AccessibilityAction::Focus, {
                        let state = state.clone();
                        let focus_handle = focus_handle.clone();
                        move |_, window, cx| {
                            if state.read(cx).open {
                                state.update(cx, |state, cx| state.close(cx));
                            }
                            window.focus(&focus_handle);
                        }
                    })
                    .on_accessibility_action(AccessibilityAction::Expand, {
                        let state = state.clone();
                        let focus_handle = focus_handle.clone();
                        move |_, window, cx| {
                            window.focus(&focus_handle);
                            state.update(cx, |state, cx| {
                                if !state.open {
                                    state.open(cx);
                                    state.highlighted_index = first_enabled;
                                    state.scroll_highlighted_into_view();
                                }
                            });
                        }
                    })
                    .on_accessibility_action(AccessibilityAction::Collapse, {
                        let state = state.clone();
                        let focus_handle = focus_handle.clone();
                        move |_, window, cx| {
                            if state.read(cx).open {
                                state.update(cx, |state, cx| state.close(cx));
                            }
                            window.focus(&focus_handle);
                        }
                    })
                    .cursor_pointer()
                    .on_click({
                        let state = state.clone();
                        let activate_highlighted = activate_highlighted.clone();
                        move |event, window, cx| {
                            window.focus(&focus_handle);
                            if event.is_keyboard() && state.read(cx).open {
                                activate_highlighted(window, cx);
                            } else {
                                state.update(cx, |s, cx| {
                                    s.toggle(cx);
                                    if s.open {
                                        s.highlighted_index = first_enabled;
                                        s.scroll_highlighted_into_view();
                                    }
                                });
                            }
                        }
                    })
                    .child(self.trigger)
                    .child({
                        let state = state.clone();
                        canvas_with_prepaint(
                            move |bounds, _, cx| {
                                state.update(cx, |s, _| {
                                    s.trigger_bounds = bounds;
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full()
                    }),
            )
            .when(is_open, |this| {
                let mut anchor = anchored().snap_to_window_with_margin(Edges::all(GAP));
                if measured {
                    anchor = anchor.anchor(anchor_corner).position(anchor_pos);
                }
                this.child(
                    deferred(
                        anchor.child(
                            div()
                                .id(("dropdown-menu", entity_id))
                                .relative()
                                .child({
                                    let state = state.downgrade();
                                    canvas_with_prepaint(
                                        move |bounds, _, cx| {
                                            _ = state.update(cx, |state, _| {
                                                state.menu_bounds = bounds;
                                            });
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0()
                                })
                                .accessibility({
                                    let mut attributes =
                                        AccessibilityAttributes::new(AccessibilityRole::Menu)
                                            .label("Menu")
                                            .states(if is_focused {
                                                AccessibilityState::FOCUSED
                                            } else {
                                                AccessibilityState::NONE
                                            })
                                            .actions(vec![AccessibilityAction::Focus]);
                                    if let Some(id) = highlighted_index
                                        .and_then(|index| item_accessibility_ids[index])
                                    {
                                        attributes = attributes.active_descendant(id);
                                    }
                                    attributes
                                })
                                .track_focus(&tracked_focus_handle)
                                .key_context("Dropdown")
                                .on_key_down({
                                    let key_handler = key_handler.clone();
                                    move |event, window, cx| key_handler(event, window, cx)
                                })
                                .capture_key_up({
                                    let key_up_handler = key_up_handler.clone();
                                    move |event, window, cx| key_up_handler(event, window, cx)
                                })
                                .occlude()
                                .when(use_mb, |d| d.mb(GAP))
                                .when(!use_mb, |d| d.mt(GAP))
                                .when_some(min_width, |d, w| d.min_w(w))
                                .bg(theme.tokens.popover)
                                .rounded(theme.tokens.radius_lg)
                                .shadow(theme.tokens.shadow_md.to_vec())
                                .p(px(4.0))
                                .gap(px(2.0))
                                .max_h(px(360.0))
                                .overflow_y_scroll()
                                .track_scroll(&state.read(cx).scroll_handle)
                                .children(items.iter().enumerate().map(|(item_index, item)| {
                                    if item.is_separator() {
                                        return div()
                                            .accessibility(AccessibilityAttributes::new(
                                                AccessibilityRole::Separator,
                                            ))
                                            .h(px(1.0))
                                            .my(px(4.0))
                                            .bg(theme.tokens.border)
                                            .into_any_element();
                                    }

                                    let text_color = if item.disabled {
                                        theme.tokens.muted_foreground
                                    } else if item.destructive {
                                        theme.tokens.destructive
                                    } else {
                                        theme.tokens.foreground
                                    };

                                    let hover_bg = if item.destructive {
                                        theme.tokens.destructive.opacity(0.1)
                                    } else {
                                        overlay_hover
                                    };

                                    let on_click = item.on_click.clone();
                                    let state_for_click = state.clone();
                                    let disabled = item.disabled;
                                    let activation_claim = activation_claim.clone();
                                    let highlighted = highlighted_index == Some(item_index);
                                    let mut item_state = AccessibilityState::NONE;
                                    if disabled {
                                        item_state |= AccessibilityState::DISABLED;
                                    }

                                    div()
                                        .id(ElementId::Name(
                                            format!("dropdown-{entity_id}-{}", item.id).into(),
                                        ))
                                        .accessibility({
                                            let mut attributes = AccessibilityAttributes::new(
                                                AccessibilityRole::MenuItem,
                                            )
                                            .id(item_accessibility_ids[item_index]
                                                .expect("non-separator has an identity"))
                                            .label(item.label.to_string())
                                            .states(item_state);
                                            if let Some(description) = item.description.as_ref() {
                                                attributes =
                                                    attributes.description(description.to_string());
                                            }
                                            if !disabled {
                                                attributes = attributes
                                                    .actions(vec![AccessibilityAction::Click]);
                                            }
                                            attributes
                                        })
                                        .flex()
                                        .items_center()
                                        .gap(px(8.0))
                                        .px(px(8.0))
                                        .py(px(6.0))
                                        .rounded(theme.tokens.radius_md)
                                        .text_size(px(14.0))
                                        .text_color(text_color)
                                        .font_family(theme.tokens.font_family.clone())
                                        .transition(theme.tokens.transition_fast)
                                        .when(highlighted, |d| d.bg(hover_bg))
                                        .when(disabled, |d| d.opacity(0.5))
                                        .when(!disabled, |d| {
                                            d.cursor_pointer().hover(move |s| s.bg(hover_bg))
                                        })
                                        .when(!disabled, move |d| {
                                            d.on_click(move |_, window, cx| {
                                                let current = state_for_click.read(cx);
                                                if !current.open
                                                    || current.open_generation != open_generation
                                                    || activation_claim.replace(true)
                                                {
                                                    return;
                                                }
                                                if let Some(ref handler) = on_click {
                                                    handler(window, cx);
                                                }
                                                state_for_click.update(cx, |s, cx| s.close(cx));
                                            })
                                        })
                                        .when_some(item.icon.as_ref(), |d, icon| {
                                            d.child(
                                                div()
                                                    .size(px(20.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .flex_shrink_0()
                                                    .child(
                                                        Icon::new(icon.clone())
                                                            .size(px(16.0))
                                                            .color(text_color),
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
                                                        .line_height(px(20.0))
                                                        .overflow_hidden()
                                                        .text_ellipsis()
                                                        .whitespace_nowrap()
                                                        .text_color(text_color)
                                                        .child(item.label.clone()),
                                                )
                                                .when_some(
                                                    item.description.clone(),
                                                    |d, description| {
                                                        d.child(
                                                            div()
                                                                .text_size(px(12.0))
                                                                .line_height(px(16.0))
                                                                .text_color(
                                                                    theme.tokens.muted_foreground,
                                                                )
                                                                .child(description),
                                                        )
                                                    },
                                                ),
                                        )
                                        .when_some(item.end_content.clone(), |d, content| {
                                            d.child(
                                                div()
                                                    .ml_auto()
                                                    .flex_shrink_0()
                                                    .text_size(px(12.0))
                                                    .line_height(px(16.0))
                                                    .text_color(theme.tokens.muted_foreground)
                                                    .child(content),
                                            )
                                        })
                                        .into_any_element()
                                })),
                        ),
                    )
                    .with_priority(1),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Dropdown, DropdownItem, DropdownState, next_enabled_index};
    use kael::{
        AppContext as _, Context, Entity, IntoElement, KeyUpEvent, Keystroke, ParentElement as _,
        Render, TestAppContext, Window, div, px,
    };
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn keyboard_navigation_skips_disabled_and_separator_indices() {
        let enabled = [1, 3, 6];
        assert_eq!(next_enabled_index(&enabled, None, false), Some(1));
        assert_eq!(next_enabled_index(&enabled, Some(1), false), Some(3));
        assert_eq!(next_enabled_index(&enabled, Some(3), true), Some(1));
    }

    #[test]
    fn keyboard_navigation_wraps_and_handles_empty_menus() {
        let enabled = [2, 5];
        assert_eq!(next_enabled_index(&enabled, Some(5), false), Some(2));
        assert_eq!(next_enabled_index(&enabled, Some(2), true), Some(5));
        assert_eq!(next_enabled_index(&[], None, false), None);
    }

    #[test]
    fn scroll_highlighted_into_view_follows_keyboard_highlight() {
        let cx = kael::TestAppContext::single();
        let state = cx.update(|cx| cx.new(DropdownState::new));

        // No highlight yet: no scroll state changes.
        cx.update(|cx| state.read(cx).scroll_highlighted_into_view());
        cx.update(|cx| {
            assert_eq!(state.read(cx).scroll_handle.offset().y, px(0.0));
        });

        // Highlight below the 360px viewport (row 14 and later) scrolls down.
        cx.update(|cx| {
            state.update(cx, |state, _| {
                state.highlighted_index = Some(14);
            });
            state.read(cx).scroll_highlighted_into_view();
            assert_eq!(state.read(cx).scroll_handle.offset().y, px(-64.0));
        });

        // Returning to the top row scrolls back to the top.
        cx.update(|cx| {
            state.update(cx, |state, _| {
                state.highlighted_index = Some(0);
            });
            state.read(cx).scroll_highlighted_into_view();
            assert_eq!(state.read(cx).scroll_handle.offset().y, px(0.0));
        });
    }

    struct KeyboardDropdownHost {
        state: Entity<DropdownState>,
        first: Rc<Cell<usize>>,
        second: Rc<Cell<usize>>,
    }

    impl Render for KeyboardDropdownHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let first = self.first.clone();
            let second = self.second.clone();
            Dropdown::new(self.state.clone(), div().child("Open actions")).items(vec![
                DropdownItem::new("first", "First")
                    .on_click(move |_, _| first.set(first.get() + 1)),
                DropdownItem::new("second", "Second")
                    .on_click(move |_, _| second.set(second.get() + 1)),
            ])
        }
    }

    #[test]
    fn keyboard_click_sequence_opens_and_commits_exactly_once() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(DropdownState::new);
        let first = Rc::new(Cell::new(0));
        let second = Rc::new(Cell::new(0));
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            let first = first.clone();
            let second = second.clone();
            move |_, _| KeyboardDropdownHost {
                state,
                first,
                second,
            }
        });
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("tab enter");
        assert!(
            !window.update(|_, cx| state.read(cx).is_open()),
            "key-down alone must not toggle before the standard click"
        );
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("valid keystroke"),
        });
        assert!(window.update(|_, cx| state.read(cx).is_open()));

        window.simulate_keystrokes("down enter");
        assert!(window.update(|_, cx| state.read(cx).is_open()));
        assert_eq!(first.get(), 0);
        assert_eq!(second.get(), 0, "key-down must not commit early");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("valid keystroke"),
        });
        assert!(!window.update(|_, cx| state.read(cx).is_open()));
        assert_eq!(first.get(), 0);
        assert_eq!(second.get(), 1, "the highlighted action must run once");
    }
}

#[cfg(test)]
mod native_contract_tests {
    use crate::prelude::{Dropdown, DropdownItem, DropdownState};
    use kael::{
        AccessibilityAction, AccessibilityActionRequest, AccessibilityId, AccessibilityRole,
        AccessibilityState, AppContext as _, Context, Entity, IntoElement, KeyUpEvent, Keystroke,
        Modifiers, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext,
        Window, div, point, px,
    };
    use std::{cell::Cell, rc::Rc};

    struct Host {
        state: Entity<DropdownState>,
        first: Rc<Cell<usize>>,
        second: Rc<Cell<usize>>,
        disabled: Rc<Cell<usize>>,
        disable_first: bool,
        omit_first: bool,
        inset: kael::Pixels,
    }
    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let first = self.first.clone();
            let second = self.second.clone();
            let state_for_second = self.state.clone();
            let disabled = self.disabled.clone();
            div().w(px(300.0)).p(self.inset).child(
                Dropdown::new(self.state.clone(), div().h(px(36.0)).child("Open actions"))
                    .accessibility_label("Actions")
                    .items({
                        let mut items = vec![
                            DropdownItem::new("first", "First")
                                .disabled(self.disable_first)
                                .on_click(move |_, _| first.set(first.get() + 1)),
                            DropdownItem::new("disabled", "Disabled")
                                .disabled(true)
                                .on_click(move |_, _| disabled.set(disabled.get() + 1)),
                            DropdownItem::separator(),
                            DropdownItem::new("second", "Second").on_click(move |_, cx| {
                                assert!(
                                    state_for_second.read(cx).is_open(),
                                    "callback runs before Dropdown's own close"
                                );
                                second.set(second.get() + 1);
                            }),
                        ];
                        if self.omit_first {
                            items.remove(0);
                        }
                        items
                    }),
            )
        }
    }
    struct Fixture {
        host: Entity<Host>,
        state: Entity<DropdownState>,
        window: VisualTestContext,
        first: Rc<Cell<usize>>,
        second: Rc<Cell<usize>>,
        disabled: Rc<Cell<usize>>,
    }
    fn mounted() -> Fixture {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(DropdownState::new);
        let first = Rc::new(Cell::new(0));
        let second = Rc::new(Cell::new(0));
        let disabled = Rc::new(Cell::new(0));
        let (host, window) = cx.add_window_view({
            let state = state.clone();
            let first = first.clone();
            let second = second.clone();
            let disabled = disabled.clone();
            move |_, _| Host {
                state,
                first,
                second,
                disabled,
                disable_first: false,
                omit_first: false,
                inset: px(0.0),
            }
        });
        window.update(|w, cx| w.draw(cx).clear());
        Fixture {
            host,
            state,
            window: window.clone(),
            first,
            second,
            disabled,
        }
    }
    fn node(window: &Window, role: AccessibilityRole, label: &str) -> AccessibilityId {
        window
            .accessibility_tree()
            .nodes
            .values()
            .find(|n| n.role == role && n.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing {role:?} {label}"))
            .id
    }
    fn trigger(f: &mut Fixture) -> AccessibilityId {
        f.window
            .update(|w, _| node(w, AccessibilityRole::Button, "Actions"))
    }
    fn dispatch(f: &mut Fixture, id: AccessibilityId, action: AccessibilityAction) {
        f.window.update(|w, _| {
            w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(id, action))
        });
        f.window.run_until_parked();
    }
    fn draw(f: &mut Fixture) {
        f.window.update(|w, cx| w.draw(cx).clear());
    }
    fn open_pointer(f: &mut Fixture) {
        let center = f.window.update(|w, _| {
            let b = w
                .accessibility_tree()
                .get(node(w, AccessibilityRole::Button, "Actions"))
                .unwrap()
                .bounds
                .unwrap();
            point(
                px((b.x + b.width / 2.0) as f32),
                px((b.y + b.height / 2.0) as f32),
            )
        });
        f.window.simulate_click(center, Modifiers::default());
        f.window.run_until_parked();
        draw(f);
        assert!(f.window.update(|_, cx| f.state.read(cx).is_open()));
    }
    fn open(f: &mut Fixture) -> bool {
        f.window.update(|_, cx| f.state.read(cx).is_open())
    }

    #[test]
    fn native_expand_is_registered_and_idempotently_opens() {
        let mut f = mounted();
        let id = trigger(&mut f);
        f.window.update(|w, _| {
            assert!(
                w.accessibility_tree()
                    .get(id)
                    .unwrap()
                    .actions
                    .contains(&AccessibilityAction::Expand)
            );
            eprintln!(
                "EXPAND_REGISTERED={}",
                w.has_accessibility_action_handler(id, AccessibilityAction::Expand)
            );
            for _ in 0..2 {
                w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Expand,
                ));
            }
        });
        f.window.run_until_parked();
        assert!(
            open(&mut f),
            "two actual native Expand requests must leave the menu open"
        );
        assert_eq!((f.first.get(), f.second.get()), (0, 0));
    }
    #[test]
    fn native_collapse_is_registered_and_idempotently_closes() {
        let mut f = mounted();
        open_pointer(&mut f);
        let id = trigger(&mut f);
        f.window.update(|w, _| {
            assert!(
                w.accessibility_tree()
                    .get(id)
                    .unwrap()
                    .actions
                    .contains(&AccessibilityAction::Collapse)
            );
            eprintln!(
                "COLLAPSE_REGISTERED={}",
                w.has_accessibility_action_handler(id, AccessibilityAction::Collapse)
            );
            for _ in 0..2 {
                w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Collapse,
                ));
            }
        });
        f.window.run_until_parked();
        assert!(
            !open(&mut f),
            "two actual native Collapse requests must leave the menu closed"
        );
        assert_eq!((f.first.get(), f.second.get()), (0, 0));
    }
    #[test]
    fn opened_menu_has_one_focus_owner_and_valid_active_item() {
        let mut f = mounted();
        open_pointer(&mut f);
        f.window.update(|w, _| {
            let tree = w.accessibility_tree();
            eprintln!(
                "FOCUSED={:?}",
                tree.nodes
                    .values()
                    .filter(|n| n.states.contains(AccessibilityState::FOCUSED))
                    .map(|n| (n.id, n.role, n.label.clone(), n.parent, n.active_descendant))
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                tree.focused_node_count(),
                1,
                "only the actual keyboard owner may carry FOCUSED"
            );
            let owner = tree.get(tree.focused_node().unwrap()).unwrap();
            let active = tree
                .get(
                    owner
                        .active_descendant
                        .expect("focused menu control needs active descendant"),
                )
                .unwrap();
            assert_eq!(active.label.as_deref(), Some("First"));
            assert_eq!(active.role, AccessibilityRole::MenuItem);
            assert_eq!(owner.role, AccessibilityRole::Menu);
            let exported = tree.to_accesskit_tree_update(None, None);
            let native = exported
                .nodes
                .iter()
                .find(|(id, _)| id.0 == owner.id.0)
                .unwrap();
            assert_eq!(native.1.active_descendant().unwrap().0, active.id.0);
            assert!(native.1.children().iter().any(|id| id.0 == active.id.0));

            assert!(!active.states.contains(AccessibilityState::FOCUSED));
            let mut parent = active.parent;
            let mut found = false;
            while let Some(id) = parent {
                if id == owner.id {
                    found = true;
                    break;
                }
                parent = tree.get(id).and_then(|n| n.parent);
            }
            assert!(
                found,
                "active item must be an actual logical descendant of its focus owner"
            );
        });
    }
    #[test]
    fn native_focus_and_click_bridges_open_and_disabled_item_rejects() {
        let mut f = mounted();
        let id = trigger(&mut f);
        dispatch(&mut f, id, AccessibilityAction::Focus);
        draw(&mut f);
        f.window
            .update(|w, _| assert_eq!(w.accessibility_tree().focused_node(), Some(id)));
        dispatch(&mut f, id, AccessibilityAction::Click);
        draw(&mut f);
        assert!(open(&mut f));
        let disabled = f.window.update(|w, _| {
            let id = node(w, AccessibilityRole::MenuItem, "Disabled");
            assert!(
                w.accessibility_tree()
                    .get(id)
                    .unwrap()
                    .states
                    .contains(AccessibilityState::DISABLED)
            );
            assert!(!w.has_accessibility_action_handler(id, AccessibilityAction::Click));
            id
        });
        dispatch(&mut f, disabled, AccessibilityAction::Click);
        assert!(open(&mut f));
        assert_eq!(f.disabled.get(), 0);
    }
    #[test]
    fn duplicate_native_item_clicks_commit_only_once_then_retire() {
        let mut f = mounted();
        open_pointer(&mut f);
        let id = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "Second"));
        f.window.update(|w, _| {
            for _ in 0..2 {
                w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                    id,
                    AccessibilityAction::Click,
                ));
            }
        });
        f.window.run_until_parked();
        assert!(!open(&mut f));
        assert_eq!(
            f.second.get(),
            1,
            "same rendered menu session can commit only once"
        );
        draw(&mut f);
        dispatch(&mut f, id, AccessibilityAction::Click);
        assert_eq!(f.second.get(), 1);
        open_pointer(&mut f);
        let fresh = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "Second"));
        dispatch(&mut f, fresh, AccessibilityAction::Click);
        assert_eq!(f.second.get(), 2, "fresh reopen permits a fresh action");
    }
    #[test]
    fn queued_item_action_after_public_close_does_not_activate() {
        let mut f = mounted();
        open_pointer(&mut f);
        let id = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "First"));
        f.window.update(|w, cx| {
            w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Click,
            ));
            f.state.update(cx, |s, cx| s.close(cx));
            assert!(
                w.accessibility_tree().get(id).is_some(),
                "old painted node still present at queueing checkpoint"
            );
        });
        f.window.run_until_parked();
        assert_eq!(
            f.first.get(),
            0,
            "closed menu session must reject its pending item action"
        );
    }
    #[test]
    fn dynamic_disabled_repaint_rejects_previously_queued_native_item() {
        let mut f = mounted();
        open_pointer(&mut f);
        let id = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "First"));
        f.window.update(|w, cx| {
            w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Click,
            ));
            f.host.update(cx, |host, cx| {
                host.disable_first = true;
                cx.notify();
            });
            w.draw(cx).clear();
            assert!(
                w.accessibility_tree()
                    .get(id)
                    .unwrap()
                    .states
                    .contains(AccessibilityState::DISABLED)
            );
        });
        f.window.run_until_parked();
        assert_eq!(f.first.get(), 0);
    }
    #[test]
    fn ordinary_pointer_and_keyboard_commit_preserve_callback_counts() {
        let mut f = mounted();
        open_pointer(&mut f);
        f.window.simulate_keystrokes("down enter");
        assert!(open(&mut f));
        assert_eq!((f.first.get(), f.second.get()), (0, 0));
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert!(!open(&mut f));
        assert_eq!((f.first.get(), f.second.get(), f.disabled.get()), (0, 1, 0));
    }
    #[test]
    fn queued_old_session_item_does_not_commit_after_close_and_fresh_reopen() {
        let mut f = mounted();
        open_pointer(&mut f);
        let id = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "Second"));
        f.window.update(|w, cx| {
            w.dispatch_accessibility_action_for_test(AccessibilityActionRequest::new(
                id,
                AccessibilityAction::Click,
            ));
            f.state.update(cx, |s, cx| {
                s.close(cx);
                s.open(cx);
            });
            assert!(f.state.read(cx).is_open());
            assert!(
                w.accessibility_tree().get(id).is_some(),
                "old painted session still exists at queueing checkpoint"
            );
        });
        f.window.run_until_parked();
        assert_eq!(
            f.second.get(),
            0,
            "retired session action must not select into fresh public reopen"
        );
        assert!(open(&mut f), "fresh reopen must remain available");
        draw(&mut f);
        let fresh = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "Second"));
        dispatch(&mut f, fresh, AccessibilityAction::Click);
        assert_eq!(
            f.second.get(),
            1,
            "fresh visible session still selects exactly once"
        );
    }
    #[test]
    fn dynamic_disable_revalidates_keyboard_highlight_before_commit() {
        let mut f = mounted();
        open_pointer(&mut f);
        f.host.update(&mut f.window, |host, cx| {
            host.disable_first = true;
            cx.notify();
        });
        draw(&mut f);
        f.window.update(|w, _| {
            let id = node(w, AccessibilityRole::MenuItem, "First");
            assert!(
                w.accessibility_tree()
                    .get(id)
                    .unwrap()
                    .states
                    .contains(AccessibilityState::DISABLED)
            );
        });
        f.window.simulate_keystrokes("enter");
        assert_eq!(f.first.get(), 0);
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(
            f.first.get(),
            0,
            "a newly disabled highlighted item must never run its callback"
        );
        assert_eq!(
            f.second.get(),
            1,
            "commit must match the effective first enabled highlight"
        );
        assert!(!open(&mut f));
    }
    #[test]
    fn unpaired_menu_key_up_never_activates() {
        let mut f = mounted();
        open_pointer(&mut f);
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(
            (f.first.get(), f.second.get()),
            (0, 0),
            "unpaired key release is not an activation"
        );
        assert!(open(&mut f));
    }
    #[test]
    fn key_down_rerender_key_up_preserves_exactly_once_activation() {
        let mut f = mounted();
        open_pointer(&mut f);
        f.window.simulate_keystrokes("enter");
        draw(&mut f);
        assert_eq!(f.first.get(), 0);
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(f.first.get(), 1);
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(f.first.get(), 1);
    }
    #[test]
    fn key_down_cannot_arm_a_later_reopened_menu() {
        let mut f = mounted();
        open_pointer(&mut f);
        f.window.simulate_keystrokes("enter");
        f.window.update(|_, cx| {
            f.state.update(cx, |state, cx| {
                state.close(cx);
                state.open(cx);
            })
        });
        draw(&mut f);
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(
            f.first.get(),
            0,
            "old physical press cannot select into reopened session"
        );
        assert!(open(&mut f));
        f.window.simulate_keystrokes("enter");
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(f.first.get(), 1, "new session remains usable");
    }
    #[test]
    fn key_down_retires_after_actual_focus_loss_and_return() {
        use kael::Focusable as _;
        let mut f = mounted();
        f.window.deactivate_window();
        f.window.update(|w, _| w.activate_window());
        f.window.run_until_parked();
        f.window.update(|w, _| assert!(w.is_window_active()));
        open_pointer(&mut f);
        f.window.simulate_keystrokes("enter");
        f.window.update(|w, cx| {
            w.blur();
            w.draw(cx).clear();
        });
        f.window.run_until_parked();
        f.window.update(|w, cx| {
            w.focus(&f.state.read(cx).focus_handle(cx));
            w.draw(cx).clear();
        });
        f.window.run_until_parked();
        f.window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        assert_eq!(f.first.get(), 0, "focus loss must retire key arming");
        assert!(open(&mut f));
    }
    #[test]
    fn native_trigger_focus_closes_popup_and_restores_truthful_button_owner() {
        let mut f = mounted();
        open_pointer(&mut f);
        let id = trigger(&mut f);
        dispatch(&mut f, id, AccessibilityAction::Focus);
        draw(&mut f);
        assert!(!open(&mut f));
        f.window.update(|w, _| {
            assert_eq!(w.accessibility_tree().focused_node_count(), 1);
            assert_eq!(w.accessibility_tree().focused_node(), Some(id));
            assert_eq!(
                w.accessibility_tree().get(id).unwrap().role,
                AccessibilityRole::Button
            );
            assert!(
                !w.accessibility_tree()
                    .nodes
                    .values()
                    .any(|n| n.role == AccessibilityRole::Menu)
            );
        });
        assert_eq!((f.first.get(), f.second.get()), (0, 0));
        dispatch(&mut f, id, AccessibilityAction::Click);
        draw(&mut f);
        assert!(open(&mut f), "ordinary fresh native Click remains usable");
    }
    #[test]
    fn item_identity_is_stable_across_reopen_and_prunes_removed_items() {
        let mut f = mounted();
        open_pointer(&mut f);
        let first = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "First"));
        let second = f
            .window
            .update(|w, _| node(w, AccessibilityRole::MenuItem, "Second"));
        f.window
            .update(|_, cx| f.state.update(cx, |s, cx| s.close(cx)));
        draw(&mut f);
        open_pointer(&mut f);
        f.window.update(|w, cx| {
            assert_eq!(node(w, AccessibilityRole::MenuItem, "First"), first);
            assert_eq!(node(w, AccessibilityRole::MenuItem, "Second"), second);
            assert_eq!(f.state.read(cx).item_accessibility_ids.len(), 3);
        });
        f.host.update(&mut f.window, |host, cx| {
            host.omit_first = true;
            cx.notify();
        });
        draw(&mut f);
        f.window.update(|w, cx| {
            assert!(w.accessibility_tree().get(first).is_none());
            assert_eq!(node(w, AccessibilityRole::MenuItem, "Second"), second);
            assert_eq!(f.state.read(cx).item_accessibility_ids.len(), 2);
            assert!(
                !f.state
                    .read(cx)
                    .item_accessibility_ids
                    .contains_key("first")
            );
        });
        f.host.update(&mut f.window, |host, cx| {
            host.omit_first = false;
            cx.notify();
        });
        draw(&mut f);
        f.window.update(|w, cx| {
            assert_ne!(
                node(w, AccessibilityRole::MenuItem, "First"),
                first,
                "retired item identity must not alias a later reintroduced item"
            );
            assert_eq!(node(w, AccessibilityRole::MenuItem, "Second"), second);
            assert_eq!(f.state.read(cx).item_accessibility_ids.len(), 3);
        });
    }
    #[test]
    fn focus_subscription_and_rendered_actions_do_not_retain_retired_owner() {
        let mut f = mounted();
        open_pointer(&mut f);
        let state = f.state.downgrade();
        let host = f.host.downgrade();
        f.window
            .update(|_, cx| assert!(f.state.read(cx).focus_subscription.is_some()));
        f.window.update(|w, _| w.remove_window());
        drop(f.host);
        drop(f.state);
        // Entity values are released by App's effect flush, not executor drain.
        f.window.cx.update(|_| {});
        f.window.run_until_parked();
        assert!(
            state.upgrade().is_none(),
            "weak blur subscription must not retain state"
        );
        assert!(host.upgrade().is_none(), "callbacks must not retain host");
    }
    #[test]
    fn actual_pointer_item_click_invokes_callback_before_closing() {
        let mut f = mounted();
        open_pointer(&mut f);
        let center = f.window.update(|w, _| {
            let id = node(w, AccessibilityRole::MenuItem, "Second");
            let bounds = w.accessibility_tree().get(id).unwrap().bounds.unwrap();
            point(
                px((bounds.x + bounds.width / 2.0) as f32),
                px((bounds.y + bounds.height / 2.0) as f32),
            )
        });
        f.window.simulate_click(center, Modifiers::default());
        f.window.run_until_parked();
        assert_eq!(
            f.second.get(),
            1,
            "real pointer item activation must survive outside-dismiss routing"
        );
        assert_eq!((f.first.get(), f.disabled.get()), (0, 0));
        assert!(!open(&mut f));
    }
    #[test]
    fn closed_trigger_pairs_enter_and_space_across_redraw_and_escape() {
        for key in ["enter", "space"] {
            let mut f = mounted();
            f.window.simulate_keystrokes("tab");
            f.window.update(|w, cx| {
                assert!(f.state.read(cx).focus_handle.is_focused(w));
                assert!(!f.state.read(cx).is_open());
            });
            f.window.simulate_event(KeyUpEvent {
                keystroke: Keystroke::parse(key).unwrap(),
            });
            assert!(!open(&mut f), "unpaired closed {key} release must not open");
            for repaint in [false, true] {
                f.window.simulate_keystrokes(key);
                assert!(!open(&mut f), "{key} key-down must not open early");
                if repaint {
                    draw(&mut f);
                }
                f.window.simulate_event(KeyUpEvent {
                    keystroke: Keystroke::parse(key).unwrap(),
                });
                assert!(
                    open(&mut f),
                    "paired closed {key} must open once (repaint={repaint})"
                );
                f.window.simulate_event(KeyUpEvent {
                    keystroke: Keystroke::parse(key).unwrap(),
                });
                assert!(
                    open(&mut f),
                    "duplicate {key} release must not commit or close"
                );
                assert_eq!((f.first.get(), f.second.get(), f.disabled.get()), (0, 0, 0));
                f.window.simulate_keystrokes("escape");
                assert!(
                    !open(&mut f),
                    "Escape must close after physical {key} opening"
                );
            }
        }
    }
    #[test]
    fn popup_padding_and_disabled_pointer_item_do_not_dismiss() {
        let mut f = mounted();
        open_pointer(&mut f);
        let (padding, disabled) = f.window.update(|w, cx| {
            let menu = w
                .accessibility_tree()
                .get(node(w, AccessibilityRole::Menu, "Menu"))
                .unwrap()
                .bounds
                .unwrap();
            let first = w
                .accessibility_tree()
                .get(node(w, AccessibilityRole::MenuItem, "First"))
                .unwrap()
                .bounds
                .unwrap();
            let last = w
                .accessibility_tree()
                .get(node(w, AccessibilityRole::MenuItem, "Second"))
                .unwrap()
                .bounds
                .unwrap();
            let disabled = w
                .accessibility_tree()
                .get(node(w, AccessibilityRole::MenuItem, "Disabled"))
                .unwrap()
                .bounds
                .unwrap();
            let measured = f.state.read(cx).menu_bounds;
            assert_eq!(measured.origin, point(px(menu.x as f32), px(menu.y as f32)));
            assert_eq!(
                measured.size,
                kael::size(px(menu.width as f32), px(menu.height as f32))
            );
            assert!(first.y > menu.y, "fixture exposes actual top padding");
            assert!(
                last.y + last.height < menu.y + menu.height,
                "fixture exposes actual bottom padding"
            );
            assert!(first.x > menu.x, "fixture exposes actual left padding");
            assert!(
                first.x + first.width < menu.x + menu.width,
                "fixture exposes actual right padding"
            );
            (
                [
                    point(
                        px((menu.x + menu.width / 2.0) as f32),
                        px(((menu.y + first.y) / 2.0) as f32),
                    ),
                    point(
                        px((menu.x + menu.width / 2.0) as f32),
                        px(((last.y + last.height + menu.y + menu.height) / 2.0) as f32),
                    ),
                    point(
                        px(((menu.x + first.x) / 2.0) as f32),
                        px((first.y + first.height / 2.0) as f32),
                    ),
                    point(
                        px(((first.x + first.width + menu.x + menu.width) / 2.0) as f32),
                        px((first.y + first.height / 2.0) as f32),
                    ),
                ],
                point(
                    px((disabled.x + disabled.width / 2.0) as f32),
                    px((disabled.y + disabled.height / 2.0) as f32),
                ),
            )
        });
        for (edge, point) in padding.into_iter().enumerate() {
            f.window.simulate_click(point, Modifiers::default());
            f.window.run_until_parked();
            assert!(
                open(&mut f),
                "popup padding edge {edge} is inside dismissal boundary"
            );
        }
        f.window.simulate_click(disabled, Modifiers::default());
        f.window.run_until_parked();
        assert!(
            open(&mut f),
            "disabled pointer items do not dismiss or activate"
        );
        assert_eq!((f.first.get(), f.second.get(), f.disabled.get()), (0, 0, 0));
    }
    #[test]
    fn actual_pointer_outside_trigger_and_popup_still_dismisses() {
        let mut f = mounted();
        // Leave real on-screen space on each side for adjacent outside clicks.
        let inset = f.window.update(|w, _| {
            let viewport = w.viewport_size();
            viewport.width.min(viewport.height) / 10.0
        });
        f.host.update(&mut f.window, |host, cx| {
            host.inset = inset;
            cx.notify();
        });
        draw(&mut f);
        for edge in 0..4 {
            open_pointer(&mut f);
            let outside = f.window.update(|w, cx| {
                let state = f.state.read(cx);
                let menu = state.menu_bounds;
                let center = menu.center();
                let outside = match edge {
                    0 => point(center.x, menu.top() - px(1.0)),
                    1 => point(center.x, menu.bottom() + px(1.0)),
                    2 => point(menu.left() - px(1.0), center.y),
                    _ => point(menu.right() + px(1.0), center.y),
                };
                assert!(!state.trigger_bounds.contains(&outside));
                assert!(!menu.contains(&outside));
                assert!(outside.x >= px(0.0) && outside.y >= px(0.0));
                assert!(
                    outside.x < w.viewport_size().width && outside.y < w.viewport_size().height
                );
                outside
            });
            f.window.simulate_click(outside, Modifiers::default());
            f.window.run_until_parked();
            assert!(!open(&mut f), "adjacent outside edge {edge} dismisses");
            assert_eq!((f.first.get(), f.second.get(), f.disabled.get()), (0, 0, 0));
        }
    }
}
