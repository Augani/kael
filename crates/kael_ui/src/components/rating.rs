use crate::components::icon::{Icon, IconSize as IconSizeEnum};
use crate::theme::Theme;
use kael::{prelude::*, *};
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RatingSize {
    Sm,
    Md,
    Lg,
}

impl RatingSize {
    pub fn icon_size(&self) -> Pixels {
        match self {
            RatingSize::Sm => px(16.0),
            RatingSize::Md => px(24.0),
            RatingSize::Lg => px(32.0),
        }
    }

    pub fn gap(&self) -> Pixels {
        match self {
            RatingSize::Sm => px(2.0),
            RatingSize::Md => px(4.0),
            RatingSize::Lg => px(6.0),
        }
    }
}

pub struct RatingState {
    value: f32,
    max_rating: u8,
    allows_half: bool,
    focus_handle: FocusHandle,
    hover_value: Option<f32>,
}

impl RatingState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            value: 0.0,
            max_rating: 5,
            allows_half: false,
            focus_handle: cx.focus_handle(),
            hover_value: None,
        }
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn set_value(&mut self, value: f32, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        let max = self.max_rating as f32;
        let clamped = value.clamp(0.0, max);
        let stepped = if self.allows_half {
            (clamped * 2.0).round() / 2.0
        } else {
            clamped.round()
        };
        if (self.value - stepped).abs() > f32::EPSILON {
            self.value = stepped;
            cx.notify();
        }
    }

    pub fn max_rating(&self) -> u8 {
        self.max_rating
    }

    pub fn set_max_rating(&mut self, max: u8, cx: &mut Context<Self>) {
        let max = max.max(1);
        self.max_rating = max;
        self.value = self.value.min(max as f32);
        cx.notify();
    }

    pub fn allows_half(&self) -> bool {
        self.allows_half
    }

    pub fn set_allows_half(&mut self, allows: bool, cx: &mut Context<Self>) {
        self.allows_half = allows;
        if !allows {
            self.value = self.value.round();
        }
        cx.notify();
    }

    fn set_hover_value(&mut self, value: Option<f32>, cx: &mut Context<Self>) {
        if self.hover_value != value {
            self.hover_value = value;
            cx.notify();
        }
    }

    fn display_value(&self) -> f32 {
        self.hover_value.unwrap_or(self.value)
    }

    fn increment(&mut self, cx: &mut Context<Self>) {
        let step = if self.allows_half { 0.5 } else { 1.0 };
        let new_value = (self.value + step).min(self.max_rating as f32);
        self.set_value(new_value, cx);
    }

    fn decrement(&mut self, cx: &mut Context<Self>) {
        let step = if self.allows_half { 0.5 } else { 1.0 };
        let new_value = (self.value - step).max(0.0);
        self.set_value(new_value, cx);
    }
}

impl Focusable for RatingState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for RatingState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct Rating {
    state: Entity<RatingState>,
    size: RatingSize,
    read_only: bool,
    filled_icon: SharedString,
    empty_icon: SharedString,
    half_icon: SharedString,
    active_color: Option<Hsla>,
    inactive_color: Option<Hsla>,
    on_change: Option<Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

impl Rating {
    pub fn new(state: Entity<RatingState>) -> Self {
        Self {
            state,
            size: RatingSize::Md,
            read_only: false,
            filled_icon: "star".into(),
            empty_icon: "star".into(),
            half_icon: "star-half".into(),
            active_color: None,
            inactive_color: None,
            on_change: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn size(mut self, size: RatingSize) -> Self {
        self.size = size;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn filled_icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.filled_icon = icon.into();
        self
    }

    pub fn empty_icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.empty_icon = icon.into();
        self
    }

    pub fn half_icon(mut self, icon: impl Into<SharedString>) -> Self {
        self.half_icon = icon.into();
        self
    }

    pub fn active_color(mut self, color: Hsla) -> Self {
        self.active_color = Some(color);
        self
    }

    pub fn inactive_color(mut self, color: Hsla) -> Self {
        self.inactive_color = Some(color);
        self
    }

    pub fn on_change(mut self, handler: impl Fn(f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for Rating {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Rating {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tokens = &Theme::of(cx).tokens;
        let muted_foreground = tokens.muted_foreground;
        let radius_sm = tokens.radius_sm;
        let focus_ring = tokens.focus_ring_light();

        let state = self.state.read(cx);
        let focus_handle = state.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let max_rating = state.max_rating;
        let allows_half = state.allows_half;
        let display_value = state.display_value();

        let icon_size = self.size.icon_size();
        let gap = self.size.gap();

        let active_color = self.active_color.unwrap_or(hsla(0.12, 0.9, 0.55, 1.0));
        let inactive_color = self.inactive_color.unwrap_or(muted_foreground.opacity(0.4));

        let user_style = self.style.clone();
        let rating_id: ElementId = ("rating", self.state.entity_id()).into();
        let focus_on_mouse = focus_handle.clone();
        let star_bounds = Rc::new(std::cell::RefCell::new(Vec::<Bounds<Pixels>>::new()));

        div()
            .on_children_prepainted({
                let star_bounds = star_bounds.clone();
                move |bounds, _, _| *star_bounds.borrow_mut() = bounds
            })
            .id(rating_id)
            .accessibility(
                AccessibilityAttributes::slider(
                    "Rating",
                    state.value as f64,
                    0.0,
                    max_rating as f64,
                    Some(if allows_half { 0.5 } else { 1.0 }),
                )
                .disabled(self.read_only)
                .focused(is_focused),
            )
            .flex()
            .items_center()
            .gap(gap)
            .when(!self.read_only, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
            })
            .when(is_focused && !self.read_only, |this| {
                this.rounded(radius_sm)
                    .shadow(smallvec::smallvec![focus_ring])
            })
            .when(!self.read_only, |this| {
                let state_for_key = self.state.clone();
                let on_change_for_key = self.on_change.clone();
                this.on_key_down(window.listener_for(
                    &state_for_key,
                    move |state, e: &KeyDownEvent, window, cx| {
                        let key = e.keystroke.key.as_str();
                        let handled = match key {
                            "left" | "down" => {
                                state.decrement(cx);
                                if let Some(ref handler) = on_change_for_key {
                                    handler(state.value, window, cx);
                                }
                                true
                            }
                            "right" | "up" => {
                                state.increment(cx);
                                if let Some(ref handler) = on_change_for_key {
                                    handler(state.value, window, cx);
                                }
                                true
                            }
                            "home" => {
                                state.set_value(0.0, cx);
                                if let Some(ref handler) = on_change_for_key {
                                    handler(state.value, window, cx);
                                }
                                true
                            }
                            "end" => {
                                state.set_value(state.max_rating as f32, cx);
                                if let Some(ref handler) = on_change_for_key {
                                    handler(state.value, window, cx);
                                }
                                true
                            }
                            _ => false,
                        };
                        if handled {
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    },
                ))
            })
            .when(!self.read_only, |this| {
                this.on_mouse_down(MouseButton::Left, move |_, window, _| {
                    window.focus(&focus_on_mouse);
                })
            })
            .when(!self.read_only, |this| {
                let state_for_leave = self.state.clone();
                this.on_mouse_move(
                    window.listener_for(&state_for_leave, move |_, _: &MouseMoveEvent, _, _| {}),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    window.listener_for(&state_for_leave, move |state, _, _, cx| {
                        state.set_hover_value(None, cx);
                    }),
                )
            })
            .children((0..max_rating).map(|index| {
                let position = index as f32 + 1.0;
                let fill_state = get_star_fill_state(display_value, position, allows_half);

                let icon_name = match fill_state {
                    StarFillState::Full => self.filled_icon.clone(),
                    StarFillState::Half => self.half_icon.clone(),
                    StarFillState::Empty => self.empty_icon.clone(),
                };

                let color = match fill_state {
                    StarFillState::Full | StarFillState::Half => active_color,
                    StarFillState::Empty => inactive_color,
                };

                let state_for_star = self.state.clone();
                let on_change_for_star = self.on_change.clone();
                let state_for_hover = self.state.clone();
                let bounds_for_click = star_bounds.clone();
                let bounds_for_hover = star_bounds.clone();
                let read_only = self.read_only;

                div()
                    .cursor(if read_only {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .when(!read_only, |this| {
                        this.on_mouse_down(
                            MouseButton::Left,
                            window.listener_for(
                                &state_for_star,
                                move |state, e: &MouseDownEvent, window, cx| {
                                    let new_value = calculate_click_value(
                                        e.position,
                                        position,
                                        bounds_for_click.borrow().get(index as usize).copied(),
                                        allows_half,
                                    );
                                    state.set_value(new_value, cx);
                                    state.set_hover_value(None, cx);
                                    if let Some(ref handler) = on_change_for_star {
                                        handler(state.value, window, cx);
                                    }
                                },
                            ),
                        )
                        .on_mouse_move(window.listener_for(
                            &state_for_hover,
                            move |state, e: &MouseMoveEvent, _, cx| {
                                let hover_val = calculate_click_value(
                                    e.position,
                                    position,
                                    bounds_for_hover.borrow().get(index as usize).copied(),
                                    allows_half,
                                );
                                state.set_hover_value(Some(hover_val), cx);
                            },
                        ))
                    })
                    .child(
                        Icon::new(icon_name.as_ref())
                            .size(IconSizeEnum::Custom(icon_size))
                            .color(color),
                    )
            }))
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum StarFillState {
    Full,
    Half,
    Empty,
}

fn get_star_fill_state(value: f32, position: f32, allows_half: bool) -> StarFillState {
    if value >= position {
        StarFillState::Full
    } else if allows_half && value >= position - 0.5 {
        StarFillState::Half
    } else {
        StarFillState::Empty
    }
}

fn calculate_click_value(
    position: Point<Pixels>,
    star_position: f32,
    star_bounds: Option<Bounds<Pixels>>,
    allows_half: bool,
) -> f32 {
    if allows_half
        && star_bounds.is_some_and(|bounds| position.x < bounds.origin.x + bounds.size.width / 2.0)
    {
        star_position - 0.5
    } else {
        star_position
    }
}

#[cfg(test)]
mod tests {
    use super::{RatingState, calculate_click_value};
    use kael::{AppContext, Bounds, TestAppContext, point, px, size};

    #[test]
    fn half_ratings_use_the_clicked_side_of_each_star() {
        let bounds = Bounds::new(point(px(100.0), px(20.0)), size(px(24.0), px(24.0)));
        assert_eq!(
            calculate_click_value(point(px(105.0), px(25.0)), 3.0, Some(bounds), true),
            2.5
        );
        assert_eq!(
            calculate_click_value(point(px(119.0), px(25.0)), 3.0, Some(bounds), true),
            3.0
        );
        assert_eq!(
            calculate_click_value(point(px(105.0), px(25.0)), 3.0, Some(bounds), false),
            3.0
        );
    }

    #[kael::test]
    fn rating_ignores_non_finite_values(cx: &mut TestAppContext) {
        let state = cx.new(RatingState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_value(3.0, cx);
                state.set_value(f32::NAN, cx);
                state.set_value(f32::INFINITY, cx);
                assert_eq!(state.value(), 3.0);
            });
        });
    }
}
