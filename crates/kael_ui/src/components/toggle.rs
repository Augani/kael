//! Toggle component - Toggle/Switch component with animations and keyboard support.

use crate::{
    astryx,
    components::{
        field::FieldStatusType,
        field_status::{FieldStatus, FieldStatusTone, FieldStatusVariant},
        spinner::{Spinner, SpinnerSize},
    },
    theme::use_theme,
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

actions!(toggle, [ToggleAction]);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ToggleSize {
    Sm,
    Md,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LabelSide {
    Left,
    Right,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwitchLabelPosition {
    Start,
    End,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum SwitchLabelSpacing {
    #[default]
    Default,
    Spread,
}

#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    base: Stateful<Div>,
    checked: bool,
    disabled: bool,
    label: Option<SharedString>,
    description: Option<SharedString>,
    label_hidden: bool,
    loading: bool,
    optional: bool,
    required: bool,
    status: Option<(FieldStatusType, SharedString)>,
    label_side: LabelSide,
    label_spacing: SwitchLabelSpacing,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
    size: ToggleSize,
    style: StyleRefinement,
}

impl Toggle {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            id: id.clone(),
            base: div().id(id),
            checked: false,
            disabled: false,
            label: None,
            description: None,
            label_hidden: false,
            loading: false,
            optional: false,
            required: false,
            status: None,
            label_side: LabelSide::Right,
            label_spacing: SwitchLabelSpacing::Default,
            on_click: None,
            size: ToggleSize::Md,
            style: StyleRefinement::default(),
        }
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn value(self, value: bool) -> Self {
        self.checked(value)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn label<T: Into<SharedString>>(mut self, label: T) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn description<T: Into<SharedString>>(mut self, description: T) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn is_label_hidden(mut self, hidden: bool) -> Self {
        self.label_hidden = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    pub fn is_loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.is_loading(loading)
    }

    pub fn is_optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.is_optional(optional)
    }

    pub fn is_required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.is_required(required)
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.status = Some((status, message.into()));
        self
    }

    pub fn label_side(mut self, side: LabelSide) -> Self {
        self.label_side = side;
        self
    }

    pub fn label_position(mut self, position: SwitchLabelPosition) -> Self {
        self.label_side = match position {
            SwitchLabelPosition::Start => LabelSide::Left,
            SwitchLabelPosition::End => LabelSide::Right,
        };
        self
    }

    #[allow(non_snake_case)]
    pub fn labelPosition(self, position: SwitchLabelPosition) -> Self {
        self.label_position(position)
    }

    pub fn label_spacing(mut self, spacing: SwitchLabelSpacing) -> Self {
        self.label_spacing = spacing;
        self
    }

    #[allow(non_snake_case)]
    pub fn labelSpacing(self, spacing: SwitchLabelSpacing) -> Self {
        self.label_spacing(spacing)
    }

    pub fn on_click<F>(mut self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn on_change<F>(self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_click(handler)
    }

    #[allow(non_snake_case)]
    pub fn onChange<F>(self, handler: F) -> Self
    where
        F: Fn(&bool, &mut Window, &mut App) + 'static,
    {
        self.on_change(handler)
    }

    pub fn size(mut self, size: ToggleSize) -> Self {
        self.size = size;
        self
    }
}

impl Styled for Toggle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for Toggle {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Toggle {}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;

        let (bg_width, bg_height, thumb_off, thumb_on, inset, on_right_inset) = match self.size {
            ToggleSize::Sm => (px(32.0), px(18.0), px(12.0), px(14.0), px(3.0), px(2.0)),
            ToggleSize::Md => (px(40.0), px(24.0), px(16.0), px(20.0), px(4.0), px(2.0)),
        };

        let checked = self.checked;
        let dark = theme.tokens.background.l < 0.5;
        let track_off = theme.tokens.muted;
        let thumb = theme.tokens.card;

        let (bg, toggle_bg) = if self.disabled {
            let bg_color = if checked {
                theme.tokens.primary.opacity(0.5)
            } else {
                track_off.opacity(0.5)
            };
            (bg_color, thumb.opacity(0.7))
        } else if checked {
            (theme.tokens.primary, thumb)
        } else {
            (track_off, thumb)
        };

        let radius = bg_height;

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();

        let is_focused = focus_handle.is_focused(window);
        let focus_on_mouse = focus_handle.clone();
        let is_busy = self.loading;
        let is_interactive = !self.disabled && !is_busy;
        let accessibility_label = self
            .label
            .clone()
            .unwrap_or_else(|| "Switch".into())
            .to_string();

        let row = self
            .base
            .accessibility(
                AccessibilityAttributes::switch(accessibility_label, checked)
                    .disabled(!is_interactive)
                    .focused(is_focused),
            )
            .when(is_interactive, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
            })
            .flex()
            .items_center()
            .gap(px(8.0))
            .when(self.label_spacing == SwitchLabelSpacing::Spread, |this| {
                this.w_full().justify_between()
            })
            .when(self.label_side == LabelSide::Left, |this| {
                this.flex_row_reverse()
            })
            .child(
                div()
                    .id(ElementId::Name(format!("{}-track", self.id).into()))
                    .w(bg_width)
                    .h(bg_height)
                    .rounded(radius)
                    .flex()
                    .items_center()
                    .bg(bg)
                    .transition(theme.tokens.transition_fast)
                    .when(is_focused && !self.disabled, |this| {
                        this.shadow(vec![astryx::focus_ring_outer(theme.tokens.primary)])
                    })
                    .cursor(if self.disabled || is_busy {
                        CursorStyle::Arrow
                    } else {
                        CursorStyle::PointingHand
                    })
                    .when(is_interactive, |this| {
                        this.hover(|style| {
                            if checked {
                                style.bg(theme.tokens.primary.opacity(0.9))
                            } else {
                                style.bg(track_off.blend(astryx::overlay_hover(dark)))
                            }
                        })
                    })
                    .child(toggle_thumb(
                        self.id.clone(),
                        checked,
                        toggle_bg,
                        bg_width,
                        thumb_off,
                        thumb_on,
                        inset,
                        on_right_inset,
                        radius,
                        self.disabled,
                        is_busy,
                        window,
                        cx,
                    )),
            )
            .when_some(
                self.label.clone().filter(|_| !self.label_hidden),
                |this, label| {
                    this.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .min_h(bg_height)
                            .justify_center()
                            .text_size(match self.size {
                                ToggleSize::Sm => px(13.0),
                                ToggleSize::Md => px(14.0),
                            })
                            .font_family(theme.tokens.font_family.clone())
                            .text_color(if self.disabled {
                                theme.tokens.muted_foreground
                            } else {
                                theme.tokens.foreground
                            })
                            .cursor(if self.disabled || is_busy {
                                CursorStyle::Arrow
                            } else {
                                CursorStyle::PointingHand
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label)
                                    .when(self.required && !self.optional, |this| {
                                        this.child(
                                            div().text_color(theme.tokens.destructive).child("*"),
                                        )
                                    })
                                    .when(self.optional && !self.required, |this| {
                                        this.child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(theme.tokens.muted_foreground)
                                                .child("(optional)"),
                                        )
                                    }),
                            )
                            .when_some(self.description.clone(), |this, description| {
                                this.child(
                                    div()
                                        .text_size(px(12.0))
                                        .line_height(px(16.0))
                                        .text_color(theme.tokens.muted_foreground)
                                        .child(description),
                                )
                            }),
                    )
                },
            )
            .when(is_interactive, |this| {
                this.when_some(self.on_click.clone(), |this, on_click| {
                    let on_click_for_key = on_click.clone();
                    this.on_click(move |_, window, cx| {
                        let new_checked = !checked;
                        (on_click)(&new_checked, window, cx);
                    })
                    .on_key_down(move |event, window, cx| {
                        if event.keystroke.key == "space" || event.keystroke.key == "enter" {
                            let new_checked = !checked;
                            (on_click_for_key)(&new_checked, window, cx);
                            cx.stop_propagation();
                        }
                    })
                })
            })
            .when(is_interactive, |this| {
                this.on_mouse_down(MouseButton::Left, move |_, window, _| {
                    window.focus(&focus_on_mouse);
                })
            });

        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .when(self.label_spacing == SwitchLabelSpacing::Spread, |this| {
                this.w_full()
            })
            .child(row)
            .when_some(self.status, |this, (status, message)| {
                let tone = match status {
                    FieldStatusType::Warning => FieldStatusTone::Warning,
                    FieldStatusType::Error => FieldStatusTone::Error,
                    FieldStatusType::Success => FieldStatusTone::Success,
                };
                this.child(
                    FieldStatus::new(tone, message)
                        .variant(FieldStatusVariant::Detached)
                        .show_icon(true),
                )
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

fn toggle_thumb(
    id: ElementId,
    checked: bool,
    color: Hsla,
    bg_width: Pixels,
    thumb_off: Pixels,
    thumb_on: Pixels,
    inset: Pixels,
    on_right_inset: Pixels,
    radius: Pixels,
    disabled: bool,
    loading: bool,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let toggle_state = window.use_keyed_state(id.clone(), cx, |_, _| checked);

    div()
        .rounded(radius)
        .bg(color)
        .size(if checked { thumb_on } else { thumb_off })
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .transition(std::time::Duration::from_millis(150))
        .when(loading, |this| {
            this.child(Spinner::new().size(SpinnerSize::Xs))
        })
        .map(|this| {
            let prev_checked = *toggle_state.read(cx);
            let prev_thumb = if prev_checked { thumb_on } else { thumb_off };
            let current_thumb = if checked { thumb_on } else { thumb_off };
            let prev_x = if prev_checked {
                bg_width - prev_thumb - on_right_inset
            } else {
                inset
            };
            let current_x = if checked {
                bg_width - current_thumb - on_right_inset
            } else {
                inset
            };

            if !disabled && prev_checked != checked {
                let duration = std::time::Duration::from_millis(150);
                cx.spawn({
                    let toggle_state = toggle_state.clone();
                    async move |cx| {
                        cx.background_executor().timer(duration).await;
                        _ = toggle_state.update(cx, |state, _| *state = checked);
                    }
                })
                .detach();

                this.with_animation(
                    ElementId::NamedInteger("toggle-slide".into(), checked as u64),
                    Animation::new(duration),
                    move |this, delta| {
                        let x = prev_x + (current_x - prev_x) * delta;
                        this.left(x)
                    },
                )
                .into_any_element()
            } else {
                this.left(current_x).into_any_element()
            }
        })
}
