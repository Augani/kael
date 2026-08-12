//! Textarea component - Multi-line text input component.

use crate::astryx;
use crate::components::{
    field::{Field, FieldStatusType},
    icon::Icon,
    icon_source::IconSource,
    input::{InputColors, InputSize, InputVariant},
    spinner::{Spinner, SpinnerSize},
};
use crate::theme::Theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(Clone)]
struct MaxLengthMask(usize);

impl kael::InputMask for MaxLengthMask {
    fn correct(&self, _was: &str, _cursor: usize, now: &mut String, new_cursor: &mut usize) {
        if let Some((cutoff, _)) = now.char_indices().nth(self.0) {
            now.truncate(cutoff);
            *new_cursor = (*new_cursor).min(cutoff);
        }
    }
}

#[derive(IntoElement)]
pub struct Textarea {
    id: SharedString,
    label: Option<SharedString>,
    label_hidden: bool,
    description: Option<SharedString>,
    optional: bool,
    required: bool,
    value: SharedString,
    placeholder: SharedString,
    variant: InputVariant,
    size: InputSize,
    disabled: bool,
    error: bool,
    loading: bool,
    status: Option<(FieldStatusType, SharedString)>,
    start_icon: Option<IconSource>,
    rows: usize,
    min_rows: Option<usize>,
    max_rows: Option<usize>,
    auto_grow: bool,
    resizable: bool,
    max_length: Option<usize>,
    spell_check: bool,
    auto_focus: bool,
    controller: Option<TextInputController>,
    html_name: Option<SharedString>,
    on_change: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_blur: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    on_focus: Option<Rc<dyn Fn(SharedString, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl Textarea {
    pub fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: None,
            label_hidden: false,
            description: None,
            optional: false,
            required: false,
            value: "".into(),
            placeholder: "".into(),
            variant: InputVariant::Default,
            size: InputSize::default(),
            disabled: false,
            error: false,
            loading: false,
            status: None,
            start_icon: None,
            rows: 3,
            min_rows: None,
            max_rows: None,
            auto_grow: false,
            resizable: true,
            max_length: None,
            spell_check: true,
            auto_focus: false,
            controller: None,
            html_name: None,
            on_change: None,
            on_blur: None,
            on_focus: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn is_optional(mut self, optional: bool) -> Self {
        self.optional = optional;
        self
    }

    #[allow(non_snake_case)]
    pub fn isOptional(self, optional: bool) -> Self {
        self.is_optional(optional)
    }

    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    #[allow(non_snake_case)]
    pub fn isRequired(self, required: bool) -> Self {
        self.required(required)
    }

    pub fn value(mut self, value: impl Into<SharedString>) -> Self {
        self.value = value.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn variant(mut self, variant: InputVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: InputSize) -> Self {
        self.size = size;
        self
    }

    /// Use a fully custom color set, setting the variant to [`InputVariant::Custom`].
    pub fn colors(mut self, colors: InputColors) -> Self {
        self.variant = InputVariant::Custom(colors);
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

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    pub fn status(mut self, status: FieldStatusType, message: impl Into<SharedString>) -> Self {
        self.error = matches!(status, FieldStatusType::Error | FieldStatusType::Warning);
        self.status = Some((status, message.into()));
        self
    }

    pub fn is_loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    #[allow(non_snake_case)]
    pub fn isLoading(self, loading: bool) -> Self {
        self.is_loading(loading)
    }

    pub fn start_icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.start_icon = Some(icon.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn startIcon(self, icon: impl Into<IconSource>) -> Self {
        self.start_icon(icon)
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    pub fn min_rows(mut self, min_rows: usize) -> Self {
        self.min_rows = Some(min_rows.max(1));
        self
    }

    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows.max(1));
        self
    }

    pub fn auto_grow(mut self, auto_grow: bool) -> Self {
        self.auto_grow = auto_grow;
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxLength(self, max_length: usize) -> Self {
        self.max_length(max_length)
    }

    pub fn has_spell_check(mut self, spell_check: bool) -> Self {
        self.spell_check = spell_check;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasSpellCheck(self, spell_check: bool) -> Self {
        self.has_spell_check(spell_check)
    }

    pub fn has_auto_focus(mut self, auto_focus: bool) -> Self {
        self.auto_focus = auto_focus;
        self
    }

    #[allow(non_snake_case)]
    pub fn hasAutoFocus(self, auto_focus: bool) -> Self {
        self.has_auto_focus(auto_focus)
    }

    /// Attach an externally owned controller for focus and selection placement.
    pub fn controller(mut self, controller: TextInputController) -> Self {
        self.controller = Some(controller);
        self
    }

    pub fn html_name(mut self, name: impl Into<SharedString>) -> Self {
        self.html_name = Some(name.into());
        self
    }

    #[allow(non_snake_case)]
    pub fn htmlName(self, name: impl Into<SharedString>) -> Self {
        self.html_name(name)
    }

    pub fn on_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(callback));
        self
    }

    #[allow(non_snake_case)]
    pub fn onChange<F>(self, callback: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_change(callback)
    }

    pub fn on_blur<F>(mut self, callback: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_blur = Some(Rc::new(callback));
        self
    }

    pub fn on_focus<F>(mut self, callback: F) -> Self
    where
        F: Fn(SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_focus = Some(Rc::new(callback));
        self
    }

    fn calculate_height(&self) -> Pixels {
        let line_height = match self.size {
            InputSize::Sm => 18.0,
            InputSize::Md => 20.0,
            InputSize::Lg => 22.0,
        };
        let padding_y = match self.size {
            InputSize::Sm => 4.0,
            InputSize::Md => 6.0,
            InputSize::Lg => 8.0,
        };
        let rows = self.min_rows.map_or(self.rows, |min| self.rows.max(min));
        px(rows as f32 * line_height + padding_y * 2.0)
    }

    fn padding_y(&self) -> Pixels {
        match self.size {
            InputSize::Sm => px(4.0),
            InputSize::Md => px(6.0),
            InputSize::Lg => px(8.0),
        }
    }

    fn font_size(&self) -> Pixels {
        match self.size {
            InputSize::Sm => px(13.0),
            InputSize::Md => px(14.0),
            InputSize::Lg => px(16.0),
        }
    }
}

impl Styled for Textarea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Textarea {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style.clone();
        let height = self.calculate_height();
        let padding_y = self.padding_y();
        let font_size = self.font_size();
        let is_busy = self.loading;
        let effectively_disabled = self.disabled || is_busy;

        let (bg_color, border_color, text_color) = if effectively_disabled {
            (
                theme.tokens.muted.opacity(0.5),
                theme.tokens.border,
                theme.tokens.muted_foreground,
            )
        } else if self.error {
            match self.variant {
                InputVariant::Default => (
                    theme.tokens.card,
                    theme.tokens.destructive,
                    theme.tokens.foreground,
                ),
                InputVariant::Outline => (
                    theme.tokens.card,
                    theme.tokens.destructive,
                    theme.tokens.foreground,
                ),
                InputVariant::Ghost => (
                    kael::transparent_black(),
                    theme.tokens.destructive.opacity(0.3),
                    theme.tokens.foreground,
                ),
                InputVariant::Custom(colors) => {
                    (colors.background, theme.tokens.destructive, colors.text)
                }
            }
        } else {
            match self.variant {
                InputVariant::Default => (
                    theme.tokens.card,
                    theme.tokens.input,
                    theme.tokens.foreground,
                ),
                InputVariant::Outline => (
                    theme.tokens.card,
                    theme.tokens.border,
                    theme.tokens.foreground,
                ),
                InputVariant::Ghost => (
                    kael::transparent_black(),
                    theme.tokens.border.opacity(0.3),
                    theme.tokens.foreground,
                ),
                InputVariant::Custom(colors) => (colors.background, colors.border, colors.text),
            }
        };

        let textarea_id = self.id.clone();
        let has_value = !self.value.is_empty();
        let value_len = self.value.chars().count();
        let over_limit = self
            .max_length
            .is_some_and(|max_length| value_len > max_length);
        let hover_ring = astryx::input_hover_ring(if self.error {
            theme.tokens.destructive
        } else {
            theme.tokens.input
        });
        let focus_ring = astryx::focus_ring(if self.error {
            theme.tokens.destructive
        } else {
            theme.tokens.primary
        });

        let max_visible_rows = self.max_rows.unwrap_or(self.rows).max(1);
        let accessibility_label = self.label.clone();
        let accessibility_description = self.description.clone();
        let mut editor = text_input(
            (ElementId::Name(self.id.clone()), "editor"),
            self.value.clone(),
        )
        .placeholder(self.placeholder.clone())
        .multi_line()
        .max_lines(max_visible_rows);
        if let Some(controller) = self.controller {
            editor = editor.controller(controller);
        }
        if let Some(label) = accessibility_label {
            editor = editor.accessibility_label(label);
        }
        if let Some(description) = accessibility_description {
            editor = editor.accessibility_description(description);
        }
        if let Some(max_length) = self.max_length {
            editor = editor.mask(MaxLengthMask(max_length));
        }
        if let Some(on_change) = self.on_change.clone() {
            editor = editor.on_change(move |value, window, cx| {
                on_change(value, window, cx);
            });
        }
        if let Some(on_focus) = self.on_focus.clone() {
            editor = editor.on_focus(move |value, window, cx| {
                on_focus(value, window, cx);
            });
        }
        if let Some(on_blur) = self.on_blur.clone() {
            editor = editor.on_blur(move |value, window, cx| {
                on_blur(value, window, cx);
            });
        }

        let editor = editor.render_with({
            let foreground = text_color;
            let muted_foreground = theme.tokens.muted_foreground;
            let primary = if self.error {
                theme.tokens.destructive
            } else {
                theme.tokens.primary
            };
            let radius = theme.tokens.radius_md;
            move |render_state, window, cx| {
                render_state.paint_selection(primary.opacity(0.22), window);
                window.with_text_style(
                    Some(TextStyleRefinement {
                        color: Some(if render_state.showing_placeholder {
                            muted_foreground
                        } else {
                            foreground
                        }),
                        ..Default::default()
                    }),
                    |window| render_state.paint_text(window, cx),
                );
                render_state.paint_cursor(primary, window);
                if render_state.focused {
                    window.paint_quad(
                        outline(
                            render_state.outer_bounds,
                            primary.opacity(0.5),
                            BorderStyle::default(),
                        )
                        .corner_radii(radius),
                    );
                }
            }
        });

        let content = if effectively_disabled {
            div()
                .flex_1()
                .min_w_0()
                .text_size(font_size)
                .font_family(theme.tokens.font_family.clone())
                .text_color(text_color)
                .line_height(relative(1.4))
                .child(if has_value {
                    self.value.to_string()
                } else {
                    self.placeholder.to_string()
                })
                .when(!has_value, |this| {
                    this.text_color(theme.tokens.muted_foreground)
                })
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_w_0()
                .size_full()
                .text_size(font_size)
                .font_family(theme.tokens.font_family.clone())
                .text_color(text_color)
                .line_height(relative(1.4))
                .child(editor)
                .into_any_element()
        };

        let mut control = div()
            .id(textarea_id)
            .relative()
            .w_full()
            .min_h(height)
            .when(!self.auto_grow, |this| this.h(height))
            .px(px(8.0))
            .py(padding_y)
            .bg(bg_color)
            .border_1()
            .border_color(if self.error {
                theme.tokens.destructive
            } else {
                border_color
            })
            .rounded(theme.tokens.radius_md)
            .font_family(theme.tokens.font_family.clone())
            .transition(theme.tokens.transition_fast)
            .shadow(smallvec::smallvec![astryx::focus_ring(
                kael::transparent_black()
            )])
            .when(!effectively_disabled, |this| {
                this.hover(|style| {
                    style
                        .border_color(if self.error {
                            theme.tokens.destructive
                        } else {
                            theme.tokens.input
                        })
                        .shadow(smallvec::smallvec![hover_ring])
                })
            })
            .when(self.error, |this| {
                this.shadow(smallvec::smallvec![focus_ring])
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.0))
                    .size_full()
                    .child(
                        div()
                            .when_some(self.start_icon.clone(), |this, icon| {
                                this.child(
                                    Icon::new(icon)
                                        .size(px(16.0))
                                        .color(theme.tokens.muted_foreground),
                                )
                            })
                            .mt(px(2.0)),
                    )
                    .child(content),
            )
            .when(is_busy, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(8.0))
                        .right(px(8.0))
                        .child(Spinner::new().size(SpinnerSize::Sm)),
                )
            });

        if let Some((status, _)) = self.status.clone() {
            let status_color = match status {
                FieldStatusType::Warning => theme.tokens.warning,
                FieldStatusType::Error => theme.tokens.destructive,
                FieldStatusType::Success => theme.tokens.success,
            };
            let status_icon = match status {
                FieldStatusType::Warning => "triangle-alert",
                FieldStatusType::Error => "circle-alert",
                FieldStatusType::Success => "circle-check",
            };

            control = control.child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .right(px(8.0))
                    .child(Icon::new(status_icon).size(px(16.0)).color(status_color)),
            );
        }

        let control_with_counter = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(control)
            .when_some(self.max_length, |this, max_length| {
                this.child(
                    div()
                        .flex()
                        .justify_end()
                        .text_size(px(12.0))
                        .line_height(px(16.0))
                        .font_family(theme.tokens.font_family.clone())
                        .text_color(if over_limit {
                            theme.tokens.destructive
                        } else {
                            theme.tokens.muted_foreground
                        })
                        .child(format!("{value_len}/{max_length}")),
                )
            });

        match self.label {
            Some(label) => {
                let mut field = Field::new(label, control_with_counter)
                    .hidden_label(self.label_hidden)
                    .optional(self.optional)
                    .required(self.required)
                    .disabled(self.disabled);

                if let Some(description) = self.description {
                    field = field.description(description);
                }

                if let Some((status, message)) = self.status {
                    field = field.status(status, message);
                }

                field.into_any_element()
            }
            None => control_with_counter.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use kael::{AccessibilityRole, Render, TestAppContext, TextInputController, Window};

    use super::Textarea;

    struct Host {
        controller: TextInputController,
    }

    impl Render for Host {
        fn render(
            &mut self,
            _window: &mut Window,
            _cx: &mut kael::Context<Self>,
        ) -> impl kael::IntoElement {
            Textarea::new("notes")
                .label("Meeting notes")
                .description("Describe the decision")
                .value("Approved")
                .controller(self.controller.clone())
        }
    }

    #[kael::test]
    fn label_and_description_are_bound_to_the_text_input(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::components::input::init(cx);
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (host, window) = cx.add_window_view(|_, cx| Host {
            controller: TextInputController::new(cx.focus_handle()),
        });
        window.update(|window, cx| {
            window.draw(cx).clear();
            host.read(cx).controller.focus(window);
            assert!(host.read(cx).controller.focus_handle().is_focused(window));
            let node = window
                .accessibility_tree()
                .nodes
                .values()
                .find(|node| node.role == AccessibilityRole::TextInput)
                .expect("textarea should expose its text input");
            assert_eq!(node.label.as_deref(), Some("Meeting notes"));
            assert_eq!(node.description.as_deref(), Some("Describe the decision"));
        });
    }
}
