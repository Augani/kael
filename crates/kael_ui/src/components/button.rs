//! Button component with multiple variants and sizes.

use crate::components::icon_source::IconSource;
use crate::components::ripple::Ripple;
use crate::components::text::{Text, TextVariant};
use crate::icon_config::resolve_icon_path;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

/// Render an icon from IconSource
fn render_icon(icon_src: IconSource, size: Pixels, color: Hsla) -> impl IntoElement {
    let svg_path = match icon_src {
        IconSource::FilePath(path) => path,
        IconSource::Named(name) => SharedString::from(resolve_icon_path(&name)),
    };

    div()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .line_height(px(0.0))
        .child(svg().path(svg_path).size(size).text_color(color))
}

/// Render an animated loading spinner that matches the button's text color.
fn render_loading_spinner(size: Pixels, color: Hsla) -> impl IntoElement {
    use crate::components::spinner::{Spinner, SpinnerSize};
    let spinner_size = if size <= px(16.0) {
        SpinnerSize::Sm
    } else {
        SpinnerSize::Md
    };
    Spinner::new()
        .size(spinner_size)
        .color(color)
        .decorative(true)
}

/// A fully custom color set for [`ButtonVariant::Custom`], letting an app define any
/// button look — including hover colors — without forking the component.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ButtonColors {
    /// Resting background fill.
    pub background: Hsla,
    /// Resting text/icon color.
    pub foreground: Hsla,
    /// Border color (drawn only when `has_border` is set).
    pub border: Hsla,
    /// Background fill on hover.
    pub hover_background: Hsla,
    /// Text/icon color on hover.
    pub hover_foreground: Hsla,
    /// Whether to draw the resting/hover drop shadow.
    pub has_shadow: bool,
    /// Whether to draw a 1px border.
    pub has_border: bool,
}

impl ButtonColors {
    /// A solid filled button with a slightly translucent hover.
    pub fn solid(background: impl Into<Hsla>, foreground: impl Into<Hsla>) -> Self {
        let background = background.into();
        let foreground = foreground.into();
        Self {
            background,
            foreground,
            border: background,
            hover_background: background.opacity(0.9),
            hover_foreground: foreground,
            has_shadow: true,
            has_border: false,
        }
    }

    /// An outlined button: transparent fill with a colored border and text.
    pub fn outline(border: impl Into<Hsla>, foreground: impl Into<Hsla>) -> Self {
        let border = border.into();
        let foreground = foreground.into();
        Self {
            background: kael::transparent_black(),
            foreground,
            border,
            hover_background: border.opacity(0.1),
            hover_foreground: foreground,
            has_shadow: false,
            has_border: true,
        }
    }

    /// Override the hover colors.
    pub fn hover(mut self, background: impl Into<Hsla>, foreground: impl Into<Hsla>) -> Self {
        self.hover_background = background.into();
        self.hover_foreground = foreground.into();
        self
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ButtonVariant {
    Default,
    Secondary,
    Destructive,
    Outline,
    Ghost,
    Link,
    /// An app-defined color set — build any look (including hover) without forking.
    Custom(ButtonColors),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ButtonSize {
    Sm,
    Md,
    Lg,
    Icon,
}
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: Stateful<Div>,
    label: SharedString,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    selected: bool,
    pressed: Option<bool>,
    expanded: Option<bool>,
    loading: bool,
    icon: Option<IconSource>,
    icon_position: IconPosition,
    tooltip: Option<SharedString>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    ripple_enabled: bool,
    style: StyleRefinement,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IconPosition {
    Start,
    End,
}

impl Button {
    /// Create a new button with a unique ID and label.
    ///
    /// # Example
    /// ```rust,ignore
    /// Button::new("my-button", "Click me")
    /// ```
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        let id = id.into();
        let label = label.into();

        Self {
            id: id.clone(),
            base: div().flex_shrink_0().id(id),
            label,
            variant: ButtonVariant::Default,
            size: ButtonSize::Md,
            disabled: false,
            selected: false,
            pressed: None,
            expanded: None,
            loading: false,
            icon: None,
            icon_position: IconPosition::Start,
            tooltip: None,
            on_click: None,
            ripple_enabled: false,

            style: StyleRefinement::default(),
        }
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Use a fully custom color set, setting the variant to [`ButtonVariant::Custom`].
    /// Lets you give a button a distinctive look — including hover colors — without
    /// hand-rolling it from `div()`.
    pub fn colors(mut self, colors: ButtonColors) -> Self {
        self.variant = ButtonVariant::Custom(colors);
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Expose this button as a two-state toggle to assistive technology.
    ///
    /// Leave unset for ordinary push buttons. Passing either value advertises
    /// the button's current pressed state without changing its visual styling.
    pub fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = Some(pressed);
        self
    }

    /// Expose whether this button controls expanded or collapsed content.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn icon(mut self, icon: impl Into<IconSource>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn icon_position(mut self, position: IconPosition) -> Self {
        self.icon_position = position;
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn ripple(mut self, enabled: bool) -> Self {
        self.ripple_enabled = enabled;
        self
    }

    fn clickable(&self) -> bool {
        !self.disabled && !self.loading && self.on_click.is_some()
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for Button {}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();

        let (height, px_h, text_size) = match self.size {
            ButtonSize::Sm => (px(28.0), px(10.0), px(13.0)),
            ButtonSize::Md => (px(32.0), px(12.0), px(14.0)),
            ButtonSize::Lg => (px(36.0), px(16.0), px(14.0)),
            ButtonSize::Icon => (px(32.0), px(0.0), px(14.0)),
        };
        let is_icon_only = matches!(self.size, ButtonSize::Icon);

        let dark = theme.tokens.background.l < 0.5;
        let ink = |c: Hsla, amt: f32| {
            if dark {
                hsla(c.h, c.s, (c.l + amt).min(1.0), c.a)
            } else {
                hsla(c.h, c.s, (c.l - amt).max(0.0), c.a)
            }
        };

        let (bg, fg, border, hover_bg, hover_fg, has_shadow, has_border) = match self.variant {
            ButtonVariant::Default => (
                theme.tokens.primary,
                theme.tokens.primary_foreground,
                kael::transparent_black(),
                ink(theme.tokens.primary, 0.05),
                theme.tokens.primary_foreground,
                false,
                false,
            ),
            ButtonVariant::Secondary => (
                theme.tokens.secondary,
                theme.tokens.secondary_foreground,
                kael::transparent_black(),
                ink(theme.tokens.secondary, 0.04),
                theme.tokens.secondary_foreground,
                false,
                false,
            ),
            ButtonVariant::Destructive => (
                theme.tokens.destructive,
                theme.tokens.destructive_foreground,
                kael::transparent_black(),
                ink(theme.tokens.destructive, 0.05),
                theme.tokens.destructive_foreground,
                false,
                false,
            ),
            ButtonVariant::Outline => (
                kael::transparent_black(),
                theme.tokens.foreground,
                theme.tokens.border,
                theme.tokens.accent,
                theme.tokens.accent_foreground,
                false,
                true,
            ),
            ButtonVariant::Ghost => (
                kael::transparent_black(),
                theme.tokens.foreground,
                kael::transparent_black(),
                theme.tokens.accent,
                theme.tokens.accent_foreground,
                false,
                false,
            ),
            ButtonVariant::Link => (
                kael::transparent_black(),
                theme.tokens.primary,
                kael::transparent_black(),
                kael::transparent_black(),
                ink(theme.tokens.primary, 0.1),
                false,
                false,
            ),
            ButtonVariant::Custom(colors) => (
                colors.background,
                colors.foreground,
                colors.border,
                colors.hover_background,
                colors.hover_foreground,
                colors.has_shadow,
                colors.has_border,
            ),
        };
        let active_bg = if bg.a > 0.0 {
            ink(bg, 0.1)
        } else {
            ink(theme.tokens.accent, 0.06)
        };

        let clickable = self.clickable();
        let handler = self.on_click.clone();
        let ripple_enabled = self.ripple_enabled && clickable;
        let ripple_id = ElementId::Name(format!("{}-ripple", self.id).into());
        let ripple_color = fg;

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);
        let ring_color = theme.tokens.ring;
        let accessibility_label = if !self.label.is_empty() {
            self.label.clone()
        } else {
            self.tooltip
                .clone()
                .unwrap_or_else(|| SharedString::from("Button"))
        };
        let mut accessibility_state = AccessibilityState::NONE;
        if self.disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        if self.selected && self.pressed.is_none() {
            accessibility_state |= AccessibilityState::SELECTED;
        }
        if self.pressed == Some(true) {
            accessibility_state |= AccessibilityState::PRESSED;
        }
        if let Some(expanded) = self.expanded {
            accessibility_state |= if expanded {
                AccessibilityState::EXPANDED
            } else {
                AccessibilityState::COLLAPSED
            };
        }
        if self.loading {
            accessibility_state |= AccessibilityState::BUSY;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(accessibility_label.to_string())
            .states(accessibility_state);
        if clickable {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        let label_text = Text::new(self.label.clone())
            .variant(TextVariant::Custom)
            .size(text_size)
            .weight(FontWeight::MEDIUM)
            .font(theme.tokens.font_family.clone())
            .color(fg)
            .accessibility_hidden(true);

        let icon_size = if is_icon_only {
            px(16.0)
        } else {
            text_size * 1.2
        };
        let icon = self.icon.clone();
        let icon_pos = self.icon_position;
        let is_loading = self.loading;
        let is_selected = self.selected;
        let user_style = self.style;
        let focus_on_mouse = focus_handle.clone();

        self.base
            .when(!self.disabled && !is_loading, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(true))
            })
            .relative()
            .accessibility(accessibility)
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .when(!is_icon_only, |this| this.gap_2())
            .h(height)
            .px(px_h)
            .when(is_icon_only, |this| this.w(height))
            .rounded(theme.tokens.radius_md)
            .text_color(fg)
            .bg(bg)
            .when(has_shadow, |this| {
                this.shadow(theme.tokens.shadow_xs.to_vec())
            })
            .when(has_border, |this| this.border_1().border_color(border))
            .when(is_focused && !self.disabled, |this| {
                this.shadow(smallvec::smallvec![crate::astryx::focus_ring_outer(
                    ring_color
                )])
            })
            .when(is_selected && !self.disabled, |this| {
                this.bg(theme.tokens.accent)
                    .text_color(theme.tokens.accent_foreground)
                    .border_color(theme.tokens.accent)
            })
            .when(is_loading, |this| {
                this.opacity(0.7).cursor(CursorStyle::Arrow)
            })
            .when(self.disabled && !is_loading, |this| {
                this.opacity(0.5).cursor(CursorStyle::Arrow)
            })
            .when(!self.disabled && !is_loading, |this| {
                this.cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(hover_bg).text_color(hover_fg))
                    .active(move |style| style.bg(active_bg).scale(0.98))
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .on_mouse_down(MouseButton::Left, move |_event, window, _| {
                window.prevent_default();
                if !is_loading && !self.disabled {
                    window.focus(&focus_on_mouse);
                }
                if ripple_enabled {
                    window.refresh();
                }
            })
            .when_some(handler.filter(|_| clickable), |this, on_click| {
                this.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    (on_click)(event, window, cx);
                })
            })
            .when(self.ripple_enabled && clickable, |this| {
                let size = height;
                this.child(
                    Ripple::new(ripple_id, point(size / 2.0, size / 2.0), ripple_color)
                        .max_size(size * 2.5),
                )
            })
            .when(is_icon_only, |this| {
                this.child(if is_loading {
                    render_loading_spinner(icon_size, fg).into_any_element()
                } else if let Some(icon_src) = icon.clone() {
                    render_icon(icon_src, icon_size, fg).into_any_element()
                } else {
                    div().size(icon_size).into_any_element()
                })
            })
            .when(!is_icon_only, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(icon_pos == IconPosition::Start && !is_loading, |this| {
                            this.when_some(icon.clone(), |this, icon_src| {
                                this.child(render_icon(icon_src, icon_size, fg))
                            })
                        })
                        .when(is_loading && icon_pos == IconPosition::Start, |this| {
                            this.child(render_loading_spinner(icon_size, fg))
                        })
                        .child(
                            div()
                                .when(self.variant == ButtonVariant::Link, |this| this.underline())
                                .child(label_text),
                        )
                        .when(icon_pos == IconPosition::End && !is_loading, |this| {
                            this.when_some(icon.clone(), |this, icon_src| {
                                this.child(render_icon(icon_src, icon_size, fg))
                            })
                        })
                        .when(is_loading && icon_pos == IconPosition::End, |this| {
                            this.child(render_loading_spinner(icon_size, fg))
                        }),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, ButtonColors, ButtonVariant};
    use kael::{
        Context, InteractiveElement as _, KeyUpEvent, Keystroke, Modifiers, MouseButton, Render,
        TestAppContext, Window,
    };
    use std::{cell::Cell, rc::Rc};

    struct ButtonActivationHost {
        activations: Rc<Cell<usize>>,
    }

    impl Render for ButtonActivationHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl kael::IntoElement {
            let activations = self.activations.clone();
            Button::new("activation-button", "Activate")
                .debug_selector(|| "activation-button".to_owned())
                .on_click(move |_, _, _| activations.set(activations.get() + 1))
        }
    }

    #[test]
    fn colors_sets_custom_variant() {
        let palette = ButtonColors::solid(kael::black(), kael::white());
        let button = Button::new("b", "Hi").colors(palette);
        assert!(matches!(button.variant, ButtonVariant::Custom(_)));
    }

    #[test]
    fn solid_and_outline_presets_differ() {
        let solid = ButtonColors::solid(kael::black(), kael::white());
        assert!(solid.has_shadow && !solid.has_border);

        let outline = ButtonColors::outline(kael::black(), kael::white());
        assert!(outline.has_border && !outline.has_shadow);

        let custom = outline.hover(kael::white(), kael::black());
        assert_eq!(custom.hover_background, kael::white());
        assert_eq!(custom.hover_foreground, kael::black());
    }

    #[test]
    fn pressed_state_is_opt_in_for_toggle_buttons() {
        let push_button = Button::new("push", "Push");
        let toggle_button = Button::new("toggle", "Toggle").pressed(true);

        assert_eq!(push_button.pressed, None);
        assert_eq!(toggle_button.pressed, Some(true));
    }

    #[kael::test]
    fn pointer_and_keyboard_each_activate_button_once(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let activations = Rc::new(Cell::new(0));
        let (_view, window) = cx.add_window_view({
            let activations = activations.clone();
            move |_, _| ButtonActivationHost { activations }
        });
        window.update(|window, cx| window.draw(cx).clear());
        let bounds = window
            .debug_bounds("activation-button")
            .expect("button bounds");

        window.simulate_mouse_down(bounds.center(), MouseButton::Left, Modifiers::default());
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_mouse_up(bounds.center(), MouseButton::Left, Modifiers::default());
        assert_eq!(
            activations.get(),
            1,
            "one physical click must activate once"
        );

        activations.set(0);
        window.update(|window, cx| window.draw(cx).clear());
        window.simulate_keystrokes("enter");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").expect("valid keystroke"),
        });
        assert_eq!(activations.get(), 1, "Enter must activate once");

        activations.set(0);
        window.simulate_keystrokes("space");
        window.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("space").expect("valid keystroke"),
        });
        assert_eq!(activations.get(), 1, "Space must activate once");
    }
}
