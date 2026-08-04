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

type NumberChangeHandler = Rc<dyn Fn(f64, &mut Window, &mut App)>;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NumberInputSize {
    Sm,
    Md,
    Lg,
}

pub struct NumberInputState {
    value: f64,
    has_value: bool,
    draft: Option<String>,
    min: Option<f64>,
    max: Option<f64>,
    step: f64,
    precision: usize,
    focus_handle: FocusHandle,
    was_focused: bool,
}

impl NumberInputState {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            value: 0.0,
            has_value: true,
            draft: None,
            min: None,
            max: None,
            step: 1.0,
            precision: 0,
            focus_handle: cx.focus_handle(),
            was_focused: false,
        }
    }

    pub fn with_value(cx: &mut Context<Self>, value: f64) -> Self {
        Self {
            value,
            has_value: value.is_finite(),
            draft: None,
            min: None,
            max: None,
            step: 1.0,
            precision: 0,
            focus_handle: cx.focus_handle(),
            was_focused: false,
        }
    }

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn value_option(&self) -> Option<f64> {
        self.has_value.then_some(self.value)
    }

    pub fn has_value(&self) -> bool {
        self.has_value
    }

    pub fn set_value(&mut self, value: f64, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        let value = self.clamp_value(value);
        let changed = !self.has_value || (self.value - value).abs() > f64::EPSILON;
        self.value = value;
        self.has_value = true;
        self.draft = None;
        if changed {
            cx.notify();
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.has_value || self.draft.is_some() {
            self.has_value = false;
            self.draft = None;
            cx.notify();
        }
    }

    pub fn set_min(&mut self, min: Option<f64>, cx: &mut Context<Self>) {
        let previous = (self.min, self.max, self.value);
        self.min = min.filter(|value| value.is_finite());
        if let (Some(min), Some(max)) = (self.min, self.max)
            && min > max
        {
            self.max = Some(min);
        }
        self.value = self.clamp_value(self.value);
        if previous != (self.min, self.max, self.value) {
            cx.notify();
        }
    }

    pub fn set_max(&mut self, max: Option<f64>, cx: &mut Context<Self>) {
        let previous = (self.min, self.max, self.value);
        self.max = max.filter(|value| value.is_finite());
        if let (Some(min), Some(max)) = (self.min, self.max)
            && max < min
        {
            self.min = Some(max);
        }
        self.value = self.clamp_value(self.value);
        if previous != (self.min, self.max, self.value) {
            cx.notify();
        }
    }

    pub fn set_step(&mut self, step: f64, cx: &mut Context<Self>) {
        if step.is_finite() && step > 0.0 && (self.step - step).abs() > f64::EPSILON {
            self.step = step;
            cx.notify();
        }
    }

    pub fn set_precision(&mut self, precision: usize, cx: &mut Context<Self>) {
        let precision = precision.min(15);
        if self.precision != precision {
            self.precision = precision;
            cx.notify();
        }
    }

    pub fn increment(&mut self, cx: &mut Context<Self>) {
        self.set_value(
            if self.has_value {
                self.value + self.step
            } else {
                self.min.unwrap_or(0.0)
            },
            cx,
        );
    }

    pub fn decrement(&mut self, cx: &mut Context<Self>) {
        self.set_value(
            if self.has_value {
                self.value - self.step
            } else {
                self.max.unwrap_or(0.0)
            },
            cx,
        );
    }

    pub fn can_increment(&self) -> bool {
        !self.has_value || self.max.is_none_or(|max| self.value < max)
    }

    pub fn can_decrement(&self) -> bool {
        !self.has_value || self.min.is_none_or(|min| self.value > min)
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

    fn update_draft(&mut self, key: &str, cx: &mut Context<Self>) {
        let draft = self.draft.get_or_insert_with(String::new);
        match key {
            "backspace" => {
                draft.pop();
            }
            "." if !draft.contains('.') => draft.push('.'),
            "-" if draft.is_empty() && self.min.is_none_or(|min| min < 0.0) => draft.push('-'),
            digit if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() => {
                draft.push_str(digit)
            }
            _ => return,
        }
        cx.notify();
    }

    fn commit_draft(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(draft) = self.draft.take() else {
            return false;
        };
        let Ok(value) = draft.parse::<f64>() else {
            cx.notify();
            return false;
        };
        let previous = self.value_option();
        self.set_value(value, cx);
        previous != self.value_option()
    }

    fn cancel_draft(&mut self, cx: &mut Context<Self>) {
        if self.draft.take().is_some() {
            cx.notify();
        }
    }

    fn format_value(&self) -> String {
        if let Some(draft) = self.draft.as_ref() {
            return draft.clone();
        }
        if !self.has_value {
            return String::new();
        }
        if self.precision == 0 {
            format!("{:.0}", self.value)
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

fn handle_number_accessibility_action(
    state: &Entity<NumberInputState>,
    request: &AccessibilityActionRequest,
    on_change: Option<&NumberChangeHandler>,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |state, cx| {
        let previous = state.value_option();
        match request.action {
            AccessibilityAction::Increment => state.increment(cx),
            AccessibilityAction::Decrement => state.decrement(cx),
            AccessibilityAction::SetValue => {
                let requested_value = match request.payload.as_ref() {
                    Some(AccessibilityActionPayload::NumericValue(value)) => Some(*value),
                    Some(AccessibilityActionPayload::Value(value)) => value.parse().ok(),
                    None => None,
                };
                if let Some(value) = requested_value {
                    state.set_value(value, cx);
                }
            }
            _ => {}
        }

        if state.value_option() != previous
            && let Some(handler) = on_change
        {
            handler(state.value, window, cx);
        }
    });
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
    read_only: bool,
    show_buttons: bool,
    clearable: bool,
    start_icon: Option<IconSource>,
    units: Option<SharedString>,
    status: Option<FieldStatusType>,
    status_message: Option<SharedString>,
    on_change: Option<NumberChangeHandler>,
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
            read_only: false,
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

    /// Keep the value focusable and selectable while preventing edits.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    #[allow(non_snake_case)]
    pub fn isReadOnly(self, read_only: bool) -> Self {
        self.read_only(read_only)
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
        self.state.update(cx, |state, cx| state.set_step(step, cx));
        self
    }

    pub fn precision(self, precision: usize, cx: &mut App) -> Self {
        self.state
            .update(cx, |state, cx| state.set_precision(precision, cx));
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
        let currently_focused = self.state.read(cx).focus_handle.is_focused(window);
        let on_change_for_blur = self.on_change.clone();
        self.state.update(cx, |state, cx| {
            let lost_focus = state.was_focused && !currently_focused;
            state.was_focused = currently_focused;
            if lost_focus
                && state.commit_draft(cx)
                && let Some(handler) = on_change_for_blur.as_ref()
            {
                handler(state.value, window, cx);
            }
        });
        let theme = Theme::of(cx);
        let user_style = self.style;
        let state_data = self.state.read(cx);
        let value_text = state_data.format_value();
        let can_increment = state_data.can_increment();
        let can_decrement = state_data.can_decrement();
        let has_value = state_data.has_value();
        let focus_handle = state_data.focus_handle(cx);
        let is_focused = focus_handle.is_focused(window);
        let state = self.state.clone();
        let hover_ring = astryx::input_hover_ring(theme.tokens.input);
        let entity_id = self.state.entity_id().as_u64();
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

        let (height, padding_x, text_size, icon_size, button_size) = match self.size {
            NumberInputSize::Sm => (px(28.0), px(8.0), px(14.0), px(16.0), px(24.0)),
            NumberInputSize::Md => (px(32.0), px(10.0), px(14.0), px(16.0), px(28.0)),
            NumberInputSize::Lg => (px(36.0), px(12.0), px(14.0), px(16.0), px(32.0)),
        };
        let on_change_for_keys = self.on_change.clone();
        let on_clear_handler = self.on_clear.clone();
        let disabled = self.disabled;
        let read_only = self.read_only;
        let accessible_label = self
            .label
            .clone()
            .unwrap_or_else(|| SharedString::from("Number"));
        let mut accessibility_state = AccessibilityState::NONE;
        if disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if read_only {
            accessibility_state |= AccessibilityState::READ_ONLY;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        if self.required {
            accessibility_state |= AccessibilityState::REQUIRED;
        }
        if matches!(self.status, Some(FieldStatusType::Error)) {
            accessibility_state |= AccessibilityState::INVALID;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::TextInput)
            .label(accessible_label.to_string())
            .placeholder(
                self.placeholder
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "Enter a number".to_owned()),
            )
            .states(accessibility_state);
        if has_value {
            accessibility = accessibility.value(AccessibilityValue::Number(state_data.value()));
        }
        if !disabled && !read_only {
            accessibility = accessibility.actions(vec![
                AccessibilityAction::Focus,
                AccessibilityAction::SetValue,
                AccessibilityAction::Increment,
                AccessibilityAction::Decrement,
            ]);
        } else if !disabled {
            accessibility = accessibility.actions(vec![AccessibilityAction::Focus]);
        }

        let control = div()
            .flex()
            .items_center()
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            })
            .child(
                div()
                    .id(("number-input", entity_id))
                    .when(!disabled, |this| {
                        this.track_focus(&focus_handle.clone().tab_index(0).tab_stop(true))
                    })
                    .accessibility(accessibility)
                    .when(!disabled && !read_only, {
                        let state = state.clone();
                        let on_change = self.on_change.clone();
                        move |this| {
                            let bind = |this: Stateful<Div>, action| {
                                let state = state.clone();
                                let on_change = on_change.clone();
                                this.on_accessibility_action(action, move |request, window, cx| {
                                    handle_number_accessibility_action(
                                        &state,
                                        request,
                                        on_change.as_ref(),
                                        window,
                                        cx,
                                    );
                                })
                            };
                            bind(
                                bind(
                                    bind(this, AccessibilityAction::Increment),
                                    AccessibilityAction::Decrement,
                                ),
                                AccessibilityAction::SetValue,
                            )
                        }
                    })
                    .flex()
                    .items_center()
                    .h(height)
                    .gap(px(8.0))
                    .px(padding_x)
                    .bg(if read_only {
                        theme.tokens.muted.opacity(0.35)
                    } else {
                        theme.tokens.card
                    })
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
                    .when(!is_focused && !self.disabled && !read_only, |this| {
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
                        let focus_handle = focus_handle.clone();
                        move |this| {
                            this.cursor(if read_only {
                                CursorStyle::Arrow
                            } else {
                                CursorStyle::IBeam
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                move |_, window, _| {
                                    window.focus(&focus_handle);
                                },
                            )
                        }
                    })
                    .when(!disabled && !read_only, {
                        let state = state.clone();
                        move |this| {
                            this.on_key_down(move |event, window, cx| {
                                let key = event.keystroke.key.as_str();
                                let handled = match key {
                                    "up" | "down" => {
                                        state.update(cx, |s, cx| {
                                            let previous = s.value_option();
                                            if key == "up" {
                                                s.increment(cx);
                                            } else {
                                                s.decrement(cx);
                                            }
                                            if s.value_option() != previous
                                                && let Some(handler) = on_change_for_keys.as_ref()
                                            {
                                                handler(s.value, window, cx);
                                            }
                                        });
                                        true
                                    }
                                    "enter" => {
                                        state.update(cx, |s, cx| {
                                            if s.commit_draft(cx)
                                                && let Some(handler) = on_change_for_keys.as_ref()
                                            {
                                                handler(s.value, window, cx);
                                            }
                                        });
                                        true
                                    }
                                    "escape" => {
                                        state.update(cx, |s, cx| s.cancel_draft(cx));
                                        true
                                    }
                                    "backspace" | "." | "-" => {
                                        state.update(cx, |s, cx| s.update_draft(key, cx));
                                        true
                                    }
                                    digit
                                        if digit.len() == 1
                                            && digit.as_bytes()[0].is_ascii_digit() =>
                                    {
                                        state.update(cx, |s, cx| s.update_draft(key, cx));
                                        true
                                    }
                                    _ => false,
                                };
                                if handled {
                                    cx.stop_propagation();
                                    window.prevent_default();
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
                            .text_color(if has_value || !value_text.is_empty() {
                                theme.tokens.foreground
                            } else {
                                theme.tokens.muted_foreground
                            })
                            .child(if value_text.is_empty() {
                                self.placeholder
                                    .clone()
                                    .unwrap_or_else(|| SharedString::from("Enter a number"))
                            } else {
                                value_text.into()
                            }),
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
                    .when(self.clearable && has_value && !disabled && !read_only, {
                        let state = state.clone();
                        move |this| {
                            this.child(
                                div()
                                    .id(("number-input-clear", entity_id))
                                    .accessibility(
                                        AccessibilityAttributes::new(AccessibilityRole::Button)
                                            .label("Clear number")
                                            .actions(vec![AccessibilityAction::Click]),
                                    )
                                    .size(button_size)
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
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        state.update(cx, |state, cx| {
                                            state.clear(cx);
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
                                            .id(("number-input-decrement", entity_id))
                                            .accessibility({
                                                let mut attributes = AccessibilityAttributes::new(
                                                    AccessibilityRole::Button,
                                                )
                                                .label("Decrease value");
                                                if can_decrement && !disabled && !read_only {
                                                    attributes = attributes
                                                        .actions(vec![AccessibilityAction::Click]);
                                                } else {
                                                    attributes = attributes
                                                        .states(AccessibilityState::DISABLED);
                                                }
                                                attributes
                                            })
                                            .size(button_size)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(theme.tokens.radius_sm)
                                            .text_color(
                                                if can_decrement && !disabled && !read_only {
                                                    theme.tokens.foreground
                                                } else {
                                                    theme.tokens.muted_foreground
                                                },
                                            )
                                            .when(can_decrement && !disabled && !read_only, |d| {
                                                d.cursor_pointer().hover(|s| {
                                                    s.bg(theme.tokens.accent.opacity(0.5))
                                                })
                                            })
                                            .when(can_decrement && !disabled && !read_only, {
                                                let state = state.clone();
                                                let on_change = on_change.clone();
                                                move |d| {
                                                    d.on_click(move |_, window, cx| {
                                                        state.update(cx, |s, cx| {
                                                            let previous = s.value_option();
                                                            s.decrement(cx);
                                                            if s.value_option() != previous
                                                                && let Some(ref handler) = on_change
                                                            {
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
                                            .id(("number-input-increment", entity_id))
                                            .accessibility({
                                                let mut attributes = AccessibilityAttributes::new(
                                                    AccessibilityRole::Button,
                                                )
                                                .label("Increase value");
                                                if can_increment && !disabled && !read_only {
                                                    attributes = attributes
                                                        .actions(vec![AccessibilityAction::Click]);
                                                } else {
                                                    attributes = attributes
                                                        .states(AccessibilityState::DISABLED);
                                                }
                                                attributes
                                            })
                                            .size(button_size)
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(theme.tokens.radius_sm)
                                            .text_color(
                                                if can_increment && !disabled && !read_only {
                                                    theme.tokens.foreground
                                                } else {
                                                    theme.tokens.muted_foreground
                                                },
                                            )
                                            .when(can_increment && !disabled && !read_only, |d| {
                                                d.cursor_pointer().hover(|s| {
                                                    s.bg(theme.tokens.accent.opacity(0.5))
                                                })
                                            })
                                            .when(
                                                can_increment && !disabled && !read_only,
                                                move |d| {
                                                    d.on_click(move |_, window, cx| {
                                                        state.update(cx, |s, cx| {
                                                            let previous = s.value_option();
                                                            s.increment(cx);
                                                            if s.value_option() != previous
                                                                && let Some(ref handler) = on_change
                                                            {
                                                                handler(s.value, window, cx);
                                                            }
                                                        });
                                                    })
                                                },
                                            )
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

#[cfg(test)]
mod tests {
    use super::{NumberInput, NumberInputState, handle_number_accessibility_action};
    use kael::{
        AccessibilityAction, AccessibilityRole, AccessibilityState, AppContext, Context, Entity,
        IntoElement, Render, TestAppContext, Window,
    };

    struct NumberInputHost {
        state: Entity<NumberInputState>,
        read_only: bool,
    }

    impl Render for NumberInputHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            NumberInput::new(self.state.clone())
                .label("Quantity")
                .read_only(self.read_only)
        }
    }

    #[kael::test]
    fn bounds_are_normalized_and_non_finite_values_are_ignored(cx: &mut TestAppContext) {
        let state = cx.new(NumberInputState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_max(Some(5.0), cx);
                state.set_min(Some(10.0), cx);
                state.set_value(99.0, cx);
                state.set_value(f64::NAN, cx);
            });
        });

        cx.read(|cx| {
            let state = state.read(cx);
            assert_eq!(state.min, Some(10.0));
            assert_eq!(state.max, Some(10.0));
            assert_eq!(state.value(), 10.0);
        });
    }

    #[kael::test]
    fn clear_preserves_zero_as_a_real_value(cx: &mut TestAppContext) {
        let state = cx.new(NumberInputState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_value(0.0, cx);
                assert_eq!(state.value_option(), Some(0.0));
                state.clear(cx);
            });
        });
        assert_eq!(cx.read(|cx| state.read(cx).value_option()), None);
    }

    #[kael::test]
    fn zero_precision_rounds_without_narrowing_to_an_integer(cx: &mut TestAppContext) {
        let state = cx.new(NumberInputState::new);
        cx.update(|cx| {
            state.update(cx, |state, cx| {
                state.set_value(1.6, cx);
                assert_eq!(state.format_value(), "2");
                state.set_value(10_000_000_000_000_000_000.0, cx);
                assert_eq!(state.format_value(), "10000000000000000000");
            });
        });
    }

    #[kael::test]
    fn draft_commits_when_focus_leaves_the_control(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(NumberInputState::new);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| NumberInputHost {
                state,
                read_only: false,
            }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.focus(&state.read(cx).focus_handle);
        });
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert!(state.read(cx).focus_handle.is_focused(window));
        });
        window.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.update_draft("1", cx);
                state.update_draft("2", cx);
            });
        });
        window.update(|_, cx| {
            assert_eq!(state.read(cx).draft.as_deref(), Some("12"));
        });
        window.update(|window, _| window.blur());
        window.update(|window, cx| window.draw(cx).clear());
        window.run_until_parked();

        assert_eq!(cx.read(|cx| state.read(cx).value_option()), Some(12.0));
    }

    #[kael::test]
    fn accessibility_increment_updates_the_value(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(NumberInputState::new);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| NumberInputHost {
                state,
                read_only: false,
            }
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::TextInput)
                .expect("number input should expose text-input semantics");
            assert!(
                window.has_accessibility_action_handler(node.id, AccessibilityAction::Increment,)
            );
        });
        window.update(|window, cx| {
            handle_number_accessibility_action(
                &state,
                &kael::AccessibilityActionRequest::new(
                    kael::AccessibilityId::new(),
                    AccessibilityAction::Increment,
                ),
                None,
                window,
                cx,
            );
        });

        assert_eq!(cx.read(|cx| state.read(cx).value()), 1.0);
    }

    #[kael::test]
    fn read_only_input_is_focusable_but_not_adjustable(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(NumberInputState::new);
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| NumberInputHost {
                state,
                read_only: true,
            }
        });

        window.update(|window, cx| {
            window.draw(cx).clear();
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::TextInput)
                .expect("number input should expose text-input semantics");
            assert!(node.states.contains(AccessibilityState::READ_ONLY));
            assert!(node.actions.contains(&AccessibilityAction::Focus));
            assert!(!node.actions.contains(&AccessibilityAction::Increment));
            assert!(!node.actions.contains(&AccessibilityAction::SetValue));
        });
    }
}
