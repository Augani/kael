//! Tooltip component - sugar over the core `div().tooltip_element()` system.
//!
//! The core tooltip plumbing owns show/hide timing and positions the tooltip
//! relative to the cursor (anchored to `AnyTooltip::mouse_position`, snapped to
//! the window). This component layers the kael_ui theme, an entrance fade, and a
//! familiar builder surface on top of that system rather than reimplementing it.

use crate::animations::easings;
use crate::overlays::layer::{LayerAlignment, LayerPlacement};
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
    Above,
    Below,
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipAlignment {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipFocusTrigger {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TooltipHoverIndication {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug)]
pub struct TooltipOptions {
    pub placement: TooltipPlacement,
    pub alignment: TooltipAlignment,
    pub delay: Duration,
    pub hide_delay: Duration,
    pub focus_trigger: TooltipFocusTrigger,
    pub is_enabled: bool,
    pub is_open: Option<bool>,
    pub is_default_open: bool,
}

impl Default for TooltipOptions {
    fn default() -> Self {
        Self {
            placement: TooltipPlacement::Top,
            alignment: TooltipAlignment::Center,
            delay: Duration::from_millis(200),
            hide_delay: Duration::from_millis(0),
            focus_trigger: TooltipFocusTrigger::Auto,
            is_enabled: true,
            is_open: None,
            is_default_open: false,
        }
    }
}

impl From<LayerPlacement> for TooltipPlacement {
    fn from(placement: LayerPlacement) -> Self {
        match placement {
            LayerPlacement::Top | LayerPlacement::Above => Self::Above,
            LayerPlacement::Bottom | LayerPlacement::Below => Self::Below,
            LayerPlacement::Start => Self::Before,
            LayerPlacement::End => Self::After,
            LayerPlacement::Center | LayerPlacement::Fill => Self::Top,
        }
    }
}

impl From<LayerAlignment> for TooltipAlignment {
    fn from(alignment: LayerAlignment) -> Self {
        match alignment {
            LayerAlignment::Start => Self::Start,
            LayerAlignment::Center => Self::Center,
            LayerAlignment::End => Self::End,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TooltipReturn {
    pub anchor_id: SharedString,
    pub described_by: SharedString,
    pub is_open: bool,
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
    alignment: TooltipAlignment,
    show_delay: Duration,
    hide_delay: Duration,
    focus_trigger: TooltipFocusTrigger,
    child: Option<AnyElement>,
    disabled: bool,
    is_open: Option<bool>,
    is_default_open: bool,
    hover_indication: TooltipHoverIndication,
    on_open_change: Option<Box<dyn Fn(bool, &mut Window, &mut App)>>,
    max_width: Option<Pixels>,
    style: StyleRefinement,
}

impl Tooltip {
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            content: content.into(),
            placement: TooltipPlacement::default(),
            alignment: TooltipAlignment::default(),
            show_delay: Duration::from_millis(500),
            hide_delay: Duration::from_millis(0),
            focus_trigger: TooltipFocusTrigger::default(),
            child: None,
            disabled: false,
            is_open: None,
            is_default_open: false,
            hover_indication: TooltipHoverIndication::Auto,
            on_open_change: None,
            max_width: Some(px(300.0)),
            style: StyleRefinement::default(),
        }
    }

    pub fn content(mut self, content: impl Into<SharedString>) -> Self {
        self.content = content.into();
        self
    }

    pub fn placement(mut self, placement: impl Into<TooltipPlacement>) -> Self {
        self.placement = placement.into();
        self
    }

    pub fn alignment(mut self, alignment: impl Into<TooltipAlignment>) -> Self {
        self.alignment = alignment.into();
        self
    }

    pub fn show_delay(mut self, delay: Duration) -> Self {
        self.show_delay = delay;
        self
    }

    pub fn delay(self, delay: Duration) -> Self {
        self.show_delay(delay)
    }

    pub fn hide_delay(mut self, delay: Duration) -> Self {
        self.hide_delay = delay;
        self
    }

    #[allow(non_snake_case)]
    pub fn hideDelay(self, delay: Duration) -> Self {
        self.hide_delay(delay)
    }

    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = Some(child.into_any_element());
        self
    }

    pub fn children(self, child: impl IntoElement) -> Self {
        self.child(child)
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.disabled = !enabled;
        self
    }

    pub fn is_enabled(self, enabled: bool) -> Self {
        self.enabled(enabled)
    }

    #[allow(non_snake_case)]
    pub fn isEnabled(self, enabled: bool) -> Self {
        self.enabled(enabled)
    }

    pub fn focus_trigger(mut self, focus_trigger: TooltipFocusTrigger) -> Self {
        self.focus_trigger = focus_trigger;
        self
    }

    #[allow(non_snake_case)]
    pub fn focusTrigger(self, focus_trigger: TooltipFocusTrigger) -> Self {
        self.focus_trigger(focus_trigger)
    }

    pub fn open(mut self, is_open: bool) -> Self {
        self.is_open = Some(is_open);
        self
    }

    pub fn is_open(self, is_open: bool) -> Self {
        self.open(is_open)
    }

    #[allow(non_snake_case)]
    pub fn isOpen(self, is_open: bool) -> Self {
        self.open(is_open)
    }

    pub fn default_open(mut self, is_default_open: bool) -> Self {
        self.is_default_open = is_default_open;
        self
    }

    pub fn is_default_open(self, is_default_open: bool) -> Self {
        self.default_open(is_default_open)
    }

    #[allow(non_snake_case)]
    pub fn isDefaultOpen(self, is_default_open: bool) -> Self {
        self.default_open(is_default_open)
    }

    pub fn hover_indication(mut self, indication: TooltipHoverIndication) -> Self {
        self.hover_indication = indication;
        self
    }

    pub fn has_hover_indication(mut self, indication: impl Into<TooltipHoverIndication>) -> Self {
        self.hover_indication = indication.into();
        self
    }

    #[allow(non_snake_case)]
    pub fn hasHoverIndication(self, indication: impl Into<TooltipHoverIndication>) -> Self {
        self.has_hover_indication(indication)
    }

    pub fn on_open_change(
        mut self,
        handler: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_open_change = Some(Box::new(handler));
        self
    }

    #[allow(non_snake_case)]
    pub fn onOpenChange(self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_change(handler)
    }

    pub fn max_width(mut self, width: Pixels) -> Self {
        self.max_width = Some(width);
        self
    }

    #[allow(non_snake_case)]
    pub fn maxWidth(self, width: Pixels) -> Self {
        self.max_width(width)
    }

    pub fn options(mut self, options: TooltipOptions) -> Self {
        self.placement = options.placement;
        self.alignment = options.alignment;
        self.show_delay = options.delay;
        self.hide_delay = options.hide_delay;
        self.focus_trigger = options.focus_trigger;
        self.disabled = !options.is_enabled;
        self.is_open = options.is_open;
        self.is_default_open = options.is_default_open;
        self
    }
}

impl From<bool> for TooltipHoverIndication {
    fn from(value: bool) -> Self {
        if value {
            Self::Always
        } else {
            Self::Never
        }
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
        let forced_open = self.is_open.unwrap_or(self.is_default_open);
        let placement = match self.placement {
            TooltipPlacement::Above => TooltipPlacement::Top,
            TooltipPlacement::Below => TooltipPlacement::Bottom,
            TooltipPlacement::Before => TooltipPlacement::Left,
            TooltipPlacement::After => TooltipPlacement::Right,
            placement => placement,
        };
        let alignment = self.alignment;
        let show_hover_indication = matches!(self.hover_indication, TooltipHoverIndication::Always);

        let _ = (self.show_delay, self.hide_delay, self.focus_trigger);

        let trigger_id = ElementId::Name(format!("kael-tooltip-{content}").into());

        div()
            .id(trigger_id)
            .relative()
            .when_some(self.child, |this, child| {
                this.child(
                    div()
                        .when(show_hover_indication, |this| {
                            this.border_b_1().border_color(theme.tokens.border)
                        })
                        .child(child),
                )
            })
            .when(forced_open, |this| {
                let content = content.clone();
                this.child(
                    div()
                        .absolute()
                        .when(placement == TooltipPlacement::Top, |this| {
                            this.bottom_full().mb(px(4.0))
                        })
                        .when(placement == TooltipPlacement::Bottom, |this| {
                            this.top_full().mt(px(4.0))
                        })
                        .when(placement == TooltipPlacement::Left, |this| {
                            this.right_full().mr(px(4.0))
                        })
                        .when(placement == TooltipPlacement::Right, |this| {
                            this.left_full().ml(px(4.0))
                        })
                        .when(
                            matches!(placement, TooltipPlacement::Top | TooltipPlacement::Bottom)
                                && alignment == TooltipAlignment::Start,
                            |this| this.left_0(),
                        )
                        .when(
                            matches!(placement, TooltipPlacement::Top | TooltipPlacement::Bottom)
                                && alignment == TooltipAlignment::End,
                            |this| this.right_0(),
                        )
                        .child(
                            div()
                                .px(px(8.0))
                                .py(px(4.0))
                                .bg(theme.tokens.foreground)
                                .text_color(theme.tokens.background)
                                .rounded(theme.tokens.radius_lg)
                                .text_size(px(14.0))
                                .line_height(px(20.0))
                                .font_family(theme.tokens.font_family.clone())
                                .whitespace_nowrap()
                                .when_some(max_width, |this, width| this.max_w(width))
                                .child(content),
                        ),
                )
            })
            .when(!self.disabled, move |this| {
                let content = content.clone();
                let user_style = user_style.clone();
                let fade = fade.clone();
                this.tooltip_element(move || {
                    div()
                        .px(px(8.0))
                        .py(px(4.0))
                        .bg(theme.tokens.foreground)
                        .text_color(theme.tokens.background)
                        .rounded(theme.tokens.radius_lg)
                        .text_size(px(14.0))
                        .line_height(px(20.0))
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
