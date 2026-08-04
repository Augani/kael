//! ColorPicker component - Full-featured color selection with HSL/RGB/HEX modes.

use crate::components::slider::{Slider, SliderSize, SliderState};
use crate::components::text::{Text, TextVariant};
use crate::overlays::popover::{Popover, PopoverContent};
use crate::styled_ext::StyledExt;
use crate::theme::{Theme, use_theme};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

const MAX_RECENT_COLORS: usize = 10;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ColorMode {
    HSL,
    RGB,
    HEX,
}

/// State for managing color picker interactions
pub struct ColorPickerState {
    selected_color: Hsla,
    mode: ColorMode,
    recent_colors: Vec<Hsla>,
}

impl ColorPickerState {
    pub fn new(initial_color: Hsla) -> Self {
        let mut state = Self {
            selected_color: hsla(0.0, 0.0, 0.0, 1.0),
            mode: ColorMode::HSL,
            recent_colors: Vec::new(),
        };
        state.set_color(initial_color);
        state
    }

    pub fn set_hue(&mut self, hue: f32) {
        if hue.is_finite() {
            self.selected_color.h = hue.clamp(0.0, 360.0) / 360.0;
        }
    }

    pub fn set_saturation(&mut self, saturation: f32) {
        if saturation.is_finite() {
            self.selected_color.s = saturation.clamp(0.0, 1.0);
        }
    }

    pub fn set_lightness(&mut self, lightness: f32) {
        if lightness.is_finite() {
            self.selected_color.l = lightness.clamp(0.0, 1.0);
        }
    }

    pub fn set_alpha(&mut self, alpha: f32) {
        if alpha.is_finite() {
            self.selected_color.a = alpha.clamp(0.0, 1.0);
        }
    }

    pub fn set_color(&mut self, color: Hsla) {
        self.selected_color = hsla(
            if color.h.is_finite() {
                color.h.rem_euclid(1.0)
            } else {
                0.0
            },
            if color.s.is_finite() {
                color.s.clamp(0.0, 1.0)
            } else {
                0.0
            },
            if color.l.is_finite() {
                color.l.clamp(0.0, 1.0)
            } else {
                0.0
            },
            if color.a.is_finite() {
                color.a.clamp(0.0, 1.0)
            } else {
                1.0
            },
        );
    }

    pub fn add_to_recent(&mut self, color: Hsla) {
        self.recent_colors
            .retain(|&c| !(c.h == color.h && c.s == color.s && c.l == color.l && c.a == color.a));

        self.recent_colors.insert(0, color);

        if self.recent_colors.len() > MAX_RECENT_COLORS {
            self.recent_colors.truncate(MAX_RECENT_COLORS);
        }
    }

    pub fn selected_color(&self) -> Hsla {
        self.selected_color
    }

    pub fn recent_colors(&self) -> &[Hsla] {
        &self.recent_colors
    }

    pub fn mode(&self) -> ColorMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: ColorMode) {
        self.mode = mode;
    }
}

#[derive(IntoElement)]
pub struct ColorPicker {
    id: ElementId,
    state: Entity<ColorPickerState>,
    show_alpha: bool,
    swatches: Vec<Hsla>,
    on_change: Option<Rc<dyn Fn(Hsla, &mut Window, &mut App)>>,
    disabled: bool,
    style: StyleRefinement,
}

impl ColorPicker {
    /// Create a new color picker with default settings.
    pub fn new(id: impl Into<ElementId>, state: Entity<ColorPickerState>) -> Self {
        Self {
            id: id.into(),
            state,
            show_alpha: true,
            swatches: default_swatches(),
            on_change: None,
            disabled: false,
            style: StyleRefinement::default(),
        }
    }

    /// Enable or disable the alpha/opacity slider.
    pub fn show_alpha(mut self, show: bool) -> Self {
        self.show_alpha = show;
        self
    }

    /// Set custom color swatches.
    pub fn swatches(mut self, swatches: Vec<Hsla>) -> Self {
        self.swatches = swatches;
        self
    }

    /// Set the change callback.
    pub fn on_change(mut self, handler: impl Fn(Hsla, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Enable or disable the color picker.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    /// Convert HSLA color to HEX string
    fn hsla_to_hex(color: Hsla) -> String {
        let (r_byte, g_byte, b_byte) = Self::hsla_to_rgb(color);

        if color.a < 0.999 {
            let alpha = (color.a.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("#{:02X}{:02X}{:02X}{:02X}", r_byte, g_byte, b_byte, alpha)
        } else {
            format!("#{:02X}{:02X}{:02X}", r_byte, g_byte, b_byte)
        }
    }

    /// Convert HSLA color to RGB values (0-255)
    fn hsla_to_rgb(color: Hsla) -> (u8, u8, u8) {
        let h = color.h.rem_euclid(1.0) * 360.0;
        let s = color.s.clamp(0.0, 1.0);
        let l = color.l.clamp(0.0, 1.0);

        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = l - c / 2.0;

        let (r, g, b) = if h < 60.0 {
            (c, x, 0.0)
        } else if h < 120.0 {
            (x, c, 0.0)
        } else if h < 180.0 {
            (0.0, c, x)
        } else if h < 240.0 {
            (0.0, x, c)
        } else if h < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        (
            ((r + m) * 255.0).round() as u8,
            ((g + m) * 255.0).round() as u8,
            ((b + m) * 255.0).round() as u8,
        )
    }
}

impl Styled for ColorPicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ColorPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let state = self.state.clone();
        let color = state.read(cx).selected_color();
        let show_alpha = self.show_alpha;
        let swatches = self.swatches.clone();
        let on_change = self.on_change.clone();
        let disabled = self.disabled;
        let user_style = self.style;
        let picker_id = self.id.clone();
        let hover_ring = crate::astryx::input_hover_ring(theme.tokens.input);
        let trigger_id = ElementId::NamedChild(Box::new(picker_id.clone()), "trigger".into());
        let trigger_focus = window
            .use_keyed_state(trigger_id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let open_state = window.use_keyed_state(
            ElementId::NamedChild(Box::new(picker_id.clone()), "open".into()),
            cx,
            |_, _| false,
        );
        let is_open = *open_state.read(cx);
        let open_state_for_popover = open_state.clone();
        let trigger_focus_on_mouse = trigger_focus.clone();
        let picker_id_for_slider = picker_id.clone();
        let hue_slider = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(picker_id_for_slider.clone()), "hue".into()),
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        let mut state = SliderState::new(cx);
                        state.set_max(360.0, cx);
                        state.set_step(1.0, cx);
                        state
                    })
                },
            )
            .read(cx)
            .clone();
        let saturation_slider = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(picker_id_for_slider.clone()), "saturation".into()),
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        let mut state = SliderState::new(cx);
                        state.set_max(100.0, cx);
                        state.set_step(1.0, cx);
                        state
                    })
                },
            )
            .read(cx)
            .clone();
        let lightness_slider = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(picker_id_for_slider.clone()), "lightness".into()),
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        let mut state = SliderState::new(cx);
                        state.set_max(100.0, cx);
                        state.set_step(1.0, cx);
                        state
                    })
                },
            )
            .read(cx)
            .clone();
        let alpha_slider = window
            .use_keyed_state(
                ElementId::NamedChild(Box::new(picker_id_for_slider), "alpha".into()),
                cx,
                |_, cx| {
                    cx.new(|cx| {
                        let mut state = SliderState::new(cx);
                        state.set_max(100.0, cx);
                        state.set_step(1.0, cx);
                        state
                    })
                },
            )
            .read(cx)
            .clone();
        hue_slider.update(cx, |slider, cx| {
            slider.set_value(color.h * 360.0, cx);
        });
        saturation_slider.update(cx, |slider, cx| {
            slider.set_value(color.s * 100.0, cx);
        });
        lightness_slider.update(cx, |slider, cx| {
            slider.set_value(color.l * 100.0, cx);
        });
        alpha_slider.update(cx, |slider, cx| {
            slider.set_value(color.a * 100.0, cx);
        });
        let mut trigger_state = if is_open {
            AccessibilityState::EXPANDED
        } else {
            AccessibilityState::COLLAPSED
        };
        if disabled {
            trigger_state |= AccessibilityState::DISABLED;
        }
        let mut trigger_accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label("Choose color")
            .value(AccessibilityValue::Text(Self::hsla_to_hex(color)))
            .states(trigger_state);
        if !disabled {
            trigger_accessibility = trigger_accessibility
                .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let preview_button = div()
            .id(trigger_id)
            .accessibility(trigger_accessibility)
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(32.0))
            .px(px(8.0))
            .bg(theme.tokens.card)
            .border_1()
            .border_color(theme.tokens.input)
            .rounded(theme.tokens.radius_md)
            .transition(theme.tokens.transition_fast)
            .when(!disabled, |this| {
                this.track_focus(&trigger_focus.tab_index(0).tab_stop(true))
                    .cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.shadow(smallvec::smallvec![hover_ring]))
                    .focus_visible(|style| {
                        style.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                            theme.tokens.ring,
                        )])
                    })
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        window.focus(&trigger_focus_on_mouse);
                    })
            })
            .when(disabled, |this| this.opacity(0.5))
            .child(
                div()
                    .size(px(20.0))
                    .rounded(theme.tokens.radius_sm)
                    .bg(color)
                    .border_1()
                    .border_color(theme.tokens.border),
            )
            .child(
                Text::new(Self::hsla_to_hex(color))
                    .variant(TextVariant::Custom)
                    .size(px(14.0))
                    .color(theme.tokens.foreground),
            )
            .map(|mut this| {
                this.style().refine(&user_style);
                this
            });

        if disabled {
            return preview_button.into_any_element();
        }

        Popover::new(picker_id)
            .disabled(disabled)
            .on_open_change(move |open, _, cx| {
                open_state_for_popover.update(cx, |state, cx| {
                    *state = open;
                    cx.notify();
                });
            })
            .trigger(preview_button)
            .content(move |window, cx| {
                let swatches_for_content = swatches.clone();
                let on_change_for_content = on_change.clone();
                let state_for_content = state.clone();
                let hue_slider_for_content = hue_slider.clone();
                let saturation_slider_for_content = saturation_slider.clone();
                let lightness_slider_for_content = lightness_slider.clone();
                let alpha_slider_for_content = alpha_slider.clone();

                cx.new(|cx| {
                    PopoverContent::new(window, cx, move |_window, cx| {
                        let _theme = use_theme();

                        // Read state fresh on every render so mode changes work
                        let current_color = state_for_content.read(cx).selected_color();
                        let current_mode = state_for_content.read(cx).mode();
                        let recent_vec = state_for_content.read(cx).recent_colors().to_vec();

                        let swatches_clone = swatches_for_content.clone();
                        let on_change_clone = on_change_for_content.clone();

                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.0))
                            .w(px(280.0))
                            .child(render_color_preview(current_color))
                            .child(render_mode_selector(
                                current_mode,
                                state_for_content.clone(),
                            ))
                            .child(render_color_value(current_color, current_mode))
                            .child(render_color_controls(
                                show_alpha,
                                hue_slider_for_content.clone(),
                                saturation_slider_for_content.clone(),
                                lightness_slider_for_content.clone(),
                                alpha_slider_for_content.clone(),
                                state_for_content.clone(),
                                on_change_clone.clone(),
                            ))
                            .when(!swatches_clone.is_empty(), |this| {
                                this.child(render_swatches(
                                    swatches_clone,
                                    state_for_content.clone(),
                                    on_change_clone.clone(),
                                ))
                            })
                            .when(!recent_vec.is_empty(), |this| {
                                this.child(render_recent_colors(
                                    recent_vec,
                                    state_for_content.clone(),
                                    on_change_clone.clone(),
                                ))
                            })
                            .child(render_actions(current_color, state_for_content.clone()))
                            .into_any_element()
                    })
                })
            })
            .into_any_element()
    }
}

fn render_color_preview(color: Hsla) -> impl IntoElement {
    let theme = use_theme();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            Text::new("Selected Color")
                .variant(TextVariant::Custom)
                .size(px(12.0))
                .color(theme.tokens.muted_foreground),
        )
        .child(
            div()
                .w_full()
                .h(px(80.0))
                .rounded(theme.tokens.radius_md)
                .bg(color)
                .border_1()
                .border_color(theme.tokens.border),
        )
}

fn render_mode_selector(
    current_mode: ColorMode,
    state: Entity<ColorPickerState>,
) -> impl IntoElement {
    let theme = use_theme();

    div()
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group).label("Color format"))
        .flex()
        .gap_1()
        .p(px(2.0))
        .bg(theme.tokens.muted.opacity(0.35))
        .rounded(theme.tokens.radius_md)
        .child(render_mode_button(
            "HSL",
            ColorMode::HSL,
            current_mode,
            state.clone(),
        ))
        .child(render_mode_button(
            "RGB",
            ColorMode::RGB,
            current_mode,
            state.clone(),
        ))
        .child(render_mode_button(
            "HEX",
            ColorMode::HEX,
            current_mode,
            state,
        ))
}

fn render_mode_button(
    label: &'static str,
    mode: ColorMode,
    current_mode: ColorMode,
    state: Entity<ColorPickerState>,
) -> impl IntoElement {
    let theme = use_theme();
    let is_active = mode == current_mode;
    let id = ElementId::Name(
        format!(
            "color-picker-mode-{}-{}",
            state.entity_id().as_u64(),
            label.to_ascii_lowercase()
        )
        .into(),
    );
    button(id)
        .role(AccessibilityRole::RadioButton)
        .label(label)
        .checked(is_active)
        .on_click(move |_, window, cx| {
            state.update(cx, |state, cx| {
                state.set_mode(mode);
                cx.notify();
            });
            window.refresh();
        })
        .render_with(move |button_state, _, _| {
            div()
                .flex_1()
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.tokens.radius_sm)
                .text_size(px(12.0))
                .text_align(TextAlign::Center)
                .when(is_active, |this| {
                    this.bg(theme.tokens.primary)
                        .text_color(theme.tokens.primary_foreground)
                })
                .when(!is_active, |this| {
                    this.text_color(theme.tokens.foreground).hover(|style| {
                        style.bg(crate::astryx::overlay_hover(
                            theme.tokens.background.l < 0.5,
                        ))
                    })
                })
                .when(button_state.focused, |this| {
                    this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                        theme.tokens.ring,
                    )])
                })
                .child(label)
                .into_any_element()
        })
}

fn render_color_value(color: Hsla, mode: ColorMode) -> impl IntoElement {
    let theme = use_theme();

    let value = match mode {
        ColorMode::HSL => {
            if color.a < 0.999 {
                format!(
                    "hsla({:.0}, {:.0}%, {:.0}%, {:.2})",
                    color.h * 360.0,
                    color.s * 100.0,
                    color.l * 100.0,
                    color.a,
                )
            } else {
                format!(
                    "hsl({:.0}, {:.0}%, {:.0}%)",
                    color.h * 360.0,
                    color.s * 100.0,
                    color.l * 100.0
                )
            }
        }
        ColorMode::RGB => {
            let (r, g, b) = ColorPicker::hsla_to_rgb(color);
            if color.a < 0.999 {
                format!("rgba({}, {}, {}, {:.2})", r, g, b, color.a)
            } else {
                format!("rgb({}, {}, {})", r, g, b)
            }
        }
        ColorMode::HEX => ColorPicker::hsla_to_hex(color),
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .p(px(8.0))
        .bg(theme.tokens.muted.opacity(0.35))
        .rounded(theme.tokens.radius_sm)
        .border_1()
        .border_color(theme.tokens.border.opacity(0.6))
        .child(
            Text::new(value)
                .variant(TextVariant::Custom)
                .size(px(13.0))
                .color(theme.tokens.foreground),
        )
}

fn render_color_controls(
    show_alpha: bool,
    hue: Entity<SliderState>,
    saturation: Entity<SliderState>,
    lightness: Entity<SliderState>,
    alpha: Entity<SliderState>,
    state: Entity<ColorPickerState>,
    on_change: Option<Rc<dyn Fn(Hsla, &mut Window, &mut App)>>,
) -> impl IntoElement {
    let theme = use_theme();

    let row = |label: &'static str, slider: Slider| {
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .w(px(72.0))
                    .flex_shrink_0()
                    .text_size(px(12.0))
                    .text_color(theme.tokens.muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().min_w(px(0.0)).child(slider))
    };

    let hue_state = state.clone();
    let hue_change = on_change.clone();
    let saturation_state = state.clone();
    let saturation_change = on_change.clone();
    let lightness_state = state.clone();
    let lightness_change = on_change.clone();
    let alpha_state = state;

    div()
        .accessibility(
            AccessibilityAttributes::new(AccessibilityRole::Group).label("Color channels"),
        )
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(row(
            "Hue",
            Slider::new(hue)
                .size(SliderSize::Sm)
                .show_value(true)
                .accessibility_label("Hue in degrees")
                .on_change(move |value, window, cx| {
                    let color = hue_state.update(cx, |state, cx| {
                        state.set_hue(value);
                        cx.notify();
                        state.selected_color()
                    });
                    if let Some(handler) = hue_change.as_ref() {
                        handler(color, window, cx);
                    }
                    window.refresh();
                }),
        ))
        .child(row(
            "Saturation",
            Slider::new(saturation)
                .size(SliderSize::Sm)
                .show_value(true)
                .accessibility_label("Saturation percentage")
                .on_change(move |value, window, cx| {
                    let color = saturation_state.update(cx, |state, cx| {
                        state.set_saturation(value / 100.0);
                        cx.notify();
                        state.selected_color()
                    });
                    if let Some(handler) = saturation_change.as_ref() {
                        handler(color, window, cx);
                    }
                    window.refresh();
                }),
        ))
        .child(row(
            "Lightness",
            Slider::new(lightness)
                .size(SliderSize::Sm)
                .show_value(true)
                .accessibility_label("Lightness percentage")
                .on_change(move |value, window, cx| {
                    let color = lightness_state.update(cx, |state, cx| {
                        state.set_lightness(value / 100.0);
                        cx.notify();
                        state.selected_color()
                    });
                    if let Some(handler) = lightness_change.as_ref() {
                        handler(color, window, cx);
                    }
                    window.refresh();
                }),
        ))
        .when(show_alpha, |this| {
            this.child(row(
                "Opacity",
                Slider::new(alpha)
                    .size(SliderSize::Sm)
                    .show_value(true)
                    .accessibility_label("Opacity percentage")
                    .on_change(move |value, window, cx| {
                        let color = alpha_state.update(cx, |state, cx| {
                            state.set_alpha(value / 100.0);
                            cx.notify();
                            state.selected_color()
                        });
                        if let Some(handler) = on_change.as_ref() {
                            handler(color, window, cx);
                        }
                        window.refresh();
                    }),
            ))
        })
}

fn render_swatches(
    swatches: Vec<Hsla>,
    state: Entity<ColorPickerState>,
    on_change: Option<Rc<dyn Fn(Hsla, &mut Window, &mut App)>>,
) -> impl IntoElement {
    let theme = use_theme();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            Text::new("Swatches")
                .variant(TextVariant::Custom)
                .size(px(12.0))
                .color(theme.tokens.muted_foreground),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(
                    swatches
                        .into_iter()
                        .enumerate()
                        .map(move |(index, swatch)| {
                            render_color_swatch(
                                "swatch",
                                index,
                                swatch,
                                state.clone(),
                                on_change.clone(),
                            )
                        }),
                ),
        )
}

fn render_recent_colors(
    recent: Vec<Hsla>,
    state: Entity<ColorPickerState>,
    on_change: Option<Rc<dyn Fn(Hsla, &mut Window, &mut App)>>,
) -> impl IntoElement {
    let theme = use_theme();

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            Text::new("Recent Colors")
                .variant(TextVariant::Custom)
                .size(px(12.0))
                .color(theme.tokens.muted_foreground),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(recent.into_iter().enumerate().map(move |(index, color)| {
                    render_color_swatch("recent", index, color, state.clone(), on_change.clone())
                })),
        )
}

fn render_color_swatch(
    group: &'static str,
    index: usize,
    color: Hsla,
    state: Entity<ColorPickerState>,
    on_change: Option<Rc<dyn Fn(Hsla, &mut Window, &mut App)>>,
) -> impl IntoElement {
    let theme = use_theme();
    let hex = ColorPicker::hsla_to_hex(color);
    let id = ElementId::Name(
        format!(
            "color-picker-{}-{}-{}",
            state.entity_id().as_u64(),
            group,
            index
        )
        .into(),
    );

    button(id)
        .label(format!("Select color {hex}"))
        .on_click(move |_, window, cx| {
            state.update(cx, |state, cx| {
                state.set_color(color);
                cx.notify();
            });

            if let Some(handler) = on_change.as_ref() {
                handler(color, window, cx);
            }
            window.refresh();
        })
        .render_with(move |button_state, _, _| {
            div()
                .size(px(28.0))
                .rounded(theme.tokens.radius_sm)
                .bg(color)
                .border_1()
                .border_color(theme.tokens.border)
                .hover(|style| style.inset_ring(crate::astryx::overlay_hover(false), px(2.0)))
                .when(button_state.focused, |this| {
                    this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                        theme.tokens.ring,
                    )])
                })
                .into_any_element()
        })
}

fn render_actions(color: Hsla, state: Entity<ColorPickerState>) -> impl IntoElement {
    let theme = use_theme();
    let copy_theme = theme.clone();
    let copy_id =
        ElementId::Name(format!("color-picker-copy-{}", state.entity_id().as_u64()).into());
    let apply_id =
        ElementId::Name(format!("color-picker-apply-{}", state.entity_id().as_u64()).into());

    div()
        .flex()
        .gap_2()
        .child(
            button(copy_id)
                .label("Copy color value")
                .on_click(move |_, _, cx| {
                    let hex = ColorPicker::hsla_to_hex(color);
                    cx.write_to_clipboard(ClipboardItem::new_string(hex));
                })
                .render_with(move |button_state, _, _| {
                    div()
                        .flex_1()
                        .py(px(8.0))
                        .px(px(12.0))
                        .bg(copy_theme.tokens.secondary)
                        .text_color(copy_theme.tokens.secondary_foreground)
                        .rounded(copy_theme.tokens.radius_sm)
                        .text_size(px(13.0))
                        .text_align(TextAlign::Center)
                        .hover(|style| style.bg(copy_theme.tokens.secondary.opacity(0.8)))
                        .when(button_state.focused, |this| {
                            this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                                copy_theme.tokens.ring
                            )])
                        })
                        .child("Copy")
                        .into_any_element()
                }),
        )
        .child(
            button(apply_id)
                .label("Save color to recent colors")
                .on_click(move |_, _, cx| {
                    state.update(cx, |state, cx| {
                        state.add_to_recent(color);
                        cx.notify();
                    });
                })
                .render_with(move |button_state, _, _| {
                    div()
                        .flex_1()
                        .py(px(8.0))
                        .px(px(12.0))
                        .bg(theme.tokens.primary)
                        .text_color(theme.tokens.primary_foreground)
                        .rounded(theme.tokens.radius_sm)
                        .text_size(px(13.0))
                        .text_align(TextAlign::Center)
                        .hover(|style| style.bg(theme.tokens.primary.opacity(0.9)))
                        .when(button_state.focused, |this| {
                            this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                                theme.tokens.ring
                            )])
                        })
                        .child("Save")
                        .into_any_element()
                }),
        )
}

fn default_swatches() -> Vec<Hsla> {
    crate::astryx::CHART_PALETTE
        .iter()
        .map(|color| rgba((*color << 8) | 0xFF).into())
        .chain([
            rgba(0x0A1317FF).into(),
            rgba(0x647685FF).into(),
            rgba(0xE7EAEDFF).into(),
            rgba(0xFFFFFFFF).into(),
        ])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ColorPicker, ColorPickerState, MAX_RECENT_COLORS};
    use kael::hsla;

    #[test]
    fn color_conversion_rounds_channels_and_preserves_alpha() {
        assert_eq!(
            ColorPicker::hsla_to_hex(hsla(0.0, 1.0, 0.5, 1.0)),
            "#FF0000"
        );
        assert_eq!(
            ColorPicker::hsla_to_hex(hsla(1.0 / 3.0, 1.0, 0.5, 0.5)),
            "#00FF0080"
        );
        assert_eq!(
            ColorPicker::hsla_to_rgb(hsla(0.0, 0.0, 0.5, 1.0)),
            (128, 128, 128)
        );
    }

    #[test]
    fn state_sanitizes_channels_and_bounds_recent_colors() {
        let mut state = ColorPickerState::new(hsla(f32::NAN, 2.0, -1.0, f32::NAN));
        assert_eq!(state.selected_color(), hsla(0.0, 1.0, 0.0, 1.0));

        for index in 0..(MAX_RECENT_COLORS + 3) {
            state.add_to_recent(hsla(index as f32 / 20.0, 0.5, 0.5, 1.0));
        }
        assert_eq!(state.recent_colors().len(), MAX_RECENT_COLORS);

        let newest = state.recent_colors()[0];
        state.add_to_recent(newest);
        assert_eq!(state.recent_colors()[0], newest);
        assert_eq!(state.recent_colors().len(), MAX_RECENT_COLORS);
    }
}
