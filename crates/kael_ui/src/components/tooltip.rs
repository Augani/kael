//! Tooltip component - sugar over the core `div().tooltip_element()` system.
//!
//! The core tooltip plumbing owns show/hide timing and positions the tooltip
//! relative to the cursor (anchored to `AnyTooltip::mouse_position`, snapped to
//! the window). This component layers the kael_ui theme, an entrance fade, and a
//! familiar builder surface on top of that system rather than reimplementing it.

use crate::animations::easings;
use crate::theme::use_theme;
use kael::{prelude::*, *};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipPlacement {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Default)]
pub struct TooltipState {
    is_visible: bool,
}

impl TooltipState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible
    }
}

pub struct Tooltip {
    content: SharedString,
    placement: TooltipPlacement,
    show_delay: Duration,
    hide_delay: Duration,
    child: Option<AnyElement>,
    disabled: bool,
    max_width: Option<Pixels>,
    style: StyleRefinement,
}

impl Tooltip {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            placement: TooltipPlacement::default(),
            show_delay: Duration::from_millis(500),
            hide_delay: Duration::from_millis(0),
            child: None,
            disabled: false,
            max_width: Some(px(300.0)),
            style: StyleRefinement::default(),
        }
    }

    pub fn placement(mut self, placement: TooltipPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn show_delay(mut self, delay: Duration) -> Self {
        self.show_delay = delay;
        self
    }

    pub fn hide_delay(mut self, delay: Duration) -> Self {
        self.hide_delay = delay;
        self
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(child.into_any_element());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = Some(width);
        self
    }
}

impl Styled for Tooltip {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for Tooltip {
    type Element = Stateful<Div>;

    fn into_element(self) -> Self::Element {
        let theme = use_theme();
        let content = self.content;
        let user_style = self.style;
        let max_width = self.max_width;
        let fade = Animation::new(theme.tokens.duration_fast).with_easing(easings::ease_out_cubic);

        let _ = (self.placement, self.show_delay, self.hide_delay);

        let trigger_id = ElementId::Name(format!("kael-tooltip-{content}").into());

        div()
            .id(trigger_id)
            .relative()
            .when_some(self.child, |this, child| this.child(child))
            .when(!self.disabled, move |this| {
                let content = content.clone();
                let user_style = user_style.clone();
                let fade = fade.clone();
                this.tooltip_element(move || {
                    div()
                        .px(px(8.0))
                        .py(px(4.0))
                        .bg(theme.tokens.popover)
                        .text_color(theme.tokens.popover_foreground)
                        .border_1()
                        .border_color(theme.tokens.border)
                        .rounded(theme.tokens.radius_sm)
                        .shadow_md()
                        .text_size(px(12.0))
                        .font_family(theme.tokens.font_family.clone())
                        .whitespace_nowrap()
                        .when_some(max_width, |this, width| this.max_w(width))
                        .map(|mut this| {
                            this.style().refine(&user_style);
                            this
                        })
                        .child(content.clone())
                        .with_animation("tooltip-fade-in", fade.clone(), |el, delta| {
                            el.opacity(delta)
                        })
                })
            })
    }
}

pub fn tooltip<E: IntoElement>(child: E, content: impl Into<SharedString>) -> Tooltip {
    Tooltip::new(content).child(child)
}
