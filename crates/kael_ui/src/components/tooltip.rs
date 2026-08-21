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
use std::panic::Location;
use std::time::Duration;

fn tooltip_content_metrics(content: &str, max_width: Option<Pixels>) -> (Pixels, bool) {
    let estimated = (content.chars().count() as f32 * 7.2 + 16.0).max(48.0);
    let limit = max_width
        .map(f32::from)
        .filter(|width| width.is_finite() && *width > 0.0)
        .unwrap_or(estimated);
    (px(estimated.min(limit)), estimated > limit)
}

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

#[derive(IntoElement)]
pub struct Tooltip {
    id: ElementId,
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
    #[track_caller]
    pub fn new(content: impl Into<SharedString>) -> Self {
        let caller = Location::caller();
        let content = content.into();
        Self {
            id: ElementId::Name(
                format!(
                    "tooltip:{}:{}:{}",
                    caller.file(),
                    caller.line(),
                    caller.column()
                )
                .into(),
            ),
            content: if content.trim().is_empty() {
                "Tooltip".into()
            } else {
                content
            },
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
        let content = content.into();
        self.content = if content.trim().is_empty() {
            "Tooltip".into()
        } else {
            content
        };
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
        let width = f32::from(width);
        self.max_width = Some(px(if width.is_finite() && width > 0.0 {
            width
        } else {
            300.0
        }));
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
        if value { Self::Always } else { Self::Never }
    }
}

impl Styled for Tooltip {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tooltip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = use_theme();
        let content = self.content;
        let user_style = self.style;
        let max_width = self.max_width;
        let (content_width, content_wraps) = tooltip_content_metrics(content.as_ref(), max_width);
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
        let core_side = match placement {
            TooltipPlacement::Top => TooltipSide::Top,
            TooltipPlacement::Bottom => TooltipSide::Bottom,
            TooltipPlacement::Left => TooltipSide::Left,
            TooltipPlacement::Right => TooltipSide::Right,
            _ => TooltipSide::Top,
        };
        let core_align = match alignment {
            TooltipAlignment::Start => TooltipAlign::Start,
            TooltipAlignment::Center => TooltipAlign::Center,
            TooltipAlignment::End => TooltipAlign::End,
        };
        let show_hover_indication = matches!(self.hover_indication, TooltipHoverIndication::Always);
        let show_delay = self.show_delay;
        let hide_delay = self.hide_delay;
        let on_open_change = self.on_open_change;
        let focus_behavior = match self.focus_trigger {
            TooltipFocusTrigger::Auto => TooltipFocusBehavior::Auto,
            TooltipFocusTrigger::Always => TooltipFocusBehavior::Always,
            TooltipFocusTrigger::Never => TooltipFocusBehavior::Never,
        };
        let focus_handle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();
        let anchor_bounds = window.use_keyed_state(
            ElementId::NamedChild(Box::new(self.id.clone()), "anchor-bounds".into()),
            cx,
            |_, _| None,
        );
        let measured_bounds = *anchor_bounds.read(cx);
        let accessibility = if self.focus_trigger == TooltipFocusTrigger::Never {
            AccessibilityAttributes::new(AccessibilityRole::Group)
        } else {
            AccessibilityAttributes::new(AccessibilityRole::Group).description(content.to_string())
        };

        div()
            .accessibility(accessibility)
            .relative()
            // Hug the trigger so anchor-relative placement measures the
            // trigger, not a stretched wrapper.
            .self_start()
            .when(focus_behavior != TooltipFocusBehavior::Never, |this| {
                this.track_focus(&focus_handle).tab_stop(false)
            })
            .id(self.id)
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
                // Measure the trigger wrapper so the bubble can be placed in
                // window coordinates and clamped into the viewport instead of
                // overflowing behind ancestor clipping.
                this.child(
                    canvas_with_prepaint(
                        move |bounds, _, cx| {
                            anchor_bounds.update(cx, |state, _| *state = Some(bounds));
                        },
                        |_, (), _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
                .when_some(measured_bounds, |this, bounds| {
                    const GAP: Pixels = px(4.0);
                    let estimated_height = if content_wraps { px(48.0) } else { px(28.0) };
                    let (corner, position) = match placement {
                        TooltipPlacement::Top => {
                            let x = match alignment {
                                TooltipAlignment::Start => bounds.left(),
                                TooltipAlignment::Center => bounds.center().x - content_width * 0.5,
                                TooltipAlignment::End => bounds.right() - content_width,
                            };
                            (Corner::BottomLeft, point(x, bounds.top() - GAP))
                        }
                        TooltipPlacement::Bottom => {
                            let x = match alignment {
                                TooltipAlignment::Start => bounds.left(),
                                TooltipAlignment::Center => bounds.center().x - content_width * 0.5,
                                TooltipAlignment::End => bounds.right() - content_width,
                            };
                            (Corner::TopLeft, point(x, bounds.bottom() + GAP))
                        }
                        TooltipPlacement::Left => {
                            let y = match alignment {
                                TooltipAlignment::Start => bounds.top(),
                                TooltipAlignment::Center => {
                                    bounds.center().y - estimated_height * 0.5
                                }
                                TooltipAlignment::End => bounds.bottom() - estimated_height,
                            };
                            (Corner::TopRight, point(bounds.left() - GAP, y))
                        }
                        _ => {
                            let y = match alignment {
                                TooltipAlignment::Start => bounds.top(),
                                TooltipAlignment::Center => {
                                    bounds.center().y - estimated_height * 0.5
                                }
                                TooltipAlignment::End => bounds.bottom() - estimated_height,
                            };
                            (Corner::TopLeft, point(bounds.right() + GAP, y))
                        }
                    };
                    let content = content.clone();
                    this.child(deferred(
                        anchored()
                            .anchor(corner)
                            .snap_to_window()
                            .position(position)
                            .child(
                                div()
                                    .id("kael-ui-forced-tooltip-bubble")
                                    .debug_selector(|| "kael-ui-forced-tooltip-bubble".to_string())
                                    .w(content_width)
                                    .px(px(8.0))
                                    .py(px(4.0))
                                    .bg(theme.tokens.foreground)
                                    .text_color(theme.tokens.background)
                                    .rounded(theme.tokens.radius_lg)
                                    .text_size(px(14.0))
                                    .line_height(px(20.0))
                                    .font_family(theme.tokens.font_family.clone())
                                    .when(!content_wraps, |this| this.whitespace_nowrap())
                                    .when_some(max_width, |this, width| this.max_w(width))
                                    .map(|mut this| {
                                        this.style().refine(&user_style);
                                        this
                                    })
                                    .child(StyledText::new(content).accessibility_hidden(true)),
                            ),
                    ))
                })
            })
            .when(!self.disabled && !forced_open, move |this| {
                let content = content.clone();
                let user_style = user_style.clone();
                let fade = fade.clone();
                let build_tooltip = move || {
                    div()
                        .id("kael-ui-hover-tooltip-bubble")
                        .debug_selector(|| "kael-ui-hover-tooltip-bubble".to_string())
                        .w(content_width)
                        .px(px(8.0))
                        .py(px(4.0))
                        .bg(theme.tokens.foreground)
                        .text_color(theme.tokens.background)
                        .rounded(theme.tokens.radius_lg)
                        .text_size(px(14.0))
                        .line_height(px(20.0))
                        .font_family(theme.tokens.font_family.clone())
                        .when(!content_wraps, |this| this.whitespace_nowrap())
                        .when_some(max_width, |this, width| this.max_w(width))
                        .map(|mut this| {
                            this.style().refine(&user_style);
                            this
                        })
                        .child(StyledText::new(content.clone()).accessibility_hidden(true))
                        .with_animation("tooltip-fade-in", fade.clone(), |el, delta| {
                            el.opacity(delta)
                        })
                };
                if let Some(handler) = on_open_change {
                    this.tooltip_element_with_options(
                        show_delay,
                        hide_delay,
                        move |open, window, cx| handler(open, window, cx),
                        build_tooltip,
                    )
                    .tooltip_focus_behavior(focus_behavior)
                    .tooltip_anchor_placement(core_side, core_align)
                } else {
                    this.tooltip_element_with_delays(show_delay, hide_delay, build_tooltip)
                        .tooltip_focus_behavior(focus_behavior)
                        .tooltip_anchor_placement(core_side, core_align)
                }
            })
    }
}

pub fn tooltip<E: IntoElement>(child: E, content: impl Into<SharedString>) -> Tooltip {
    Tooltip::new(content).child(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn content_width_is_readable_bounded_and_invalid_values_are_normalized() {
        let (short, wraps) = tooltip_content_metrics("Tooltip on top", Some(px(300.0)));
        assert!(short > px(100.0));
        assert!(!wraps);

        let (long, wraps) = tooltip_content_metrics(&"x".repeat(100), Some(px(180.0)));
        assert_eq!(long, px(180.0));
        assert!(wraps);

        let tooltip = Tooltip::new(" ").max_width(px(f32::NAN));
        assert_eq!(tooltip.content.as_ref(), "Tooltip");
        assert_eq!(tooltip.max_width, Some(px(300.0)));
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use kael::TestAppContext;

    struct HoverTooltipHost;

    impl Render for HoverTooltipHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().pl(px(60.0)).pt(px(40.0)).child(
                Tooltip::new("Hover placement")
                    .placement(TooltipPlacement::Bottom)
                    .alignment(TooltipAlignment::Start)
                    .show_delay(Duration::ZERO)
                    .child(
                        div()
                            .id("hover-tooltip-trigger")
                            .debug_selector(|| "hover-tooltip-trigger".to_string())
                            .w(px(120.0))
                            .h(px(40.0)),
                    ),
            )
        }
    }

    #[::core::prelude::v1::test]
    fn hover_tooltip_honors_requested_placement() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_view, window) = cx.add_window_view(|_, _| HoverTooltipHost);

        let trigger = window.debug_bounds("hover-tooltip-trigger").unwrap();
        window.simulate_mouse_move(trigger.center(), None, Modifiers::default());
        window.run_until_parked();
        window.update(|window, cx| window.draw(cx).clear());

        let bubble = window
            .debug_bounds("kael-ui-hover-tooltip-bubble")
            .expect("hover tooltip must be visible");
        assert!(
            bubble.top() >= trigger.bottom(),
            "Bottom placement must put the hover tooltip below its trigger: {bubble:?} vs {trigger:?}"
        );
        assert!(
            (bubble.left() - trigger.left()).abs() <= px(1.0),
            "Start alignment must match the trigger's left edge: {bubble:?} vs {trigger:?}"
        );
    }

    struct ForcedEdgeTooltipHost;

    impl Render for ForcedEdgeTooltipHost {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().flex_col().justify_end().child(
                Tooltip::new("Forced edge tooltip")
                    .placement(TooltipPlacement::Bottom)
                    .alignment(TooltipAlignment::Start)
                    .default_open(true)
                    .child(
                        div()
                            .id("forced-tooltip-trigger")
                            .debug_selector(|| "forced-tooltip-trigger".to_string())
                            .w(px(120.0))
                            .h(px(40.0)),
                    ),
            )
        }
    }

    #[::core::prelude::v1::test]
    fn forced_open_tooltip_stays_inside_the_viewport_at_the_window_floor() {
        let mut cx = TestAppContext::single();
        cx.update(|cx| {
            crate::theme::install_theme(cx, crate::theme::Theme::astryx_neutral());
        });
        let (_view, window) = cx.add_window_view(|_, _| ForcedEdgeTooltipHost);

        window.update(|window, cx| window.draw(cx).clear());
        window.update(|window, cx| window.draw(cx).clear());

        let bubble = window
            .debug_bounds("kael-ui-forced-tooltip-bubble")
            .expect("forced-open tooltip must render after one measurement frame");
        let viewport = window.update(|window, _| window.viewport_size());
        assert!(
            bubble.bottom() <= viewport.height + px(1.0),
            "a forced-open tooltip at the window floor must clamp inside the viewport: {bubble:?}"
        );
        assert!(
            bubble.top() >= px(0.0),
            "a forced-open tooltip must not start above the window: {bubble:?}"
        );
    }
}
