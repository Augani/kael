//! Div is the central, reusable element that most GPUI trees will be built from.
//! It functions as a container for other elements, and provides a number of
//! useful features for laying out and styling its children as well as binding
//! mouse events and action handlers. It is meant to be similar to the HTML `<div>`
//! element, but for GPUI.
//!
//! # Build your own div
//!
//! GPUI does not directly provide APIs for stateful, multi step events like `click`
//! and `drag`. We want GPUI users to be able to build their own abstractions for
//! their own needs. However, as a UI framework, we're also obliged to provide some
//! building blocks to make the process of building your own elements easier.
//! For this we have the [`Interactivity`] and the [`StyleRefinement`] structs, as well
//! as several associated traits. Together, these provide the full suite of Dom-like events
//! and Tailwind-like styling that you can use to build your own custom elements. Div is
//! constructed by combining these two systems into an all-in-one element.

use crate::interpolate::{interpolate_f32, interpolate_hsla, interpolate_shadows};
use crate::scroll_elasticity::{
    advance_scroll_elasticity, apply_scroll_delta_axis, rubber_band_scroll_enabled,
};
#[allow(unused_imports)]
use crate::{
    AbsoluteLength, Action, AnyDrag, AnyElement, AnyTooltip, AnyView, App, AppContext, Background,
    Bounds, BoxShadow, ClickEvent, Context, Corners, CursorStyle, DispatchPhase, Element,
    ElementId, Entity, Fill, FocusHandle, GestureRecognizer, Global, GlobalElementId, Hitbox,
    HitboxBehavior, HitboxId, Hsla, InspectorElementId, IntoElement, IsZero, KeyContext,
    KeyDownEvent, KeyUpEvent, KeyboardButton, KeyboardClickEvent, LayoutId, LongPressGesture,
    LongPressGestureEvent, MagnifyEvent, ModifiersChangedEvent, MouseButton, MouseClickEvent,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Overflow, PanGesture, PanGestureEvent, PanState,
    ParentElement, PinchGesture, PinchGestureEvent, Pixels, Point, Render, Rgba, ScaledPixels,
    ScrollDelta, ScrollWheelEvent, SharedString, Size, Style, StyleRefinement, Styled,
    SwipeDirection, SwipeGesture, SwipeGestureEvent, TapGesture, TapGestureEvent, Task, TooltipId,
    TouchPhase, TransformationMatrix, Visibility, Window, WindowControlArea, point, px, size,
};
use collections::HashMap;
use refineable::Refineable;
use smallvec::SmallVec;
use stacksafe::{StackSafe, stacksafe};
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    cmp::Ordering,
    fmt::Debug,
    marker::PhantomData,
    mem,
    rc::Rc,
    sync::Arc,
    time::Duration,
};
use util::ResultExt;

use super::ImageCacheProvider;

const DRAG_THRESHOLD: f64 = 2.;
const TOOLTIP_SHOW_DELAY: Duration = Duration::from_millis(500);
const HOVERABLE_TOOLTIP_HIDE_DELAY: Duration = Duration::from_millis(500);
const IMPLICIT_STYLE_TRANSITION_DURATION: Duration = Duration::from_millis(150);
const AUTO_SCROLLBAR_IDLE_SECONDS: f64 = 1.2;
const CONTEXT_MENU_PANEL_WIDTH: f32 = 192.0;
const CONTEXT_MENU_PANEL_PADDING: f32 = 6.0;
const CONTEXT_MENU_ITEM_HEIGHT: f32 = 32.0;
const CONTEXT_MENU_SEPARATOR_HEIGHT: f32 = 9.0;
const TOOLTIP_PANEL_RADIUS: f32 = 8.0;

#[derive(Clone, PartialEq)]
struct ImplicitVisualStyle {
    opacity: f32,
    rotate: f32,
    scale: Point<f32>,
    transform_origin: Point<f32>,
    background: Option<Background>,
    border_color: Option<Hsla>,
    text_color: Option<Hsla>,
    corner_radii: Corners<AbsoluteLength>,
    box_shadow: SmallVec<[BoxShadow; 1]>,
}

impl From<&Style> for ImplicitVisualStyle {
    fn from(style: &Style) -> Self {
        Self {
            opacity: style.opacity.unwrap_or(1.0),
            rotate: style.rotate.unwrap_or(0.0),
            scale: style.scale.unwrap_or(Point { x: 1.0, y: 1.0 }),
            transform_origin: style.transform_origin.unwrap_or(Point { x: 0.5, y: 0.5 }),
            background: style.background.as_ref().and_then(Fill::color),
            border_color: style.border_color,
            text_color: style.text.color,
            corner_radii: style.corner_radii,
            box_shadow: style.box_shadow.clone(),
        }
    }
}

impl ImplicitVisualStyle {
    fn can_transition_to(&self, other: &Self) -> bool {
        self.opacity != other.opacity
            || self.rotate != other.rotate
            || self.scale != other.scale
            || self.transform_origin != other.transform_origin
            || self.border_color != other.border_color
            || self.text_color != other.text_color
            || self.corner_radii != other.corner_radii
            || self.box_shadow != other.box_shadow
            || optional_backgrounds_can_interpolate(self.background, other.background)
                && self.background != other.background
    }

    fn interpolate(&self, other: &Self, progress: f32) -> Self {
        Self {
            opacity: interpolate_f32(self.opacity, other.opacity, progress),
            rotate: interpolate_f32(self.rotate, other.rotate, progress),
            scale: Point {
                x: interpolate_f32(self.scale.x, other.scale.x, progress),
                y: interpolate_f32(self.scale.y, other.scale.y, progress),
            },
            transform_origin: Point {
                x: interpolate_f32(self.transform_origin.x, other.transform_origin.x, progress),
                y: interpolate_f32(self.transform_origin.y, other.transform_origin.y, progress),
            },
            background: interpolate_optional_background(
                self.background,
                other.background,
                progress,
            ),
            border_color: interpolate_optional_hsla(
                self.border_color,
                other.border_color,
                progress,
            ),
            text_color: interpolate_optional_hsla(self.text_color, other.text_color, progress),
            corner_radii: interpolate_corners(&self.corner_radii, &other.corner_radii, progress),
            box_shadow: interpolate_shadows(&self.box_shadow, &other.box_shadow, progress),
        }
    }

    fn apply_to(&self, style: &mut Style) {
        style.opacity = normalize_f32(self.opacity, 1.0);
        style.rotate = normalize_f32(self.rotate, 0.0);
        style.scale = normalize_point(self.scale, Point { x: 1.0, y: 1.0 });
        style.transform_origin = normalize_point(self.transform_origin, Point { x: 0.5, y: 0.5 });
        style.background = self.background.map(Fill::from);
        style.border_color = self.border_color;
        style.text.color = self.text_color;
        style.corner_radii = self.corner_radii;
        style.box_shadow = self.box_shadow.clone();
    }
}

/// Configuration for implicit style transitions on an element.
///
/// Controls how keyed style changes (hover, active, state-driven restyles) are
/// animated. See [`InteractiveElement::transition`].
#[derive(Clone)]
pub struct TransitionConfig {
    /// How long a property change takes to animate.
    pub duration: Duration,
    /// The easing curve. `None` uses a soft ease-out.
    pub easing: Option<crate::animation::Easing>,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            duration: IMPLICIT_STYLE_TRANSITION_DURATION,
            easing: None,
        }
    }
}

struct ImplicitStyleTransition {
    from: ImplicitVisualStyle,
    to: ImplicitVisualStyle,
    started_at: std::time::Instant,
    duration: Duration,
    easing: Option<crate::animation::Easing>,
}

impl ImplicitStyleTransition {
    fn progress(&self, now: std::time::Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.duration_since(self.started_at).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }

    fn current_style(&self, now: std::time::Instant) -> ImplicitVisualStyle {
        let progress = self.progress(now);
        let eased = match &self.easing {
            Some(easing) => easing.ease(progress),
            None => ease_out_progress(progress),
        };
        self.from.interpolate(&self.to, eased)
    }
}

#[derive(Default)]
struct ImplicitStyleAnimationState {
    target: Option<ImplicitVisualStyle>,
    active_transition: Option<ImplicitStyleTransition>,
}

impl ImplicitStyleAnimationState {
    fn animate(&mut self, style: Style, config: &TransitionConfig, window: &mut Window) -> Style {
        let (style, needs_animation_frame) = self.resolve(
            style,
            std::time::Instant::now(),
            window.animations_enabled(),
            config,
        );
        if needs_animation_frame {
            window.request_animation_frame();
        }
        style
    }

    fn resolve(
        &mut self,
        style: Style,
        now: std::time::Instant,
        animations_enabled: bool,
        config: &TransitionConfig,
    ) -> (Style, bool) {
        let target = ImplicitVisualStyle::from(&style);

        if !animations_enabled {
            self.target = Some(target);
            self.active_transition = None;
            return (style, false);
        }

        let current = self.current_style(now);

        if self.target.as_ref() != Some(&target) {
            self.target = Some(target.clone());

            if let Some(current) = current {
                if current != target && current.can_transition_to(&target) {
                    self.active_transition = Some(ImplicitStyleTransition {
                        from: current,
                        to: target.clone(),
                        started_at: now,
                        duration: config.duration,
                        easing: config.easing.clone(),
                    });
                } else {
                    self.active_transition = None;
                }
            }
        }

        let (resolved, needs_animation_frame) =
            if let Some(transition) = self.active_transition.as_ref() {
                if transition.progress(now) < 1.0 {
                    (transition.current_style(now), true)
                } else {
                    self.active_transition = None;
                    (target, false)
                }
            } else {
                (target, false)
            };

        let mut style = style;
        resolved.apply_to(&mut style);
        (style, needs_animation_frame)
    }

    fn current_style(&self, now: std::time::Instant) -> Option<ImplicitVisualStyle> {
        self.active_transition
            .as_ref()
            .map(|transition| transition.current_style(now))
            .or_else(|| self.target.clone())
    }
}

fn ease_out_progress(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(5)
}

fn normalize_f32(value: f32, default: f32) -> Option<f32> {
    ((value - default).abs() > 0.0001).then_some(value)
}

fn normalize_point(value: Point<f32>, default: Point<f32>) -> Option<Point<f32>> {
    ((value.x - default.x).abs() > 0.0001 || (value.y - default.y).abs() > 0.0001).then_some(value)
}

fn interpolate_optional_hsla(from: Option<Hsla>, to: Option<Hsla>, progress: f32) -> Option<Hsla> {
    match (from, to) {
        (None, None) => None,
        (Some(from), Some(to)) => Some(interpolate_hsla(from, to, progress)),
        (Some(from), None) => Some(interpolate_hsla(from, from.alpha(0.0), progress)),
        (None, Some(to)) => Some(interpolate_hsla(to.alpha(0.0), to, progress)),
    }
}

#[derive(Default)]
pub(crate) struct FlipLayoutState {
    prev_origin: Option<Point<Pixels>>,
    animation: Option<FlipAnimation>,
}

struct FlipAnimation {
    offset: Point<Pixels>,
    started_at: std::time::Instant,
}

impl FlipLayoutState {
    fn resolve(
        &mut self,
        origin: Point<Pixels>,
        now: std::time::Instant,
        animations_enabled: bool,
        config: &TransitionConfig,
    ) -> (Option<Point<Pixels>>, bool) {
        let prev_origin = self.prev_origin.replace(origin);

        if !animations_enabled || config.duration.is_zero() {
            self.animation = None;
            return (None, false);
        }

        if let Some(prev_origin) = prev_origin
            && prev_origin != origin
        {
            let current_offset = self.current_offset(now, config).unwrap_or(Point::default());
            let offset = current_offset + (prev_origin - origin);
            if offset != Point::default() {
                self.animation = Some(FlipAnimation {
                    offset,
                    started_at: now,
                });
            }
        }

        match self.current_offset(now, config) {
            Some(offset) => (Some(offset), true),
            None => {
                self.animation = None;
                (None, false)
            }
        }
    }

    fn current_offset(
        &self,
        now: std::time::Instant,
        config: &TransitionConfig,
    ) -> Option<Point<Pixels>> {
        let animation = self.animation.as_ref()?;
        let progress = (now.duration_since(animation.started_at).as_secs_f32()
            / config.duration.as_secs_f32())
        .clamp(0.0, 1.0);
        if progress >= 1.0 {
            return None;
        }
        let eased = match &config.easing {
            Some(easing) => easing.ease(progress),
            None => ease_out_progress(progress),
        };
        let remaining = 1.0 - eased;
        let offset = Point {
            x: px(animation.offset.x.0 * remaining),
            y: px(animation.offset.y.0 * remaining),
        };
        if offset.x.0.abs() < 0.05 && offset.y.0.abs() < 0.05 {
            None
        } else {
            Some(offset)
        }
    }
}

fn optional_backgrounds_can_interpolate(from: Option<Background>, to: Option<Background>) -> bool {
    match (from, to) {
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => true,
        (Some(from), Some(to)) => backgrounds_can_interpolate(from, to),
    }
}

fn interpolate_optional_background(
    from: Option<Background>,
    to: Option<Background>,
    progress: f32,
) -> Option<Background> {
    match (from, to) {
        (None, None) => None,
        (Some(from), Some(to)) => Some(interpolate_background(from, to, progress)),
        (Some(from), None) => Some(interpolate_background(
            from,
            transparent_background_like(from),
            progress,
        )),
        (None, Some(to)) => Some(interpolate_background(
            transparent_background_like(to),
            to,
            progress,
        )),
    }
}

fn backgrounds_can_interpolate(from: Background, to: Background) -> bool {
    from.tag == to.tag && from.stop_count == to.stop_count && from.color_space == to.color_space
}

fn interpolate_background(from: Background, to: Background, progress: f32) -> Background {
    if !backgrounds_can_interpolate(from, to) {
        return to;
    }

    let mut background = to;
    background.solid = interpolate_hsla(from.solid, to.solid, progress);
    background.gradient_angle_or_pattern_height = interpolate_f32(
        from.gradient_angle_or_pattern_height,
        to.gradient_angle_or_pattern_height,
        progress,
    );
    background.center = [
        interpolate_f32(from.center[0], to.center[0], progress),
        interpolate_f32(from.center[1], to.center[1], progress),
    ];
    background.radius = [
        interpolate_f32(from.radius[0], to.radius[0], progress),
        interpolate_f32(from.radius[1], to.radius[1], progress),
    ];
    background.colors = std::array::from_fn(|ix| crate::LinearColorStop {
        color: interpolate_hsla(from.colors[ix].color, to.colors[ix].color, progress),
        percentage: interpolate_f32(
            from.colors[ix].percentage,
            to.colors[ix].percentage,
            progress,
        ),
    });
    background
}

fn transparent_background_like(background: Background) -> Background {
    let mut transparent = background;
    transparent.solid = transparent.solid.alpha(0.0);
    transparent.colors = std::array::from_fn(|ix| crate::LinearColorStop {
        color: transparent.colors[ix].color.alpha(0.0),
        percentage: transparent.colors[ix].percentage,
    });
    transparent
}

fn interpolate_absolute_length(
    from: AbsoluteLength,
    to: AbsoluteLength,
    progress: f32,
) -> AbsoluteLength {
    match (from, to) {
        (AbsoluteLength::Pixels(from), AbsoluteLength::Pixels(to)) => {
            AbsoluteLength::Pixels(px(interpolate_f32(from.0, to.0, progress)))
        }
        (AbsoluteLength::Rems(from), AbsoluteLength::Rems(to)) => {
            AbsoluteLength::Rems(crate::Rems(interpolate_f32(from.0, to.0, progress)))
        }
        (_, to) => to,
    }
}

fn interpolate_corners(
    from: &Corners<AbsoluteLength>,
    to: &Corners<AbsoluteLength>,
    progress: f32,
) -> Corners<AbsoluteLength> {
    Corners {
        top_left: interpolate_absolute_length(from.top_left, to.top_left, progress),
        top_right: interpolate_absolute_length(from.top_right, to.top_right, progress),
        bottom_right: interpolate_absolute_length(from.bottom_right, to.bottom_right, progress),
        bottom_left: interpolate_absolute_length(from.bottom_left, to.bottom_left, progress),
    }
}

/// The styling information for a given group.
pub struct GroupStyle {
    /// The identifier for this group.
    pub group: SharedString,

    /// The specific style refinement that this group would apply
    /// to its children.
    pub style: Box<StyleRefinement>,
}

/// An event for when a drag is moving over this element, with the given state type.
pub struct DragMoveEvent<T> {
    /// The mouse move event that triggered this drag move event.
    pub event: MouseMoveEvent,

    /// The bounds of this element.
    pub bounds: Bounds<Pixels>,
    drag: PhantomData<T>,
    dragged_item: Arc<dyn Any>,
}

impl<T: 'static> DragMoveEvent<T> {
    /// Returns the drag state for this event.
    pub fn drag<'b>(&self, cx: &'b App) -> &'b T {
        cx.active_drag
            .as_ref()
            .and_then(|drag| drag.value.downcast_ref::<T>())
            .expect("DragMoveEvent is only valid when the stored active drag is of the same type.")
    }

    /// An item that is about to be dropped.
    pub fn dragged_item(&self) -> &dyn Any {
        self.dragged_item.as_ref()
    }
}

/// An event that fires when an element's bounds change size.
#[derive(Clone, Debug)]
pub struct ResizeEvent {
    /// The new size of the element.
    pub size: Size<Pixels>,
    /// The new bounds of the element.
    pub bounds: Bounds<Pixels>,
}

impl Interactivity {
    /// Create an `Interactivity`, capturing the caller location in debug mode.
    #[cfg(any(feature = "inspector", debug_assertions))]
    #[track_caller]
    pub fn new() -> Interactivity {
        Interactivity {
            source_location: Some(core::panic::Location::caller()),
            ..Default::default()
        }
    }

    /// Create an `Interactivity`, capturing the caller location in debug mode.
    #[cfg(not(any(feature = "inspector", debug_assertions)))]
    pub fn new() -> Interactivity {
        Interactivity::default()
    }

    /// Gets the source location of construction. Returns `None` when not in debug mode.
    pub fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            self.source_location
        }

        #[cfg(not(any(feature = "inspector", debug_assertions)))]
        {
            None
        }
    }

    fn sync_tracked_focus_handle(&mut self) {
        if let Some(handle) = self.tracked_focus_handle.take() {
            let mut handle = handle.tab_stop(self.tab_stop);
            if let Some(index) = self.tab_index {
                handle = handle.tab_index(index);
            }
            self.tracked_focus_handle = Some(handle);
        }
    }

    /// Bind the given callback to the mouse down event for the given mouse button, during the bubble phase
    /// The imperative API equivalent of [`InteractiveElement::on_mouse_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to the view state from this callback.
    pub fn on_mouse_down(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == button
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase
    /// The imperative API equivalent of [`InteractiveElement::capture_any_mouse_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_any_mouse_down(
        &mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event for any button, during the bubble phase
    /// the imperative API equivalent to [`InteractiveElement::on_any_mouse_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_any_mouse_down(
        &mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for the given button, during the bubble phase
    /// the imperative API equivalent to [`InteractiveElement::on_mouse_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_up(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == button
                    && hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for any button, during the capture phase
    /// the imperative API equivalent to [`InteractiveElement::capture_any_mouse_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_any_mouse_up(
        &mut self,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event for any button, during the bubble phase
    /// the imperative API equivalent to [`Interactivity::on_any_mouse_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_any_mouse_up(
        &mut self,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse down event, on any button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_down_out`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_down_out(
        &mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_down_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture && !hitbox.contains(&window.mouse_position()) {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to the mouse up event, for the given button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_up_out`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_up_out(
        &mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_up_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture
                    && event.button == button
                    && !hitbox.is_hovered(window)
                {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to the mouse move event, during the bubble phase
    /// The imperative API equivalent to [`InteractiveElement::on_mouse_move`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_mouse_move(
        &mut self,
        listener: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) {
        self.mouse_move_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to the mouse drag event of the given type. Note that this
    /// will be called for all move events, inside or outside of this element, as long as the
    /// drag was started with this element under the mouse. Useful for implementing draggable
    /// UIs that don't conform to a drag and drop style interaction, like resizing.
    /// The imperative API equivalent to [`InteractiveElement::on_drag_move`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drag_move<T>(
        &mut self,
        listener: impl Fn(&DragMoveEvent<T>, &mut Window, &mut App) + 'static,
    ) where
        T: 'static,
    {
        self.mouse_move_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Capture
                    && let Some(drag) = &cx.active_drag
                    && drag.value.as_ref().type_id() == TypeId::of::<T>()
                {
                    (listener)(
                        &DragMoveEvent {
                            event: event.clone(),
                            bounds: hitbox.bounds,
                            drag: PhantomData,
                            dragged_item: Arc::clone(&drag.value),
                        },
                        window,
                        cx,
                    );
                }
            }));
    }

    /// Bind the given callback to scroll wheel events during the bubble phase
    /// The imperative API equivalent to [`InteractiveElement::on_scroll_wheel`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_scroll_wheel(
        &mut self,
        listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) {
        self.scroll_wheel_listeners
            .push(Box::new(move |event, phase, hitbox, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                    (listener)(event, window, cx);
                }
            }));
    }

    /// Bind the given callback to pan gesture updates for this element.
    pub fn on_pan(&mut self, listener: impl Fn(&PanGestureEvent, &mut Window, &mut App) + 'static) {
        self.pan_listeners.push(Box::new(listener));
    }

    /// Bind the given callback to swipe gestures in the requested direction.
    pub fn on_swipe(
        &mut self,
        direction: SwipeDirection,
        listener: impl Fn(&SwipeGestureEvent, &mut Window, &mut App) + 'static,
    ) {
        self.swipe_listeners.push((direction, Box::new(listener)));
    }

    /// Bind the given callback to pinch gestures for this element.
    pub fn on_pinch(
        &mut self,
        listener: impl Fn(&PinchGestureEvent, &mut Window, &mut App) + 'static,
    ) {
        self.pinch_listeners.push(Box::new(listener));
    }

    /// Bind the given callback to tap gestures for this element.
    pub fn on_tap(&mut self, listener: impl Fn(&TapGestureEvent, &mut Window, &mut App) + 'static) {
        self.tap_listeners.push(Box::new(listener));
    }

    /// Bind the given callback to long-press gestures for this element.
    pub fn on_long_press(
        &mut self,
        listener: impl Fn(&LongPressGestureEvent, &mut Window, &mut App) + 'static,
    ) {
        self.long_press_listeners.push(Box::new(listener));
    }

    /// Bind the given callback to an action dispatch during the capture phase
    /// The imperative API equivalent to [`InteractiveElement::capture_action`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_action<A: Action>(
        &mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) {
        self.action_listeners.push((
            TypeId::of::<A>(),
            Box::new(move |action, phase, window, cx| {
                let action = action.downcast_ref().unwrap();
                if phase == DispatchPhase::Capture {
                    (listener)(action, window, cx)
                } else {
                    cx.propagate();
                }
            }),
        ));
    }

    /// Bind the given callback to an action dispatch during the bubble phase
    /// The imperative API equivalent to [`InteractiveElement::on_action`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_action<A: Action>(&mut self, listener: impl Fn(&A, &mut Window, &mut App) + 'static) {
        self.action_listeners.push((
            TypeId::of::<A>(),
            Box::new(move |action, phase, window, cx| {
                let action = action.downcast_ref().unwrap();
                if phase == DispatchPhase::Bubble {
                    (listener)(action, window, cx)
                }
            }),
        ));
    }

    /// Bind the given callback to an action dispatch, based on a dynamic action parameter
    /// instead of a type parameter. Useful for component libraries that want to expose
    /// action bindings to their users.
    /// The imperative API equivalent to [`InteractiveElement::on_boxed_action`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_boxed_action(
        &mut self,
        action: &dyn Action,
        listener: impl Fn(&dyn Action, &mut Window, &mut App) + 'static,
    ) {
        let action = action.boxed_clone();
        self.action_listeners.push((
            (*action).type_id(),
            Box::new(move |_, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    (listener)(&*action, window, cx)
                }
            }),
        ));
    }

    /// Bind the given callback to key down events during the bubble phase
    /// The imperative API equivalent to [`InteractiveElement::on_key_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_key_down(
        &mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.key_down_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    (listener)(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to key down events during the capture phase
    /// The imperative API equivalent to [`InteractiveElement::capture_key_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_key_down(
        &mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) {
        self.key_down_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    listener(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to key up events during the bubble phase
    /// The imperative API equivalent to [`InteractiveElement::on_key_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_key_up(&mut self, listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static) {
        self.key_up_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    listener(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to key up events during the capture phase
    /// The imperative API equivalent to [`InteractiveElement::on_key_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn capture_key_up(
        &mut self,
        listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static,
    ) {
        self.key_up_listeners
            .push(Box::new(move |event, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    listener(event, window, cx)
                }
            }));
    }

    /// Bind the given callback to modifiers changing events.
    /// The imperative API equivalent to [`InteractiveElement::on_modifiers_changed`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_modifiers_changed(
        &mut self,
        listener: impl Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) {
        self.modifiers_changed_listeners
            .push(Box::new(move |event, window, cx| {
                listener(event, window, cx)
            }));
    }

    /// Bind the given callback to drop events of the given type, whether or not the drag started on this element
    /// The imperative API equivalent to [`InteractiveElement::on_drop`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drop<T: 'static>(&mut self, listener: impl Fn(&T, &mut Window, &mut App) + 'static) {
        self.drop_listeners.push((
            TypeId::of::<T>(),
            Box::new(move |dragged_value, window, cx| {
                listener(dragged_value.downcast_ref().unwrap(), window, cx);
            }),
        ));
    }

    /// Use the given predicate to determine whether or not a drop event should be dispatched to this element
    /// The imperative API equivalent to [`InteractiveElement::can_drop`]
    pub fn can_drop(
        &mut self,
        predicate: impl Fn(&dyn Any, &mut Window, &mut App) -> bool + 'static,
    ) {
        self.can_drop_predicate = Some(Box::new(predicate));
    }

    /// Bind the given callback to click events of this element
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_click`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_click(&mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        self.click_listeners.push(Rc::new(move |event, window, cx| {
            listener(event, window, cx)
        }));
    }

    /// On drag initiation, this callback will be used to create a new view to render the dragged value for a
    /// drag and drop operation. This API should also be used as the equivalent of 'on drag start' with
    /// the [`Self::on_drag_move`] API
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_drag`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_drag<T, W>(
        &mut self,
        value: T,
        constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
    ) where
        Self: Sized,
        T: 'static,
        W: 'static + Render,
    {
        debug_assert!(
            self.drag_listener.is_none(),
            "calling on_drag more than once on the same element is not supported"
        );
        self.drag_listener = Some((
            Arc::new(value),
            Box::new(move |value, offset, window, cx| {
                constructor(value.downcast_ref().unwrap(), offset, window, cx).into()
            }),
        ));
    }

    /// Bind the given callback on the hover start and end events of this element. Note that the boolean
    /// passed to the callback is true when the hover starts and false when it ends.
    /// The imperative API equivalent to [`StatefulInteractiveElement::on_hover`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_hover(&mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        debug_assert!(
            self.hover_listener.is_none(),
            "calling on_hover more than once on the same element is not supported"
        );
        self.hover_listener = Some(Box::new(listener));
    }

    /// Bind the given callback to be called when this element's bounds change size.
    /// The imperative API equivalent of [`StatefulInteractiveElement::on_resize`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    pub fn on_resize(&mut self, listener: impl Fn(&ResizeEvent, &mut Window, &mut App) + 'static)
    where
        Self: Sized,
    {
        self.resize_listeners.push(Box::new(listener));
    }

    /// Use the given content to construct a tooltip when the mouse hovers over this element.
    /// String-like values render the default tooltip bubble, while closures can build a custom tooltip view.
    /// The imperative API equivalent to [`StatefulInteractiveElement::tooltip`]
    pub fn tooltip(&mut self, tooltip: impl TooltipContent)
    where
        Self: Sized,
    {
        debug_assert!(
            self.tooltip_builder.is_none(),
            "calling tooltip more than once on the same element is not supported"
        );
        self.tooltip_builder = Some(TooltipBuilder {
            build: tooltip.into_tooltip_renderer(),
            hoverable: false,
        });
    }

    /// Use the given callback to construct a rich tooltip element when the mouse hovers over this element.
    pub fn tooltip_element<E>(&mut self, build_tooltip: impl Fn() -> E + 'static)
    where
        Self: Sized,
        E: IntoElement,
    {
        let build_tooltip = Rc::new(move || build_tooltip().into_any_element());
        debug_assert!(
            self.tooltip_builder.is_none(),
            "calling tooltip more than once on the same element is not supported"
        );
        self.tooltip_builder = Some(TooltipBuilder {
            build: Rc::new(move |_, cx| {
                cx.new(|_| TooltipElementView {
                    build: build_tooltip.clone(),
                })
                .into()
            }),
            hoverable: false,
        });
    }

    /// Use the given callback to construct a custom tooltip view when the mouse hovers over this element.
    pub fn tooltip_view(
        &mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) where
        Self: Sized,
    {
        self.tooltip(build_tooltip);
    }

    /// Use the given callback to construct a new tooltip view when the mouse hovers over this element.
    /// The tooltip itself is also hoverable and won't disappear when the user moves the mouse into
    /// the tooltip. The imperative API equivalent to [`StatefulInteractiveElement::hoverable_tooltip`]
    pub fn hoverable_tooltip(
        &mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) where
        Self: Sized,
    {
        debug_assert!(
            self.tooltip_builder.is_none(),
            "calling tooltip more than once on the same element is not supported"
        );
        self.tooltip_builder = Some(TooltipBuilder {
            build: Rc::new(build_tooltip),
            hoverable: true,
        });
    }

    /// Use the given callback to construct an in-window context menu when the user secondary-clicks this element.
    pub fn context_menu(
        &mut self,
        build_context_menu: impl Fn(ContextMenu) -> ContextMenu + 'static,
    ) where
        Self: Sized,
    {
        debug_assert!(
            self.context_menu_builder.is_none(),
            "calling context_menu more than once on the same element is not supported"
        );
        self.context_menu_builder = Some(ContextMenuBuilder {
            build: Rc::new(move |active_context_menu, _, cx| {
                build_context_menu(ContextMenu::new())
                    .into_any_view(active_context_menu.clone(), cx)
            }),
        });
    }

    /// Use the given callback to construct a custom in-window context menu view when the user secondary-clicks this element.
    pub fn context_menu_view(
        &mut self,
        build_context_menu: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) where
        Self: Sized,
    {
        debug_assert!(
            self.context_menu_builder.is_none(),
            "calling context_menu more than once on the same element is not supported"
        );
        self.context_menu_builder = Some(ContextMenuBuilder {
            build: Rc::new(move |_, window, cx| build_context_menu(window, cx)),
        });
    }

    /// Block the mouse from all interactions with elements behind this element's hitbox. Typically
    /// `block_mouse_except_scroll` should be preferred.
    ///
    /// The imperative API equivalent to [`InteractiveElement::occlude`]
    pub fn occlude_mouse(&mut self) {
        self.hitbox_behavior = HitboxBehavior::BlockMouse;
    }

    /// Set the bounds of this element as a window control area for the platform window.
    /// The imperative API equivalent to [`InteractiveElement::window_control_area`]
    pub fn window_control_area(&mut self, area: WindowControlArea) {
        self.window_control = Some(area);
    }

    /// Block non-scroll mouse interactions with elements behind this element's hitbox. See
    /// [`Hitbox::is_hovered`] for details.
    ///
    /// The imperative API equivalent to [`InteractiveElement::block_mouse_except_scroll`]
    pub fn block_mouse_except_scroll(&mut self) {
        self.hitbox_behavior = HitboxBehavior::BlockMouseExceptScroll;
    }
}

/// A trait for elements that want to use the standard GPUI event handlers that don't
/// require any state.
pub trait InteractiveElement: Sized {
    /// Retrieve the interactivity state associated with this element
    fn interactivity(&mut self) -> &mut Interactivity;

    /// Enable implicit visual transitions for keyed style changes on this element.
    ///
    /// Only elements with a stable [`ElementId`] can transition across frames. Unkeyed
    /// elements continue to snap to their new visual state.
    fn implicit_transitions(mut self) -> Self {
        self.interactivity().implicit_transition = Some(TransitionConfig::default());
        self
    }

    /// Animate keyed style changes (hover, active, state-driven restyles) over `duration`.
    ///
    /// Interpolates background, border color, text color, opacity, corner radii,
    /// box shadows, rotation, and scale whenever the computed style changes.
    /// Requires a stable [`ElementId`]; unkeyed elements snap.
    fn transition(mut self, duration: Duration) -> Self {
        self.interactivity().implicit_transition = Some(TransitionConfig {
            duration,
            easing: None,
        });
        self
    }

    /// Animate keyed style changes with an explicit duration and easing curve.
    fn transition_with(mut self, duration: Duration, easing: crate::animation::Easing) -> Self {
        self.interactivity().implicit_transition = Some(TransitionConfig {
            duration,
            easing: Some(easing),
        });
        self
    }

    /// Animate this element's position when layout moves it (FLIP). When the
    /// element's painted origin changes between frames, it glides from the old
    /// position to the new one over `duration` instead of snapping.
    ///
    /// Requires a stable [`ElementId`]. Avoid enabling this on children of
    /// containers that scroll while the animation runs; scrolling moves the
    /// element and restarts the glide.
    fn animate_layout(mut self, duration: Duration) -> Self {
        self.interactivity().layout_animation = Some(TransitionConfig {
            duration,
            easing: None,
        });
        self
    }

    /// Animate layout position changes with an explicit duration and easing curve.
    fn animate_layout_with(mut self, duration: Duration, easing: crate::animation::Easing) -> Self {
        self.interactivity().layout_animation = Some(TransitionConfig {
            duration,
            easing: Some(easing),
        });
        self
    }

    /// Assign this element to a group of elements that can be styled together
    fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.interactivity().group = Some(group.into());
        self
    }

    /// Assign this element an ID, so that it can be used with interactivity
    fn id(mut self, id: impl Into<ElementId>) -> Stateful<Self> {
        self.interactivity().element_id = Some(id.into());

        Stateful { element: self }
    }

    /// Track the focus state of the given focus handle on this element.
    /// If the focus handle is focused by the application, this element will
    /// apply its focused styles.
    fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.interactivity().focusable = true;
        self.interactivity().tracked_focus_handle = Some(focus_handle.clone());
        self.interactivity().sync_tracked_focus_handle();
        self
    }

    /// Set whether this element is a tab stop.
    ///
    /// When false, the element remains in tab-index order but cannot be reached via keyboard navigation.
    /// Useful for container elements: focus the container, then call `window.focus_next()` to focus
    /// the first tab stop inside it while having the container element itself be unreachable via the keyboard.
    /// Should only be used with `tab_index`.
    fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.interactivity().tab_stop = tab_stop;
        self.interactivity().sync_tracked_focus_handle();
        self
    }

    /// Set index of the tab stop order, and set this node as a tab stop.
    /// This will default the element to being a tab stop. See [`Self::tab_stop`] for more information.
    /// This should only be used in conjunction with `tab_group`
    /// in order to not interfere with the tab index of other elements.
    fn tab_index(mut self, index: isize) -> Self {
        self.interactivity().focusable = true;
        self.interactivity().tab_index = Some(index);
        self.interactivity().tab_stop = true;
        self.interactivity().sync_tracked_focus_handle();
        self
    }

    /// Designate this div as a "tab group". Tab groups have their own location in the tab-index order,
    /// but for children of the tab group, the tab index is reset to 0. This can be useful for swapping
    /// the order of tab stops within the group, without having to renumber all the tab stops in the whole
    /// application.
    fn tab_group(mut self) -> Self {
        self.interactivity().tab_group = true;
        if self.interactivity().tab_index.is_none() {
            self.interactivity().tab_index = Some(0);
        }
        self
    }

    /// Set the keymap context for this element. This will be used to determine
    /// which action to dispatch from the keymap.
    fn key_context<C, E>(mut self, key_context: C) -> Self
    where
        C: TryInto<KeyContext, Error = E>,
        E: Debug,
    {
        if let Some(key_context) = key_context.try_into().log_err() {
            self.interactivity().key_context = Some(key_context);
        }
        self
    }

    /// Apply the given style to this element when the mouse hovers over it
    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        debug_assert!(
            self.interactivity().hover_style.is_none(),
            "hover style already set"
        );
        self.interactivity().hover_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Apply the given style to this element when the mouse hovers over a group member
    fn group_hover(
        mut self,
        group_name: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().group_hover_style = Some(GroupStyle {
            group: group_name.into(),
            style: Box::new(f(StyleRefinement::default())),
        });
        self
    }

    /// Bind the given callback to the mouse down event for the given mouse button,
    /// the fluent API equivalent to [`Interactivity::on_mouse_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to the view state from this callback.
    fn on_mouse_down(
        mut self,
        button: MouseButton,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_down(button, listener);
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    /// Set a key that can be used to look up this element's bounds
    /// in the `VisualTestContext::debug_bounds` map
    /// This is a noop in release builds
    fn debug_selector(mut self, f: impl FnOnce() -> String) -> Self {
        self.interactivity().debug_selector = Some(f());
        self
    }

    #[cfg(not(any(test, feature = "test-support")))]
    /// Set a key that can be used to look up this element's bounds
    /// in the `VisualTestContext::debug_bounds` map
    /// This is a noop in release builds
    #[inline]
    fn debug_selector(self, _: impl FnOnce() -> String) -> Self {
        self
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase
    /// the fluent API equivalent to [`Interactivity::capture_any_mouse_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_any_mouse_down(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_any_mouse_down(listener);
        self
    }

    /// Bind the given callback to the mouse down event for any button, during the capture phase
    /// the fluent API equivalent to [`Interactivity::on_any_mouse_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_any_mouse_down(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_any_mouse_down(listener);
        self
    }

    /// Bind the given callback to the mouse up event for the given button, during the bubble phase
    /// the fluent API equivalent to [`Interactivity::on_mouse_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_up(
        mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_up(button, listener);
        self
    }

    /// Bind the given callback to the mouse up event for any button, during the capture phase
    /// the fluent API equivalent to [`Interactivity::capture_any_mouse_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_any_mouse_up(
        mut self,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_any_mouse_up(listener);
        self
    }

    /// Bind the given callback to the mouse down event, on any button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The fluent API equivalent to [`Interactivity::on_mouse_down_out`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_down_out(
        mut self,
        listener: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_down_out(listener);
        self
    }

    /// Bind the given callback to the mouse up event, for the given button, during the capture phase,
    /// when the mouse is outside of the bounds of this element.
    /// The fluent API equivalent to [`Interactivity::on_mouse_up_out`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_up_out(
        mut self,
        button: MouseButton,
        listener: impl Fn(&MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_up_out(button, listener);
        self
    }

    /// Bind the given callback to the mouse move event, during the bubble phase
    /// The fluent API equivalent to [`Interactivity::on_mouse_move`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_mouse_move(
        mut self,
        listener: impl Fn(&MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_mouse_move(listener);
        self
    }

    /// Bind the given callback to the mouse drag event of the given type. Note that this
    /// will be called for all move events, inside or outside of this element, as long as the
    /// drag was started with this element under the mouse. Useful for implementing draggable
    /// UIs that don't conform to a drag and drop style interaction, like resizing.
    /// The fluent API equivalent to [`Interactivity::on_drag_move`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_drag_move<T: 'static>(
        mut self,
        listener: impl Fn(&DragMoveEvent<T>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_drag_move(listener);
        self
    }

    /// Bind the given callback to scroll wheel events during the bubble phase
    /// The fluent API equivalent to [`Interactivity::on_scroll_wheel`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_scroll_wheel(
        mut self,
        listener: impl Fn(&ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_scroll_wheel(listener);
        self
    }

    /// Capture the given action, before normal action dispatch can fire
    /// The fluent API equivalent to [`Interactivity::on_scroll_wheel`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_action<A: Action>(
        mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_action(listener);
        self
    }

    /// Bind the given callback to an action dispatch during the bubble phase
    /// The fluent API equivalent to [`Interactivity::on_action`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_action<A: Action>(
        mut self,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_action(listener);
        self
    }

    /// Bind the given callback to an action dispatch, based on a dynamic action parameter
    /// instead of a type parameter. Useful for component libraries that want to expose
    /// action bindings to their users.
    /// The fluent API equivalent to [`Interactivity::on_boxed_action`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_boxed_action(
        mut self,
        action: &dyn Action,
        listener: impl Fn(&dyn Action, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_boxed_action(action, listener);
        self
    }

    /// Bind the given callback to key down events during the bubble phase
    /// The fluent API equivalent to [`Interactivity::on_key_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_key_down(
        mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_key_down(listener);
        self
    }

    /// Bind the given callback to key down events during the capture phase
    /// The fluent API equivalent to [`Interactivity::capture_key_down`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_key_down(
        mut self,
        listener: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_key_down(listener);
        self
    }

    /// Bind the given callback to key up events during the bubble phase
    /// The fluent API equivalent to [`Interactivity::on_key_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_key_up(
        mut self,
        listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_key_up(listener);
        self
    }

    /// Bind the given callback to key up events during the capture phase
    /// The fluent API equivalent to [`Interactivity::capture_key_up`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn capture_key_up(
        mut self,
        listener: impl Fn(&KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().capture_key_up(listener);
        self
    }

    /// Bind the given callback to modifiers changing events.
    /// The fluent API equivalent to [`Interactivity::on_modifiers_changed`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_modifiers_changed(
        mut self,
        listener: impl Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_modifiers_changed(listener);
        self
    }

    /// Apply the given style when the given data type is dragged over this element
    fn drag_over<S: 'static>(
        mut self,
        f: impl 'static + Fn(StyleRefinement, &S, &mut Window, &mut App) -> StyleRefinement,
    ) -> Self {
        self.interactivity().drag_over_styles.push((
            TypeId::of::<S>(),
            Box::new(move |currently_dragged: &dyn Any, window, cx| {
                f(
                    StyleRefinement::default(),
                    currently_dragged.downcast_ref::<S>().unwrap(),
                    window,
                    cx,
                )
            }),
        ));
        self
    }

    /// Apply the given style when the given data type is dragged over this element's group
    fn group_drag_over<S: 'static>(
        mut self,
        group_name: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().group_drag_over_styles.push((
            TypeId::of::<S>(),
            GroupStyle {
                group: group_name.into(),
                style: Box::new(f(StyleRefinement::default())),
            },
        ));
        self
    }

    /// Bind the given callback to drop events of the given type, whether or not the drag started on this element
    /// The fluent API equivalent to [`Interactivity::on_drop`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_drop<T: 'static>(
        mut self,
        listener: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity().on_drop(listener);
        self
    }

    /// Use the given predicate to determine whether or not a drop event should be dispatched to this element
    /// The fluent API equivalent to [`Interactivity::can_drop`]
    fn can_drop(
        mut self,
        predicate: impl Fn(&dyn Any, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.interactivity().can_drop(predicate);
        self
    }

    /// Block the mouse from all interactions with elements behind this element's hitbox. Typically
    /// `block_mouse_except_scroll` should be preferred.
    /// The fluent API equivalent to [`Interactivity::occlude_mouse`]
    fn occlude(mut self) -> Self {
        self.interactivity().occlude_mouse();
        self
    }

    /// Set the bounds of this element as a window control area for the platform window.
    /// The fluent API equivalent to [`Interactivity::window_control_area`]
    fn window_control_area(mut self, area: WindowControlArea) -> Self {
        self.interactivity().window_control_area(area);
        self
    }

    /// Block non-scroll mouse interactions with elements behind this element's hitbox. See
    /// [`Hitbox::is_hovered`] for details.
    ///
    /// The fluent API equivalent to [`Interactivity::block_mouse_except_scroll`]
    fn block_mouse_except_scroll(mut self) -> Self {
        self.interactivity().block_mouse_except_scroll();
        self
    }

    /// Set the given styles to be applied when this element, specifically, is focused.
    /// Requires that the element is focusable. Elements can be made focusable using [`InteractiveElement::track_focus`].
    fn focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().focus_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the given styles to be applied when this element is inside another element that is focused.
    /// Requires that the element is focusable. Elements can be made focusable using [`InteractiveElement::track_focus`].
    fn in_focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().in_focus_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the given styles to be applied when this element is focused via keyboard navigation.
    /// Requires that the element is focusable. Elements can be made focusable using [`InteractiveElement::track_focus`].
    fn focus_visible(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().focus_visible_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the accessibility metadata for this element.
    fn accessibility(mut self, attributes: crate::AccessibilityAttributes) -> Self
    where
        Self: Sized,
    {
        self.interactivity().accessibility_attributes = Some(attributes);
        self
    }
}

/// A trait for elements that want to use the standard GPUI interactivity features
/// that require state.
pub trait StatefulInteractiveElement: InteractiveElement {
    /// Set this element to focusable.
    fn focusable(mut self) -> Self {
        self.interactivity().focusable = true;
        self
    }

    /// Set the overflow x and y to automatically scroll when content overflows.
    fn overflow_auto(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(Overflow::Auto);
        self.interactivity().base_style.overflow.y = Some(Overflow::Auto);
        self
    }

    /// Set the overflow x to automatically scroll when content overflows.
    fn overflow_x_auto(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(Overflow::Auto);
        self
    }

    /// Set the overflow y to automatically scroll when content overflows.
    fn overflow_y_auto(mut self) -> Self {
        self.interactivity().base_style.overflow.y = Some(Overflow::Auto);
        self
    }

    /// Set the overflow x and y to scroll.
    fn overflow_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(Overflow::Scroll);
        self.interactivity().base_style.overflow.y = Some(Overflow::Scroll);
        self
    }

    /// Set the overflow x to scroll.
    fn overflow_x_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(Overflow::Scroll);
        self
    }

    /// Set the overflow y to scroll.
    fn overflow_y_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.y = Some(Overflow::Scroll);
        self
    }

    /// Set the space to be reserved for rendering the scrollbar.
    ///
    /// This will only affect the layout of the element when overflow for this element is set to
    /// `Overflow::Scroll`.
    fn scrollbar_width(mut self, width: impl Into<AbsoluteLength>) -> Self {
        self.interactivity().base_style.scrollbar_width = Some(width.into());
        self
    }

    /// Track the scroll state of this element with the given handle.
    fn track_scroll(mut self, scroll_handle: &ScrollHandle) -> Self {
        self.interactivity().tracked_scroll_handle = Some(scroll_handle.clone());
        self
    }

    /// Track the scroll state of this element with the given handle.
    fn anchor_scroll(mut self, scroll_anchor: Option<ScrollAnchor>) -> Self {
        self.interactivity().scroll_anchor = scroll_anchor;
        self
    }

    /// Set the given styles to be applied when this element is active.
    fn active(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self
    where
        Self: Sized,
    {
        self.interactivity().active_style = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    /// Set the given styles to be applied when this element's group is active.
    fn group_active(
        mut self,
        group_name: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().group_active_style = Some(GroupStyle {
            group: group_name.into(),
            style: Box::new(f(StyleRefinement::default())),
        });
        self
    }

    /// Bind the given callback to click events of this element
    /// The fluent API equivalent to [`Interactivity::on_click`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_click(mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_click(listener);
        self
    }

    /// On drag initiation, this callback will be used to create a new view to render the dragged value for a
    /// drag and drop operation. This API should also be used as the equivalent of 'on drag start' with
    /// the [`InteractiveElement::on_drag_move`] API.
    /// The callback also has access to the offset of triggering click from the origin of parent element.
    /// The fluent API equivalent to [`Interactivity::on_drag`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_drag<T, W>(
        mut self,
        value: T,
        constructor: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
    ) -> Self
    where
        Self: Sized,
        T: 'static,
        W: 'static + Render,
    {
        self.interactivity().on_drag(value, constructor);
        self
    }

    /// Bind the given callback on the hover start and end events of this element. Note that the boolean
    /// passed to the callback is true when the hover starts and false when it ends.
    /// The fluent API equivalent to [`Interactivity::on_hover`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_hover(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_hover(listener);
        self
    }

    /// Bind the given callback to pan gesture updates for this element.
    fn on_pan(
        mut self,
        listener: impl Fn(&PanGestureEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_pan(listener);
        self
    }

    /// Bind the given callback to swipe gestures in the requested direction.
    fn on_swipe(
        mut self,
        direction: SwipeDirection,
        listener: impl Fn(&SwipeGestureEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_swipe(direction, listener);
        self
    }

    /// Bind the given callback to pinch gestures for this element.
    fn on_pinch(
        mut self,
        listener: impl Fn(&PinchGestureEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_pinch(listener);
        self
    }

    /// Bind the given callback to tap gestures for this element.
    fn on_tap(
        mut self,
        listener: impl Fn(&TapGestureEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_tap(listener);
        self
    }

    /// Bind the given callback to long-press gestures for this element.
    fn on_long_press(
        mut self,
        listener: impl Fn(&LongPressGestureEvent, &mut Window, &mut App) + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_long_press(listener);
        self
    }

    /// Use the given content to construct a tooltip when the mouse hovers over this element.
    /// String-like values render the default tooltip bubble, while closures can build a custom tooltip view.
    /// The fluent API equivalent to [`Interactivity::tooltip`]
    fn tooltip(mut self, tooltip: impl TooltipContent) -> Self
    where
        Self: Sized,
    {
        self.interactivity().tooltip(tooltip);
        self
    }

    /// Use the given callback to construct a rich tooltip element when the mouse hovers over this element.
    fn tooltip_element<E>(mut self, build_tooltip: impl Fn() -> E + 'static) -> Self
    where
        Self: Sized,
        E: IntoElement,
    {
        self.interactivity().tooltip_element(build_tooltip);
        self
    }

    /// Use the given callback to construct a custom tooltip view when the mouse hovers over this element.
    fn tooltip_view(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().tooltip_view(build_tooltip);
        self
    }

    /// Use the given callback to construct a new tooltip view when the mouse hovers over this element.
    /// The tooltip itself is also hoverable and won't disappear when the user moves the mouse into
    /// the tooltip. The fluent API equivalent to [`Interactivity::hoverable_tooltip`]
    fn hoverable_tooltip(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().hoverable_tooltip(build_tooltip);
        self
    }

    /// Use the given callback to construct an in-window context menu when the user secondary-clicks this element.
    fn context_menu(
        mut self,
        build_context_menu: impl Fn(ContextMenu) -> ContextMenu + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().context_menu(build_context_menu);
        self
    }

    /// Use the given callback to construct a custom in-window context menu view when the user secondary-clicks this element.
    fn context_menu_view(
        mut self,
        build_context_menu: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self
    where
        Self: Sized,
    {
        self.interactivity().context_menu_view(build_context_menu);
        self
    }

    /// Bind the given callback to be called when this element's bounds change size.
    /// The fluent API equivalent to [`Interactivity::on_resize`]
    ///
    /// See [`Context::listener`](crate::Context::listener) to get access to a view's state from this callback.
    fn on_resize(mut self, listener: impl Fn(&ResizeEvent, &mut Window, &mut App) + 'static) -> Self
    where
        Self: Sized,
    {
        self.interactivity().on_resize(listener);
        self
    }
}

pub(crate) type MouseDownListener =
    Box<dyn Fn(&MouseDownEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;
pub(crate) type MouseUpListener =
    Box<dyn Fn(&MouseUpEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type MouseMoveListener =
    Box<dyn Fn(&MouseMoveEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type ScrollWheelListener =
    Box<dyn Fn(&ScrollWheelEvent, DispatchPhase, &Hitbox, &mut Window, &mut App) + 'static>;

pub(crate) type PanListener = Box<dyn Fn(&PanGestureEvent, &mut Window, &mut App) + 'static>;

pub(crate) type SwipeListener = Box<dyn Fn(&SwipeGestureEvent, &mut Window, &mut App) + 'static>;

pub(crate) type PinchListener = Box<dyn Fn(&PinchGestureEvent, &mut Window, &mut App) + 'static>;

pub(crate) type TapListener = Box<dyn Fn(&TapGestureEvent, &mut Window, &mut App) + 'static>;

pub(crate) type LongPressListener =
    Box<dyn Fn(&LongPressGestureEvent, &mut Window, &mut App) + 'static>;

pub(crate) type ClickListener = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(crate) type DragListener =
    Box<dyn Fn(&dyn Any, Point<Pixels>, &mut Window, &mut App) -> AnyView + 'static>;

type DropListener = Box<dyn Fn(&dyn Any, &mut Window, &mut App) + 'static>;

type CanDropPredicate = Box<dyn Fn(&dyn Any, &mut Window, &mut App) -> bool + 'static>;

pub(crate) struct TooltipBuilder {
    build: Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>,
    hoverable: bool,
}

/// Accepted content sources for tooltips on interactive elements.
///
/// This trait is implemented for string-like values such as `&'static str`, `String`, and
/// [`SharedString`], as well as closures that build a custom tooltip view.
pub trait TooltipContent {
    /// Convert this content into a renderer for tooltip views.
    fn into_tooltip_renderer(self) -> Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static>;
}

impl TooltipContent for SharedString {
    fn into_tooltip_renderer(self) -> Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static> {
        let text = self;
        Rc::new(move |_, cx| cx.new(|_| TooltipTextView { text: text.clone() }).into())
    }
}

impl TooltipContent for String {
    fn into_tooltip_renderer(self) -> Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static> {
        SharedString::from(self).into_tooltip_renderer()
    }
}

impl TooltipContent for &'static str {
    fn into_tooltip_renderer(self) -> Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static> {
        SharedString::new_static(self).into_tooltip_renderer()
    }
}

impl<F> TooltipContent for F
where
    F: Fn(&mut Window, &mut App) -> AnyView + 'static,
{
    fn into_tooltip_renderer(self) -> Rc<dyn Fn(&mut Window, &mut App) -> AnyView + 'static> {
        Rc::new(self)
    }
}

/// A builder for structured in-window context menus.
#[derive(Clone, Default)]
pub struct ContextMenu {
    items: Vec<ContextMenuItem>,
}

impl ContextMenu {
    /// Create an empty context menu.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an action item to the menu.
    pub fn item<A: Action + 'static>(mut self, label: impl Into<SharedString>, action: A) -> Self {
        self.items
            .push(ContextMenuItem::Action(ContextMenuActionItem {
                label: label.into(),
                action: Box::new(action),
            }));
        self
    }

    /// Append a separator to the menu.
    pub fn separator(mut self) -> Self {
        self.items.push(ContextMenuItem::Separator);
        self
    }

    /// Append a submenu built from the given closure.
    pub fn submenu(
        mut self,
        label: impl Into<SharedString>,
        build_submenu: impl FnOnce(ContextMenu) -> ContextMenu,
    ) -> Self {
        self.items
            .push(ContextMenuItem::Submenu(ContextMenuSubmenuItem {
                label: label.into(),
                menu: build_submenu(ContextMenu::new()),
            }));
        self
    }

    fn into_any_view(
        self,
        active_context_menu: Rc<RefCell<Option<AnyTooltip>>>,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|_| StructuredContextMenu {
            menu: self,
            active_context_menu,
            open_submenu_path: Vec::new(),
        })
        .into()
    }
}

#[derive(Clone)]
enum ContextMenuItem {
    Action(ContextMenuActionItem),
    Separator,
    Submenu(ContextMenuSubmenuItem),
}

struct ContextMenuActionItem {
    label: SharedString,
    action: Box<dyn Action>,
}

impl Clone for ContextMenuActionItem {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            action: self.action.boxed_clone(),
        }
    }
}

#[derive(Clone)]
struct ContextMenuSubmenuItem {
    label: SharedString,
    menu: ContextMenu,
}

pub(crate) struct ContextMenuBuilder {
    build: Rc<dyn Fn(&Rc<RefCell<Option<AnyTooltip>>>, &mut Window, &mut App) -> AnyView + 'static>,
}

pub(crate) type KeyDownListener =
    Box<dyn Fn(&KeyDownEvent, DispatchPhase, &mut Window, &mut App) + 'static>;

pub(crate) type KeyUpListener =
    Box<dyn Fn(&KeyUpEvent, DispatchPhase, &mut Window, &mut App) + 'static>;

pub(crate) type ModifiersChangedListener =
    Box<dyn Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static>;

pub(crate) type ActionListener =
    Box<dyn Fn(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static>;

pub(crate) type ResizeListener = Box<dyn Fn(&ResizeEvent, &mut Window, &mut App) + 'static>;

/// Construct a new [`Div`] element
#[track_caller]
pub fn div() -> Div {
    Div {
        interactivity: Interactivity::new(),
        children: SmallVec::default(),
        prepaint_listener: None,
        image_cache: None,
    }
}

/// A [`Div`] element, the all-in-one element for building complex UIs in GPUI
pub struct Div {
    interactivity: Interactivity,
    children: SmallVec<[StackSafe<AnyElement>; 2]>,
    prepaint_listener: Option<Box<dyn Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static>>,
    image_cache: Option<Box<dyn ImageCacheProvider>>,
}

impl Div {
    /// Add a listener to be called when the children of this `Div` are prepainted.
    /// This allows you to store the [`Bounds`] of the children for later use.
    pub fn on_children_prepainted(
        mut self,
        listener: impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.prepaint_listener = Some(Box::new(listener));
        self
    }

    /// Add an image cache at the location of this div in the element tree.
    pub fn image_cache(mut self, cache: impl ImageCacheProvider) -> Self {
        self.image_cache = Some(Box::new(cache));
        self
    }
}

/// A frame state for a `Div` element, which contains layout IDs for its children.
///
/// This struct is used internally by the `Div` element to manage the layout state of its children
/// during the UI update cycle. It holds a small vector of `LayoutId` values, each corresponding to
/// a child element of the `Div`. These IDs are used to query the layout engine for the computed
/// bounds of the children after the layout phase is complete.
pub struct DivFrameState {
    child_layout_ids: SmallVec<[LayoutId; 2]>,
}

/// Prepaint state for a div.
pub struct DivPrepaintState {
    hitbox: Option<Hitbox>,
    auto_scrollbars: AutoScrollbarHitboxes,
}

#[derive(Default)]
struct AutoScrollbarHitboxes {
    vertical: Option<Hitbox>,
    horizontal: Option<Hitbox>,
}

/// Interactivity state displayed an manipulated in the inspector.
#[derive(Clone)]
pub struct DivInspectorState {
    /// The inspected element's base style. This is used for both inspecting and modifying the
    /// state. In the future it will make sense to separate the read and write, possibly tracking
    /// the modifications.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub base_style: Box<StyleRefinement>,
    /// Inspects the bounds of the element.
    pub bounds: Bounds<Pixels>,
    /// Size of the children of the element, or `bounds.size` if it has no children.
    pub content_size: Size<Pixels>,
}

impl Styled for Div {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl InteractiveElement for Div {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl ParentElement for Div {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children
            .extend(elements.into_iter().map(StackSafe::new))
    }
}

impl Element for Div {
    type RequestLayoutState = DivFrameState;
    type PrepaintState = DivPrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.interactivity.element_id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.interactivity.source_location()
    }

    #[stacksafe]
    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child_layout_ids = SmallVec::new();
        let image_cache = self
            .image_cache
            .as_mut()
            .map(|provider| provider.provide(window, cx));

        let layout_id = window.with_image_cache(image_cache, |window| {
            self.interactivity.request_layout(
                global_id,
                inspector_id,
                window,
                cx,
                |style, window, cx| {
                    window.with_text_style(style.text_style().cloned(), |window| {
                        child_layout_ids = self
                            .children
                            .iter_mut()
                            .map(|child| child.request_layout(window, cx))
                            .collect::<SmallVec<_>>();
                        window.request_layout(style, child_layout_ids.iter().copied(), cx)
                    })
                },
            )
        });

        (layout_id, DivFrameState { child_layout_ids })
    }

    #[stacksafe]
    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> DivPrepaintState {
        let has_prepaint_listener = self.prepaint_listener.is_some();
        let mut children_bounds = Vec::with_capacity(if has_prepaint_listener {
            request_layout.child_layout_ids.len()
        } else {
            0
        });

        let mut child_min = point(Pixels::MAX, Pixels::MAX);
        let mut child_max = Point::default();
        if let Some(handle) = self.interactivity.scroll_anchor.as_ref() {
            *handle.last_origin.borrow_mut() = bounds.origin - window.element_offset();
        }
        let content_size = if request_layout.child_layout_ids.is_empty() {
            bounds.size
        } else if let Some(scroll_handle) = self.interactivity.tracked_scroll_handle.as_ref() {
            let mut state = scroll_handle.0.borrow_mut();
            state.child_bounds = Vec::with_capacity(request_layout.child_layout_ids.len());
            for child_layout_id in &request_layout.child_layout_ids {
                let child_bounds = window.layout_bounds(*child_layout_id);
                child_min = child_min.min(&child_bounds.origin);
                child_max = child_max.max(&child_bounds.bottom_right());
                state.child_bounds.push(child_bounds);
            }
            (child_max - child_min).into()
        } else {
            for child_layout_id in &request_layout.child_layout_ids {
                let child_bounds = window.layout_bounds(*child_layout_id);
                child_min = child_min.min(&child_bounds.origin);
                child_max = child_max.max(&child_bounds.bottom_right());

                if has_prepaint_listener {
                    children_bounds.push(child_bounds);
                }
            }
            (child_max - child_min).into()
        };

        if let Some(scroll_handle) = self.interactivity.tracked_scroll_handle.as_ref() {
            scroll_handle.scroll_to_active_item();
        }

        let tracked_scroll_handle = self.interactivity.tracked_scroll_handle.clone();
        self.interactivity.prepaint(
            global_id,
            inspector_id,
            bounds,
            content_size,
            window,
            cx,
            |style, scroll_offset, hitbox, window, cx| {
                window.with_element_offset(scroll_offset, |window| {
                    for child in &mut self.children {
                        child.prepaint(window, cx);
                    }
                });

                if let Some(listener) = self.prepaint_listener.as_ref() {
                    listener(children_bounds, window, cx);
                }

                let auto_scrollbars = prepaint_auto_scrollbar_hitboxes(
                    tracked_scroll_handle.as_ref(),
                    bounds,
                    style,
                    window,
                );

                DivPrepaintState {
                    hitbox,
                    auto_scrollbars,
                }
            },
        )
    }

    #[stacksafe]
    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut DivPrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let image_cache = self
            .image_cache
            .as_mut()
            .map(|provider| provider.provide(window, cx));

        window.with_image_cache(image_cache, |window| {
            let accessibility_node =
                self.interactivity
                    .accessibility_attributes
                    .as_ref()
                    .map(|attrs| {
                        let mut node = attrs.to_node(crate::AccessibilityId::new());
                        if self
                            .interactivity
                            .tracked_focus_handle
                            .as_ref()
                            .is_some_and(|h| h.is_focused(window))
                        {
                            node.states |= crate::AccessibilityState::FOCUSED;
                        }
                        if self.interactivity.active.unwrap_or(false) {
                            node.states |= crate::AccessibilityState::PRESSED;
                        }
                        node
                    });

            self.interactivity.paint_with_auto_scrollbars(
                global_id,
                inspector_id,
                bounds,
                prepaint.hitbox.as_ref(),
                &prepaint.auto_scrollbars,
                window,
                cx,
                |_style, window, cx| {
                    if let Some(node) = accessibility_node.as_ref() {
                        window.register_accessibility_node(node.clone());
                        if node.role.is_container() {
                            window.with_accessibility_parent(node.id, |window| {
                                for child in &mut self.children {
                                    child.paint(window, cx);
                                }
                            });
                        } else {
                            for child in &mut self.children {
                                child.paint(window, cx);
                            }
                        }
                    } else {
                        for child in &mut self.children {
                            child.paint(window, cx);
                        }
                    }
                },
            )
        });
    }
}

impl IntoElement for Div {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

/// The interactivity struct. Powers all of the general-purpose
/// interactivity in the `Div` element.
#[derive(Default)]
pub struct Interactivity {
    /// The element ID of the element. In id is required to support a stateful subset of the interactivity such as on_click.
    pub element_id: Option<ElementId>,
    /// Whether the element was clicked. This will only be present after layout.
    pub active: Option<bool>,
    /// Whether the element was hovered. This will only be present after paint if an hitbox
    /// was created for the interactive element.
    pub hovered: Option<bool>,
    pub(crate) tooltip_id: Option<TooltipId>,
    pub(crate) context_menu_id: Option<TooltipId>,
    pub(crate) content_size: Size<Pixels>,
    pub(crate) key_context: Option<KeyContext>,
    pub(crate) focusable: bool,
    pub(crate) tracked_focus_handle: Option<FocusHandle>,
    pub(crate) tracked_scroll_handle: Option<ScrollHandle>,
    pub(crate) scroll_anchor: Option<ScrollAnchor>,
    pub(crate) scroll_offset: Option<Rc<RefCell<Point<Pixels>>>>,
    pub(crate) scroll_elastic_state: Option<Rc<RefCell<ScrollElasticState>>>,
    pub(crate) group: Option<SharedString>,
    pub(crate) implicit_transition: Option<TransitionConfig>,
    pub(crate) layout_animation: Option<TransitionConfig>,
    /// The base style of the element, before any modifications are applied
    /// by focus, active, etc.
    pub base_style: Box<StyleRefinement>,
    pub(crate) focus_style: Option<Box<StyleRefinement>>,
    pub(crate) in_focus_style: Option<Box<StyleRefinement>>,
    pub(crate) focus_visible_style: Option<Box<StyleRefinement>>,
    pub(crate) hover_style: Option<Box<StyleRefinement>>,
    pub(crate) group_hover_style: Option<GroupStyle>,
    pub(crate) active_style: Option<Box<StyleRefinement>>,
    pub(crate) group_active_style: Option<GroupStyle>,
    pub(crate) drag_over_styles: Vec<(
        TypeId,
        Box<dyn Fn(&dyn Any, &mut Window, &mut App) -> StyleRefinement>,
    )>,
    pub(crate) group_drag_over_styles: Vec<(TypeId, GroupStyle)>,
    pub(crate) mouse_down_listeners: Vec<MouseDownListener>,
    pub(crate) mouse_up_listeners: Vec<MouseUpListener>,
    pub(crate) mouse_move_listeners: Vec<MouseMoveListener>,
    pub(crate) scroll_wheel_listeners: Vec<ScrollWheelListener>,
    pub(crate) pan_listeners: Vec<PanListener>,
    pub(crate) swipe_listeners: Vec<(SwipeDirection, SwipeListener)>,
    pub(crate) pinch_listeners: Vec<PinchListener>,
    pub(crate) tap_listeners: Vec<TapListener>,
    pub(crate) long_press_listeners: Vec<LongPressListener>,
    pub(crate) key_down_listeners: Vec<KeyDownListener>,
    pub(crate) key_up_listeners: Vec<KeyUpListener>,
    pub(crate) modifiers_changed_listeners: Vec<ModifiersChangedListener>,
    pub(crate) action_listeners: Vec<(TypeId, ActionListener)>,
    pub(crate) drop_listeners: Vec<(TypeId, DropListener)>,
    pub(crate) can_drop_predicate: Option<CanDropPredicate>,
    pub(crate) click_listeners: Vec<ClickListener>,
    pub(crate) drag_listener: Option<(Arc<dyn Any>, DragListener)>,
    pub(crate) hover_listener: Option<Box<dyn Fn(&bool, &mut Window, &mut App)>>,
    pub(crate) resize_listeners: Vec<ResizeListener>,
    pub(crate) tooltip_builder: Option<TooltipBuilder>,
    pub(crate) context_menu_builder: Option<ContextMenuBuilder>,
    pub(crate) window_control: Option<WindowControlArea>,
    pub(crate) hitbox_behavior: HitboxBehavior,
    pub(crate) tab_index: Option<isize>,
    pub(crate) tab_group: bool,
    pub(crate) tab_stop: bool,
    pub(crate) accessibility_attributes: Option<crate::AccessibilityAttributes>,

    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) source_location: Option<&'static core::panic::Location<'static>>,

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) debug_selector: Option<String>,
}

impl Interactivity {
    /// Layout this element according to this interactivity state's configured styles
    pub fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(Style, &mut Window, &mut App) -> LayoutId,
    ) -> LayoutId {
        #[cfg(any(feature = "inspector", debug_assertions))]
        window.with_inspector_state(
            _inspector_id,
            cx,
            |inspector_state: &mut Option<DivInspectorState>, _window| {
                if let Some(inspector_state) = inspector_state {
                    self.base_style = inspector_state.base_style.clone();
                } else {
                    *inspector_state = Some(DivInspectorState {
                        base_style: self.base_style.clone(),
                        bounds: Default::default(),
                        content_size: Default::default(),
                    })
                }
            },
        );

        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());

                if let Some(element_state) = element_state.as_ref()
                    && cx.has_active_drag()
                {
                    if let Some(pending_mouse_down) = element_state.pending_mouse_down.as_ref() {
                        *pending_mouse_down.borrow_mut() = None;
                    }
                    if let Some(clicked_state) = element_state.clicked_state.as_ref() {
                        *clicked_state.borrow_mut() = ElementClickedState::default();
                    }
                }

                // Ensure we store a focus handle in our element state if we're focusable.
                // If there's an explicit focus handle we're tracking, use that. Otherwise
                // create a new handle and store it in the element state, which lives for as
                // as frames contain an element with this id.
                if self.focusable
                    && self.tracked_focus_handle.is_none()
                    && let Some(element_state) = element_state.as_mut()
                {
                    let mut handle = element_state
                        .focus_handle
                        .get_or_insert_with(|| cx.focus_handle())
                        .clone()
                        .tab_stop(self.tab_stop);

                    if let Some(index) = self.tab_index {
                        handle = handle.tab_index(index);
                    }

                    self.tracked_focus_handle = Some(handle);
                }

                if let Some(scroll_handle) = self.tracked_scroll_handle.as_ref() {
                    let scroll_handle = scroll_handle.0.borrow();
                    self.scroll_offset = Some(scroll_handle.offset.clone());
                    self.scroll_elastic_state = Some(scroll_handle.elastic.clone());
                } else if (self.base_style.overflow.x.is_some_and(uses_scroll_state)
                    || self.base_style.overflow.y.is_some_and(uses_scroll_state))
                    && let Some(element_state) = element_state.as_mut()
                {
                    self.scroll_offset = Some(
                        element_state
                            .scroll_offset
                            .get_or_insert_with(Rc::default)
                            .clone(),
                    );
                    self.scroll_elastic_state = Some(
                        element_state
                            .scroll_elastic_state
                            .get_or_insert_with(Rc::default)
                            .clone(),
                    );
                } else {
                    self.scroll_elastic_state = None;
                }

                let style = self.compute_style_internal(None, element_state.as_mut(), window, cx);
                let layout_id = f(style, window, cx);
                (layout_id, element_state)
            },
        )
    }

    /// Commit the bounds of this element according to this interactivity state's configured styles.
    pub fn prepaint<R>(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        content_size: Size<Pixels>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&Style, Point<Pixels>, Option<Hitbox>, &mut Window, &mut App) -> R,
    ) -> R {
        self.content_size = content_size;

        #[cfg(any(feature = "inspector", debug_assertions))]
        window.with_inspector_state(
            _inspector_id,
            cx,
            |inspector_state: &mut Option<DivInspectorState>, _window| {
                if let Some(inspector_state) = inspector_state {
                    inspector_state.bounds = bounds;
                    inspector_state.content_size = content_size;
                }
            },
        );

        if let Some(focus_handle) = self.tracked_focus_handle.as_ref() {
            window.set_focus_handle(focus_handle, cx);
        }
        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());
                let style = self.compute_style_internal(None, element_state.as_mut(), window, cx);

                if let Some(element_state) = element_state.as_mut() {
                    if let Some(clicked_state) = element_state.clicked_state.as_ref() {
                        let clicked_state = clicked_state.borrow();
                        self.active = Some(clicked_state.element);
                    }
                    if let Some(active_tooltip) = element_state.active_tooltip.as_ref() {
                        if self.tooltip_builder.is_some() {
                            self.tooltip_id = set_tooltip_on_window(active_tooltip, window);
                        } else {
                            // If there is no longer a tooltip builder, remove the active tooltip.
                            element_state.active_tooltip.take();
                        }
                    }
                    if let Some(active_context_menu) = element_state.active_context_menu.as_ref() {
                        if self.context_menu_builder.is_some() {
                            self.context_menu_id =
                                set_context_menu_on_window(active_context_menu, window);
                        } else {
                            element_state.active_context_menu.take();
                        }
                    }
                }

                window.with_text_style(style.text_style().cloned(), |window| {
                    window.with_content_mask(
                        style.overflow_mask(bounds, window.rem_size()),
                        |window| {
                            let hitbox = if self.should_insert_hitbox(&style, window, cx) {
                                Some(window.insert_hitbox(bounds, self.hitbox_behavior))
                            } else {
                                None
                            };

                            let scroll_offset =
                                self.clamp_scroll_position(bounds, &style, window, cx);
                            let result = f(&style, scroll_offset, hitbox, window, cx);
                            (result, element_state)
                        },
                    )
                })
            },
        )
    }

    fn should_insert_hitbox(&self, style: &Style, window: &Window, cx: &App) -> bool {
        self.hitbox_behavior != HitboxBehavior::Normal
            || self.window_control.is_some()
            || style.mouse_cursor.is_some()
            || self.group.is_some()
            || self.scroll_offset.is_some()
            || self.tracked_focus_handle.is_some()
            || self.hover_style.is_some()
            || self.group_hover_style.is_some()
            || self.hover_listener.is_some()
            || !self.mouse_up_listeners.is_empty()
            || !self.mouse_down_listeners.is_empty()
            || !self.mouse_move_listeners.is_empty()
            || !self.click_listeners.is_empty()
            || !self.scroll_wheel_listeners.is_empty()
            || !self.pan_listeners.is_empty()
            || !self.swipe_listeners.is_empty()
            || !self.pinch_listeners.is_empty()
            || !self.tap_listeners.is_empty()
            || !self.long_press_listeners.is_empty()
            || self.drag_listener.is_some()
            || !self.drop_listeners.is_empty()
            || self.tooltip_builder.is_some()
            || self.context_menu_builder.is_some()
            || window.is_inspector_picking(cx)
    }

    fn clamp_scroll_position(
        &self,
        bounds: Bounds<Pixels>,
        style: &Style,
        window: &mut Window,
        _: &mut App,
    ) -> Point<Pixels> {
        fn round_to_two_decimals(pixels: Pixels) -> Pixels {
            const ROUNDING_FACTOR: f32 = 100.0;
            (pixels * ROUNDING_FACTOR).round() / ROUNDING_FACTOR
        }

        if let Some(scroll_offset) = self.scroll_offset.as_ref() {
            let mut scroll_to_bottom = false;
            let mut tracked_scroll_handle = self
                .tracked_scroll_handle
                .as_ref()
                .map(|handle| handle.0.borrow_mut());
            if let Some(mut scroll_handle_state) = tracked_scroll_handle.as_deref_mut() {
                scroll_handle_state.overflow = style.overflow;
                scroll_to_bottom = mem::take(&mut scroll_handle_state.scroll_to_bottom);
            }

            let rem_size = window.rem_size();
            let padding = style.padding.to_pixels(bounds.size.into(), rem_size);
            let padding_size = size(padding.left + padding.right, padding.top + padding.bottom);
            // The floating point values produced by Taffy and ours often vary
            // slightly after ~5 decimal places. This can lead to cases where after
            // subtracting these, the container becomes scrollable for less than
            // 0.00000x pixels. As we generally don't benefit from a precision that
            // high for the maximum scroll, we round the scroll max to 2 decimal
            // places here.
            let padded_content_size = self.content_size + padding_size;
            let scroll_max = (padded_content_size - bounds.size)
                .map(round_to_two_decimals)
                .max(&Default::default());
            // Clamp scroll offset in case scroll max is smaller now (e.g., if children
            // were removed or the bounds became larger).
            let mut scroll_offset = scroll_offset.borrow_mut();

            scroll_offset.x = scroll_offset.x.clamp(-scroll_max.width, px(0.));
            if scroll_to_bottom {
                scroll_offset.y = -scroll_max.height;
            } else {
                scroll_offset.y = scroll_offset.y.clamp(-scroll_max.height, px(0.));
            }

            let mut display_offset = *scroll_offset;
            if let Some(scroll_elastic_state) = self.scroll_elastic_state.as_ref() {
                let mut scroll_elastic_state = scroll_elastic_state.borrow_mut();
                scroll_elastic_state.max_offset = scroll_max;

                if !axis_is_scrollable(style.overflow.x, scroll_max.width) {
                    scroll_elastic_state.overscroll.x = Pixels::ZERO;
                }
                if !axis_is_scrollable(style.overflow.y, scroll_max.height) {
                    scroll_elastic_state.overscroll.y = Pixels::ZERO;
                }
                if scroll_to_bottom {
                    scroll_elastic_state.overscroll.y = Pixels::ZERO;
                }

                if window.power_mode() == crate::PowerMode::LowPower {
                    scroll_elastic_state.reset();
                }

                display_offset += scroll_elastic_state.overscroll;

                if scroll_elastic_state.animating {
                    let mut last_advance = scroll_elastic_state.last_advance;
                    let x_animating = advance_scroll_elasticity(
                        &mut scroll_elastic_state.overscroll.x,
                        &mut last_advance,
                    );
                    let y_animating = advance_scroll_elasticity(
                        &mut scroll_elastic_state.overscroll.y,
                        &mut last_advance,
                    );
                    scroll_elastic_state.last_advance = last_advance;
                    scroll_elastic_state.animating = x_animating || y_animating;
                    if scroll_elastic_state.animating {
                        window.request_animation_frame();
                    }
                }
            }

            if let Some(mut scroll_handle_state) = tracked_scroll_handle {
                scroll_handle_state.max_offset = scroll_max;
                scroll_handle_state.bounds = bounds;
            }

            display_offset
        } else {
            Point::default()
        }
    }

    /// Paint this element according to this interactivity state's configured styles
    /// and bind the element's mouse and keyboard events.
    ///
    /// content_size is the size of the content of the element, which may be larger than the
    /// element's bounds if the element is scrollable.
    ///
    /// the final computed style will be passed to the provided function, along
    /// with the current scroll offset
    pub fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        hitbox: Option<&Hitbox>,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&Style, &mut Window, &mut App),
    ) {
        self.paint_internal(
            global_id,
            _inspector_id,
            bounds,
            hitbox,
            &AutoScrollbarHitboxes::default(),
            window,
            cx,
            f,
        );
    }

    fn paint_with_auto_scrollbars(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        hitbox: Option<&Hitbox>,
        auto_scrollbars: &AutoScrollbarHitboxes,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&Style, &mut Window, &mut App),
    ) {
        self.paint_internal(
            global_id,
            inspector_id,
            bounds,
            hitbox,
            auto_scrollbars,
            window,
            cx,
            f,
        );
    }

    fn paint_internal(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        hitbox: Option<&Hitbox>,
        auto_scrollbars: &AutoScrollbarHitboxes,
        window: &mut Window,
        cx: &mut App,
        f: impl FnOnce(&Style, &mut Window, &mut App),
    ) {
        self.hovered = hitbox.map(|hitbox| hitbox.is_hovered(window));
        window.with_optional_element_state::<InteractiveElementState, _>(
            global_id,
            |element_state, window| {
                let mut element_state =
                    element_state.map(|element_state| element_state.unwrap_or_default());

                let style = self.compute_style_internal(hitbox, element_state.as_mut(), window, cx);
                let style = self.animate_style(global_id, style, window);

                #[cfg(any(feature = "test-support", test))]
                if let Some(debug_selector) = &self.debug_selector {
                    window
                        .next_frame
                        .debug_bounds
                        .insert(debug_selector.clone(), bounds);
                }

                self.paint_hover_group_handler(window, cx);

                if !self.resize_listeners.is_empty() {
                    if let Some(element_state) = element_state.as_mut() {
                        let size_changed = element_state
                            .prev_bounds
                            .map(|prev| prev.size != bounds.size)
                            .unwrap_or(true);

                        if size_changed {
                            element_state.prev_bounds = Some(bounds);
                            let resize_event = ResizeEvent {
                                size: bounds.size,
                                bounds,
                            };
                            for listener in &self.resize_listeners {
                                listener(&resize_event, window, cx);
                            }
                        }
                    }
                }

                if style.visibility == Visibility::Hidden {
                    return ((), element_state);
                }

                let mut tab_group = None;
                if self.tab_group {
                    tab_group = self.tab_index;
                }
                if let Some(focus_handle) = &self.tracked_focus_handle {
                    window.next_frame.tab_stops.insert(focus_handle);
                }

                let flip_transform =
                    self.resolve_flip_transform(element_state.as_mut(), bounds, window);
                window.with_element_transform(flip_transform, |window| {
                    window.with_element_opacity(style.opacity, |window| {
                        style.paint(bounds, window, cx, |window: &mut Window, cx: &mut App| {
                            window.with_text_style(style.text_style().cloned(), |window| {
                                window.with_content_mask(
                                    style.overflow_mask(bounds, window.rem_size()),
                                    |window| {
                                        window.with_rounded_clip(
                                            style.rounded_overflow_clip(bounds, window.rem_size()),
                                            |window| {
                                                window.with_tab_group(tab_group, |window| {
                                                    if let Some(hitbox) = hitbox {
                                                        #[cfg(debug_assertions)]
                                                        self.paint_debug_info(
                                                            global_id, hitbox, &style, window, cx,
                                                        );

                                                        if let Some(drag) = cx.active_drag.as_ref()
                                                        {
                                                            if let Some(mouse_cursor) =
                                                                drag.cursor_style
                                                            {
                                                                window.set_window_cursor_style(
                                                                    mouse_cursor,
                                                                );
                                                            }
                                                        } else {
                                                            if let Some(mouse_cursor) =
                                                                style.mouse_cursor
                                                            {
                                                                window.set_cursor_style(
                                                                    mouse_cursor,
                                                                    hitbox,
                                                                );
                                                            }
                                                        }

                                                        if let Some(group) = self.group.clone() {
                                                            GroupHitboxes::push(
                                                                group, hitbox.id, cx,
                                                            );
                                                        }

                                                        if let Some(area) = self.window_control {
                                                            window.insert_window_control_hitbox(
                                                                area,
                                                                hitbox.clone(),
                                                            );
                                                        }

                                                        self.paint_mouse_listeners(
                                                            hitbox,
                                                            element_state.as_mut(),
                                                            window,
                                                            cx,
                                                        );
                                                        self.paint_scroll_listener(
                                                            hitbox, &style, window, cx,
                                                        );
                                                        self.paint_gesture_listeners(
                                                            hitbox,
                                                            element_state.as_mut(),
                                                            window,
                                                            cx,
                                                        );
                                                    }

                                                    self.paint_keyboard_listeners(window, cx);
                                                    f(&style, window, cx);

                                                    if let Some(_hitbox) = hitbox {
                                                        #[cfg(any(
                                                            feature = "inspector",
                                                            debug_assertions
                                                        ))]
                                                        window.insert_inspector_hitbox(
                                                            _hitbox.id,
                                                            _inspector_id,
                                                            cx,
                                                        );

                                                        if let Some(group) = self.group.as_ref() {
                                                            GroupHitboxes::pop(group, cx);
                                                        }
                                                    }
                                                })
                                            },
                                        )
                                    },
                                );
                                if let Some(element_state) = element_state.as_mut() {
                                    self.paint_auto_scrollbars(
                                        bounds,
                                        &style,
                                        element_state,
                                        auto_scrollbars,
                                        window,
                                    );
                                }
                            });
                        });
                    });
                });

                ((), element_state)
            },
        );
    }

    #[cfg(debug_assertions)]
    fn paint_debug_info(
        &self,
        global_id: Option<&GlobalElementId>,
        hitbox: &Hitbox,
        style: &Style,
        window: &mut Window,
        cx: &mut App,
    ) {
        use crate::{BorderStyle, TextAlign};

        if let Some(global_id) = global_id.as_ref().filter(|_| {
            (style.debug || style.debug_below || cx.has_global::<crate::DebugBelow>())
                && hitbox.is_hovered(window)
        }) {
            const FONT_SIZE: crate::Pixels = crate::Pixels(10.);
            let element_id = format!("{:?}", global_id);
            let str_len = element_id.len();

            let render_debug_text = |window: &mut Window| {
                if let Some(text) = window
                    .text_system()
                    .shape_text(
                        element_id.into(),
                        FONT_SIZE,
                        &[window.text_style().to_run(str_len)],
                        None,
                        None,
                    )
                    .ok()
                    .and_then(|mut text| text.pop())
                {
                    text.paint(hitbox.origin, FONT_SIZE, TextAlign::Left, None, window, cx)
                        .ok();

                    let text_bounds = crate::Bounds {
                        origin: hitbox.origin,
                        size: text.size(FONT_SIZE),
                    };
                    if let Some(location) = self.source_location
                        && text_bounds.contains(&window.mouse_position())
                        && window.modifiers().secondary()
                    {
                        let secondary_held = window.modifiers().secondary();
                        window.on_key_event({
                            move |e: &crate::ModifiersChangedEvent, _phase, window, _cx| {
                                if e.modifiers.secondary() != secondary_held
                                    && text_bounds.contains(&window.mouse_position())
                                {
                                    window.refresh();
                                }
                            }
                        });

                        let was_hovered = hitbox.is_hovered(window);
                        let current_view = window.current_view();
                        window.on_mouse_event({
                            let hitbox = hitbox.clone();
                            move |_: &MouseMoveEvent, phase, window, cx| {
                                if phase == DispatchPhase::Capture {
                                    let hovered = hitbox.is_hovered(window);
                                    if hovered != was_hovered {
                                        cx.notify(current_view)
                                    }
                                }
                            }
                        });

                        window.on_mouse_event({
                            let hitbox = hitbox.clone();
                            move |e: &crate::MouseDownEvent, phase, window, cx| {
                                if text_bounds.contains(&e.position)
                                    && phase.capture()
                                    && hitbox.is_hovered(window)
                                {
                                    cx.stop_propagation();
                                    let Ok(dir) = std::env::current_dir() else {
                                        return;
                                    };

                                    eprintln!(
                                        "This element was created at:\n{}:{}:{}",
                                        dir.join(location.file()).to_string_lossy(),
                                        location.line(),
                                        location.column()
                                    );
                                }
                            }
                        });
                        window.paint_quad(crate::outline(
                            crate::Bounds {
                                origin: hitbox.origin
                                    + crate::point(crate::px(0.), FONT_SIZE - px(2.)),
                                size: crate::Size {
                                    width: text_bounds.size.width,
                                    height: crate::px(1.),
                                },
                            },
                            crate::red(),
                            BorderStyle::default(),
                        ))
                    }
                }
            };

            window.with_text_style(
                Some(crate::TextStyleRefinement {
                    color: Some(crate::red()),
                    line_height: Some(FONT_SIZE.into()),
                    background_color: Some(crate::white()),
                    ..Default::default()
                }),
                render_debug_text,
            )
        }
    }

    fn paint_mouse_listeners(
        &mut self,
        hitbox: &Hitbox,
        element_state: Option<&mut InteractiveElementState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let is_focused = self
            .tracked_focus_handle
            .as_ref()
            .map(|handle| handle.is_focused(window))
            .unwrap_or(false);

        // If this element can be focused, register a mouse down listener
        // that will automatically transfer focus when hitting the element.
        // This behavior can be suppressed by using `cx.prevent_default()`.
        if let Some(focus_handle) = self.tracked_focus_handle.clone() {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |_: &MouseDownEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble
                    && hitbox.is_hovered(window)
                    && !window.default_prevented()
                {
                    window.focus(&focus_handle);
                    // If there is a parent that is also focusable, prevent it
                    // from transferring focus because we already did so.
                    window.prevent_default();
                }
            });
        }

        for listener in self.mouse_down_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.mouse_up_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.mouse_move_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        for listener in self.scroll_wheel_listeners.drain(..) {
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                listener(event, phase, &hitbox, window, cx);
            })
        }

        if self.hover_style.is_some()
            || self.base_style.mouse_cursor.is_some()
            || cx.active_drag.is_some() && !self.drag_over_styles.is_empty()
        {
            let hitbox = hitbox.clone();
            let was_hovered = hitbox.is_hovered(window);
            let current_view = window.current_view();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                let hovered = hitbox.is_hovered(window);
                if phase == DispatchPhase::Capture && hovered != was_hovered {
                    cx.notify(current_view);
                }
            });
        }
        let drag_cursor_style = self.base_style.as_ref().mouse_cursor;

        let mut drag_listener = mem::take(&mut self.drag_listener);
        let drop_listeners = mem::take(&mut self.drop_listeners);
        let click_listeners = mem::take(&mut self.click_listeners);
        let can_drop_predicate = mem::take(&mut self.can_drop_predicate);

        if !drop_listeners.is_empty() {
            let hitbox = hitbox.clone();
            window.on_mouse_event({
                move |_: &MouseUpEvent, phase, window, cx| {
                    if let Some(drag) = &cx.active_drag
                        && phase == DispatchPhase::Bubble
                        && hitbox.is_hovered(window)
                    {
                        let drag_state_type = drag.value.as_ref().type_id();
                        for (drop_state_type, listener) in &drop_listeners {
                            if *drop_state_type == drag_state_type {
                                let drag = cx
                                    .active_drag
                                    .take()
                                    .expect("checked for type drag state type above");

                                let mut can_drop = true;
                                if let Some(predicate) = &can_drop_predicate {
                                    can_drop = predicate(drag.value.as_ref(), window, cx);
                                }

                                if can_drop {
                                    listener(drag.value.as_ref(), window, cx);
                                    window.refresh();
                                    cx.stop_propagation();
                                }
                            }
                        }
                    }
                }
            });
        }

        if let Some(element_state) = element_state {
            if !click_listeners.is_empty() || drag_listener.is_some() {
                let pending_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();

                let clicked_state = element_state
                    .clicked_state
                    .get_or_insert_with(Default::default)
                    .clone();

                window.on_mouse_event({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let hitbox = hitbox.clone();
                    move |event: &MouseDownEvent, phase, window, _cx| {
                        if phase == DispatchPhase::Bubble
                            && event.button == MouseButton::Left
                            && hitbox.is_hovered(window)
                        {
                            *pending_mouse_down.borrow_mut() = Some(event.clone());
                            window.refresh();
                        }
                    }
                });

                window.on_mouse_event({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let hitbox = hitbox.clone();
                    move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase == DispatchPhase::Capture {
                            return;
                        }

                        let mut pending_mouse_down = pending_mouse_down.borrow_mut();
                        if let Some(mouse_down) = pending_mouse_down.clone()
                            && !cx.has_active_drag()
                            && (event.position - mouse_down.position).magnitude() > DRAG_THRESHOLD
                            && let Some((drag_value, drag_listener)) = drag_listener.take()
                        {
                            *clicked_state.borrow_mut() = ElementClickedState::default();
                            let cursor_offset = event.position - hitbox.origin;
                            let drag =
                                (drag_listener)(drag_value.as_ref(), cursor_offset, window, cx);
                            cx.active_drag = Some(AnyDrag {
                                view: drag,
                                value: drag_value,
                                cursor_offset,
                                cursor_style: drag_cursor_style,
                            });
                            pending_mouse_down.take();
                            window.refresh();
                            cx.stop_propagation();
                        }
                    }
                });

                if is_focused {
                    // Press enter, space to trigger click, when the element is focused.
                    window.on_key_event({
                        let click_listeners = click_listeners.clone();
                        let hitbox = hitbox.clone();
                        move |event: &KeyUpEvent, phase, window, cx| {
                            if phase.bubble() && !window.default_prevented() {
                                let stroke = &event.keystroke;
                                let keyboard_button = if stroke.key.eq("enter") {
                                    Some(KeyboardButton::Enter)
                                } else if stroke.key.eq("space") {
                                    Some(KeyboardButton::Space)
                                } else {
                                    None
                                };

                                if let Some(button) = keyboard_button
                                    && !stroke.modifiers.modified()
                                {
                                    let click_event = ClickEvent::Keyboard(KeyboardClickEvent {
                                        button,
                                        bounds: hitbox.bounds,
                                    });

                                    for listener in &click_listeners {
                                        listener(&click_event, window, cx);
                                    }
                                }
                            }
                        }
                    });
                }

                window.on_mouse_event({
                    let mut captured_mouse_down = None;
                    let hitbox = hitbox.clone();
                    move |event: &MouseUpEvent, phase, window, cx| match phase {
                        // Clear the pending mouse down during the capture phase,
                        // so that it happens even if another event handler stops
                        // propagation.
                        DispatchPhase::Capture => {
                            let mut pending_mouse_down = pending_mouse_down.borrow_mut();
                            if pending_mouse_down.is_some() && hitbox.is_hovered(window) {
                                captured_mouse_down = pending_mouse_down.take();
                                window.refresh();
                            } else if pending_mouse_down.is_some() {
                                // Clear the pending mouse down event (without firing click handlers)
                                // if the hitbox is not being hovered.
                                // This avoids dragging elements that changed their position
                                // immediately after being clicked.
                                // Prevents dragging elements that reposition after click
                                pending_mouse_down.take();
                                window.refresh();
                            }
                        }
                        // Fire click handlers during the bubble phase.
                        DispatchPhase::Bubble => {
                            if let Some(mouse_down) = captured_mouse_down.take() {
                                let mouse_click = ClickEvent::Mouse(MouseClickEvent {
                                    down: mouse_down,
                                    up: event.clone(),
                                });
                                for listener in &click_listeners {
                                    listener(&mouse_click, window, cx);
                                }
                            }
                        }
                    }
                });
            }

            if let Some(hover_listener) = self.hover_listener.take() {
                let hitbox = hitbox.clone();
                let was_hovered = element_state
                    .hover_state
                    .get_or_insert_with(Default::default)
                    .clone();
                let has_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();

                window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let is_hovered = has_mouse_down.borrow().is_none()
                        && !cx.has_active_drag()
                        && hitbox.is_hovered(window);
                    let mut was_hovered = was_hovered.borrow_mut();

                    if is_hovered != *was_hovered {
                        *was_hovered = is_hovered;
                        drop(was_hovered);

                        hover_listener(&is_hovered, window, cx);
                    }
                });
            }

            if let Some(tooltip_builder) = self.tooltip_builder.take() {
                let active_tooltip = element_state
                    .active_tooltip
                    .get_or_insert_with(Default::default)
                    .clone();
                let pending_mouse_down = element_state
                    .pending_mouse_down
                    .get_or_insert_with(Default::default)
                    .clone();

                let tooltip_is_hoverable = tooltip_builder.hoverable;
                let build_tooltip = Rc::new(move |window: &mut Window, cx: &mut App| {
                    Some(((tooltip_builder.build)(window, cx), tooltip_is_hoverable))
                });
                // Use bounds instead of testing hitbox since this is called during prepaint.
                let check_is_hovered_during_prepaint = Rc::new({
                    let pending_mouse_down = pending_mouse_down.clone();
                    let source_bounds = hitbox.bounds;
                    move |window: &Window| {
                        pending_mouse_down.borrow().is_none()
                            && source_bounds.contains(&window.mouse_position())
                    }
                });
                let check_is_hovered = Rc::new({
                    let hitbox = hitbox.clone();
                    move |window: &Window| {
                        pending_mouse_down.borrow().is_none() && hitbox.is_hovered(window)
                    }
                });
                register_tooltip_mouse_handlers(
                    &active_tooltip,
                    self.tooltip_id,
                    build_tooltip,
                    check_is_hovered,
                    check_is_hovered_during_prepaint,
                    window,
                );
            }

            if let Some(context_menu_builder) = self.context_menu_builder.take() {
                let active_context_menu = element_state
                    .active_context_menu
                    .get_or_insert_with(Default::default)
                    .clone();
                let build_context_menu = context_menu_builder.build.clone();
                let check_is_hovered = Rc::new({
                    let hitbox = hitbox.clone();
                    move |window: &Window| hitbox.is_hovered(window)
                });
                register_context_menu_mouse_handlers(
                    &active_context_menu,
                    self.context_menu_id,
                    build_context_menu,
                    check_is_hovered,
                    window,
                );
            }

            let active_state = element_state
                .clicked_state
                .get_or_insert_with(Default::default)
                .clone();
            if active_state.borrow().is_clicked() {
                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Capture {
                        *active_state.borrow_mut() = ElementClickedState::default();
                        window.refresh();
                    }
                });
            } else {
                let active_group_hitbox = self
                    .group_active_style
                    .as_ref()
                    .and_then(|group_active| GroupHitboxes::get(&group_active.group, cx));
                let hitbox = hitbox.clone();
                window.on_mouse_event(move |_: &MouseDownEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Bubble && !window.default_prevented() {
                        let group_hovered = active_group_hitbox
                            .is_some_and(|group_hitbox_id| group_hitbox_id.is_hovered(window));
                        let element_hovered = hitbox.is_hovered(window);
                        if group_hovered || element_hovered {
                            *active_state.borrow_mut() = ElementClickedState {
                                group: group_hovered,
                                element: element_hovered,
                            };
                            window.refresh();
                        }
                    }
                });
            }
        }
    }

    fn paint_keyboard_listeners(&mut self, window: &mut Window, _cx: &mut App) {
        let key_down_listeners = mem::take(&mut self.key_down_listeners);
        let key_up_listeners = mem::take(&mut self.key_up_listeners);
        let modifiers_changed_listeners = mem::take(&mut self.modifiers_changed_listeners);
        let action_listeners = mem::take(&mut self.action_listeners);
        if let Some(context) = self.key_context.clone() {
            window.set_key_context(context);
        }

        for listener in key_down_listeners {
            window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
                listener(event, phase, window, cx);
            })
        }

        for listener in key_up_listeners {
            window.on_key_event(move |event: &KeyUpEvent, phase, window, cx| {
                listener(event, phase, window, cx);
            })
        }

        for listener in modifiers_changed_listeners {
            window.on_modifiers_changed(move |event: &ModifiersChangedEvent, window, cx| {
                listener(event, window, cx);
            })
        }

        for (action_type, listener) in action_listeners {
            window.on_action(action_type, listener)
        }
    }

    fn paint_gesture_listeners(
        &mut self,
        hitbox: &Hitbox,
        element_state: Option<&mut InteractiveElementState>,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let Some(element_state) = element_state else {
            self.pan_listeners.clear();
            self.swipe_listeners.clear();
            self.pinch_listeners.clear();
            self.tap_listeners.clear();
            self.long_press_listeners.clear();
            return;
        };

        let pan_listeners = Rc::new(mem::take(&mut self.pan_listeners));
        let swipe_listeners = Rc::new(mem::take(&mut self.swipe_listeners));
        let pinch_listeners = Rc::new(mem::take(&mut self.pinch_listeners));
        let tap_listeners = Rc::new(mem::take(&mut self.tap_listeners));
        let long_press_listeners = Rc::new(mem::take(&mut self.long_press_listeners));
        if pan_listeners.is_empty()
            && swipe_listeners.is_empty()
            && pinch_listeners.is_empty()
            && tap_listeners.is_empty()
            && long_press_listeners.is_empty()
        {
            return;
        }

        let gesture_state = element_state
            .gesture_state
            .get_or_insert_with(Default::default)
            .clone();
        let pending_mouse_down = element_state
            .pending_mouse_down
            .get_or_insert_with(Default::default)
            .clone();
        let clicked_state = element_state
            .clicked_state
            .get_or_insert_with(Default::default)
            .clone();

        if !pan_listeners.is_empty() || !swipe_listeners.is_empty() {
            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                let hitbox = hitbox.clone();
                move |event: &MouseDownEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        gesture_state
                            .borrow_mut()
                            .pan
                            .on_event(&crate::PlatformInput::MouseDown(event.clone()));
                    }
                }
            });

            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                let pan_listeners = pan_listeners.clone();
                let swipe_listeners = swipe_listeners.clone();
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || cx.has_active_drag() {
                        return;
                    }

                    let pan_event = gesture_state
                        .borrow_mut()
                        .pan
                        .on_event(&crate::PlatformInput::MouseMove(event.clone()));
                    if let Some(pan_event) = pan_event {
                        if pan_event.state == PanState::Began {
                            pending_mouse_down.borrow_mut().take();
                            *clicked_state.borrow_mut() = ElementClickedState::default();
                            window.refresh();
                        }

                        let handled = dispatch_pan_gesture(
                            &pan_event,
                            pan_listeners.as_ref(),
                            swipe_listeners.as_ref(),
                            window,
                            cx,
                        );
                        if handled {
                            cx.stop_propagation();
                        }
                    }
                }
            });

            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                let pan_listeners = pan_listeners.clone();
                let swipe_listeners = swipe_listeners.clone();
                let mut captured_pan_event = None;
                move |event: &MouseUpEvent, phase, window, cx| match phase {
                    DispatchPhase::Capture => {
                        captured_pan_event = gesture_state
                            .borrow_mut()
                            .pan
                            .on_event(&crate::PlatformInput::MouseUp(event.clone()));
                    }
                    DispatchPhase::Bubble => {
                        if let Some(pan_event) = captured_pan_event.take() {
                            let handled = dispatch_pan_gesture(
                                &pan_event,
                                pan_listeners.as_ref(),
                                swipe_listeners.as_ref(),
                                window,
                                cx,
                            );
                            if handled {
                                cx.stop_propagation();
                            }
                        }
                    }
                }
            });
        }

        if !tap_listeners.is_empty() || !long_press_listeners.is_empty() {
            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                let hitbox = hitbox.clone();
                move |event: &MouseDownEvent, phase, window, _cx| {
                    if phase == DispatchPhase::Bubble
                        && event.button == MouseButton::Left
                        && hitbox.is_hovered(window)
                    {
                        let mut state = gesture_state.borrow_mut();
                        state
                            .tap
                            .on_event(&crate::PlatformInput::MouseDown(event.clone()));
                        state
                            .long_press
                            .on_event(&crate::PlatformInput::MouseDown(event.clone()));
                    }
                }
            });

            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                move |event: &MouseMoveEvent, phase, _window, _cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    let mut state = gesture_state.borrow_mut();
                    state
                        .tap
                        .on_event(&crate::PlatformInput::MouseMove(event.clone()));
                    state
                        .long_press
                        .on_event(&crate::PlatformInput::MouseMove(event.clone()));
                }
            });

            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                let tap_listeners = tap_listeners.clone();
                let long_press_listeners = long_press_listeners.clone();
                move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }

                    let long_press_event = gesture_state
                        .borrow_mut()
                        .long_press
                        .on_event(&crate::PlatformInput::MouseUp(event.clone()));
                    let tap_event = gesture_state
                        .borrow_mut()
                        .tap
                        .on_event(&crate::PlatformInput::MouseUp(event.clone()));

                    if let Some(long_press_event) = long_press_event {
                        for listener in long_press_listeners.iter() {
                            listener(&long_press_event, window, cx);
                        }
                        cx.stop_propagation();
                    } else if let Some(tap_event) = tap_event {
                        for listener in tap_listeners.iter() {
                            listener(&tap_event, window, cx);
                        }
                        cx.stop_propagation();
                    }
                }
            });
        }

        if !pinch_listeners.is_empty() {
            window.on_mouse_event({
                let gesture_state = gesture_state.clone();
                let pinch_listeners = pinch_listeners.clone();
                let hitbox = hitbox.clone();
                move |event: &MagnifyEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    if !hitbox.is_hovered(window) && !gesture_state.borrow().pinch.is_active() {
                        return;
                    }

                    let pinch_event = gesture_state
                        .borrow_mut()
                        .pinch
                        .on_event(&crate::PlatformInput::Magnify(event.clone()));
                    if let Some(pinch_event) = pinch_event {
                        for listener in pinch_listeners.iter() {
                            listener(&pinch_event, window, cx);
                        }
                        cx.stop_propagation();
                    }
                }
            });

            window.on_mouse_event({
                let pinch_listeners = pinch_listeners.clone();
                let hitbox = hitbox.clone();
                move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                        return;
                    }

                    let pinch_event = gesture_state
                        .borrow_mut()
                        .pinch
                        .on_event(&crate::PlatformInput::ScrollWheel(event.clone()));
                    if let Some(pinch_event) = pinch_event {
                        for listener in pinch_listeners.iter() {
                            listener(&pinch_event, window, cx);
                        }
                        cx.stop_propagation();
                    }
                }
            });
        }
    }

    fn paint_hover_group_handler(&self, window: &mut Window, cx: &mut App) {
        let group_hitbox = self
            .group_hover_style
            .as_ref()
            .and_then(|group_hover| GroupHitboxes::get(&group_hover.group, cx));

        if let Some(group_hitbox) = group_hitbox {
            let was_hovered = group_hitbox.is_hovered(window);
            let current_view = window.current_view();
            window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                let hovered = group_hitbox.is_hovered(window);
                if phase == DispatchPhase::Capture && hovered != was_hovered {
                    cx.notify(current_view);
                }
            });
        }
    }

    fn paint_scroll_listener(
        &self,
        hitbox: &Hitbox,
        style: &Style,
        window: &mut Window,
        _cx: &mut App,
    ) {
        if let Some(scroll_offset) = self.scroll_offset.clone() {
            let scroll_elastic_state = self.scroll_elastic_state.clone();
            let overflow = style.overflow;
            let allow_concurrent_scroll = style.allow_concurrent_scroll;
            let restrict_scroll_to_axis = style.restrict_scroll_to_axis;
            let line_height = window.line_height();
            let hitbox = hitbox.clone();
            let current_view = window.current_view();
            window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
                    let mut scroll_offset = scroll_offset.borrow_mut();
                    let old_scroll_offset = *scroll_offset;
                    let mut scroll_elastic_state = scroll_elastic_state
                        .as_ref()
                        .map(|scroll_elastic_state| scroll_elastic_state.borrow_mut());
                    let old_overscroll = scroll_elastic_state
                        .as_ref()
                        .map_or(Point::default(), |scroll_elastic_state| {
                            scroll_elastic_state.overscroll
                        });
                    if event.is_momentum && old_overscroll != Point::default() {
                        cx.stop_propagation();
                        return;
                    }
                    let delta = event.delta.pixel_delta(line_height);
                    let rubber_band_enabled = window.power_mode() != crate::PowerMode::LowPower
                        && rubber_band_scroll_enabled(event)
                        && !event.is_momentum;
                    let max_offset = scroll_elastic_state
                        .as_ref()
                        .map_or(Default::default(), |scroll_elastic_state| {
                            scroll_elastic_state.max_offset
                        });
                    let can_scroll_x = axis_is_scrollable(overflow.x, max_offset.width);
                    let can_scroll_y = axis_is_scrollable(overflow.y, max_offset.height);

                    let mut delta_x = Pixels::ZERO;
                    if can_scroll_x {
                        if !delta.x.is_zero() {
                            delta_x = delta.x;
                        } else if !restrict_scroll_to_axis && !can_scroll_y {
                            delta_x = delta.y;
                        }
                    }
                    let mut delta_y = Pixels::ZERO;
                    if can_scroll_y {
                        if !delta.y.is_zero() {
                            delta_y = delta.y;
                        } else if !restrict_scroll_to_axis && !can_scroll_x {
                            delta_y = delta.x;
                        }
                    }
                    if !allow_concurrent_scroll && !delta_x.is_zero() && !delta_y.is_zero() {
                        if delta_x.abs() > delta_y.abs() {
                            delta_y = Pixels::ZERO;
                        } else {
                            delta_x = Pixels::ZERO;
                        }
                    }

                    if let Some(scroll_elastic_state) = scroll_elastic_state.as_deref_mut() {
                        if rubber_band_enabled {
                            scroll_elastic_state.animating = false;
                        }

                        apply_scroll_delta_axis(
                            &mut scroll_offset.x,
                            &mut scroll_elastic_state.overscroll.x,
                            scroll_elastic_state.max_offset.width,
                            delta_x,
                            rubber_band_enabled,
                        );
                        apply_scroll_delta_axis(
                            &mut scroll_offset.y,
                            &mut scroll_elastic_state.overscroll.y,
                            scroll_elastic_state.max_offset.height,
                            delta_y,
                            rubber_band_enabled,
                        );

                        if rubber_band_enabled {
                            scroll_elastic_state.animating =
                                matches!(event.touch_phase, TouchPhase::Ended)
                                    && !scroll_elastic_state.overscroll.eq(&Point::default());
                            if scroll_elastic_state.animating {
                                window.refresh();
                            }
                        }
                    } else {
                        scroll_offset.y += delta_y;
                        scroll_offset.x += delta_x;
                    }

                    let new_overscroll = scroll_elastic_state
                        .as_ref()
                        .map_or(Point::default(), |scroll_elastic_state| {
                            scroll_elastic_state.overscroll
                        });
                    let handled_scroll =
                        *scroll_offset != old_scroll_offset || new_overscroll != old_overscroll;
                    if handled_scroll {
                        if let Some(scroll_elastic_state) = scroll_elastic_state.as_deref_mut() {
                            scroll_elastic_state.mark_scrolled();
                        }
                        cx.stop_propagation();
                        cx.notify(current_view);
                    }
                }
            });
        }
    }

    fn paint_auto_scrollbars(
        &self,
        bounds: Bounds<Pixels>,
        style: &Style,
        element_state: &mut InteractiveElementState,
        auto_scrollbars: &AutoScrollbarHitboxes,
        window: &mut Window,
    ) {
        let should_keep_fading = self.scroll_elastic_state.as_ref().is_some_and(|state| {
            let state = state.borrow();
            !state.animating && state.seconds_since_last_scroll() <= AUTO_SCROLLBAR_IDLE_SECONDS
        });
        if should_keep_fading {
            window.request_animation_frame();
        }

        let scroll_handle = match self.tracked_scroll_handle.as_ref() {
            Some(h) => h,
            None => return,
        };

        let max_offset = scroll_handle.max_offset();
        let show_y = axis_is_scrollable(style.overflow.y, max_offset.height);
        let show_x = axis_is_scrollable(style.overflow.x, max_offset.width);

        if !show_y && !show_x {
            return;
        }

        let state = element_state
            .auto_scrollbar_state
            .get_or_insert_with(Rc::default)
            .clone();
        let always_show = scroll_handle.0.borrow().always_show_scrollbars;
        let should_show_scrollbars = always_show
            || self.scroll_elastic_state.as_ref().is_none_or(|state| {
                let state = state.borrow();
                state.animating || state.seconds_since_last_scroll() <= AUTO_SCROLLBAR_IDLE_SECONDS
            });

        if show_y {
            let track_bounds = auto_scrollbar_track_bounds(bounds, true, show_x);
            if let Some(hitbox) = auto_scrollbars.vertical.as_ref() {
                self.paint_auto_scrollbar_axis(
                    track_bounds,
                    hitbox,
                    true,
                    should_show_scrollbars,
                    scroll_handle,
                    &state,
                    window,
                );
            }
        }

        if show_x {
            let track_bounds = auto_scrollbar_track_bounds(bounds, false, show_y);
            if let Some(hitbox) = auto_scrollbars.horizontal.as_ref() {
                self.paint_auto_scrollbar_axis(
                    track_bounds,
                    hitbox,
                    false,
                    should_show_scrollbars,
                    scroll_handle,
                    &state,
                    window,
                );
            }
        }
    }

    fn paint_auto_scrollbar_axis(
        &self,
        track_bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        vertical: bool,
        should_show_scrollbars: bool,
        scroll_handle: &ScrollHandle,
        state: &Rc<RefCell<AutoScrollbarState>>,
        window: &mut Window,
    ) {
        let hovered = hitbox.is_hovered(window);
        let dragging = state
            .borrow()
            .drag_state
            .is_some_and(|drag| drag.vertical == vertical);

        if hovered || dragging {
            window.set_cursor_style(CursorStyle::PointingHand, &hitbox);
        }

        if hovered && !should_show_scrollbars {
            window.refresh();
        }

        if should_show_scrollbars || hovered || dragging {
            let thumb_bounds = auto_scrollbar_thumb_bounds(track_bounds, scroll_handle, vertical);
            let thumb_color = crate::hsla(0., 0., 0.0, 0.5);
            window.paint_quad(
                crate::fill(thumb_bounds, thumb_color)
                    .corner_radii(AUTO_SCROLLBAR_THUMB_WIDTH / 2.0),
            );
        }

        let down_hitbox = hitbox.clone();
        let down_handle = scroll_handle.clone();
        let down_state = state.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !down_hitbox.is_hovered(window)
            {
                return;
            }

            let thumb_bounds = auto_scrollbar_thumb_bounds(track_bounds, &down_handle, vertical);
            let logical_offset = if thumb_bounds.contains(&event.position) {
                auto_scrollbar_logical_offset(&down_handle, vertical)
            } else {
                auto_scrollbar_logical_offset_for_position(
                    event.position,
                    track_bounds,
                    &down_handle,
                    vertical,
                    true,
                )
            };
            set_auto_scrollbar_logical_offset(&down_handle, vertical, logical_offset);
            down_state.borrow_mut().drag_state = Some(AutoScrollbarDragState {
                vertical,
                start_logical_offset: logical_offset,
                start_position: event.position,
            });
            window.refresh();
            cx.stop_propagation();
            window.prevent_default();
        });

        let move_handle = scroll_handle.clone();
        let move_state = state.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !event.dragging() {
                return;
            }

            let Some(drag_state) = move_state.borrow().drag_state else {
                return;
            };
            if drag_state.vertical != vertical {
                return;
            }

            let logical_offset = auto_scrollbar_logical_offset_for_drag(
                event.position,
                track_bounds,
                &move_handle,
                vertical,
                drag_state,
            );
            set_auto_scrollbar_logical_offset(&move_handle, vertical, logical_offset);
            window.refresh();
            cx.stop_propagation();
        });

        let up_state = state.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }

            let mut up_state = up_state.borrow_mut();
            if up_state
                .drag_state
                .is_some_and(|drag_state| drag_state.vertical == vertical)
            {
                up_state.drag_state = None;
                window.refresh();
                cx.stop_propagation();
            }
        });
    }

    /// Compute the visual style for this element, based on the current bounds and the element's state.
    pub fn compute_style(
        &self,
        global_id: Option<&GlobalElementId>,
        hitbox: Option<&Hitbox>,
        window: &mut Window,
        cx: &mut App,
    ) -> Style {
        window.with_optional_element_state(global_id, |element_state, window| {
            let mut element_state =
                element_state.map(|element_state| element_state.unwrap_or_default());
            let style = self.compute_style_internal(hitbox, element_state.as_mut(), window, cx);
            (style, element_state)
        })
    }

    /// Called from internal methods that have already called with_element_state.
    fn compute_style_internal(
        &self,
        hitbox: Option<&Hitbox>,
        element_state: Option<&mut InteractiveElementState>,
        window: &mut Window,
        cx: &mut App,
    ) -> Style {
        let mut style = Style::default();
        style.refine(&self.base_style);

        if let Some(focus_handle) = self.tracked_focus_handle.as_ref() {
            if let Some(in_focus_style) = self.in_focus_style.as_ref()
                && focus_handle.within_focused(window, cx)
            {
                style.refine(in_focus_style);
            }

            if let Some(focus_style) = self.focus_style.as_ref()
                && focus_handle.is_focused(window)
            {
                style.refine(focus_style);
            }

            if let Some(focus_visible_style) = self.focus_visible_style.as_ref()
                && focus_handle.is_focused(window)
                && window.is_keyboard_navigation_active()
            {
                style.refine(focus_visible_style);
            }
        }

        if let Some(hitbox) = hitbox {
            if !cx.has_active_drag() {
                if let Some(group_hover) = self.group_hover_style.as_ref()
                    && let Some(group_hitbox_id) = GroupHitboxes::get(&group_hover.group, cx)
                    && group_hitbox_id.is_hovered(window)
                {
                    style.refine(&group_hover.style);
                }

                if let Some(hover_style) = self.hover_style.as_ref()
                    && hitbox.is_hovered(window)
                {
                    style.refine(hover_style);
                }
            }

            if let Some(drag) = cx.active_drag.take() {
                let mut can_drop = true;
                if let Some(can_drop_predicate) = &self.can_drop_predicate {
                    can_drop = can_drop_predicate(drag.value.as_ref(), window, cx);
                }

                if can_drop {
                    for (state_type, group_drag_style) in &self.group_drag_over_styles {
                        if let Some(group_hitbox_id) =
                            GroupHitboxes::get(&group_drag_style.group, cx)
                            && *state_type == drag.value.as_ref().type_id()
                            && group_hitbox_id.is_hovered(window)
                        {
                            style.refine(&group_drag_style.style);
                        }
                    }

                    for (state_type, build_drag_over_style) in &self.drag_over_styles {
                        if *state_type == drag.value.as_ref().type_id() && hitbox.is_hovered(window)
                        {
                            style.refine(&build_drag_over_style(drag.value.as_ref(), window, cx));
                        }
                    }
                }

                style.mouse_cursor = drag.cursor_style;
                cx.active_drag = Some(drag);
            }
        }

        if let Some(element_state) = element_state {
            let clicked_state = element_state
                .clicked_state
                .get_or_insert_with(Default::default)
                .borrow();
            if clicked_state.group
                && let Some(group) = self.group_active_style.as_ref()
            {
                style.refine(&group.style)
            }

            if let Some(active_style) = self.active_style.as_ref()
                && clicked_state.element
            {
                style.refine(active_style)
            }
        }

        style
    }

    fn resolve_flip_transform(
        &self,
        element_state: Option<&mut InteractiveElementState>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
    ) -> Option<TransformationMatrix> {
        let config = self.layout_animation.as_ref()?;
        let element_state = element_state?;
        let flip_state = element_state
            .flip_state
            .get_or_insert_with(Default::default)
            .clone();
        let mut flip_state = flip_state.borrow_mut();
        let (offset, needs_frame) = flip_state.resolve(
            bounds.origin,
            std::time::Instant::now(),
            window.animations_enabled(),
            config,
        );
        if needs_frame {
            window.request_animation_frame();
        }
        let offset = offset?;
        let scale_factor = window.scale_factor();
        Some(TransformationMatrix::unit().translate(Point {
            x: ScaledPixels(offset.x.0 * scale_factor),
            y: ScaledPixels(offset.y.0 * scale_factor),
        }))
    }

    fn animate_style(
        &self,
        global_id: Option<&GlobalElementId>,
        style: Style,
        window: &mut Window,
    ) -> Style {
        let Some(config) = self.implicit_transition.clone() else {
            return style;
        };

        window.with_optional_element_state::<ImplicitStyleAnimationState, _>(
            global_id,
            |animation_state, window| {
                let Some(animation_state) = animation_state else {
                    return (style, None);
                };

                let mut animation_state = animation_state.unwrap_or_default();
                let style = animation_state.animate(style, &config, window);
                (style, Some(animation_state))
            },
        )
    }
}

/// The per-frame state of an interactive element. Used for tracking stateful interactions like clicks
/// and scroll offsets.
#[derive(Default)]
pub struct InteractiveElementState {
    pub(crate) focus_handle: Option<FocusHandle>,
    pub(crate) clicked_state: Option<Rc<RefCell<ElementClickedState>>>,
    pub(crate) hover_state: Option<Rc<RefCell<bool>>>,
    pub(crate) pending_mouse_down: Option<Rc<RefCell<Option<MouseDownEvent>>>>,
    pub(crate) gesture_state: Option<Rc<RefCell<ElementGestureState>>>,
    pub(crate) scroll_offset: Option<Rc<RefCell<Point<Pixels>>>>,
    pub(crate) scroll_elastic_state: Option<Rc<RefCell<ScrollElasticState>>>,
    pub(crate) active_tooltip: Option<Rc<RefCell<Option<ActiveTooltip>>>>,
    pub(crate) active_context_menu: Option<Rc<RefCell<Option<AnyTooltip>>>>,
    pub(crate) auto_scrollbar_state: Option<Rc<RefCell<AutoScrollbarState>>>,
    pub(crate) flip_state: Option<Rc<RefCell<FlipLayoutState>>>,
    pub(crate) prev_bounds: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Debug)]
struct AutoScrollbarDragState {
    vertical: bool,
    start_logical_offset: Pixels,
    start_position: Point<Pixels>,
}

#[derive(Default, Debug)]
pub(crate) struct AutoScrollbarState {
    drag_state: Option<AutoScrollbarDragState>,
}

#[derive(Default)]
pub(crate) struct ElementGestureState {
    pan: PanGesture,
    pinch: PinchGesture,
    tap: TapGesture,
    long_press: LongPressGesture,
}

/// Whether or not the element or a group that contains it is clicked by the mouse.
#[derive(Copy, Clone, Default, Eq, PartialEq)]
pub struct ElementClickedState {
    /// True if this element's group has been clicked, false otherwise
    pub group: bool,

    /// True if this element has been clicked, false otherwise
    pub element: bool,
}

impl ElementClickedState {
    fn is_clicked(&self) -> bool {
        self.group || self.element
    }
}

pub(crate) enum ActiveTooltip {
    /// Currently delaying before showing the tooltip.
    WaitingForShow { _task: Task<()> },
    /// Tooltip is visible, element was hovered or for hoverable tooltips, the tooltip was hovered.
    Visible {
        tooltip: AnyTooltip,
        is_hoverable: bool,
    },
    /// Tooltip is visible and hoverable, but the mouse is no longer hovering. Currently delaying
    /// before hiding it.
    WaitingForHide {
        tooltip: AnyTooltip,
        _task: Task<()>,
    },
}

struct ContextMenuOverlay {
    active_context_menu: Rc<RefCell<Option<AnyTooltip>>>,
    content: AnyView,
}

struct TooltipTextView {
    text: SharedString,
}

impl Render for TooltipTextView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "tooltip-text".to_string())
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(TOOLTIP_PANEL_RADIUS))
            .bg(crate::hsla(0.0, 0.0, 0.08, 0.96))
            .text_xs()
            .text_color(crate::hsla(0.0, 0.0, 1.0, 1.0))
            .shadow_lg()
            .child(self.text.clone())
    }
}

struct TooltipElementView {
    build: Rc<dyn Fn() -> AnyElement>,
}

impl Render for TooltipElementView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child((self.build)())
    }
}

struct StructuredContextMenu {
    menu: ContextMenu,
    active_context_menu: Rc<RefCell<Option<AnyTooltip>>>,
    open_submenu_path: Vec<usize>,
}

impl Render for StructuredContextMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let open_size = context_menu_open_size(&self.menu, &self.open_submenu_path);

        div()
            .relative()
            .w(open_size.width)
            .h(open_size.height)
            .child(render_context_menu_panel(
                &self.menu,
                &[],
                &self.open_submenu_path,
                self.active_context_menu.clone(),
                cx,
            ))
    }
}

impl Render for ContextMenuOverlay {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let dismiss_on_click_outside = self.active_context_menu.clone();
        let dismiss_on_escape = self.active_context_menu.clone();

        div()
            .occlude()
            .child(self.content.clone())
            .on_mouse_down_out(move |_, window, _| {
                clear_active_context_menu(&dismiss_on_click_outside, window);
            })
            .capture_key_down(move |event, window, _| {
                if event.keystroke.key == "escape" {
                    clear_active_context_menu(&dismiss_on_escape, window);
                }
            })
    }
}

fn render_context_menu_panel(
    menu: &ContextMenu,
    path: &[usize],
    open_submenu_path: &[usize],
    active_context_menu: Rc<RefCell<Option<AnyTooltip>>>,
    cx: &mut Context<StructuredContextMenu>,
) -> AnyElement {
    let open_submenu_index = open_submenu_path.first().copied();
    let panel_selector = context_menu_panel_selector(path);
    let mut panel = div()
        .debug_selector(move || panel_selector)
        .flex()
        .flex_col()
        .w(px(CONTEXT_MENU_PANEL_WIDTH))
        .py(px(CONTEXT_MENU_PANEL_PADDING))
        .bg(crate::hsla(0.0, 0.0, 1.0, 0.98))
        .border_1()
        .border_color(crate::hsla(0.0, 0.0, 0.78, 1.0))
        .rounded(px(10.0))
        .shadow_lg()
        .accessibility(crate::AccessibilityAttributes::new(
            crate::AccessibilityRole::Menu,
        ));

    for (index, item) in menu.items.iter().enumerate() {
        let mut item_path = path.to_vec();
        item_path.push(index);

        match item {
            ContextMenuItem::Action(item) => {
                let active_context_menu = active_context_menu.clone();
                let action = item.action.boxed_clone();
                let item_selector = context_menu_item_selector(&item_path);
                let item_id = context_menu_path_key(&item_path);
                panel = panel.child(
                    div()
                        .id(("context-menu-item", item_id))
                        .debug_selector(move || item_selector)
                        .accessibility(
                            crate::AccessibilityAttributes::new(crate::AccessibilityRole::MenuItem)
                                .label(item.label.to_string())
                                .actions(vec![crate::AccessibilityAction::Click]),
                        )
                        .flex()
                        .items_center()
                        .w(px(CONTEXT_MENU_PANEL_WIDTH))
                        .h(px(CONTEXT_MENU_ITEM_HEIGHT))
                        .px(px(12.0))
                        .rounded(px(8.0))
                        .cursor_pointer()
                        .text_sm()
                        .text_color(crate::rgb(0x171717))
                        .hover(|style| style.bg(crate::rgb(0xe8f0ff)))
                        .child(item.label.clone())
                        .on_click(move |_, window, cx| {
                            clear_active_context_menu(&active_context_menu, window);
                            window.dispatch_action(action.boxed_clone(), cx);
                        }),
                );
            }
            ContextMenuItem::Separator => {
                let separator_selector = context_menu_separator_selector(&item_path);
                panel = panel.child(
                    div()
                        .debug_selector(move || separator_selector)
                        .accessibility(crate::AccessibilityAttributes::new(
                            crate::AccessibilityRole::Separator,
                        ))
                        .flex()
                        .items_center()
                        .h(px(CONTEXT_MENU_SEPARATOR_HEIGHT))
                        .child(
                            div()
                                .mx(px(8.0))
                                .h(px(1.0))
                                .w(px(CONTEXT_MENU_PANEL_WIDTH - 16.0))
                                .bg(crate::rgb(0xe3e5ea)),
                        ),
                );
            }
            ContextMenuItem::Submenu(item) => {
                let item_selector = context_menu_item_selector(&item_path);
                let next_path = item_path.clone();
                let parent_path = path.to_vec();
                let is_open = open_submenu_index == Some(index);
                let item_id = context_menu_path_key(&item_path);
                let mut submenu_item = div()
                    .id(("context-menu-item", item_id))
                    .debug_selector(move || item_selector)
                    .accessibility(
                        crate::AccessibilityAttributes::new(crate::AccessibilityRole::MenuItem)
                            .label(item.label.to_string())
                            .states(if is_open {
                                crate::AccessibilityState::SELECTED
                                    | crate::AccessibilityState::EXPANDED
                            } else {
                                crate::AccessibilityState::COLLAPSED
                            })
                            .actions(vec![crate::AccessibilityAction::ShowMenu]),
                    )
                    .relative()
                    .flex()
                    .items_center()
                    .justify_between()
                    .w(px(CONTEXT_MENU_PANEL_WIDTH))
                    .h(px(CONTEXT_MENU_ITEM_HEIGHT))
                    .px(px(12.0))
                    .rounded(px(8.0))
                    .cursor_pointer()
                    .text_sm()
                    .text_color(crate::rgb(0x171717))
                    .bg(if is_open {
                        crate::rgb(0xe8f0ff)
                    } else {
                        crate::rgba(0x00000000)
                    })
                    .hover(|style| style.bg(crate::rgb(0xe8f0ff)))
                    .child(item.label.clone())
                    .child(div().font_weight(crate::FontWeight::SEMIBOLD).child("›"))
                    .on_click(cx.listener(move |this, _, window, _| {
                        if this.open_submenu_path == next_path {
                            this.open_submenu_path = parent_path.clone();
                        } else {
                            this.open_submenu_path = next_path.clone();
                        }
                        window.refresh();
                    }));

                if is_open {
                    submenu_item = submenu_item.child(
                        div()
                            .absolute()
                            .top_0()
                            .left(px(CONTEXT_MENU_PANEL_WIDTH + 8.0))
                            .child(render_context_menu_panel(
                                &item.menu,
                                &item_path,
                                open_submenu_path.get(1..).unwrap_or(&[]),
                                active_context_menu.clone(),
                                cx,
                            )),
                    );
                }

                panel = panel.child(submenu_item);
            }
        }
    }

    panel.into_any_element()
}

fn context_menu_panel_selector(path: &[usize]) -> String {
    format!("context-menu-panel-{}", context_menu_path_suffix(path))
}

fn context_menu_open_size(menu: &ContextMenu, open_submenu_path: &[usize]) -> Size<Pixels> {
    let mut width = px(CONTEXT_MENU_PANEL_WIDTH);
    let mut height = context_menu_panel_height(menu);

    if let Some(selected_index) = open_submenu_path.first().copied()
        && let Some(ContextMenuItem::Submenu(submenu)) = menu.items.get(selected_index)
    {
        let nested_size =
            context_menu_open_size(&submenu.menu, open_submenu_path.get(1..).unwrap_or(&[]));
        width += px(8.0) + nested_size.width;
        height = height.max(context_menu_item_offset(menu, selected_index) + nested_size.height);
    }

    Size { width, height }
}

fn context_menu_panel_height(menu: &ContextMenu) -> Pixels {
    let mut height = CONTEXT_MENU_PANEL_PADDING * 2.0;

    for item in &menu.items {
        height += match item {
            ContextMenuItem::Separator => CONTEXT_MENU_SEPARATOR_HEIGHT,
            ContextMenuItem::Action(_) | ContextMenuItem::Submenu(_) => CONTEXT_MENU_ITEM_HEIGHT,
        };
    }

    px(height)
}

fn context_menu_item_offset(menu: &ContextMenu, index: usize) -> Pixels {
    let mut offset = CONTEXT_MENU_PANEL_PADDING;

    for item in menu.items.iter().take(index) {
        offset += match item {
            ContextMenuItem::Separator => CONTEXT_MENU_SEPARATOR_HEIGHT,
            ContextMenuItem::Action(_) | ContextMenuItem::Submenu(_) => CONTEXT_MENU_ITEM_HEIGHT,
        };
    }

    px(offset)
}

fn context_menu_item_selector(path: &[usize]) -> String {
    format!("context-menu-item-{}", context_menu_path_suffix(path))
}

fn context_menu_separator_selector(path: &[usize]) -> String {
    format!("context-menu-separator-{}", context_menu_path_suffix(path))
}

fn context_menu_path_suffix(path: &[usize]) -> String {
    if path.is_empty() {
        return "root".to_string();
    }

    path.iter()
        .map(|segment| segment.to_string())
        .collect::<Vec<_>>()
        .join("-")
}

fn context_menu_path_key(path: &[usize]) -> usize {
    path.iter().fold(0, |key, segment| {
        key.saturating_mul(97)
            .saturating_add(segment.saturating_add(1))
    })
}

pub(crate) fn clear_active_tooltip(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) {
    match active_tooltip.borrow_mut().take() {
        None => {}
        Some(ActiveTooltip::WaitingForShow { .. }) => {}
        Some(ActiveTooltip::Visible { .. }) => window.refresh(),
        Some(ActiveTooltip::WaitingForHide { .. }) => window.refresh(),
    }
}

pub(crate) fn clear_active_tooltip_if_not_hoverable(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) {
    let should_clear = match active_tooltip.borrow().as_ref() {
        None => false,
        Some(ActiveTooltip::WaitingForShow { .. }) => false,
        Some(ActiveTooltip::Visible { is_hoverable, .. }) => !is_hoverable,
        Some(ActiveTooltip::WaitingForHide { .. }) => false,
    };
    if should_clear {
        active_tooltip.borrow_mut().take();
        window.refresh();
    }
}

pub(crate) fn clear_active_context_menu(
    active_context_menu: &Rc<RefCell<Option<AnyTooltip>>>,
    window: &mut Window,
) {
    if active_context_menu.borrow_mut().take().is_some() {
        window.refresh();
    }
}

pub(crate) fn set_tooltip_on_window(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    window: &mut Window,
) -> Option<TooltipId> {
    let tooltip = match active_tooltip.borrow().as_ref() {
        None => return None,
        Some(ActiveTooltip::WaitingForShow { .. }) => return None,
        Some(ActiveTooltip::Visible { tooltip, .. }) => tooltip.clone(),
        Some(ActiveTooltip::WaitingForHide { tooltip, .. }) => tooltip.clone(),
    };
    Some(window.set_tooltip(tooltip))
}

pub(crate) fn set_context_menu_on_window(
    active_context_menu: &Rc<RefCell<Option<AnyTooltip>>>,
    window: &mut Window,
) -> Option<TooltipId> {
    active_context_menu
        .borrow()
        .clone()
        .map(|context_menu| window.set_tooltip(context_menu))
}

pub(crate) fn register_tooltip_mouse_handlers(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    tooltip_id: Option<TooltipId>,
    build_tooltip: Rc<dyn Fn(&mut Window, &mut App) -> Option<(AnyView, bool)>>,
    check_is_hovered: Rc<dyn Fn(&Window) -> bool>,
    check_is_hovered_during_prepaint: Rc<dyn Fn(&Window) -> bool>,
    window: &mut Window,
) {
    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        let build_tooltip = build_tooltip.clone();
        let check_is_hovered = check_is_hovered.clone();
        move |_: &MouseMoveEvent, phase, window, cx| {
            handle_tooltip_mouse_move(
                &active_tooltip,
                &build_tooltip,
                &check_is_hovered,
                &check_is_hovered_during_prepaint,
                phase,
                window,
                cx,
            )
        }
    });

    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        move |_: &MouseDownEvent, _phase, window: &mut Window, _cx| {
            if !tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)) {
                clear_active_tooltip_if_not_hoverable(&active_tooltip, window);
            }
        }
    });

    window.on_mouse_event({
        let active_tooltip = active_tooltip.clone();
        move |_: &ScrollWheelEvent, _phase, window: &mut Window, _cx| {
            if !tooltip_id.is_some_and(|tooltip_id| tooltip_id.is_hovered(window)) {
                clear_active_tooltip_if_not_hoverable(&active_tooltip, window);
            }
        }
    });
}

pub(crate) fn register_context_menu_mouse_handlers(
    active_context_menu: &Rc<RefCell<Option<AnyTooltip>>>,
    context_menu_id: Option<TooltipId>,
    build_context_menu: Rc<
        dyn Fn(&Rc<RefCell<Option<AnyTooltip>>>, &mut Window, &mut App) -> AnyView,
    >,
    check_is_hovered: Rc<dyn Fn(&Window) -> bool>,
    window: &mut Window,
) {
    window.on_mouse_event({
        let active_context_menu = active_context_menu.clone();
        let build_context_menu = build_context_menu.clone();
        let check_is_hovered = check_is_hovered.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || window.default_prevented() {
                return;
            }

            let menu_is_hovered = context_menu_id.is_some_and(|id| id.is_hovered(window));
            if event.button == MouseButton::Right && check_is_hovered(window) {
                let active_context_menu_for_visibility = active_context_menu.clone();
                let context_menu = build_context_menu(&active_context_menu, window, cx);
                let overlay = cx.new(|_| ContextMenuOverlay {
                    active_context_menu: active_context_menu.clone(),
                    content: context_menu,
                });
                active_context_menu.borrow_mut().replace(AnyTooltip {
                    view: overlay.into(),
                    mouse_position: event.position,
                    check_visible_and_update: Rc::new(move |_, _, _| {
                        active_context_menu_for_visibility.borrow().is_some()
                    }),
                });
                window.refresh();
            } else if !menu_is_hovered {
                clear_active_context_menu(&active_context_menu, window);
            }
        }
    });

    window.on_mouse_event({
        let active_context_menu = active_context_menu.clone();
        move |event: &MouseUpEvent, phase, window, _cx| {
            let menu_is_hovered = context_menu_id.is_some_and(|id| id.is_hovered(window));
            if phase == DispatchPhase::Bubble
                && event.button != MouseButton::Right
                && active_context_menu.borrow().is_some()
                && !menu_is_hovered
            {
                clear_active_context_menu(&active_context_menu, window);
            }
        }
    });

    window.on_mouse_event({
        let active_context_menu = active_context_menu.clone();
        move |_: &ScrollWheelEvent, _phase, window, _cx| {
            clear_active_context_menu(&active_context_menu, window);
        }
    });

    window.on_key_event({
        let active_context_menu = active_context_menu.clone();
        move |event: &KeyDownEvent, phase, window, _cx| {
            if phase == DispatchPhase::Bubble && event.keystroke.key == "escape" {
                clear_active_context_menu(&active_context_menu, window);
            }
        }
    });
}

/// Handles displaying tooltips when an element is hovered.
///
/// The mouse hovering logic also relies on being called from window prepaint in order to handle the
/// case where the element the tooltip is on is not rendered - in that case its mouse listeners are
/// also not registered. During window prepaint, the hitbox information is not available, so
/// `check_is_hovered_during_prepaint` is used which bases the check off of the absolute bounds of
/// the element.
///
/// TODO: There's a minor bug due to the use of absolute bounds while checking during prepaint - it
/// does not know if the hitbox is occluded. In the case where a tooltip gets displayed and then
/// gets occluded after display, it will stick around until the mouse exits the hover bounds.
fn handle_tooltip_mouse_move(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    build_tooltip: &Rc<dyn Fn(&mut Window, &mut App) -> Option<(AnyView, bool)>>,
    check_is_hovered: &Rc<dyn Fn(&Window) -> bool>,
    check_is_hovered_during_prepaint: &Rc<dyn Fn(&Window) -> bool>,
    phase: DispatchPhase,
    window: &mut Window,
    cx: &mut App,
) {
    // Separates logic for what mutation should occur from applying it, to avoid overlapping
    // RefCell borrows.
    enum Action {
        None,
        CancelShow,
        ScheduleShow,
    }

    let action = match active_tooltip.borrow().as_ref() {
        None => {
            let is_hovered = check_is_hovered(window);
            if is_hovered && phase.bubble() {
                Action::ScheduleShow
            } else {
                Action::None
            }
        }
        Some(ActiveTooltip::WaitingForShow { .. }) => {
            let is_hovered = check_is_hovered(window);
            if is_hovered {
                Action::None
            } else {
                Action::CancelShow
            }
        }
        // These are handled in check_visible_and_update.
        Some(ActiveTooltip::Visible { .. }) | Some(ActiveTooltip::WaitingForHide { .. }) => {
            Action::None
        }
    };

    match action {
        Action::None => {}
        Action::CancelShow => {
            // Cancel waiting to show tooltip when it is no longer hovered.
            active_tooltip.borrow_mut().take();
        }
        Action::ScheduleShow => {
            let delayed_show_task = window.spawn(cx, {
                let active_tooltip = active_tooltip.clone();
                let build_tooltip = build_tooltip.clone();
                let check_is_hovered_during_prepaint = check_is_hovered_during_prepaint.clone();
                async move |cx| {
                    cx.background_executor().timer(TOOLTIP_SHOW_DELAY).await;
                    cx.update(|window, cx| {
                        let new_tooltip =
                            build_tooltip(window, cx).map(|(view, tooltip_is_hoverable)| {
                                let active_tooltip = active_tooltip.clone();
                                ActiveTooltip::Visible {
                                    tooltip: AnyTooltip {
                                        view,
                                        mouse_position: window.mouse_position(),
                                        check_visible_and_update: Rc::new(
                                            move |tooltip_bounds, window, cx| {
                                                handle_tooltip_check_visible_and_update(
                                                    &active_tooltip,
                                                    tooltip_is_hoverable,
                                                    &check_is_hovered_during_prepaint,
                                                    tooltip_bounds,
                                                    window,
                                                    cx,
                                                )
                                            },
                                        ),
                                    },
                                    is_hoverable: tooltip_is_hoverable,
                                }
                            });
                        *active_tooltip.borrow_mut() = new_tooltip;
                        window.refresh();
                    })
                    .ok();
                }
            });
            active_tooltip
                .borrow_mut()
                .replace(ActiveTooltip::WaitingForShow {
                    _task: delayed_show_task,
                });
        }
    }
}

/// Returns a callback which will be called by window prepaint to update tooltip visibility. The
/// purpose of doing this logic here instead of the mouse move handler is that the mouse move
/// handler won't get called when the element is not painted (e.g. via use of `visible_on_hover`).
fn handle_tooltip_check_visible_and_update(
    active_tooltip: &Rc<RefCell<Option<ActiveTooltip>>>,
    tooltip_is_hoverable: bool,
    check_is_hovered: &Rc<dyn Fn(&Window) -> bool>,
    tooltip_bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    // Separates logic for what mutation should occur from applying it, to avoid overlapping RefCell
    // borrows.
    enum Action {
        None,
        Hide,
        ScheduleHide(AnyTooltip),
        CancelHide(AnyTooltip),
    }

    let is_hovered = check_is_hovered(window)
        || (tooltip_is_hoverable && tooltip_bounds.contains(&window.mouse_position()));
    let action = match active_tooltip.borrow().as_ref() {
        Some(ActiveTooltip::Visible { tooltip, .. }) => {
            if is_hovered {
                Action::None
            } else {
                if tooltip_is_hoverable {
                    Action::ScheduleHide(tooltip.clone())
                } else {
                    Action::Hide
                }
            }
        }
        Some(ActiveTooltip::WaitingForHide { tooltip, .. }) => {
            if is_hovered {
                Action::CancelHide(tooltip.clone())
            } else {
                Action::None
            }
        }
        None | Some(ActiveTooltip::WaitingForShow { .. }) => Action::None,
    };

    match action {
        Action::None => {}
        Action::Hide => clear_active_tooltip(active_tooltip, window),
        Action::ScheduleHide(tooltip) => {
            let delayed_hide_task = window.spawn(cx, {
                let active_tooltip = active_tooltip.clone();
                async move |cx| {
                    cx.background_executor()
                        .timer(HOVERABLE_TOOLTIP_HIDE_DELAY)
                        .await;
                    if active_tooltip.borrow_mut().take().is_some() {
                        cx.update(|window, _cx| window.refresh()).ok();
                    }
                }
            });
            active_tooltip
                .borrow_mut()
                .replace(ActiveTooltip::WaitingForHide {
                    tooltip,
                    _task: delayed_hide_task,
                });
        }
        Action::CancelHide(tooltip) => {
            // Cancel waiting to hide tooltip when it becomes hovered.
            active_tooltip.borrow_mut().replace(ActiveTooltip::Visible {
                tooltip,
                is_hoverable: true,
            });
        }
    }

    active_tooltip.borrow().is_some()
}

#[derive(Default)]
pub(crate) struct GroupHitboxes(HashMap<SharedString, SmallVec<[HitboxId; 1]>>);

impl Global for GroupHitboxes {}

impl GroupHitboxes {
    pub fn get(name: &SharedString, cx: &mut App) -> Option<HitboxId> {
        cx.default_global::<Self>()
            .0
            .get(name)
            .and_then(|bounds_stack| bounds_stack.last())
            .cloned()
    }

    pub fn push(name: SharedString, hitbox_id: HitboxId, cx: &mut App) {
        cx.default_global::<Self>()
            .0
            .entry(name)
            .or_default()
            .push(hitbox_id);
    }

    pub fn pop(name: &SharedString, cx: &mut App) {
        cx.default_global::<Self>().0.get_mut(name).unwrap().pop();
    }
}

/// A wrapper around an element that can store state, produced after assigning an ElementId.
pub struct Stateful<E> {
    pub(crate) element: E,
}

impl<E> Styled for Stateful<E>
where
    E: Styled,
{
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E> StatefulInteractiveElement for Stateful<E>
where
    E: Element,
    Self: InteractiveElement,
{
}

impl<E> InteractiveElement for Stateful<E>
where
    E: InteractiveElement,
{
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}

impl<E> Element for Stateful<E>
where
    E: Element,
{
    type RequestLayoutState = E::RequestLayoutState;
    type PrepaintState = E::PrepaintState;

    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        self.element.source_location()
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.element.request_layout(id, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> E::PrepaintState {
        self.element
            .prepaint(id, inspector_id, bounds, state, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element.paint(
            id,
            inspector_id,
            bounds,
            request_layout,
            prepaint,
            window,
            cx,
        );
    }
}

impl<E> IntoElement for Stateful<E>
where
    E: Element,
{
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E> ParentElement for Stateful<E>
where
    E: ParentElement,
{
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.element.extend(elements)
    }
}

/// Represents an element that can be scrolled *to* in its parent element.
/// Contrary to `ScrollHandle::scroll_to_active_item`, an anchored element does not have to be an immediate child of the parent.
#[derive(Clone)]
pub struct ScrollAnchor {
    handle: ScrollHandle,
    last_origin: Rc<RefCell<Point<Pixels>>>,
}

impl ScrollAnchor {
    /// Creates a [ScrollAnchor] associated with a given [ScrollHandle].
    pub fn for_handle(handle: ScrollHandle) -> Self {
        Self {
            handle,
            last_origin: Default::default(),
        }
    }
    /// Request scroll to this item on the next frame.
    pub fn scroll_to(&self, window: &mut Window, _cx: &mut App) {
        let this = self.clone();

        window.on_next_frame(move |_, _| {
            let viewport_bounds = this.handle.bounds();
            let self_bounds = *this.last_origin.borrow();
            this.handle.set_offset(viewport_bounds.origin - self_bounds);
        });
    }
}

#[derive(Default, Debug)]
pub(crate) struct ScrollElasticState {
    max_offset: Size<Pixels>,
    overscroll: Point<Pixels>,
    animating: bool,
    last_scrolled: Option<std::time::Instant>,
    last_advance: Option<std::time::Instant>,
}

impl ScrollElasticState {
    fn reset(&mut self) {
        self.overscroll = Point::default();
        self.animating = false;
    }

    pub(crate) fn mark_scrolled(&mut self) {
        self.last_scrolled = Some(std::time::Instant::now());
    }

    pub(crate) fn seconds_since_last_scroll(&self) -> f64 {
        self.last_scrolled
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(f64::MAX)
    }
}

fn uses_scroll_state(overflow: Overflow) -> bool {
    matches!(overflow, Overflow::Scroll | Overflow::Auto)
}

fn axis_is_scrollable(overflow: Overflow, max_offset: Pixels) -> bool {
    match overflow {
        Overflow::Scroll | Overflow::Auto => max_offset > Pixels::ZERO,
        Overflow::Visible | Overflow::Clip | Overflow::Hidden => false,
    }
}

fn prepaint_auto_scrollbar_hitboxes(
    scroll_handle: Option<&ScrollHandle>,
    bounds: Bounds<Pixels>,
    style: &Style,
    window: &mut Window,
) -> AutoScrollbarHitboxes {
    let Some(scroll_handle) = scroll_handle else {
        return AutoScrollbarHitboxes::default();
    };

    let max_offset = scroll_handle.max_offset();
    let show_y = axis_is_scrollable(style.overflow.y, max_offset.height);
    let show_x = axis_is_scrollable(style.overflow.x, max_offset.width);

    AutoScrollbarHitboxes {
        vertical: show_y.then(|| {
            window.insert_hitbox(
                auto_scrollbar_track_bounds(bounds, true, show_x),
                HitboxBehavior::Normal,
            )
        }),
        horizontal: show_x.then(|| {
            window.insert_hitbox(
                auto_scrollbar_track_bounds(bounds, false, show_y),
                HitboxBehavior::Normal,
            )
        }),
    }
}

const AUTO_SCROLLBAR_HITBOX_WIDTH: Pixels = px(12.0);
const AUTO_SCROLLBAR_THUMB_WIDTH: Pixels = px(7.0);
const AUTO_SCROLLBAR_EDGE_MARGIN: Pixels = px(3.0);
const AUTO_SCROLLBAR_END_MARGIN: Pixels = px(4.0);

fn auto_scrollbar_track_bounds(
    bounds: Bounds<Pixels>,
    vertical: bool,
    other_axis_visible: bool,
) -> Bounds<Pixels> {
    let other_axis_reserved = if other_axis_visible {
        AUTO_SCROLLBAR_HITBOX_WIDTH
    } else {
        Pixels::ZERO
    };

    if vertical {
        Bounds::new(
            point(bounds.right() - AUTO_SCROLLBAR_HITBOX_WIDTH, bounds.top()),
            size(
                AUTO_SCROLLBAR_HITBOX_WIDTH,
                bounds.size.height - other_axis_reserved,
            ),
        )
    } else {
        Bounds::new(
            point(bounds.left(), bounds.bottom() - AUTO_SCROLLBAR_HITBOX_WIDTH),
            size(
                bounds.size.width - other_axis_reserved,
                AUTO_SCROLLBAR_HITBOX_WIDTH,
            ),
        )
    }
}

fn auto_scrollbar_thumb_bounds(
    track_bounds: Bounds<Pixels>,
    scroll_handle: &ScrollHandle,
    vertical: bool,
) -> Bounds<Pixels> {
    let offset = scroll_handle.offset();
    let max_offset = scroll_handle.max_offset();
    let viewport = axis_pixels(track_bounds.size, vertical);
    let max_axis_offset = logical_axis_size(max_offset, vertical);
    let content = viewport + max_axis_offset;
    let thumb_ratio = if content > Pixels::ZERO {
        (viewport.0 / content.0).clamp(0.05, 1.0)
    } else {
        1.0
    };
    let track_length = (viewport - AUTO_SCROLLBAR_END_MARGIN * 2.0).max(Pixels::ZERO);
    let thumb_length = (track_length * thumb_ratio).max(px(20.0)).min(track_length);
    let available = (track_length - thumb_length).max(Pixels::ZERO);
    let logical_offset = logical_scroll_offset(offset, vertical);
    let fraction = if max_axis_offset > Pixels::ZERO {
        (logical_offset.0 / max_axis_offset.0).clamp(0.0, 1.0)
    } else {
        0.0
    };

    if vertical {
        Bounds::new(
            point(
                track_bounds.right() - AUTO_SCROLLBAR_THUMB_WIDTH - AUTO_SCROLLBAR_EDGE_MARGIN,
                track_bounds.top() + AUTO_SCROLLBAR_END_MARGIN + available * fraction,
            ),
            size(AUTO_SCROLLBAR_THUMB_WIDTH, thumb_length),
        )
    } else {
        Bounds::new(
            point(
                track_bounds.left() + AUTO_SCROLLBAR_END_MARGIN + available * fraction,
                track_bounds.bottom() - AUTO_SCROLLBAR_THUMB_WIDTH - AUTO_SCROLLBAR_EDGE_MARGIN,
            ),
            size(thumb_length, AUTO_SCROLLBAR_THUMB_WIDTH),
        )
    }
}

fn auto_scrollbar_logical_offset(scroll_handle: &ScrollHandle, vertical: bool) -> Pixels {
    logical_scroll_offset(scroll_handle.offset(), vertical)
}

fn axis_pixels(size: Size<Pixels>, vertical: bool) -> Pixels {
    if vertical { size.height } else { size.width }
}

fn logical_axis_size(size: Size<Pixels>, vertical: bool) -> Pixels {
    axis_pixels(size, vertical)
}

fn logical_scroll_offset(offset: Point<Pixels>, vertical: bool) -> Pixels {
    if vertical { -offset.y } else { -offset.x }
}

fn clamp_pixels(value: Pixels, min: Pixels, max: Pixels) -> Pixels {
    value.max(min).min(max)
}

fn auto_scrollbar_logical_offset_for_position(
    position: Point<Pixels>,
    track_bounds: Bounds<Pixels>,
    scroll_handle: &ScrollHandle,
    vertical: bool,
    center_thumb: bool,
) -> Pixels {
    let max_offset = logical_axis_size(scroll_handle.max_offset(), vertical);
    if max_offset <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let track_length = (axis_pixels(track_bounds.size, vertical) - AUTO_SCROLLBAR_END_MARGIN * 2.0)
        .max(Pixels::ZERO);
    let thumb_length = axis_pixels(
        auto_scrollbar_thumb_bounds(track_bounds, scroll_handle, vertical).size,
        vertical,
    );
    let available = (track_length - thumb_length).max(Pixels::ZERO);
    if available <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let pointer = if vertical {
        position.y - track_bounds.top() - AUTO_SCROLLBAR_END_MARGIN
    } else {
        position.x - track_bounds.left() - AUTO_SCROLLBAR_END_MARGIN
    };
    let anchor = if center_thumb {
        pointer - thumb_length / 2.0
    } else {
        pointer
    };
    let fraction = clamp_pixels(anchor, Pixels::ZERO, available).0 / available.0;
    max_offset * fraction
}

fn auto_scrollbar_logical_offset_for_drag(
    position: Point<Pixels>,
    track_bounds: Bounds<Pixels>,
    scroll_handle: &ScrollHandle,
    vertical: bool,
    drag_state: AutoScrollbarDragState,
) -> Pixels {
    let max_offset = logical_axis_size(scroll_handle.max_offset(), vertical);
    if max_offset <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let track_length = (axis_pixels(track_bounds.size, vertical) - AUTO_SCROLLBAR_END_MARGIN * 2.0)
        .max(Pixels::ZERO);
    let thumb_length = axis_pixels(
        auto_scrollbar_thumb_bounds(track_bounds, scroll_handle, vertical).size,
        vertical,
    );
    let available = (track_length - thumb_length).max(Pixels::ZERO);
    if available <= Pixels::ZERO {
        return Pixels::ZERO;
    }

    let delta = if vertical {
        position.y - drag_state.start_position.y
    } else {
        position.x - drag_state.start_position.x
    };
    clamp_pixels(
        drag_state.start_logical_offset + max_offset * (delta.0 / available.0),
        Pixels::ZERO,
        max_offset,
    )
}

fn set_auto_scrollbar_logical_offset(
    scroll_handle: &ScrollHandle,
    vertical: bool,
    logical_offset: Pixels,
) {
    let mut offset = scroll_handle.offset();
    if vertical {
        offset.y = -logical_offset;
    } else {
        offset.x = -logical_offset;
    }
    scroll_handle.set_offset(offset);
    scroll_handle
        .0
        .borrow()
        .elastic
        .borrow_mut()
        .mark_scrolled();
}

#[derive(Default, Debug)]
struct ScrollHandleState {
    offset: Rc<RefCell<Point<Pixels>>>,
    elastic: Rc<RefCell<ScrollElasticState>>,
    bounds: Bounds<Pixels>,
    max_offset: Size<Pixels>,
    child_bounds: Vec<Bounds<Pixels>>,
    scroll_to_bottom: bool,
    overflow: Point<Overflow>,
    active_item: Option<ScrollActiveItem>,
    always_show_scrollbars: bool,
}

#[derive(Default, Debug, Clone, Copy)]
struct ScrollActiveItem {
    index: usize,
    strategy: ScrollStrategy,
}

#[derive(Default, Debug, Clone, Copy)]
enum ScrollStrategy {
    #[default]
    FirstVisible,
    Top,
}

/// A handle to the scrollable aspects of an element.
/// Used for accessing scroll state, like the current scroll offset,
/// and for mutating the scroll state, like scrolling to a specific child.
#[derive(Clone, Debug)]
pub struct ScrollHandle(Rc<RefCell<ScrollHandleState>>);

impl Default for ScrollHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollHandle {
    /// Construct a new scroll handle.
    pub fn new() -> Self {
        Self(Rc::default())
    }

    /// Seconds since the last scroll event, or `f64::MAX` if never scrolled.
    pub fn seconds_since_last_scroll(&self) -> f64 {
        self.0.borrow().elastic.borrow().seconds_since_last_scroll()
    }

    /// Get the current scroll offset.
    pub fn offset(&self) -> Point<Pixels> {
        *self.0.borrow().offset.borrow()
    }

    /// Get the maximum scroll offset.
    pub fn max_offset(&self) -> Size<Pixels> {
        self.0.borrow().max_offset
    }

    /// Get the top child that's scrolled into view.
    pub fn top_item(&self) -> usize {
        let state = self.0.borrow();
        let top = state.bounds.top() - state.offset.borrow().y;

        match state.child_bounds.binary_search_by(|bounds| {
            if top < bounds.top() {
                Ordering::Greater
            } else if top > bounds.bottom() {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }) {
            Ok(ix) => ix,
            Err(ix) => ix.min(state.child_bounds.len().saturating_sub(1)),
        }
    }

    /// Get the bottom child that's scrolled into view.
    pub fn bottom_item(&self) -> usize {
        let state = self.0.borrow();
        let bottom = state.bounds.bottom() - state.offset.borrow().y;

        match state.child_bounds.binary_search_by(|bounds| {
            if bottom < bounds.top() {
                Ordering::Greater
            } else if bottom > bounds.bottom() {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        }) {
            Ok(ix) => ix,
            Err(ix) => ix.min(state.child_bounds.len().saturating_sub(1)),
        }
    }

    /// Return the bounds into which this child is painted
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.0.borrow().bounds
    }

    /// Get the bounds for a specific child.
    pub fn bounds_for_item(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.0.borrow().child_bounds.get(ix).cloned()
    }

    /// Update `ScrollHandleState`'s active item for scrolling to in prepaint
    pub fn scroll_to_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        state.active_item = Some(ScrollActiveItem {
            index: ix,
            strategy: ScrollStrategy::default(),
        });
    }

    /// Make scrollbars always visible instead of auto-hiding after idle.
    pub fn always_show_scrollbars(self) -> Self {
        self.0.borrow_mut().always_show_scrollbars = true;
        self
    }

    /// Scroll the minimal amount to ensure the child is the first visible element.
    pub fn scroll_to_top_of_item(&self, ix: usize) {
        let mut state = self.0.borrow_mut();
        state.active_item = Some(ScrollActiveItem {
            index: ix,
            strategy: ScrollStrategy::Top,
        });
    }

    /// Scrolls the minimal amount to either ensure that the child is
    /// fully visible or the top element of the view depends on the
    /// scroll strategy
    fn scroll_to_active_item(&self) {
        let mut state = self.0.borrow_mut();

        let Some(active_item) = state.active_item else {
            return;
        };

        let active_item = match state.child_bounds.get(active_item.index) {
            Some(bounds) => {
                let mut scroll_offset = state.offset.borrow_mut();

                match active_item.strategy {
                    ScrollStrategy::FirstVisible => {
                        if axis_is_scrollable(state.overflow.y, state.max_offset.height) {
                            if bounds.top() + scroll_offset.y < state.bounds.top() {
                                scroll_offset.y = state.bounds.top() - bounds.top();
                            } else if bounds.bottom() + scroll_offset.y > state.bounds.bottom() {
                                scroll_offset.y = state.bounds.bottom() - bounds.bottom();
                            }
                        }
                    }
                    ScrollStrategy::Top => {
                        scroll_offset.y = state.bounds.top() - bounds.top();
                    }
                }

                if axis_is_scrollable(state.overflow.x, state.max_offset.width) {
                    if bounds.left() + scroll_offset.x < state.bounds.left() {
                        scroll_offset.x = state.bounds.left() - bounds.left();
                    } else if bounds.right() + scroll_offset.x > state.bounds.right() {
                        scroll_offset.x = state.bounds.right() - bounds.right();
                    }
                }
                state.elastic.borrow_mut().reset();
                None
            }
            None => Some(active_item),
        };
        state.active_item = active_item;
    }

    /// Scrolls to the bottom.
    pub fn scroll_to_bottom(&self) {
        let mut state = self.0.borrow_mut();
        state.scroll_to_bottom = true;
    }

    /// Set the offset explicitly. The offset is the distance from the top left of the
    /// parent container to the top left of the first child.
    /// As you scroll further down the offset becomes more negative.
    pub fn set_offset(&self, mut position: Point<Pixels>) {
        let state = self.0.borrow();
        *state.offset.borrow_mut() = position;
        state.elastic.borrow_mut().reset();
    }

    /// Get the logical scroll top, based on a child index and a pixel offset.
    pub fn logical_scroll_top(&self) -> (usize, Pixels) {
        let ix = self.top_item();
        let state = self.0.borrow();

        if let Some(child_bounds) = state.child_bounds.get(ix) {
            (
                ix,
                child_bounds.top() + state.offset.borrow().y - state.bounds.top(),
            )
        } else {
            (ix, px(0.))
        }
    }

    /// Get the logical scroll bottom, based on a child index and a pixel offset.
    pub fn logical_scroll_bottom(&self) -> (usize, Pixels) {
        let ix = self.bottom_item();
        let state = self.0.borrow();

        if let Some(child_bounds) = state.child_bounds.get(ix) {
            (
                ix,
                child_bounds.bottom() + state.offset.borrow().y - state.bounds.bottom(),
            )
        } else {
            (ix, px(0.))
        }
    }

    /// Get the count of children for scrollable item.
    pub fn children_count(&self) -> usize {
        self.0.borrow().child_bounds.len()
    }
}

#[cfg(test)]
mod test {
    use super::{
        ImplicitStyleAnimationState, ImplicitVisualStyle, TOOLTIP_SHOW_DELAY, TransitionConfig,
    };
    use crate::scroll_elasticity::{
        add_scroll_elasticity, advance_scroll_elasticity, apply_scroll_delta_axis,
    };
    use crate::{AbsoluteLength, BoxShadow};
    use crate::{
        AccessibilityAttributes, AccessibilityRole, AccessibilityState, AppContext, Context,
        FocusHandle, InteractiveElement, Interactivity, MouseButton, MouseDownEvent,
        MouseMoveEvent, MouseUpEvent, PanState, ParentElement, PinchState, Render, Rgba,
        ScrollDelta, ScrollHandle, ScrollWheelEvent, StatefulInteractiveElement, StyleRefinement,
        Styled, SwipeDirection, TestAppContext, VisualContext, Window, div, point, px,
    };
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        time::{Duration, Instant},
    };

    crate::actions!(context_menu_test, [PrimaryMenuAction, ShareViaLinkAction]);

    #[test]
    fn scroll_elasticity_consumes_reverse_delta_before_scrolling_content() {
        let mut offset = px(0.0);
        let mut overscroll = px(18.0);

        apply_scroll_delta_axis(&mut offset, &mut overscroll, px(120.0), px(-30.0), true);

        assert_eq!(overscroll, px(0.0));
        assert_eq!(offset, px(-12.0));
    }

    #[test]
    fn scroll_elasticity_resists_boundary_overscroll() {
        let overscroll = add_scroll_elasticity(px(0.0), px(30.0));

        assert!(overscroll > px(0.0));
        assert!(overscroll < px(30.0));
    }

    #[test]
    fn scroll_elasticity_animates_back_to_zero() {
        use std::time::{Duration, Instant};

        let mut overscroll = px(40.0);
        let mut last_advance = None;
        let frame_interval = Duration::from_millis(16);

        for _ in 0..40 {
            last_advance = last_advance
                .map(|t: Instant| t - frame_interval)
                .or(Some(Instant::now() - frame_interval));
            if !advance_scroll_elasticity(&mut overscroll, &mut last_advance) {
                break;
            }
        }

        assert_eq!(overscroll, px(0.0));
    }

    #[kael::test]
    fn horizontal_div_scroll_updates_offset_and_child_bounds(cx: &mut TestAppContext) {
        let scroll_handle = ScrollHandle::new();

        struct TestView(ScrollHandle);

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div()
                    .w(px(100.))
                    .h(px(100.))
                    .id("horizontal-scroll-test")
                    .overflow_x_scroll()
                    .track_scroll(&self.0)
                    .child(
                        div()
                            .w(px(300.))
                            .h(px(40.))
                            .debug_selector(|| "horizontal-child".to_string()),
                    )
            }
        }

        let (_view, cx) = cx.add_window_view(|_, _| TestView(scroll_handle.clone()));

        let initial_bounds = cx.debug_bounds("horizontal-child").unwrap();
        assert_eq!(initial_bounds.left(), px(0.));
        assert_eq!(scroll_handle.offset().x, px(0.));

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(20.)),
            delta: ScrollDelta::Pixels(point(px(-40.), px(0.))),
            ..Default::default()
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        let scrolled_bounds = cx.debug_bounds("horizontal-child").unwrap();
        assert_eq!(scroll_handle.offset().x, px(-40.));
        assert_eq!(scrolled_bounds.left(), px(-40.));
    }

    #[kael::test]
    fn scroll_restarts_frame_polling_after_idle(cx: &mut TestAppContext) {
        let scroll_handle = ScrollHandle::new();

        struct TestView(ScrollHandle);

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div()
                    .w(px(100.))
                    .h(px(100.))
                    .id("idle-scroll-test")
                    .overflow_y_scroll()
                    .track_scroll(&self.0)
                    .child(div().w(px(100.)).h(px(300.)))
            }
        }

        let (_view, mut cx) = cx.add_window_view(|_, _| TestView(scroll_handle.clone()));
        let test_window = cx.test_window(cx.window_handle());
        test_window.0.lock().frame_polling_active = false;

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(50.)),
            delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
            ..Default::default()
        });

        assert_eq!(scroll_handle.offset().y, px(-40.));
        assert!(test_window.0.lock().frame_polling_active);
    }

    #[kael::test]
    fn auto_scrollbar_drag_updates_scroll_offset(cx: &mut TestAppContext) {
        let scroll_handle = ScrollHandle::new();

        struct TestView(ScrollHandle);

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div()
                    .w(px(100.))
                    .h(px(100.))
                    .id("auto-scrollbar-drag-test")
                    .overflow_y_scroll()
                    .track_scroll(&self.0)
                    .child(div().w(px(100.)).h(px(300.)))
            }
        }

        let (_view, mut cx) = cx.add_window_view(|_, _| TestView(scroll_handle.clone()));

        cx.simulate_event(MouseMoveEvent {
            position: point(px(96.), px(10.)),
            ..Default::default()
        });
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(96.), px(10.)),
            click_count: 1,
            ..Default::default()
        });
        cx.simulate_event(MouseMoveEvent {
            position: point(px(96.), px(40.)),
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        });
        cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(96.), px(40.)),
            click_count: 1,
            ..Default::default()
        });

        assert!(scroll_handle.offset().y < px(-80.));
    }

    #[kael::test]
    fn horizontal_div_overflow_auto_scrolls_when_content_overflows(cx: &mut TestAppContext) {
        let scroll_handle = ScrollHandle::new();

        struct TestView(ScrollHandle);

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div()
                    .w(px(100.))
                    .h(px(100.))
                    .id("horizontal-auto-scroll-test")
                    .overflow_x_auto()
                    .track_scroll(&self.0)
                    .child(
                        div()
                            .w(px(300.))
                            .h(px(40.))
                            .debug_selector(|| "horizontal-auto-child".to_string()),
                    )
            }
        }

        let (_view, cx) = cx.add_window_view(|_, _| TestView(scroll_handle.clone()));

        let initial_bounds = cx.debug_bounds("horizontal-auto-child").unwrap();
        assert_eq!(initial_bounds.left(), px(0.));
        assert_eq!(scroll_handle.offset().x, px(0.));

        cx.simulate_event(ScrollWheelEvent {
            position: point(px(50.), px(20.)),
            delta: ScrollDelta::Pixels(point(px(-40.), px(0.))),
            ..Default::default()
        });

        cx.update(|window, cx| {
            window.draw(cx).clear();
        });

        let scrolled_bounds = cx.debug_bounds("horizontal-auto-child").unwrap();
        assert_eq!(scroll_handle.offset().x, px(-40.));
        assert_eq!(scrolled_bounds.left(), px(-40.));
    }

    #[test]
    fn implicit_transitions_interpolate_hover_background() {
        let mut state = ImplicitStyleAnimationState::default();
        let config = TransitionConfig::default();
        let start = Instant::now();

        let mut from_style = crate::Style::default();
        from_style.background = Some(crate::rgb(0xff0000).into());
        let (initial, initial_needs_frame) = state.resolve(from_style, start, true, &config);
        let initial_color = Rgba::from(
            ImplicitVisualStyle::from(&initial)
                .background
                .expect("missing initial background")
                .solid,
        );
        assert!(initial_color.r > 0.95);
        assert!(initial_color.b < 0.05);
        assert!(!initial_needs_frame);

        let mut to_style = crate::Style::default();
        to_style.background = Some(crate::rgb(0x0000ff).into());
        let (first_frame, first_frame_needs_frame) =
            state.resolve(to_style.clone(), start, true, &config);
        let first_frame_color = Rgba::from(
            ImplicitVisualStyle::from(&first_frame)
                .background
                .expect("missing transition background")
                .solid,
        );
        assert!(first_frame_color.r > 0.95);
        assert!(first_frame_color.b < 0.05);
        assert!(first_frame_needs_frame);

        let (interpolated, interpolated_needs_frame) =
            state.resolve(to_style, start + Duration::from_millis(75), true, &config);
        let interpolated_color = Rgba::from(
            ImplicitVisualStyle::from(&interpolated)
                .background
                .expect("missing interpolated background")
                .solid,
        );
        assert!(interpolated_color.r < 0.95);
        assert!(interpolated_color.b > 0.05);
        assert!(interpolated_needs_frame);
    }

    #[test]
    fn implicit_transitions_snap_when_animations_are_disabled() {
        let mut state = ImplicitStyleAnimationState::default();
        let config = TransitionConfig::default();
        let start = Instant::now();

        let mut from_style = crate::Style::default();
        from_style.background = Some(crate::rgb(0xff0000).into());
        let _ = state.resolve(from_style, start, true, &config);

        let mut to_style = crate::Style::default();
        to_style.background = Some(crate::rgb(0x0000ff).into());
        let (resolved, needs_animation_frame) =
            state.resolve(to_style.clone(), start, false, &config);

        assert!(ImplicitVisualStyle::from(&resolved) == ImplicitVisualStyle::from(&to_style));
        assert!(!needs_animation_frame);
        assert!(state.active_transition.is_none());
    }

    #[test]
    fn flip_layout_state_glides_between_origins() {
        let mut state = super::FlipLayoutState::default();
        let config = TransitionConfig {
            duration: Duration::from_millis(200),
            easing: Some(crate::animation::Easing::Linear),
        };
        let start = Instant::now();

        let (offset, needs_frame) = state.resolve(point(px(0.), px(0.)), start, true, &config);
        assert!(offset.is_none());
        assert!(!needs_frame);

        let (offset, needs_frame) = state.resolve(point(px(100.), px(40.)), start, true, &config);
        let offset = offset.expect("expected flip offset after move");
        assert!((offset.x.0 + 100.).abs() < 0.01);
        assert!((offset.y.0 + 40.).abs() < 0.01);
        assert!(needs_frame);

        let (offset, _) = state.resolve(
            point(px(100.), px(40.)),
            start + Duration::from_millis(100),
            true,
            &config,
        );
        let offset = offset.expect("expected mid-flight offset");
        assert!((offset.x.0 + 50.).abs() < 1.0);

        let (offset, needs_frame) = state.resolve(
            point(px(100.), px(40.)),
            start + Duration::from_millis(250),
            true,
            &config,
        );
        assert!(offset.is_none());
        assert!(!needs_frame);
    }

    #[test]
    fn flip_layout_state_snaps_when_animations_disabled() {
        let mut state = super::FlipLayoutState::default();
        let config = TransitionConfig::default();
        let start = Instant::now();

        let _ = state.resolve(point(px(0.), px(0.)), start, false, &config);
        let (offset, needs_frame) = state.resolve(point(px(50.), px(0.)), start, false, &config);
        assert!(offset.is_none());
        assert!(!needs_frame);
    }

    #[test]
    fn implicit_transitions_honor_custom_duration() {
        let mut state = ImplicitStyleAnimationState::default();
        let config = TransitionConfig {
            duration: Duration::from_millis(600),
            easing: None,
        };
        let start = Instant::now();

        let mut from_style = crate::Style::default();
        from_style.opacity = Some(0.0);
        let _ = state.resolve(from_style, start, true, &config);

        let mut to_style = crate::Style::default();
        to_style.opacity = Some(1.0);
        let _ = state.resolve(to_style.clone(), start, true, &config);

        let (mid, mid_needs_frame) = state.resolve(
            to_style.clone(),
            start + Duration::from_millis(300),
            true,
            &config,
        );
        let mid_opacity = mid.opacity.unwrap_or(1.0);
        assert!(mid_opacity > 0.0 && mid_opacity < 1.0);
        assert!(mid_needs_frame);

        let (done, done_needs_frame) =
            state.resolve(to_style, start + Duration::from_millis(700), true, &config);
        assert!(done.opacity.is_none() || done.opacity == Some(1.0));
        assert!(!done_needs_frame);
    }

    #[test]
    fn implicit_transitions_use_linear_easing_when_configured() {
        let mut state = ImplicitStyleAnimationState::default();
        let config = TransitionConfig {
            duration: Duration::from_millis(100),
            easing: Some(crate::animation::Easing::Linear),
        };
        let start = Instant::now();

        let mut from_style = crate::Style::default();
        from_style.opacity = Some(0.0);
        let _ = state.resolve(from_style, start, true, &config);

        let mut to_style = crate::Style::default();
        to_style.opacity = Some(1.0);
        let _ = state.resolve(to_style.clone(), start, true, &config);

        let (mid, _) = state.resolve(to_style, start + Duration::from_millis(50), true, &config);
        let mid_opacity = mid.opacity.expect("expected mid-transition opacity");
        assert!((mid_opacity - 0.5).abs() < 0.01);
    }

    #[test]
    fn implicit_transitions_interpolate_corner_radii_shadows_and_text_color() {
        let mut state = ImplicitStyleAnimationState::default();
        let config = TransitionConfig {
            duration: Duration::from_millis(100),
            easing: Some(crate::animation::Easing::Linear),
        };
        let start = Instant::now();

        let mut from_style = crate::Style::default();
        from_style.corner_radii.top_left = AbsoluteLength::Pixels(px(0.));
        from_style.text.color = Some(crate::rgb(0x000000).into());
        let _ = state.resolve(from_style, start, true, &config);

        let mut to_style = crate::Style::default();
        to_style.corner_radii.top_left = AbsoluteLength::Pixels(px(16.));
        to_style.text.color = Some(crate::rgb(0xffffff).into());
        to_style.box_shadow.push(BoxShadow {
            color: crate::hsla(0., 0., 0., 0.4),
            offset: point(px(0.), px(4.)),
            blur_radius: px(8.),
            spread_radius: px(0.),
            inset: false,
        });
        let _ = state.resolve(to_style.clone(), start, true, &config);

        let (mid, _) = state.resolve(to_style, start + Duration::from_millis(50), true, &config);

        let AbsoluteLength::Pixels(mid_radius) = mid.corner_radii.top_left else {
            panic!("expected pixel corner radius");
        };
        assert!((mid_radius.0 - 8.0).abs() < 0.5);

        let mid_text = Rgba::from(mid.text.color.expect("expected mid text color"));
        assert!(mid_text.r > 0.2 && mid_text.r < 0.8);

        assert_eq!(mid.box_shadow.len(), 1);
        let mid_shadow_alpha = Rgba::from(mid.box_shadow[0].color).a;
        assert!(mid_shadow_alpha > 0.05 && mid_shadow_alpha < 0.35);
        assert!((mid.box_shadow[0].blur_radius.0 - 8.0).abs() < 0.01);
    }

    #[kael::test]
    fn pointer_pan_and_swipe_gestures_emit_expected_events(cx: &mut TestAppContext) {
        struct TestView {
            pan_events: Rc<RefCell<Vec<crate::PanGestureEvent>>>,
            swipe_count: Rc<RefCell<usize>>,
        }

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                let pan_events = self.pan_events.clone();
                let swipe_count = self.swipe_count.clone();
                div()
                    .w(px(200.0))
                    .h(px(200.0))
                    .id("gesture-pan")
                    .on_pan(move |event, _, _| pan_events.borrow_mut().push(*event))
                    .on_swipe(SwipeDirection::Left, move |_, _, _| {
                        *swipe_count.borrow_mut() += 1;
                    })
            }
        }

        let pan_events = Rc::new(RefCell::new(Vec::new()));
        let swipe_count = Rc::new(RefCell::new(0usize));
        let (_view, mut window) = cx.add_window_view(|_, _| TestView {
            pan_events: pan_events.clone(),
            swipe_count: swipe_count.clone(),
        });

        window.simulate_mouse_down(
            point(px(150.0), px(80.0)),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.simulate_mouse_move(
            point(px(90.0), px(82.0)),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.simulate_mouse_move(
            point(px(40.0), px(84.0)),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.simulate_mouse_up(
            point(px(20.0), px(84.0)),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );

        let pan_events = pan_events.borrow();
        assert_eq!(pan_events.len(), 3);
        assert_eq!(pan_events[0].state, PanState::Began);
        assert_eq!(pan_events[1].state, PanState::Changed);
        assert_eq!(pan_events[2].state, PanState::Ended);
        assert_eq!(*swipe_count.borrow(), 1);
    }

    #[kael::test]
    fn pinch_gesture_uses_scroll_zoom_fallback(cx: &mut TestAppContext) {
        struct TestView(Rc<RefCell<Vec<crate::PinchGestureEvent>>>);

        impl Render for TestView {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                let events = self.0.clone();
                div()
                    .w(px(200.0))
                    .h(px(200.0))
                    .id("gesture-pinch")
                    .on_pinch(move |event, _, _| events.borrow_mut().push(*event))
            }
        }

        let pinch_events = Rc::new(RefCell::new(Vec::new()));
        let (_view, mut window) = cx.add_window_view(|_, _| TestView(pinch_events.clone()));

        window.simulate_event(ScrollWheelEvent {
            position: point(px(60.0), px(60.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(120.0))),
            modifiers: crate::Modifiers {
                control: true,
                ..crate::Modifiers::default()
            },
            touch_phase: crate::TouchPhase::Started,
            is_momentum: false,
        });
        window.simulate_event(ScrollWheelEvent {
            position: point(px(60.0), px(60.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(80.0))),
            modifiers: crate::Modifiers {
                control: true,
                ..crate::Modifiers::default()
            },
            touch_phase: crate::TouchPhase::Ended,
            is_momentum: false,
        });

        let pinch_events = pinch_events.borrow();
        assert_eq!(pinch_events.len(), 2);
        assert_eq!(pinch_events[0].state, PinchState::Began);
        assert_eq!(pinch_events[1].state, PinchState::Ended);
        assert!(pinch_events[1].scale > 1.0);
    }

    #[kael::test]
    fn secondary_click_context_menu_opens_and_dismisses(cx: &mut TestAppContext) {
        struct ContextMenuRoot {
            menu_clicks: Rc<Cell<usize>>,
        }

        impl Render for ContextMenuRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                let menu_clicks = self.menu_clicks.clone();
                div().relative().size_full().child(
                    div()
                        .id("context-target")
                        .debug_selector(|| "context-target".to_string())
                        .w(px(120.0))
                        .h(px(72.0))
                        .bg(crate::rgb(0xf2f2f2))
                        .context_menu_view(move |_, cx| {
                            cx.new(|_| ContextMenuEntry {
                                clicks: menu_clicks.clone(),
                            })
                            .into()
                        }),
                )
            }
        }

        struct ContextMenuEntry {
            clicks: Rc<Cell<usize>>,
        }

        impl Render for ContextMenuEntry {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                let clicks = self.clicks.clone();
                div()
                    .id("context-menu")
                    .debug_selector(|| "context-menu".to_string())
                    .w(px(96.0))
                    .h(px(36.0))
                    .bg(crate::rgb(0xffffff))
                    .border_1()
                    .border_color(crate::rgb(0xd0d0d0))
                    .on_click(move |_, _, _| clicks.set(clicks.get() + 1))
            }
        }

        let menu_clicks = Rc::new(Cell::new(0));
        let (_view, mut window) = cx.add_window_view(|_, _| ContextMenuRoot {
            menu_clicks: menu_clicks.clone(),
        });

        let target_bounds = window.debug_bounds("context-target").unwrap();
        window.simulate_mouse_down(
            target_bounds.center(),
            crate::MouseButton::Right,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let menu_bounds = window.debug_bounds("context-menu").unwrap();
        assert!(menu_bounds.origin.x >= target_bounds.origin.x);

        window.simulate_click(menu_bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert_eq!(menu_clicks.get(), 1);

        window.simulate_mouse_down(
            point(px(180.0), px(180.0)),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        window.simulate_click(menu_bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert_eq!(menu_clicks.get(), 1);
    }

    #[kael::test]
    fn tooltip_text_and_element_show_after_delay(cx: &mut TestAppContext) {
        struct TooltipRoot;

        impl Render for TooltipRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div()
                    .relative()
                    .size_full()
                    .child(
                        div()
                            .id("simple-tooltip-target")
                            .debug_selector(|| "simple-tooltip-target".to_string())
                            .w(px(120.0))
                            .h(px(40.0))
                            .bg(crate::rgb(0xf2f2f2))
                            .tooltip("Click to save"),
                    )
                    .child(
                        div()
                            .id("element-tooltip-target")
                            .debug_selector(|| "element-tooltip-target".to_string())
                            .mt(px(80.0))
                            .w(px(120.0))
                            .h(px(40.0))
                            .bg(crate::rgb(0xe8eefc))
                            .tooltip_element(|| {
                                div()
                                    .debug_selector(|| "custom-tooltip".to_string())
                                    .px(px(10.0))
                                    .py(px(8.0))
                                    .rounded(px(6.0))
                                    .bg(crate::rgb(0x1d4ed8))
                                    .text_color(crate::hsla(0.0, 0.0, 1.0, 1.0))
                                    .child("Custom tooltip")
                            }),
                    )
            }
        }

        let (_view, mut window) = cx.add_window_view(|_, _| TooltipRoot);

        let simple_bounds = window.debug_bounds("simple-tooltip-target").unwrap();
        window.simulate_mouse_move(simple_bounds.center(), None, crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert!(window.debug_bounds("tooltip-text").is_none());

        window.executor().advance_clock(TOOLTIP_SHOW_DELAY);
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert!(window.debug_bounds("tooltip-text").is_some());

        let element_bounds = window.debug_bounds("element-tooltip-target").unwrap();
        window.simulate_mouse_move(element_bounds.center(), None, crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert!(window.debug_bounds("custom-tooltip").is_none());

        window.executor().advance_clock(TOOLTIP_SHOW_DELAY);
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert!(window.debug_bounds("custom-tooltip").is_some());
    }

    #[kael::test]
    fn tab_and_arrow_keys_move_focus(cx: &mut TestAppContext) {
        struct KeyboardNavigationRoot {
            first: FocusHandle,
            second: FocusHandle,
            third: FocusHandle,
            outside: FocusHandle,
        }

        impl Render for KeyboardNavigationRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div()
                    .relative()
                    .size_full()
                    .child(
                        div()
                            .tab_group()
                            .child(
                                div()
                                    .track_focus(&self.first)
                                    .tab_index(0)
                                    .w(px(32.0))
                                    .h(px(32.0)),
                            )
                            .child(
                                div()
                                    .track_focus(&self.second)
                                    .tab_index(1)
                                    .w(px(32.0))
                                    .h(px(32.0)),
                            )
                            .child(
                                div()
                                    .track_focus(&self.third)
                                    .tab_index(2)
                                    .w(px(32.0))
                                    .h(px(32.0)),
                            ),
                    )
                    .child(
                        div()
                            .track_focus(&self.outside)
                            .tab_index(1)
                            .mt(px(72.0))
                            .w(px(32.0))
                            .h(px(32.0)),
                    )
            }
        }

        let (view, mut window) = cx.add_window_view(|_, cx| KeyboardNavigationRoot {
            first: cx.focus_handle(),
            second: cx.focus_handle(),
            third: cx.focus_handle(),
            outside: cx.focus_handle(),
        });

        let (first, second, third, outside) = window.update(|window, cx| {
            let root = view.read(cx);
            let handles = (
                root.first.clone(),
                root.second.clone(),
                root.third.clone(),
                root.outside.clone(),
            );
            window.focus(&root.first);
            window.draw(cx).clear();
            handles
        });

        window.update(|window, cx| {
            window.focus_next_in_group();
            assert_eq!(window.focused(cx).unwrap().id, second.id);
            window.focus(&first);
        });

        window.simulate_keystrokes("right");
        window.update(|window, cx| {
            assert_eq!(window.focused(cx).unwrap().id, second.id);
        });

        window.simulate_keystrokes("down");
        window.update(|window, cx| {
            assert_eq!(window.focused(cx).unwrap().id, third.id);
        });

        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            assert_eq!(window.focused(cx).unwrap().id, outside.id);
        });

        window.simulate_keystrokes("shift-tab");
        window.update(|window, cx| {
            assert_eq!(window.focused(cx).unwrap().id, third.id);
        });

        window.update(|window, _| {
            window.focus(&first);
        });
        window.simulate_keystrokes("left");
        window.update(|window, cx| {
            assert_eq!(window.focused(cx).unwrap().id, third.id);
        });
    }

    #[kael::test]
    fn focus_visible_style_tracks_keyboard_navigation(cx: &mut TestAppContext) {
        struct FocusVisibleRoot {
            focus_handle: FocusHandle,
        }

        impl Render for FocusVisibleRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div().size_full().child(
                    div()
                        .track_focus(&self.focus_handle)
                        .w(px(40.0))
                        .h(px(40.0)),
                )
            }
        }

        let (view, mut window) = cx.add_window_view(|_, cx| FocusVisibleRoot {
            focus_handle: cx.focus_handle(),
        });

        let focus_handle = window.update(|window, cx| {
            let root = view.read(cx);
            let focus_handle = root.focus_handle.clone();
            window.focus(&root.focus_handle);
            window.draw(cx).clear();
            focus_handle
        });

        let mut interactivity = Interactivity::default();
        interactivity.tracked_focus_handle = Some(focus_handle.clone());
        interactivity.focus_visible_style = Some(Box::new(StyleRefinement::default().opacity(0.4)));

        window.update(|window, cx| {
            let style = interactivity.compute_style_internal(None, None, window, cx);
            assert_eq!(style.opacity, None);
        });

        window.simulate_keystrokes("tab");
        window.update(|window, cx| {
            let style = interactivity.compute_style_internal(None, None, window, cx);
            assert_eq!(style.opacity, Some(0.4));
        });

        window.simulate_mouse_down(
            point(px(4.0), px(4.0)),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            let style = interactivity.compute_style_internal(None, None, window, cx);
            assert_eq!(style.opacity, None);
        });
    }

    #[kael::test]
    fn accessibility_tree_preserves_rendered_container_hierarchy(cx: &mut TestAppContext) {
        struct AccessibilityTreeRoot;

        impl Render for AccessibilityTreeRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div().size_full().child(
                    div()
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Group)
                                .label("Container"),
                        )
                        .child(
                            div()
                                .accessibility(
                                    AccessibilityAttributes::new(AccessibilityRole::Button)
                                        .label("Child Button"),
                                )
                                .w(px(24.0))
                                .h(px(24.0)),
                        ),
                )
            }
        }

        let (_view, mut window) = cx.add_window_view(|_, _| AccessibilityTreeRoot);

        window.update(|window, cx| {
            window.draw(cx).clear();

            let tree = &window.accessibility_tree;
            let root = tree.get(tree.root).unwrap();
            assert_eq!(root.children.len(), 1);

            let container = tree.get(root.children[0]).unwrap();
            assert_eq!(container.role, AccessibilityRole::Group);
            assert_eq!(container.label.as_deref(), Some("Container"));
            assert_eq!(container.children.len(), 1);

            let button = tree.get(container.children[0]).unwrap();
            assert_eq!(button.role, AccessibilityRole::Button);
            assert_eq!(button.label.as_deref(), Some("Child Button"));
            assert_eq!(button.parent, Some(container.id));
        });
    }

    #[kael::test]
    fn accessibility_pressed_state_tracks_active_click(cx: &mut TestAppContext) {
        struct PressedAccessibilityRoot;

        impl Render for PressedAccessibilityRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div().size_full().child(
                    div()
                        .id("pressed-target")
                        .debug_selector(|| "pressed-target".to_string())
                        .w(px(48.0))
                        .h(px(32.0))
                        .accessibility(
                            AccessibilityAttributes::new(AccessibilityRole::Button)
                                .label("Press Me"),
                        )
                        .on_click(|_, _, _| {}),
                )
            }
        }

        let (_view, mut window) = cx.add_window_view(|_, _| PressedAccessibilityRoot);

        let target_bounds = window.debug_bounds("pressed-target").unwrap();
        window.simulate_mouse_down(
            target_bounds.center(),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();

            let pressed_node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Press Me")
                })
                .unwrap();
            assert!(pressed_node.states.contains(AccessibilityState::PRESSED));
        });

        window.simulate_mouse_up(
            target_bounds.center(),
            crate::MouseButton::Left,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();

            let pressed_node = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Button
                        && node.label.as_deref() == Some("Press Me")
                })
                .unwrap();
            assert!(!pressed_node.states.contains(AccessibilityState::PRESSED));
        });
    }

    #[kael::test]
    fn context_menu_accessibility_tracks_submenu_state(cx: &mut TestAppContext) {
        struct ContextMenuAccessibilityRoot {
            focus_handle: FocusHandle,
        }

        impl Render for ContextMenuAccessibilityRoot {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
                div().size_full().track_focus(&self.focus_handle).child(
                    div()
                        .id("context-target")
                        .debug_selector(|| "context-target".to_string())
                        .w(px(120.0))
                        .h(px(72.0))
                        .bg(crate::rgb(0xf2f2f2))
                        .context_menu(|menu| {
                            menu.item("Copy", PrimaryMenuAction)
                                .separator()
                                .submenu("Share", |submenu| {
                                    submenu.item("Link", ShareViaLinkAction)
                                })
                        }),
                )
            }
        }

        let (view, mut window) = cx.add_window_view(|_, cx| ContextMenuAccessibilityRoot {
            focus_handle: cx.focus_handle(),
        });

        window.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.clone();
            window.focus(&focus_handle);
            window.draw(cx).clear();
        });

        let target_bounds = window.debug_bounds("context-target").unwrap();
        window.simulate_mouse_down(
            target_bounds.center(),
            crate::MouseButton::Right,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();

            let share_item = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::MenuItem
                        && node.label.as_deref() == Some("Share")
                })
                .unwrap();
            assert!(share_item.states.contains(AccessibilityState::COLLAPSED));
            assert!(!share_item.states.contains(AccessibilityState::EXPANDED));
        });

        let submenu_bounds = window.debug_bounds("context-menu-item-2").unwrap();
        window.simulate_click(submenu_bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();

            let share_item = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::MenuItem
                        && node.label.as_deref() == Some("Share")
                })
                .unwrap();
            assert!(share_item.states.contains(AccessibilityState::SELECTED));
            assert!(share_item.states.contains(AccessibilityState::EXPANDED));
            assert!(!share_item.states.contains(AccessibilityState::COLLAPSED));

            let nested_menu = window
                .accessibility_tree
                .nodes
                .values()
                .find(|node| {
                    node.role == AccessibilityRole::Menu && node.parent == Some(share_item.id)
                })
                .unwrap();
            assert!(nested_menu.children.iter().any(|child_id| {
                let child = window.accessibility_tree.get(*child_id).unwrap();
                child.role == AccessibilityRole::MenuItem && child.label.as_deref() == Some("Link")
            }));
        });
    }

    #[kael::test]
    fn secondary_click_context_menu_builder_dispatches_items_and_submenus(cx: &mut TestAppContext) {
        struct ContextMenuDslRoot {
            focus_handle: FocusHandle,
            primary_actions: Rc<Cell<usize>>,
            share_actions: Rc<Cell<usize>>,
        }

        impl Render for ContextMenuDslRoot {
            fn render(
                &mut self,
                _: &mut Window,
                cx: &mut Context<Self>,
            ) -> impl crate::IntoElement {
                div()
                    .relative()
                    .size_full()
                    .track_focus(&self.focus_handle)
                    .on_action(cx.listener(|this: &mut Self, _: &PrimaryMenuAction, _, _| {
                        this.primary_actions
                            .set(this.primary_actions.get().saturating_add(1));
                    }))
                    .on_action(
                        cx.listener(|this: &mut Self, _: &ShareViaLinkAction, _, _| {
                            this.share_actions
                                .set(this.share_actions.get().saturating_add(1));
                        }),
                    )
                    .child(
                        div()
                            .id("context-target")
                            .debug_selector(|| "context-target".to_string())
                            .w(px(120.0))
                            .h(px(72.0))
                            .bg(crate::rgb(0xf2f2f2))
                            .context_menu(|menu| {
                                menu.item("Copy", PrimaryMenuAction)
                                    .separator()
                                    .submenu("Share", |submenu| {
                                        submenu.item("Link", ShareViaLinkAction)
                                    })
                            }),
                    )
            }
        }

        let primary_actions = Rc::new(Cell::new(0));
        let share_actions = Rc::new(Cell::new(0));
        let (view, mut window) = cx.add_window_view(|_, cx| ContextMenuDslRoot {
            focus_handle: cx.focus_handle(),
            primary_actions: primary_actions.clone(),
            share_actions: share_actions.clone(),
        });

        window.update(|window, cx| {
            let focus_handle = view.read(cx).focus_handle.clone();
            window.focus(&focus_handle);
            window.draw(cx).clear();
        });

        let target_bounds = window.debug_bounds("context-target").unwrap();
        window.simulate_mouse_down(
            target_bounds.center(),
            crate::MouseButton::Right,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert!(window.debug_bounds("context-menu-separator-1").is_some());

        let primary_item_bounds = window.debug_bounds("context-menu-item-0").unwrap();
        window.simulate_click(primary_item_bounds.center(), crate::Modifiers::default());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
        });
        assert_eq!(primary_actions.get(), 1);

        window.simulate_mouse_down(
            target_bounds.center(),
            crate::MouseButton::Right,
            crate::Modifiers::default(),
        );
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let submenu_bounds = window.debug_bounds("context-menu-item-2").unwrap();
        window.simulate_click(submenu_bounds.center(), crate::Modifiers::default());
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        let nested_item_bounds = window.debug_bounds("context-menu-item-2-0").unwrap();
        window.simulate_click(nested_item_bounds.center(), crate::Modifiers::default());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(share_actions.get(), 1);

        window.simulate_click(nested_item_bounds.center(), crate::Modifiers::default());
        window.run_until_parked();
        window.update(|window, cx| {
            window.draw(cx).clear();
        });

        assert_eq!(share_actions.get(), 1);
    }
}

fn dispatch_pan_gesture(
    event: &PanGestureEvent,
    pan_listeners: &[PanListener],
    swipe_listeners: &[(SwipeDirection, SwipeListener)],
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let mut handled = false;

    for listener in pan_listeners {
        listener(event, window, cx);
        handled = true;
    }

    for (direction, listener) in swipe_listeners {
        if let Some(swipe_event) = SwipeGesture::new(*direction).recognize(event) {
            listener(&swipe_event, window, cx);
            handled = true;
        }
    }

    handled
}
