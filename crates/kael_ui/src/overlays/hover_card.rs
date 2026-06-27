//! Hover card component with popover on hover.

use kael::{prelude::FluentBuilder as _, *};
use std::time::Duration;

use crate::overlays::layer::{LayerAlignment, LayerPlacement};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoverCardPosition {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
    Above,
    Below,
    Before,
    After,
}

pub type HoverCardPlacement = HoverCardPosition;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoverCardAlignment {
    Start,
    #[default]
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoverCardFocusTrigger {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoverCardHoverIndication {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug)]
pub struct HoverCardOptions {
    pub placement: HoverCardPosition,
    pub alignment: HoverCardAlignment,
    pub delay: Duration,
    pub hide_delay: Duration,
    pub focus_trigger: HoverCardFocusTrigger,
    pub is_enabled: bool,
    pub is_open: Option<bool>,
    pub is_default_open: bool,
}

impl Default for HoverCardOptions {
    fn default() -> Self {
        Self {
            placement: HoverCardPosition::Above,
            alignment: HoverCardAlignment::Center,
            delay: Duration::from_millis(300),
            hide_delay: Duration::from_millis(200),
            focus_trigger: HoverCardFocusTrigger::Auto,
            is_enabled: true,
            is_open: None,
            is_default_open: false,
        }
    }
}

impl From<LayerPlacement> for HoverCardPosition {
    fn from(placement: LayerPlacement) -> Self {
        match placement {
            LayerPlacement::Top | LayerPlacement::Above => Self::Above,
            LayerPlacement::Bottom | LayerPlacement::Below => Self::Below,
            LayerPlacement::Start => Self::Before,
            LayerPlacement::End => Self::After,
            LayerPlacement::Center | LayerPlacement::Fill => Self::Above,
        }
    }
}

impl From<LayerAlignment> for HoverCardAlignment {
    fn from(alignment: LayerAlignment) -> Self {
        match alignment {
            LayerAlignment::Start => Self::Start,
            LayerAlignment::Center => Self::Center,
            LayerAlignment::End => Self::End,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HoverCardReturn {
    pub anchor_id: SharedString,
    pub described_by: SharedString,
    pub is_open: bool,
}

#[derive(IntoElement)]
pub struct HoverCard {
    trigger: AnyElement,
    content: AnyElement,
    position: HoverCardPosition,
    alignment: HoverCardAlignment,
    open_delay: Duration,
    close_delay: Duration,
    focus_trigger: HoverCardFocusTrigger,
    is_enabled: bool,
    is_open: Option<bool>,
    is_default_open: bool,
    hover_indication: HoverCardHoverIndication,
    on_open_change: Option<Box<dyn Fn(bool, &mut Window, &mut App)>>,
    _arrow: bool,
    style: StyleRefinement,
}

impl HoverCard {
    pub fn new() -> Self {
        Self {
            trigger: div().into_any_element(),
            content: div().into_any_element(),
            position: HoverCardPosition::default(),
            alignment: HoverCardAlignment::default(),
            open_delay: Duration::from_millis(300),
            close_delay: Duration::from_millis(200),
            focus_trigger: HoverCardFocusTrigger::default(),
            is_enabled: true,
            is_open: None,
            is_default_open: false,
            hover_indication: HoverCardHoverIndication::Auto,
            on_open_change: None,
            _arrow: true,
            style: StyleRefinement::default(),
        }
    }

    pub fn children(self, trigger: impl IntoElement) -> Self {
        self.trigger(trigger)
    }

    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = trigger.into_any_element();
        self
    }

    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = content.into_any_element();
        self
    }

    pub fn position(mut self, position: impl Into<HoverCardPosition>) -> Self {
        self.position = position.into();
        self
    }

    pub fn placement(mut self, placement: impl Into<HoverCardPosition>) -> Self {
        self.position = placement.into();
        self
    }

    pub fn alignment(mut self, alignment: impl Into<HoverCardAlignment>) -> Self {
        self.alignment = alignment.into();
        self
    }

    pub fn open_delay(mut self, delay: Duration) -> Self {
        self.open_delay = delay;
        self
    }

    pub fn delay(self, delay: Duration) -> Self {
        self.open_delay(delay)
    }

    pub fn hide_delay(mut self, delay: Duration) -> Self {
        self.close_delay = delay;
        self
    }

    #[allow(non_snake_case)]
    pub fn hideDelay(self, delay: Duration) -> Self {
        self.hide_delay(delay)
    }

    pub fn close_delay(mut self, delay: Duration) -> Self {
        self.close_delay = delay;
        self
    }

    pub fn arrow(mut self, show_arrow: bool) -> Self {
        self._arrow = show_arrow;
        self
    }

    pub fn is_open(mut self, is_open: bool) -> Self {
        self.is_open = Some(is_open);
        self
    }

    pub fn open(self, is_open: bool) -> Self {
        self.is_open(is_open)
    }

    #[allow(non_snake_case)]
    pub fn isOpen(self, is_open: bool) -> Self {
        self.is_open(is_open)
    }

    pub fn is_default_open(mut self, is_default_open: bool) -> Self {
        self.is_default_open = is_default_open;
        self
    }

    pub fn default_open(self, is_default_open: bool) -> Self {
        self.is_default_open(is_default_open)
    }

    #[allow(non_snake_case)]
    pub fn isDefaultOpen(self, is_default_open: bool) -> Self {
        self.is_default_open(is_default_open)
    }

    pub fn is_enabled(mut self, is_enabled: bool) -> Self {
        self.is_enabled = is_enabled;
        self
    }

    #[allow(non_snake_case)]
    pub fn isEnabled(self, is_enabled: bool) -> Self {
        self.is_enabled(is_enabled)
    }

    pub fn focus_trigger(mut self, focus_trigger: HoverCardFocusTrigger) -> Self {
        self.focus_trigger = focus_trigger;
        self
    }

    #[allow(non_snake_case)]
    pub fn focusTrigger(self, focus_trigger: HoverCardFocusTrigger) -> Self {
        self.focus_trigger(focus_trigger)
    }

    pub fn hover_indication(mut self, indication: HoverCardHoverIndication) -> Self {
        self.hover_indication = indication;
        self
    }

    pub fn has_hover_indication(mut self, indication: impl Into<HoverCardHoverIndication>) -> Self {
        self.hover_indication = indication.into();
        self
    }

    #[allow(non_snake_case)]
    pub fn hasHoverIndication(self, indication: impl Into<HoverCardHoverIndication>) -> Self {
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

    pub fn options(mut self, options: HoverCardOptions) -> Self {
        self.position = options.placement;
        self.alignment = options.alignment;
        self.open_delay = options.delay;
        self.close_delay = options.hide_delay;
        self.focus_trigger = options.focus_trigger;
        self.is_enabled = options.is_enabled;
        self.is_open = options.is_open;
        self.is_default_open = options.is_default_open;
        self
    }
}

impl From<bool> for HoverCardHoverIndication {
    fn from(value: bool) -> Self {
        if value {
            Self::Always
        } else {
            Self::Never
        }
    }
}

impl Default for HoverCard {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for HoverCard {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for HoverCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let user_style = self.style;
        let position = match self.position {
            HoverCardPosition::Above => HoverCardPosition::Top,
            HoverCardPosition::Below => HoverCardPosition::Bottom,
            HoverCardPosition::Before => HoverCardPosition::Left,
            HoverCardPosition::After => HoverCardPosition::Right,
            position => position,
        };
        let is_open = self.is_enabled && self.is_open.unwrap_or(self.is_default_open);
        let show_hover_indication =
            matches!(self.hover_indication, HoverCardHoverIndication::Always);
        let _ = (
            self.open_delay,
            self.close_delay,
            self.focus_trigger,
            self.on_open_change,
        );

        div()
            .relative()
            .child(
                div()
                    .when(show_hover_indication, |this| {
                        this.border_b_1().border_color(theme.tokens.border)
                    })
                    .child(self.trigger),
            )
            .when(is_open, |this: Div| {
                this.child(
                    div()
                        .absolute()
                        .when(position == HoverCardPosition::Bottom, |this: Div| {
                            this.top_full().mt(px(8.0))
                        })
                        .when(position == HoverCardPosition::Top, |this: Div| {
                            this.bottom_full().mb(px(8.0))
                        })
                        .when(position == HoverCardPosition::Left, |this: Div| {
                            this.right_full().mr(px(8.0))
                        })
                        .when(position == HoverCardPosition::Right, |this: Div| {
                            this.left_full().ml(px(8.0))
                        })
                        .when(
                            (position == HoverCardPosition::Top
                                || position == HoverCardPosition::Bottom)
                                && self.alignment == HoverCardAlignment::Start,
                            |this: Div| this.left_0(),
                        )
                        .when(
                            (position == HoverCardPosition::Top
                                || position == HoverCardPosition::Bottom)
                                && self.alignment == HoverCardAlignment::End,
                            |this: Div| this.right_0(),
                        )
                        .child(
                            div()
                                .min_w(px(200.0))
                                .max_w(px(400.0))
                                .p(px(12.0))
                                .bg(theme.tokens.popover)
                                .rounded(theme.tokens.radius_lg)
                                .shadow(theme.tokens.shadow_md.to_vec())
                                .map(|this| {
                                    let mut div = this;
                                    div.style().refine(&user_style);
                                    div
                                })
                                .child(self.content),
                        ),
                )
            })
    }
}
