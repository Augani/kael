use crate::theme::Theme;
use crate::{
    astryx,
    components::{
        field::{Field, FieldStatusType},
        field_status::FieldStatusVariant,
        icon::Icon,
        icon_source::IconSource,
    },
};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NumberInputSize {
    Sm,
    Md,
    Lg,
}

pub struct NumberInputState {
    value: f64,
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
    precision: usize,
    focus_handle: FocusHandle,
}

impl NumberInputState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            value: 0.0,
            min: None,
            max: None,
            step: 1.0,
            precision: 0,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn with_value(cx: &mut Context<Self>, value: f64) -> Self {
        Self {
            value,
            min: None,
            max: None,
            step: 1.0,
            precision: 0,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn set_value(&mut self, value: f64, cx: &mut Context<Self>) {
        self.value = self.clamp_value(value);
        cx.notify();
    }

    pub fn set_min(&mut self, min: Option<f64>, cx: &mut Context<Self>) {
        self.min = min;
        self.value = self.clamp_value(self.value);
        cx.notify();
    }

    pub fn set_max(&mut self, max: Option<f64>, cx: &mut Context<Self>) {
        self.max = max;
        self.value = self.clamp_value(self.value);
        cx.notify();
    }

    pub fn set_step(&mut self, step: f64) {
        self.step = step.max(0.001);
    }

    pub fn set_precision(&mut self, precision: usize) {
        self.precision = precision;
    }

    pub fn increment(&mut self, cx: &mut Context<Self>) {
        self.set_value(self.value + self.step, cx);
    }

    pub fn decrement(&mut self, cx: &mut Context<Self>) {
        self.set_value(self.value - self.step, cx);
    }

    pub fn can_increment(&self) -> bool {
        self.max.is_none_or(|max| self.value < max)
    }

    pub fn can_decrement(&self) -> bool {
        self.min.is_none_or(|min| self.value > min)
    }

    fn clamp_value(&self, value: f64) -> f64 {
        let mut v = value;
        if let Some(min) = self.min {
            v = v.max(min);
        }
        if let Some(max) = self.max {
            v = v.min(max);
        }
        v
    }

    fn format_value(&self) -> String {
        if self.precision == 0 {
            format!("{}", self.value as i64)
        } else {
            format!("{:.prec$}", self.value, prec = self.precision)
        }
    }
}

impl Focusable for NumberInputState {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for NumberInputState {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[derive(IntoElement)]
pub struct NumberInput {
    state: Entity<NumberInputState>,
    label: Option<SharedString>,
    hidden_label: bool,
    description: Option<SharedString>,
    optional: bool,
    required: bool,
    size: NumberInputSize,
    placeholder: Option<SharedString>,
    disabled: bool,
    show_buttons: bool,
    clearable: bool,
    start_icon: Option<IconSource>,
    units: Option<SharedString>,
    status: Option<FieldStatusType>,
    status_message: Option<SharedString>,
    on_change: Option<Rc<dyn Fn(f64, &mut Window, &mut App)>>,
    on_clear: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl NumberInput {
    pub fn new(state: Entity<NumberInputState>) -> Self {
        Self {
            state,
            label: None,
            hidden_label: false,
            description: None,
            optional: false,
            required: false,
            size: NumberInputSize::Md,
            placeholder: None,
            disabled: false,
            show_buttons: false,
            clearable: false,
            start_icon: None,
            units: None,
            status: None,
            status_message: None,
            on_change: None,
            on_clear: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn is_label_hidden(mut self, hidden: bool) -> Self {
        self.hidden_label = hidden;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLabelHidden(self, hidden: bool) -> Self {
        self.is_label_hidden(hidden)
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.optional(optional)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn size(mut self, size: NumberInputSize) -> Self {
        self.size = size;
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
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

    pub fn show_buttons(mut self, show: bool) -> Self {
        self.show_buttons = show;
        self
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasClear(self, clearable: bool) -> Self {
        self.clearable(clearable)
    }

    pub fn start_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.start_icon = Some(icon.into());
        self
    }

    pub fn units(mut self, units: impl Into<SharedString>) -> Self {
        self.units = Some(units.into());
        self
    }

    pub fn status(mut self, status: FieldStatusType) -> Self {
        self.status = Some(status);
        self
    }

    pub fn status_message(mut self, message: impl Into<SharedString>) -> Self {
        self.status_message = Some(message.into());
        self
    }

    pub fn min(self, min: f64, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, cx| state.set_min(Some(min), cx));
        self
    }

    pub fn max(self, max: f64, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, cx| state.set_max(Some(max), cx));
        self
    }

    pub fn step(self, step: f64, cx: &mut App) -> Self {
        self.state.update(cx, |state, _| state.set_step(step));
        self
    }

    pub fn precision(self, precision: usize, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, _| state.set_precision(precision));
        self
    }

    pub fn on_change(mut self, handler: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onChange(self, handler: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change(handler)
    }

    pub fn on_clear(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }
}

impl Styled for NumberInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for NumberInput {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let state_data = self.state.read(cx);
        let value_text = state_data.format_value();
        let can_increment = state_data.can_increment();
        let can_decrement = state_data.can_decrement();
        let focus_handle = state_data.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let state = self.state.clone();
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let has_value = state_data.value() != 0.0;
        let status_color = self.status.map(|status| match status {
            FieldStatusType::Warning => theme.tokens.warning,
            FieldStatusType::Error => theme.tokens.destructive,
            FieldStatusType::Success => theme.tokens.success,
        });
        let status_icon = self.status.map(|status| match status {
            FieldStatusType::Warning => "triangle-alert",
            FieldStatusType::Error => "circle-alert",
            FieldStatusType::Success => "circle-check",
        });

        let (height, padding_x, text_size, icon_size) = match self.size {
            NumberInputSize::Sm => (px(28.0), px(12.0), px(14.0), px(16.0)),
            NumberInputSize::Md => (px(32.0), px(12.0), px(14.0), px(16.0)),
            NumberInputSize::Lg => (px(36.0), px(12.0), px(14.0), px(16.0)),
        };
        let on_change_for_keys = self.on_change.clone();
        let on_clear_handler = self.on_clear.clone();
        let disabled = self.disabled;

        let control = div()
            .flex()
            .items_center()
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(
                div()
                    .when(!disabled, |this| {
                        this.track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    })
                    .flex()
                    .items_center()
                    .h(height)
                    .gap(px(8.0))
                    .px(padding_x)
                    .bg(theme.tokens.card)
                    .border_1()
                    .border_color(if let Some(color) = status_color {
                        color
                    } else if is_focused {
                        theme.tokens.primary
                    } else {
                        theme.tokens.input
                    })
                    .rounded(theme.tokens.radius_md)
                    .font_family(theme.tokens.font_family.clone())
                    .transition(theme.tokens.transition_fast)
                    .shadow(smallvec::smallvec![astryx::focus_ring(
                        kael::transparent_black()
                    )])
                    .when(is_focused && !self.disabled, |this| {
                        this.shadow(smallvec::smallvec![astryx::focus_ring(
                            status_color.unwrap_or(theme.tokens.primary)
                        )])
                    })
                    .when(!is_focused && !self.disabled, |this| {
                        this.hover(|style| {
                            style
                                .border_color(status_color.unwrap_or(theme.tokens.input))
                                .shadow(smallvec::smallvec![
                                    status_color.map_or(hover_ring, astryx::input_hover_ring,)
                                ])
                        })
                    })
                    .when(self.disabled, |d| d.opacity(0.5))
                    .when(!disabled, {
                        let state = state.clone();
                        move |this| {
                            this.on_key_down(move |event, window, cx| {
                                let key = event.keystroke.key.as_str();
                                if key == "up" || key == "down" {
                                    state.update(cx, |s, cx| {
                                        if key == "up" {
                                            s.increment(cx);
                                        } else {
                                            s.decrement(cx);
                                        }
                                        if let Some(handler) = on_change_for_keys.as_ref() {
                                            handler(s.value, window, cx);
                                        }
                                    });
                                    cx.stop_propagation();
                                }
                            })
                        }
                    })
                    .when_some(self.start_icon, |this, icon| {
                        this.child(
                            div()
                                .size(icon_size)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(icon)
                                        .size(icon_size)
                                        .color(theme.tokens.muted_foreground),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_start()
                            .h_full()
                            .min_w(px(60.0))
                            .text_size(text_size)
                            .line_height(px(20.0))
                            .text_color(theme.tokens.foreground)
                            .child(value_text),
                    )
                    .when_some(self.units, |this, units| {
                        this.child(
                            div()
                                .flex_none()
                                .text_size(px(14.0))
                                .line_height(px(20.0))
                                .text_color(theme.tokens.muted_foreground)
                                .child(units),
                        )
                    })
                    .when(self.clearable && has_value && !disabled, {
                        let state = state.clone();
                        move |this| {
                            this.child(
                                div()
                                    .size(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(theme.tokens.radius_sm)
                                    .cursor_pointer()
                                    .hover(|style| {
                                        style.bg(astryx::overlay_hover(
                                            theme.tokens.background.l < 0.5,
                                        ))
                                    })
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        state.update(cx, |state, cx| {
                                            state.set_value(0.0, cx);
                                        });
                                        if let Some(handler) = on_clear_handler.as_ref() {
                                            handler(window, cx);
                                        }
                                    })
                                    .child(
                                        Icon::new("x")
                                            .size(icon_size)
                                            .color(theme.tokens.muted_foreground),
                                    ),
                            )
                        }
                    })
                    .when(self.show_buttons, {
                        let state = state.clone();
                        let on_change = self.on_change.clone();
                        let disabled = self.disabled;
                        move |d| {
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.0))
                                    .child(
                                        div()
                                            .id("decrement")
                                            .size(px(20.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(theme.tokens.radius_sm)
                                            .text_color(if can_decrement && !disabled {
                                                theme.tokens.foreground
                                            } else {
                                                theme.tokens.muted_foreground
                                            })
                                            .when(can_decrement && !disabled, |d| {
                                                d.cursor_pointer().hover(|s| {
                                                    s.bg(theme.tokens.accent.opacity(0.5))
                                                })
                                            })
                                            .when(can_decrement && !disabled, {
                                                let state = state.clone();
                                                let on_change = on_change.clone();
                                                move |d| {
                                                    d.on_click(move |_, window, cx| {
                                                        state.update(cx, |s, cx| {
                                                            s.decrement(cx);
                                                            if let Some(ref handler) = on_change {
                                                                handler(s.value, window, cx);
                                                            }
                                                        });
                                                    })
                                                }
                                            })
                                            .child(
                                                Icon::new(IconSource::Named("minus".into()))
                                                    .size(px(14.0))
                                                    .color(theme.tokens.muted_foreground),
                                            ),
                                    )
                                    .child({
                                        let state = state.clone();
                                        let on_change = on_change.clone();
                                        div()
                                            .id("increment")
                                            .size(px(20.0))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(theme.tokens.radius_sm)
                                            .text_color(if can_increment && !disabled {
                                                theme.tokens.foreground
                                            } else {
                                                theme.tokens.muted_foreground
                                            })
                                            .when(can_increment && !disabled, |d| {
                                                d.cursor_pointer().hover(|s| {
                                                    s.bg(theme.tokens.accent.opacity(0.5))
                                                })
                                            })
                                            .when(can_increment && !disabled, move |d| {
                                                d.on_click(move |_, window, cx| {
                                                    state.update(cx, |s, cx| {
                                                        s.increment(cx);
                                                        if let Some(ref handler) = on_change {
                                                            handler(s.value, window, cx);
                                                        }
                                                    });
                                                })
                                            })
                                            .child(
                                                Icon::new(IconSource::Named("plus".into()))
                                                    .size(px(14.0))
                                                    .color(theme.tokens.muted_foreground),
                                            )
                                    }),
                            )
                        }
                    })
                    .when_some(status_icon, |this, icon| {
                        this.child(
                            Icon::new(IconSource::Named(icon.into()))
                                .size(icon_size)
                                .color(status_color.unwrap_or(theme.tokens.muted_foreground)),
                        )
                    }),
            );

        match self.label {
            Some(label) => {
                let mut field = Field::new(label, control)
                    .hidden_label(self.hidden_label)
                    .optional(self.optional)
                    .required(self.required)
                    .disabled(self.disabled)
                    .status_variant(FieldStatusVariant::Detached);

                if let Some(description) = self.description {
                    field = field.description(description);
                }

                if let Some((status, message)) = self.status.zip(self.status_message) {
                    field = field.status(status, message);
                }

                field.into_any_element()
            }
            None => control.into_any_element(),
        }
    }
}
