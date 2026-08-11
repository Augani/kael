//! Icon button component for icon-only actions with multiple variants.

use crate::components::button::ButtonVariant;
use crate::components::icon_source::IconSource;
use crate::components::ripple::Ripple;
use crate::icon_config::resolve_icon_path;
use crate::theme::use_theme;
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

fn icon_path_from_name(name: &str) -> String {
    resolve_icon_path(name)
}

#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    base: Stateful<Div>,
    icon_source: IconSource,
    label: Option<SharedString>,
    variant: ButtonVariant,
    size: Pixels,
    icon_size: Option<Pixels>,
    disabled: bool,
    tab_stop: bool,
    no_background: bool,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
    ripple_enabled: bool,
    rotation: Option<Radians>,
    style: StyleRefinement,
}

impl IconButton {
    pub fn new(icon: impl Into<IconSource>) -> Self {
        let icon_source = icon.into();

        let id_string = match &icon_source {
            IconSource::Named(name) => format!("icon-button-{}", name),
            IconSource::FilePath(path) => format!("icon-button-{}", path),
        };
        let id = ElementId::Name(SharedString::from(id_string));

        Self {
            id: id.clone(),
            base: div().flex_shrink_0().id(id),
            icon_source,
            label: None,
            variant: ButtonVariant::Secondary,
            size: px(32.0),
            icon_size: None,
            disabled: false,
            tab_stop: true,
            no_background: false,
            on_click: None,
            ripple_enabled: false,
            rotation: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn ripple(mut self, enabled: bool) -> Self {
        self.ripple_enabled = enabled;
        self
    }

    pub fn rotate(mut self, radians: impl Into<Radians>) -> Self {
        self.rotation = Some(radians.into());
        self
    }

    /// Override the generated element id when multiple instances use the same icon.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self.base.interactivity().element_id = Some(self.id.clone());
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = Some(size);
        self
    }

    /// Set the accessible name for this icon-only action.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Control whether the button participates in sequential keyboard navigation.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    pub fn no_background(mut self, no_background: bool) -> Self {
        self.no_background = no_background;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn clickable(&self) -> bool {
        !self.disabled && self.on_click.is_some()
    }

    fn get_svg_path(&self) -> Option<SharedString> {
        match &self.icon_source {
            IconSource::FilePath(path) => Some(path.clone()),
            IconSource::Named(name) => Some(SharedString::from(icon_path_from_name(name))),
        }
    }
}

impl Styled for IconButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for IconButton {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for IconButton {}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();

        let icon_size = self.icon_size.unwrap_or_else(|| {
            if self.size > px(32.0) {
                px(20.0)
            } else {
                px(16.0)
            }
        });
        let dark = theme.tokens.background.l < 0.5;
        let ink = |c: Hsla, amt: f32| {
            if dark {
                hsla(c.h, c.s, (c.l + amt).min(1.0), c.a)
            } else {
                hsla(c.h, c.s, (c.l - amt).max(0.0), c.a)
            }
        };

        let (bg, fg, border, hover_bg, hover_fg, has_border) = match self.variant {
            ButtonVariant::Default => (
                theme.tokens.primary,
                theme.tokens.primary_foreground,
                kael::transparent_black(),
                ink(theme.tokens.primary, 0.05),
                theme.tokens.primary_foreground,
                false,
            ),
            ButtonVariant::Secondary => (
                theme.tokens.secondary,
                theme.tokens.foreground,
                kael::transparent_black(),
                ink(theme.tokens.secondary, 0.04),
                theme.tokens.foreground,
                false,
            ),
            ButtonVariant::Destructive => (
                theme.tokens.destructive,
                theme.tokens.destructive_foreground,
                kael::transparent_black(),
                ink(theme.tokens.destructive, 0.05),
                theme.tokens.destructive_foreground,
                false,
            ),
            ButtonVariant::Outline => (
                kael::transparent_black(),
                theme.tokens.foreground,
                theme.tokens.border,
                theme.tokens.accent,
                theme.tokens.accent_foreground,
                true,
            ),
            ButtonVariant::Ghost => (
                kael::transparent_black(),
                theme.tokens.foreground,
                kael::transparent_black(),
                theme.tokens.accent,
                theme.tokens.accent_foreground,
                false,
            ),
            ButtonVariant::Link => (
                kael::transparent_black(),
                theme.tokens.primary,
                kael::transparent_black(),
                kael::transparent_black(),
                theme.tokens.primary.opacity(0.8),
                false,
            ),
            ButtonVariant::Custom(colors) => (
                colors.background,
                colors.foreground,
                colors.border,
                colors.hover_background,
                colors.hover_foreground,
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
        let svg_path = self.get_svg_path();
        let user_style = self.style;
        let ripple_enabled = self.ripple_enabled && clickable;
        let ripple_id = ElementId::Name(format!("{}-ripple", self.id).into());
        let ripple_color = fg;

        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let is_focused = focus_handle.is_focused(window);
        let focus_ring = crate::astryx::focus_ring_outer(theme.tokens.ring);
        let accessibility_label = self
            .label
            .clone()
            .unwrap_or_else(|| match &self.icon_source {
                IconSource::Named(name) => SharedString::from(name.replace(['-', '_'], " ")),
                IconSource::FilePath(_) => SharedString::from("Icon button"),
            });
        let mut accessibility_state = AccessibilityState::NONE;
        if self.disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        if is_focused {
            accessibility_state |= AccessibilityState::FOCUSED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::Button)
            .label(accessibility_label.to_string())
            .states(accessibility_state);
        if clickable {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }
        let focus_on_mouse = focus_handle.clone();

        self.base
            .when(!self.disabled, |this| {
                this.track_focus(&focus_handle.tab_index(0).tab_stop(self.tab_stop))
            })
            .relative()
            .accessibility(accessibility)
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .size(self.size)
            .rounded(theme.tokens.radius_md)
            .transition(theme.tokens.transition_fast)
            .when(!self.no_background, |this| {
                this.bg(bg)
                    .text_color(fg)
                    .when(has_border, |this| this.border_1().border_color(border))
            })
            .when(is_focused && !self.disabled, |this| {
                this.shadow(smallvec::smallvec![focus_ring])
            })
            .when(self.disabled, |this| {
                this.opacity(0.5).cursor(CursorStyle::Arrow)
            })
            .when(!self.disabled, |this| {
                this.cursor(CursorStyle::PointingHand)
                    .when(!self.no_background, |this| {
                        this.hover(move |style| style.bg(hover_bg).text_color(hover_fg))
                    })
                    .when(self.no_background, |this| {
                        this.hover(|style| style.opacity(0.7))
                    })
                    .active(move |style| style.bg(active_bg).scale(0.98))
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.prevent_default();
                if !self.disabled {
                    window.focus(&focus_on_mouse);
                }
                if ripple_enabled {
                    window.refresh();
                }
            })
            .when(self.ripple_enabled && clickable, |this| {
                let center = self.size / 2.0;
                this.child(
                    Ripple::new(ripple_id, point(center, center), ripple_color)
                        .max_size(self.size * 2.0),
                )
            })
            .when_some(handler.filter(|_| clickable), |this, on_click| {
                this.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    (on_click)(event, window, cx);
                })
            })
            .when_some(svg_path, |this, path| {
                this.child(
                    div()
                        .size(icon_size)
                        .flex()
                        .items_center()
                        .justify_center()
                        .line_height(px(0.0))
                        .child(
                            svg()
                                .path(path)
                                .size(icon_size)
                                .text_color(if self.disabled {
                                    theme.tokens.muted_foreground
                                } else if self.no_background {
                                    theme.tokens.primary
                                } else {
                                    fg
                                })
                                .when_some(self.rotation, |this, rotation| {
                                    this.with_transformation(Transformation::rotate(rotation))
                                }),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::IconButton;
    use kael::{
        Context, InteractiveElement as _, KeyUpEvent, Keystroke, Modifiers, MouseButton, Render,
        TestAppContext, Window,
    };
    use std::{cell::Cell, rc::Rc};

    struct IconButtonActivationHost {
        activations: Rc<Cell<usize>>,
    }

    impl Render for IconButtonActivationHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl kael::IntoElement {
            let activations = self.activations.clone();
            IconButton::new("x")
                .id("activation-icon-button")
                .label("Close")
                .debug_selector(|| "activation-icon-button".to_owned())
                .on_click(move |_, _, _| activations.set(activations.get() + 1))
        }
    }

    #[kael::test]
    fn pointer_and_keyboard_each_activate_icon_button_once(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let activations = Rc::new(Cell::new(0));
        let (_view, window) = cx.add_window_view({
            let activations = activations.clone();
            move |_, _| IconButtonActivationHost { activations }
        });
        window.update(|window, cx| window.draw(cx).clear());
        let bounds = window
            .debug_bounds("activation-icon-button")
            .expect("icon button bounds");

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
