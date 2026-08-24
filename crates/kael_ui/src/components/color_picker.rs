//! ColorPicker component - Full-featured color selection with HSL/RGB/HEX modes.

use crate::components::input::{Input, InputState};
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

/// Parse a typed hex color (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`,
/// case-insensitive, `#` optional) into an [`Hsla`]. Returns `None` for any
/// other input; callers must not commit invalid values.
pub fn parse_hex_color(input: &str) -> Option<Hsla> {
    let hex = input.trim().strip_prefix('#').unwrap_or(input.trim());
    if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |slice: &str| -> Option<u16> { u16::from_str_radix(slice, 16).ok() };
    let mut channels = [0u8; 4];
    match hex.len() {
        3 | 4 => {
            for (index, out) in hex.as_bytes().chunks(1).enumerate() {
                let digit = char::from(out[0]).to_digit(16)?;
                // #RGB expands by doubling each nibble (CSS semantics).
                channels[index] = (digit * 17) as u8;
            }
            if hex.len() == 3 {
                channels[3] = 255;
            }
        }
        6 | 8 => {
            for (index, pair) in hex.as_bytes().chunks(2).enumerate() {
                let pair = std::str::from_utf8(pair).ok()?;
                channels[index] = channel(pair)? as u8;
            }
            if hex.len() == 6 {
                channels[3] = 255;
            }
        }
        _ => return None,
    }
    Some(rgb_bytes_to_hsla(
        channels[0],
        channels[1],
        channels[2],
        channels[3],
    ))
}

fn rgb_bytes_to_hsla(r: u8, g: u8, b: u8, a: u8) -> Hsla {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.0;
    let s = if delta < f32::EPSILON {
        0.0
    } else if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };
    let h = if delta < f32::EPSILON {
        0.0
    } else if (max - r).abs() < f32::EPSILON {
        ((g - b) / delta + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if (max - g).abs() < f32::EPSILON {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    hsla(h, s, l, f32::from(a) / 255.0)
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

        Popover::new(picker_id.clone())
            .disabled(disabled)
            .on_open_change(move |open, _, cx| {
                open_state_for_popover.update(cx, |state, cx| {
                    *state = open;
                    cx.notify();
                });
            })
            .trigger(preview_button)
            .content(move |window, cx| {
                let picker_id_for_content = picker_id.clone();
                let swatches_for_content = swatches.clone();
                let on_change_for_content = on_change.clone();
                let state_for_content = state.clone();
                let hue_slider_for_content = hue_slider.clone();
                let saturation_slider_for_content = saturation_slider.clone();
                let lightness_slider_for_content = lightness_slider.clone();
                let alpha_slider_for_content = alpha_slider.clone();

                cx.new(|cx| {
                    PopoverContent::new(window, cx, move |window, cx| {
                        let _theme = use_theme();

                        // Read state fresh on every render so mode changes work
                        let current_color = state_for_content.read(cx).selected_color();
                        let current_mode = state_for_content.read(cx).mode();
                        let recent_vec = state_for_content.read(cx).recent_colors().to_vec();

                        let swatches_clone = swatches_for_content.clone();
                        let on_change_clone = on_change_for_content.clone();
                        let on_change_for_hex = on_change_for_content.clone();
                        // Created during render (not at popover-open time) so
                        // keyed element state is available; kept stable across
                        // frames by key.
                        let hex_input = window
                            .use_keyed_state(
                                ElementId::NamedChild(
                                    Box::new(picker_id_for_content.clone()),
                                    "hex-input".into(),
                                ),
                                cx,
                                |_, cx| cx.new(InputState::new),
                            )
                            .read(cx)
                            .clone();
                        // Keep the field in sync with the current color
                        // unless the user is typing in it.
                        if !hex_input.read(cx).focus_handle(cx).is_focused(window) {
                            let current_hex = ColorPicker::hsla_to_hex(
                                state_for_content.read(cx).selected_color(),
                            );
                            hex_input.update(cx, |input, cx| {
                                input.set_value(current_hex, window, cx);
                            });
                        }

                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.0))
                            .w(px(280.0))
                            .child(render_color_preview(current_color))
                            .child(render_mode_selector(
                                window,
                                cx,
                                current_mode,
                                state_for_content.clone(),
                            ))
                            .child(render_color_value(
                                current_color,
                                current_mode,
                                hex_input,
                                state_for_content.clone(),
                                on_change_for_hex,
                            ))
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
    window: &mut Window,
    cx: &mut App,
    current_mode: ColorMode,
    state: Entity<ColorPickerState>,
) -> impl IntoElement {
    let theme = use_theme();
    let modes = [
        ("HSL", ColorMode::HSL),
        ("RGB", ColorMode::RGB),
        ("HEX", ColorMode::HEX),
    ];

    let group_key =
        ElementId::Name(format!("color-picker-modes-{}", state.entity_id().as_u64()).into());
    let handles: Vec<FocusHandle> = modes
        .iter()
        .map(|(label, _)| {
            window
                .use_keyed_state(
                    ElementId::NamedChild(
                        Box::new(group_key.clone()),
                        (*label).to_ascii_lowercase().into(),
                    ),
                    cx,
                    |_, cx| cx.focus_handle(),
                )
                .read(cx)
                .clone()
        })
        .collect();

    div()
        .accessibility(AccessibilityAttributes::new(AccessibilityRole::Group).label("Color format"))
        .flex()
        .gap_1()
        .p(px(2.0))
        .bg(theme.tokens.muted.opacity(0.35))
        .rounded(theme.tokens.radius_md)
        .children(modes.iter().enumerate().map(|(index, (label, mode))| {
            render_mode_button(
                label,
                *mode,
                current_mode,
                handles[index].is_focused(window),
                state.clone(),
                handles.clone(),
                index,
            )
        }))
}

fn render_mode_button(
    label: &'static str,
    mode: ColorMode,
    current_mode: ColorMode,
    is_focused: bool,
    state: Entity<ColorPickerState>,
    handles: Vec<FocusHandle>,
    index: usize,
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
    let mut accessibility_state = AccessibilityState::NONE;
    if is_active {
        accessibility_state |= AccessibilityState::CHECKED;
    }
    if is_focused {
        accessibility_state |= AccessibilityState::FOCUSED;
    }
    let tracked = handles[index]
        .clone()
        .tab_index(if is_active { 0 } else { -1 })
        .tab_stop(is_active);

    div()
        .id(id)
        .accessibility(
            AccessibilityAttributes::new(AccessibilityRole::RadioButton)
                .label(label)
                .states(accessibility_state)
                .actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]),
        )
        .track_focus(&tracked)
        .flex_1()
        .h(px(28.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme.tokens.radius_sm)
        .text_size(px(12.0))
        .cursor(CursorStyle::PointingHand)
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
        .focus_visible(|style| {
            style.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                theme.tokens.ring,
            )])
        })
        .on_mouse_down(MouseButton::Left, {
            let handles = handles.clone();
            move |_, window, _| {
                window.focus(&handles[index]);
            }
        })
        .on_click({
            let handles = handles.clone();
            let state = state.clone();
            move |_, window, cx| {
                window.focus(&handles[index]);
                state.update(cx, |state, cx| {
                    state.set_mode(mode);
                    cx.notify();
                });
                window.refresh();
            }
        })
        .on_key_down(move |event: &KeyDownEvent, window, cx| {
            if event.keystroke.modifiers.modified() {
                return;
            }
            let key = event.keystroke.key.as_str();
            if matches!(key, "enter" | "space") {
                // Div synthesizes the radio click on key-up. Consuming the
                // press here prevents Space from scrolling without applying
                // the mode twice.
                cx.stop_propagation();
                window.prevent_default();
                return;
            }
            let target = match key {
                "left" | "up" => Some((index + handles.len() - 1) % handles.len()),
                "right" | "down" => Some((index + 1) % handles.len()),
                "home" => Some(0),
                "end" => Some(handles.len() - 1),
                _ => None,
            };
            if let Some(target) = target {
                window.focus(&handles[target]);
                state.update(cx, |state, cx| {
                    state.set_mode(modes_for_index(target));
                    cx.notify();
                });
                window.refresh();
                cx.stop_propagation();
                window.prevent_default();
            }
        })
        .child(label)
}

fn modes_for_index(index: usize) -> ColorMode {
    [ColorMode::HSL, ColorMode::RGB, ColorMode::HEX][index]
}

fn render_color_value(
    color: Hsla,
    mode: ColorMode,
    hex_input: Entity<InputState>,
    state: Entity<ColorPickerState>,
    on_change: Option<Rc<dyn Fn(Hsla, &mut Window, &mut App)>>,
) -> impl IntoElement {
    let theme = use_theme();

    let formatted = match mode {
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
                    color.l * 100.0,
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

    let state_for_submit = state.clone();
    let on_change_for_submit = on_change.clone();

    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    Text::new("HEX")
                        .variant(TextVariant::Custom)
                        .size(px(12.0))
                        .color(theme.tokens.muted_foreground),
                )
                .child(
                    Input::new(&hex_input)
                        .placeholder("#RRGGBB or #RRGGBBAA")
                        .aria_label("Hex color value")
                        .custom_validator(|value| {
                            if value.trim().is_empty() || parse_hex_color(value).is_some() {
                                Ok(())
                            } else {
                                Err("Enter a hex color like #RRGGBB".to_string())
                            }
                        })
                        .on_submit(move |value, window, cx| {
                            let Some(parsed) = parse_hex_color(&value) else {
                                return;
                            };
                            state_for_submit.update(cx, |state, cx| {
                                state.set_color(parsed);
                                cx.notify();
                            });
                            if let Some(handler) = on_change_for_submit.as_ref() {
                                handler(parsed, window, cx);
                            }
                            window.refresh();
                        }),
                ),
        )
        .child(
            Text::new(formatted)
                .variant(TextVariant::Custom)
                .size(px(12.0))
                .color(theme.tokens.muted_foreground),
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
    use super::{ColorPicker, ColorPickerState, MAX_RECENT_COLORS, parse_hex_color};
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

    #[test]
    fn hex_parsing_accepts_css_shapes_and_rejects_garbage() {
        let red = parse_hex_color("#F00").unwrap();
        assert!((red.h - 0.0).abs() < 1e-3 && red.s > 0.999 && (red.l - 0.5).abs() < 1e-3);
        assert_eq!(parse_hex_color("#ff0000").unwrap().a, 1.0);
        let half = parse_hex_color("#ff000080").unwrap();
        assert!((half.a - 128.0 / 255.0).abs() < 1e-3);
        // Case-insensitive and hash-optional.
        assert!(parse_hex_color("AbCdEf").is_some());
        // Invalid inputs must fail closed.
        assert!(parse_hex_color("#12345").is_none());
        assert!(parse_hex_color("#GG0000").is_none());
        assert!(parse_hex_color("green").is_none());
        assert!(parse_hex_color("").is_none());
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use kael::TestAppContext;
    use std::cell::Cell;

    struct ColorPickerHost {
        state: Entity<ColorPickerState>,
        changes: Rc<Cell<usize>>,
    }

    impl Render for ColorPickerHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let changes = self.changes.clone();
            ColorPicker::new("host-color-picker", self.state.clone())
                .on_change(move |_, _, _| changes.set(changes.get() + 1))
        }
    }

    fn open_picker(
        cx: &mut TestAppContext,
        changes: Rc<Cell<usize>>,
    ) -> (Entity<ColorPickerState>, &mut kael::VisualTestContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let state = cx.new(|_| ColorPickerState::new(hsla(0.0, 1.0, 0.5, 1.0)));
        let (_host, window) = cx.add_window_view({
            let state = state.clone();
            move |_, _| ColorPickerHost { state, changes }
        });
        window.update(|window, cx| window.draw(cx).clear());
        // Tab to the trigger and open the popover.
        window.simulate_keystrokes("tab");
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("enter");
        window.update(|window, cx| {
            window.draw(cx).clear();
            window.draw(cx).clear();
        });
        (state, window)
    }

    #[::core::prelude::v1::test]
    fn mode_selector_arrows_move_selection_and_focus_together() {
        let changes = Rc::new(Cell::new(0));
        let changes_for_open = changes.clone();
        let mut cx = TestAppContext::single();
        let (state, window) = open_picker(&mut cx, changes_for_open);

        // Tab into the popover: the active format radio is the tab stop.
        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let focused_radio = tree.nodes.values().any(|node| {
                node.role == AccessibilityRole::RadioButton
                    && node.states.contains(AccessibilityState::FOCUSED)
                    && node.label.as_deref() == Some("HSL")
            });
            assert!(
                focused_radio,
                "the active HSL radio must own keyboard focus"
            );
        });

        window.simulate_keystrokes("cmd-right");
        window.update(|_, cx| {
            assert_eq!(
                state.read(cx).mode(),
                super::ColorMode::HSL,
                "modified arrows must remain available to the application"
            );
        });

        window.simulate_keystrokes("right");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(state.read(cx).mode(), super::ColorMode::RGB);
            let tree = window.accessibility_tree();
            let rgb = tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::RadioButton
                        && node.label.as_deref() == Some("RGB")
                })
                .expect("RGB radio");
            assert!(rgb.states.contains(AccessibilityState::CHECKED));
            assert!(rgb.states.contains(AccessibilityState::FOCUSED));
        });

        window.simulate_keystrokes("left");
        window.update(|window, cx| {
            window.draw(cx).clear();
            assert_eq!(state.read(cx).mode(), super::ColorMode::HSL);
        });
    }

    #[::core::prelude::v1::test]
    fn typed_hex_commits_only_valid_colors() {
        let changes = Rc::new(Cell::new(0));
        let changes_for_open = changes.clone();
        let mut cx = TestAppContext::single();
        let (state, window) = open_picker(&mut cx, changes_for_open);

        // Tab past the format radios into the hex input.
        window.simulate_keystrokes("tab");
        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            window.draw(cx).clear();
            let tree = window.accessibility_tree();
            let hex_focused = tree.nodes.values().any(|node| {
                node.role == AccessibilityRole::TextInput
                    && node.label.as_deref() == Some("Hex color value")
                    && node.states.contains(AccessibilityState::FOCUSED)
            });
            assert!(hex_focused, "the hex input must receive keyboard focus");
        });

        // Replace the field and submit a valid green.
        // Key bindings for inputs inside popover overlays do not resolve yet
        // (recorded as an R1 remainder), so drive SelectAll/Enter through
        // their actions; the typing itself uses the real insert path.
        window.dispatch_action(crate::components::input::SelectAll);
        window.simulate_input("00FF00");
        window.dispatch_action(crate::components::input::Enter);
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|_, cx| {
            let color = state.read(cx).selected_color();
            assert!(
                (color.h - 1.0 / 3.0).abs() < 0.002,
                "green hue expected, got {}",
                color.h
            );
        });
        assert_eq!(changes.get(), 1, "a valid submit must fire on_change once");

        // Invalid text must not commit.
        let before = window.update(|_, cx| state.read(cx).selected_color());
        window.dispatch_action(crate::components::input::SelectAll);
        window.simulate_input("nope");
        window.dispatch_action(crate::components::input::Enter);
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        window.update(|_, cx| {
            let after = state.read(cx).selected_color();
            assert_eq!(after, before, "invalid hex must not change the color");
        });
        assert_eq!(changes.get(), 1, "invalid submit must not fire on_change");
    }
}
