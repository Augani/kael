use crate::components::icon::Icon;
use crate::components::icon_source::IconSource;
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

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
    focus_handle: FocusHandle,
    trigger_bounds: Bounds<Pixels>,
}

impl DropdownState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            focus_handle: cx.focus_handle(),
            trigger_bounds: Bounds::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.notify();
    }

    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
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
}

impl Styled for Dropdown {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Dropdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        const GAP: Pixels = px(4.0);
        let theme = Theme::of(cx);
        let user_style = self.style;
        let state = self.state.clone();
        let is_open = state.read(cx).open;
        let trigger_bounds = state.read(cx).trigger_bounds;
        let dark = theme.tokens.background.l < 0.5;
        let overlay_hover = crate::astryx::overlay_hover(dark);
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

        div()
            .relative()
            .on_mouse_down_out({
                let state = state.clone();
                move |_, _, cx| {
                    state.update(cx, |s, cx| {
                        if s.open {
                            s.close(cx);
                        }
                    });
                }
            })
            .child(
                div()
                    .id("dropdown-trigger")
                    .relative()
                    .cursor_pointer()
                    .on_click({
                        let state = state.clone();
                        move |_, _, cx| {
                            state.update(cx, |s, cx| s.toggle(cx));
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
                                .occlude()
                                .when(use_mb, |d| d.mb(GAP))
                                .when(!use_mb, |d| d.mt(GAP))
                                .when_some(min_width, |d, w| d.min_w(w))
                                .bg(theme.tokens.popover)
                                .rounded(theme.tokens.radius_lg)
                                .shadow(theme.tokens.shadow_md.to_vec())
                                .p(px(4.0))
                                .gap(px(2.0))
                                .children(items.iter().map(|item| {
                                    if item.is_separator() {
                                        return div()
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

                                    div()
                                        .id(item.id.clone())
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
                                        .when(disabled, |d| d.opacity(0.5))
                                        .when(!disabled, |d| {
                                            d.cursor_pointer().hover(move |s| s.bg(hover_bg))
                                        })
                                        .when(!disabled, move |d| {
                                            d.on_click(move |_, window, cx| {
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
