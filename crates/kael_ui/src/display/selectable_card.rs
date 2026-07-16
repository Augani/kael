//! SelectableCard - a card that behaves like a large radio/checkbox option.

use crate::components::icon::Icon;
use crate::display::card::CardVariant;
use crate::theme::use_theme;
use crate::{astryx, styled_ext::StyledExt};
use kael::{prelude::FluentBuilder as _, *};
use std::rc::Rc;

#[derive(IntoElement)]
pub struct SelectableCard {
    base: Stateful<Div>,
    label: Option<SharedString>,
    selected: bool,
    disabled: bool,
    show_check: bool,
    variant: CardVariant,
    padding: Option<Pixels>,
    width: Option<Pixels>,
    height: Option<Pixels>,
    max_width: Option<Pixels>,
    min_height: Option<Pixels>,
    content: Option<AnyElement>,
    children: Vec<AnyElement>,
    on_click: Option<Rc<dyn Fn(&bool, &mut Window, &mut App)>>,
    style: StyleRefinement,
}

impl SelectableCard {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            base: div().id(id.into()),
            label: None,
            selected: false,
            disabled: false,
            show_check: false,
            variant: CardVariant::Default,
            padding: None,
            width: None,
            height: None,
            max_width: None,
            min_height: None,
            content: None,
            children: Vec::new(),
            on_click: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    #[allow(non_snake_case)]
    pub fn isSelected(self, selected: bool) -> Self {
        self.selected(selected)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isDisabled(self, disabled: bool) -> Self {
        self.disabled(disabled)
    }

    pub fn show_check(mut self, show: bool) -> Self {
        self.show_check = show;
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(|child| child.into_any_element()));
        self
    }

    pub fn variant(mut self, variant: CardVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        let value = f32::from(padding);
        self.padding = (value.is_finite() && value >= 0.0).then_some(padding);
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        let value = f32::from(width);
        self.width = (value.is_finite() && value > 0.0).then_some(width);
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        let value = f32::from(height);
        self.height = (value.is_finite() && value > 0.0).then_some(height);
        self
    }

    pub fn max_width(mut self, max_width: Pixels) -> Self {
        let value = f32::from(max_width);
        self.max_width = (value.is_finite() && value > 0.0).then_some(max_width);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxWidth(self, max_width: Pixels) -> Self {
        self.max_width(max_width)
    }

    pub fn min_height(mut self, min_height: Pixels) -> Self {
        let value = f32::from(min_height);
        self.min_height = (value.is_finite() && value >= 0.0).then_some(min_height);
        self
    }

    #[allow(non_snake_case)]
    pub fn minHeight(self, min_height: Pixels) -> Self {
        self.min_height(min_height)
    }

    pub fn on_click(mut self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }

    pub fn on_change(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_click(handler)
    }

    #[allow(non_snake_case)]
    pub fn onChange(self, handler: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change(handler)
    }
}

impl Styled for SelectableCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl InteractiveElement for SelectableCard {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl StatefulInteractiveElement for SelectableCard {}

impl RenderOnce for SelectableCard {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let user_style = self.style;
        let selected = self.selected;
        let disabled = self.disabled;
        let on_click = self.on_click.clone();
        let accessible_label = self
            .label
            .clone()
            .unwrap_or_else(|| SharedString::from("Select card"));
        let is_interactive = on_click.is_some();
        let dark = theme.tokens.background.l < 0.5;
        let bg = self.variant.background(&theme);
        let selected_ring = match self.variant {
            CardVariant::Default | CardVariant::Transparent | CardVariant::Muted => {
                theme.tokens.ring
            }
            CardVariant::Blue => astryx::Hue::Blue.colors(dark).border,
            CardVariant::Cyan => astryx::Hue::Cyan.colors(dark).border,
            CardVariant::Gray => astryx::Hue::Gray.colors(dark).border,
            CardVariant::Green => astryx::Hue::Green.colors(dark).border,
            CardVariant::Orange => astryx::Hue::Orange.colors(dark).border,
            CardVariant::Pink => astryx::Hue::Pink.colors(dark).border,
            CardVariant::Purple => astryx::Hue::Purple.colors(dark).border,
            CardVariant::Red => astryx::Hue::Red.colors(dark).border,
            CardVariant::Teal => astryx::Hue::Teal.colors(dark).border,
            CardVariant::Yellow => astryx::Hue::Yellow.colors(dark).border,
        };

        let border = if selected {
            selected_ring
        } else if self.variant == CardVariant::Default {
            theme.tokens.input
        } else {
            transparent_black()
        };

        let mut accessibility_state = AccessibilityState::NONE;
        if selected {
            accessibility_state |= AccessibilityState::CHECKED;
        }
        if disabled {
            accessibility_state |= AccessibilityState::DISABLED;
        }
        let mut accessibility = AccessibilityAttributes::new(AccessibilityRole::CheckBox)
            .label(accessible_label.to_string())
            .states(accessibility_state);
        if !disabled && on_click.is_some() {
            accessibility =
                accessibility.actions(vec![AccessibilityAction::Focus, AccessibilityAction::Click]);
        }

        self.base
            .accessibility(accessibility)
            .when(!disabled && on_click.is_some(), |this| {
                this.focusable()
                    .tab_index(0)
                    .tab_stop(true)
                    .focus_visible(move |style| {
                        style.shadow(smallvec::smallvec![
                            astryx::focus_ring_outer(selected_ring,)
                        ])
                    })
            })
            .relative()
            .p(self.padding.unwrap_or(px(16.0)))
            .when_some(self.width, |this, width| this.w(width))
            .when_some(self.height, |this, height| this.h(height).overflow_hidden())
            .when_some(self.max_width, |this, max_width| this.max_w(max_width))
            .when_some(self.min_height, |this, min_height| this.min_h(min_height))
            .bg(bg)
            .border_1()
            .border_color(border)
            .rounded(theme.tokens.radius_lg)
            .transition(theme.tokens.transition_fast)
            .when(selected, |this| this.inset_ring(selected_ring, px(2.0)))
            .when(disabled, |this| this.opacity(0.5))
            .when(!disabled && is_interactive, |this| {
                this.cursor(CursorStyle::PointingHand)
                    .hover(move |style| style.bg(astryx::overlay_hover(dark)))
            })
            .when_some(self.content, |this, content| this.child(content))
            .children(self.children)
            .when(selected && self.show_check, |this| {
                this.child(
                    div()
                        .absolute()
                        .top(px(10.0))
                        .right(px(10.0))
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(theme.tokens.primary)
                        .child(
                            Icon::new("check")
                                .size(px(12.0))
                                .color(theme.tokens.primary_foreground),
                        ),
                )
            })
            .when(!disabled && is_interactive, |this| {
                this.when_some(on_click, |this, handler| {
                    let on_key = handler.clone();
                    this.on_click(move |_, window, cx| {
                        let next = !selected;
                        (handler)(&next, window, cx);
                    })
                    .on_key_down(move |event, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            let next = !selected;
                            (on_key)(&next, window, cx);
                            cx.stop_propagation();
                            window.prevent_default();
                        }
                    })
                })
            })
            .map(|this| {
                let mut div = this;
                div.style().refine(&user_style);
                div
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn invalid_geometry_is_ignored() {
        let card = SelectableCard::new("test")
            .padding(px(-1.0))
            .width(px(f32::NAN))
            .height(px(0.0))
            .max_width(px(-2.0))
            .min_height(px(f32::NAN));
        assert!(card.padding.is_none());
        assert!(card.width.is_none());
        assert!(card.height.is_none());
        assert!(card.max_width.is_none());
        assert!(card.min_height.is_none());
    }
}
