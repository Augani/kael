#[cfg(any(feature = "inspector", debug_assertions))]
use crate::Inspector;
use crate::{
    AbsoluteLength, Action, AnyDrag, AnyElement, AnyImageCache, AnyTooltip, AnyView, App,
    AppContext, Arena, Asset, AsyncWindowContext, AvailableSpace, Background, BlendMode,
    BorderStyle, Bounds, BoxShadow, Capslock, ColorFilter, Context, Corners, CursorStyle,
    Decorations, DefiniteLength, DevicePixels, DispatchActionListener, DispatchNodeId,
    DispatchTree, DisplayId, Edges, Effect, Entity, EntityId, EventEmitter, FileDropEvent, FontId,
    Global, GlobalElementId, GlyphId, GlyphRasterMode, GpuSpecs, Hsla, InputHandler, IsZero,
    KeyBinding, KeyContext, KeyDownEvent, KeyEvent, Keystroke, KeystrokeEvent, LayoutId,
    LineLayoutIndex, Modifiers, ModifiersChangedEvent, MonochromeSprite, MouseButton, MouseEvent,
    MouseMoveEvent, MouseUpEvent, POLYCHROME_SPRITE_KIND_COLOR,
    POLYCHROME_SPRITE_KIND_CONTENT_BLURRED, POLYCHROME_SPRITE_KIND_CONTENT_SHADOW,
    POLYCHROME_SPRITE_KIND_PREMULTIPLIED, POLYCHROME_SPRITE_KIND_SUBPIXEL_TEXT, Path, Pixels,
    PlatformAtlas, PlatformDisplay, PlatformInput, PlatformInputHandler, PlatformWindow, Point,
    PolychromeSprite, PowerMode, PrintDialogMode, PrintJob, PrintRequest, ProgressBarState,
    PromptButton, PromptLevel, Quad, Render, RenderGlyphParams, RenderImage, RenderImageParams,
    RenderSvgParams, Replay, ResizeEdge, SMOOTH_SVG_SCALE_FACTOR, SUBPIXEL_VARIANTS_X,
    SUBPIXEL_VARIANTS_Y, ScaledPixels, Scene, Shadow, SharedString, Size, StrikethroughStyle,
    Style, SubscriberSet, Subscription, SystemWindowTab, SystemWindowTabController, TabStopMap,
    TaffyLayoutEngine, Task, TextStyle, TextStyleRefinement, TooltipAlign, TooltipAnchor,
    TooltipSide, TransformationMatrix, Underline, UnderlineStyle, UndoRedoManager,
    WindowAppearance, WindowBackgroundAppearance, WindowBounds, WindowControls, WindowDecorations,
    WindowOptions, WindowParams, WindowState, WindowTextSystem, point,
    prelude::*,
    px, rems, size, transparent_black,
    webview::{PlatformWebView, PlatformWebViewCommand},
};
use anyhow::{Context as _, Result, anyhow};
use collections::{FxHashMap, FxHashSet};
#[cfg(target_os = "macos")]
use core_video::pixel_buffer::CVPixelBuffer;
use derive_more::{Deref, DerefMut};
use futures::FutureExt;
use futures::channel::oneshot;
use itertools::FoldWhile::{Continue, Done};
use itertools::Itertools;
use parking_lot::RwLock;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use refineable::Refineable;
use slotmap::SlotMap;
use smallvec::SmallVec;
use std::{
    any::{Any, TypeId},
    borrow::Cow,
    cell::{Cell, RefCell},
    cmp,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    marker::PhantomData,
    mem,
    ops::{DerefMut, Range},
    path::{Path as StdPath, PathBuf},
    rc::Rc,
    sync::{
        Arc, Weak,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::{Duration, Instant},
};
use util::post_inc;
use util::{ResultExt, measure};
use uuid::Uuid;

mod prompts;

use crate::util::atomic_incr_if_not_zero;
pub use prompts::*;

pub(crate) const DEFAULT_WINDOW_SIZE: Size<Pixels> = size(px(1536.), px(864.));

const MAX_CHECKED_WINDOW_CONTENT_SIZE: f32 = 32768.0;
const MAX_CHECKED_WINDOW_CLIENT_INSET: f32 = 512.0;
const MIN_CHECKED_WINDOW_REM_SIZE: f32 = 4.0;
const MAX_CHECKED_WINDOW_REM_SIZE: f32 = 128.0;
const MIN_UI_ZOOM_FACTOR: f32 = 0.75;
const MAX_UI_ZOOM_FACTOR: f32 = 2.0;
const UI_ZOOM_STEP: f32 = 0.1;
const MAX_CHECKED_AUTOSCROLL_BOUND_SIZE: f32 = 32768.0;

/// Checked runtime content size for a native window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowContentSize {
    size: Size<Pixels>,
}

impl WindowContentSize {
    /// Return the validated content size.
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Whether the content size is wider than it is tall.
    pub fn is_landscape(&self) -> bool {
        self.size.width > self.size.height
    }

    /// Whether the content size is taller than it is wide.
    pub fn is_portrait(&self) -> bool {
        self.size.height > self.size.width
    }

    /// Whether the content size is square.
    pub fn is_square(&self) -> bool {
        self.size.width == self.size.height
    }

    /// Content-safe summary for resize traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window content size: orientation {}",
            window_size_orientation(self.size)
        )
    }
}

/// Builder for checked runtime native window resizing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowContentSizeBuilder {
    size: Size<Pixels>,
}

impl WindowContentSizeBuilder {
    /// Create a checked content-size request.
    pub fn new(size: Size<Pixels>) -> Self {
        Self { size }
    }

    /// Create a checked content-size request from width and height.
    pub fn dimensions(width: Pixels, height: Pixels) -> Self {
        Self::new(size(width, height))
    }

    /// Return the configured content size.
    pub fn size(&self) -> Size<Pixels> {
        self.size
    }

    /// Whether the content size is wider than it is tall.
    pub fn is_landscape(&self) -> bool {
        self.size.width > self.size.height
    }

    /// Whether the content size is taller than it is wide.
    pub fn is_portrait(&self) -> bool {
        self.size.height > self.size.width
    }

    /// Whether the content size is square.
    pub fn is_square(&self) -> bool {
        self.size.width == self.size.height
    }

    /// Content-safe summary for resize traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window content size builder: orientation {}",
            window_size_orientation(self.size)
        )
    }

    /// Validate the content size before passing it to platform resize APIs.
    pub fn validate(&self) -> Result<()> {
        validate_window_content_dimension(self.size.width, "window content width")?;
        validate_window_content_dimension(self.size.height, "window content height")?;
        Ok(())
    }

    /// Build the checked content-size request.
    pub fn build_checked(self) -> Result<WindowContentSize> {
        self.validate()?;
        Ok(WindowContentSize { size: self.size })
    }
}

fn validate_window_content_dimension(value: Pixels, label: &str) -> Result<()> {
    anyhow::ensure!(value.0.is_finite(), "{label} must be finite");
    anyhow::ensure!(value.0 > 0.0, "{label} must be greater than zero");
    anyhow::ensure!(
        value.0 <= MAX_CHECKED_WINDOW_CONTENT_SIZE,
        "{label} cannot be larger than {MAX_CHECKED_WINDOW_CONTENT_SIZE} pixels"
    );
    Ok(())
}

fn window_size_orientation(size: Size<Pixels>) -> &'static str {
    if size.width > size.height {
        "landscape"
    } else if size.height > size.width {
        "portrait"
    } else {
        "square"
    }
}

/// Builder for checked taskbar/dock progress state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowProgressBuilder {
    state: ProgressBarState,
}

impl WindowProgressBuilder {
    /// Clear the taskbar/dock progress indicator.
    pub fn none() -> Self {
        Self {
            state: ProgressBarState::None,
        }
    }

    /// Show an indeterminate progress indicator.
    pub fn indeterminate() -> Self {
        Self {
            state: ProgressBarState::Indeterminate,
        }
    }

    /// Show normal determinate progress using an inclusive `0.0..=1.0` fraction.
    pub fn normal(fraction: f64) -> Self {
        Self {
            state: ProgressBarState::Normal(fraction),
        }
    }

    /// Show normal determinate progress from an inclusive `0..=100` percentage.
    pub fn normal_percent(percent: u8) -> Self {
        Self::normal(f64::from(percent) / 100.0)
    }

    /// Show a paused determinate progress indicator.
    pub fn paused(fraction: f64) -> Self {
        Self {
            state: ProgressBarState::Paused(fraction),
        }
    }

    /// Show a paused determinate progress indicator from an inclusive `0..=100` percentage.
    pub fn paused_percent(percent: u8) -> Self {
        Self::paused(f64::from(percent) / 100.0)
    }

    /// Show an error determinate progress indicator.
    pub fn error(fraction: f64) -> Self {
        Self {
            state: ProgressBarState::Error(fraction),
        }
    }

    /// Show an error determinate progress indicator from an inclusive `0..=100` percentage.
    pub fn error_percent(percent: u8) -> Self {
        Self::error(f64::from(percent) / 100.0)
    }

    /// Return the configured platform progress state.
    pub fn state(&self) -> ProgressBarState {
        self.state
    }

    /// Stable state name for diagnostics and generated UI.
    pub fn kind(&self) -> &'static str {
        self.state.kind()
    }

    /// Whether this progress request carries a fraction.
    pub fn is_determinate(&self) -> bool {
        self.state.is_determinate()
    }

    /// Whether this progress request clears the platform indicator.
    pub fn is_clear(&self) -> bool {
        self.state.is_clear()
    }

    /// Content-safe summary for taskbar/dock progress traces.
    pub fn to_text(&self) -> String {
        self.state.to_text()
    }

    /// Validate the progress state before passing it to platform taskbar/dock APIs.
    pub fn validate(&self) -> Result<()> {
        self.state.validate()
    }

    /// Build the checked platform progress state.
    pub fn build_checked(self) -> Result<ProgressBarState> {
        self.validate()?;
        Ok(self.state)
    }
}

impl Default for WindowProgressBuilder {
    fn default() -> Self {
        Self::none()
    }
}

impl From<ProgressBarState> for WindowProgressBuilder {
    fn from(state: ProgressBarState) -> Self {
        Self { state }
    }
}

/// Runtime snapshot of native window state for diagnostics, chrome, and agents.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowRuntimeSnapshot {
    bounds: Bounds<Pixels>,
    window_bounds: WindowBounds,
    viewport_size: Size<Pixels>,
    display_id: Option<DisplayId>,
    scale_factor: f32,
    appearance: WindowAppearance,
    active: bool,
    hovered: bool,
    visible: bool,
    fullscreen: bool,
    maximized: bool,
    power_mode: PowerMode,
    reduce_motion: bool,
}

impl WindowRuntimeSnapshot {
    /// Native window bounds in global coordinates.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Persistable platform window bounds/state.
    pub fn window_bounds(&self) -> WindowBounds {
        self.window_bounds
    }

    /// Drawable viewport size inside the window.
    pub fn viewport_size(&self) -> Size<Pixels> {
        self.viewport_size
    }

    /// Display currently associated with the platform window, when known.
    pub fn display_id(&self) -> Option<DisplayId> {
        self.display_id
    }

    /// Platform scale factor for this window.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Current native appearance for this window.
    pub fn appearance(&self) -> WindowAppearance {
        self.appearance
    }

    /// Whether the OS reports this window as active/focused.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Whether the OS reports this window as owning the cursor.
    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    /// Whether the platform window is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Whether the platform window is fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Whether the platform window is maximized/zoomed.
    pub fn is_maximized(&self) -> bool {
        self.maximized
    }

    /// System power mode observed for this window frame.
    pub fn power_mode(&self) -> PowerMode {
        self.power_mode
    }

    /// Whether reduce-motion was active for this window frame.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Whether animation-heavy chrome should run at full fidelity.
    pub fn animations_enabled(&self) -> bool {
        self.power_mode != PowerMode::LowPower && !self.reduce_motion
    }
}

/// Builder for checked runtime window snapshot queries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowRuntimeSnapshotQueryBuilder {
    require_visible: bool,
    require_active: bool,
    require_display: bool,
}

impl WindowRuntimeSnapshotQueryBuilder {
    /// Create a permissive runtime snapshot query.
    pub fn new() -> Self {
        Self::default()
    }

    /// Require the window to be visible.
    pub fn require_visible(mut self) -> Self {
        self.require_visible = true;
        self
    }

    /// Require the window to be active/focused.
    pub fn require_active(mut self) -> Self {
        self.require_active = true;
        self
    }

    /// Require the window to be associated with a display.
    pub fn require_display(mut self) -> Self {
        self.require_display = true;
        self
    }

    /// Whether visibility is required.
    pub fn requires_visible(&self) -> bool {
        self.require_visible
    }

    /// Whether active/focused state is required.
    pub fn requires_active(&self) -> bool {
        self.require_active
    }

    /// Whether a known display is required.
    pub fn requires_display(&self) -> bool {
        self.require_display
    }

    /// Validate a runtime window snapshot against this query.
    pub fn validate_snapshot(&self, snapshot: &WindowRuntimeSnapshot) -> Result<()> {
        anyhow::ensure!(
            !self.require_visible || snapshot.is_visible(),
            "window is not visible"
        );
        anyhow::ensure!(
            !self.require_active || snapshot.is_active(),
            "window is not active"
        );
        anyhow::ensure!(
            !self.require_display || snapshot.display_id().is_some(),
            "window display is unknown"
        );
        Ok(())
    }
}

/// Checked autoscroll bounds for native drag, selection, and editor surfaces.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowAutoscrollRequest {
    bounds: Bounds<Pixels>,
}

impl WindowAutoscrollRequest {
    /// Return the validated autoscroll bounds.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Whether the autoscroll region is empty.
    pub fn is_empty(&self) -> bool {
        self.bounds.size.width == Pixels::ZERO || self.bounds.size.height == Pixels::ZERO
    }

    /// Content-safe summary for autoscroll traces and generated UI.
    pub fn to_text(&self) -> String {
        format!("window autoscroll: empty {}", self.is_empty())
    }
}

/// Builder for checked native autoscroll requests.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowAutoscrollRequestBuilder {
    bounds: Bounds<Pixels>,
}

impl WindowAutoscrollRequestBuilder {
    /// Create a checked autoscroll request.
    pub fn new(bounds: Bounds<Pixels>) -> Self {
        Self { bounds }
    }

    /// Return the configured bounds.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds
    }

    /// Whether the configured autoscroll region is empty.
    pub fn is_empty(&self) -> bool {
        self.bounds.size.width == Pixels::ZERO || self.bounds.size.height == Pixels::ZERO
    }

    /// Content-safe summary for autoscroll traces and generated UI.
    pub fn to_text(&self) -> String {
        format!("window autoscroll builder: empty {}", self.is_empty())
    }

    /// Validate the autoscroll bounds before they can affect containing scroll elements.
    pub fn validate(&self) -> Result<()> {
        validate_window_autoscroll_bounds(self.bounds)
    }

    /// Build the checked autoscroll request.
    pub fn build_checked(self) -> Result<WindowAutoscrollRequest> {
        self.validate()?;
        Ok(WindowAutoscrollRequest {
            bounds: self.bounds,
        })
    }
}

fn validate_window_autoscroll_bounds(bounds: Bounds<Pixels>) -> Result<()> {
    anyhow::ensure!(
        bounds.origin.x.0.is_finite()
            && bounds.origin.y.0.is_finite()
            && bounds.size.width.0.is_finite()
            && bounds.size.height.0.is_finite(),
        "window autoscroll bounds must use finite values"
    );
    anyhow::ensure!(
        bounds.size.width >= Pixels::ZERO && bounds.size.height >= Pixels::ZERO,
        "window autoscroll bounds cannot have negative size"
    );
    anyhow::ensure!(
        bounds.size.width.0 <= MAX_CHECKED_AUTOSCROLL_BOUND_SIZE
            && bounds.size.height.0 <= MAX_CHECKED_AUTOSCROLL_BOUND_SIZE,
        "window autoscroll bounds cannot be larger than {MAX_CHECKED_AUTOSCROLL_BOUND_SIZE} pixels"
    );
    Ok(())
}

/// Checked base `rem` size for native window UI scaling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowRemSize {
    rem_size: Pixels,
}

impl WindowRemSize {
    /// Return the validated rem size.
    pub fn rem_size(&self) -> Pixels {
        self.rem_size
    }

    /// Size class for generated density/zoom summaries.
    pub fn size_class(&self) -> &'static str {
        window_rem_size_class(self.rem_size)
    }

    /// Content-safe summary for layout scale traces and generated UI.
    pub fn to_text(&self) -> String {
        format!("window rem size: class {}", self.size_class())
    }
}

/// Builder for checked native window rem-size changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowRemSizeBuilder {
    rem_size: Pixels,
}

impl WindowRemSizeBuilder {
    /// Create a checked rem-size request.
    pub fn new(rem_size: impl Into<Pixels>) -> Self {
        Self {
            rem_size: rem_size.into(),
        }
    }

    /// Return the configured rem size.
    pub fn rem_size(&self) -> Pixels {
        self.rem_size
    }

    /// Size class for generated density/zoom summaries.
    pub fn size_class(&self) -> &'static str {
        window_rem_size_class(self.rem_size)
    }

    /// Content-safe summary for layout scale traces and generated UI.
    pub fn to_text(&self) -> String {
        format!("window rem size builder: class {}", self.size_class())
    }

    /// Validate the rem size before it can rescale native layout.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.rem_size.0.is_finite(),
            "window rem size must be finite"
        );
        anyhow::ensure!(
            self.rem_size.0 >= MIN_CHECKED_WINDOW_REM_SIZE,
            "window rem size cannot be smaller than {MIN_CHECKED_WINDOW_REM_SIZE} pixels"
        );
        anyhow::ensure!(
            self.rem_size.0 <= MAX_CHECKED_WINDOW_REM_SIZE,
            "window rem size cannot be larger than {MAX_CHECKED_WINDOW_REM_SIZE} pixels"
        );
        Ok(())
    }

    /// Build the checked rem-size request.
    pub fn build_checked(self) -> Result<WindowRemSize> {
        self.validate()?;
        Ok(WindowRemSize {
            rem_size: self.rem_size,
        })
    }
}

fn window_rem_size_class(rem_size: Pixels) -> &'static str {
    if rem_size.0 < 12.0 {
        "compact"
    } else if rem_size.0 <= 24.0 {
        "standard"
    } else {
        "large"
    }
}

/// Checked custom-chrome client inset for native window decorations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowClientInset {
    inset: Pixels,
}

impl WindowClientInset {
    /// Return the validated inset.
    pub fn inset(&self) -> Pixels {
        self.inset
    }

    /// Whether this inset removes the custom chrome client margin.
    pub fn is_zero(&self) -> bool {
        self.inset == Pixels::ZERO
    }

    /// Content-safe summary for custom chrome traces and generated UI.
    pub fn to_text(&self) -> String {
        format!("window client inset: zero {}", self.is_zero())
    }
}

/// Builder for checked client-side decoration insets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowClientInsetBuilder {
    inset: Pixels,
}

impl WindowClientInsetBuilder {
    /// Create a checked client inset request.
    pub fn new(inset: Pixels) -> Self {
        Self { inset }
    }

    /// Return the configured inset.
    pub fn inset(&self) -> Pixels {
        self.inset
    }

    /// Whether this inset removes the custom chrome client margin.
    pub fn is_zero(&self) -> bool {
        self.inset == Pixels::ZERO
    }

    /// Content-safe summary for custom chrome traces and generated UI.
    pub fn to_text(&self) -> String {
        format!("window client inset builder: zero {}", self.is_zero())
    }

    /// Validate the inset before passing it to platform custom-chrome APIs.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.inset.0.is_finite(),
            "window client inset must be finite"
        );
        anyhow::ensure!(
            self.inset.0 >= 0.0,
            "window client inset cannot be negative"
        );
        anyhow::ensure!(
            self.inset.0 <= MAX_CHECKED_WINDOW_CLIENT_INSET,
            "window client inset cannot be larger than {MAX_CHECKED_WINDOW_CLIENT_INSET} pixels"
        );
        Ok(())
    }

    /// Build the checked client inset request.
    pub fn build_checked(self) -> Result<WindowClientInset> {
        self.validate()?;
        Ok(WindowClientInset { inset: self.inset })
    }
}

/// Checked render policy for native window performance behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRenderPolicy {
    frame_skip_enabled: bool,
    reason: Option<String>,
}

impl WindowRenderPolicy {
    /// Return whether whole-frame damage skipping should be enabled.
    pub fn frame_skip_enabled(&self) -> bool {
        self.frame_skip_enabled
    }

    /// Optional diagnostic reason for enabling frame skipping.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// Builder for checked native window render/performance policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRenderPolicyBuilder {
    frame_skip_enabled: bool,
    reason: Option<String>,
}

impl WindowRenderPolicyBuilder {
    /// Enable whole-frame damage skipping for mostly-static native UI.
    pub fn frame_skip(reason: impl Into<String>) -> Self {
        Self {
            frame_skip_enabled: true,
            reason: Some(reason.into()),
        }
    }

    /// Disable whole-frame damage skipping.
    pub fn no_frame_skip() -> Self {
        Self {
            frame_skip_enabled: false,
            reason: None,
        }
    }

    /// Return whether this policy enables whole-frame damage skipping.
    pub fn frame_skip_requested(&self) -> bool {
        self.frame_skip_enabled
    }

    /// Optional diagnostic reason for enabling frame skipping.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the render policy before applying it.
    pub fn validate(&self) -> Result<()> {
        if self.frame_skip_enabled {
            let Some(reason) = &self.reason else {
                anyhow::bail!("enabling frame skipping requires a reason");
            };
            validate_window_policy_reason(reason, "window render policy reason")?;
        } else {
            anyhow::ensure!(
                self.reason.is_none(),
                "disabled frame skipping policy cannot include a reason"
            );
        }

        Ok(())
    }

    /// Build the checked render policy.
    pub fn build_checked(self) -> Result<WindowRenderPolicy> {
        self.validate()?;
        Ok(WindowRenderPolicy {
            frame_skip_enabled: self.frame_skip_enabled,
            reason: self.reason,
        })
    }
}

fn validate_window_policy_reason(reason: &str, label: &str) -> Result<()> {
    anyhow::ensure!(!reason.trim().is_empty(), "{label} cannot be empty");
    anyhow::ensure!(
        reason == reason.trim(),
        "{label} cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !reason.chars().any(char::is_control),
        "{label} cannot contain control characters"
    );
    anyhow::ensure!(
        reason.chars().count() <= 256,
        "{label} cannot be longer than 256 characters"
    );
    Ok(())
}

/// Builder for checked assistive-technology announcements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessibilityAnnouncementBuilder {
    message: String,
}

impl AccessibilityAnnouncementBuilder {
    /// Create an announcement from user-facing status text.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Return the configured announcement message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Validate the announcement before passing it to assistive technology.
    pub fn validate(&self) -> Result<()> {
        validate_window_policy_reason(&self.message, "accessibility announcement")
    }

    /// Build the validated announcement message.
    pub fn build_checked(self) -> Result<String> {
        self.validate()?;
        Ok(self.message)
    }
}

/// Builder for checked accessibility focus changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessibilityFocusBuilder {
    node_id: crate::AccessibilityId,
}

impl AccessibilityFocusBuilder {
    /// Focus an existing accessibility node.
    pub fn new(node_id: crate::AccessibilityId) -> Self {
        Self { node_id }
    }

    /// Return the target node id.
    pub fn node_id(&self) -> crate::AccessibilityId {
        self.node_id
    }

    /// Validate the target against an accessibility tree.
    pub fn validate_tree(&self, tree: &crate::AccessibilityTree) -> Result<()> {
        let Some(node) = tree.get(self.node_id) else {
            anyhow::bail!(
                "accessibility focus target {} is not present in the current tree",
                self.node_id.0
            );
        };
        anyhow::ensure!(
            !node.states.contains(crate::AccessibilityState::HIDDEN),
            "accessibility focus target {} is hidden",
            self.node_id.0
        );
        Ok(())
    }

    /// Validate the target against the window's current accessibility tree.
    pub fn validate(&self, window: &Window) -> Result<()> {
        self.validate_tree(&window.accessibility_tree)
    }

    /// Build the validated focus target.
    pub fn build_checked(self, window: &Window) -> Result<crate::AccessibilityId> {
        self.validate(window)?;
        Ok(self.node_id)
    }
}

/// Checked native window opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowOpacity {
    fraction: f32,
}

impl WindowOpacity {
    /// Return the validated opacity fraction in the inclusive `0.0..=1.0` range.
    pub fn fraction(&self) -> f32 {
        self.fraction
    }

    /// Whether this opacity is fully opaque.
    pub fn is_opaque(&self) -> bool {
        self.fraction >= 1.0
    }

    /// Whether this opacity makes the native window translucent.
    pub fn is_translucent(&self) -> bool {
        self.fraction < 1.0
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window opacity: {}, translucent {}",
            if self.is_opaque() {
                "opaque"
            } else {
                "fractional"
            },
            self.is_translucent()
        )
    }
}

/// Builder for checked native window opacity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowOpacityBuilder {
    fraction: f32,
}

impl WindowOpacityBuilder {
    /// Create a native window opacity from a fraction in the inclusive `0.0..=1.0` range.
    pub fn fraction(fraction: f32) -> Self {
        Self { fraction }
    }

    /// Create a fully opaque native window.
    pub fn opaque() -> Self {
        Self::fraction(1.0)
    }

    /// Return the configured opacity fraction.
    pub fn value(&self) -> f32 {
        self.fraction
    }

    /// Whether this opacity request is fully opaque.
    pub fn is_opaque(&self) -> bool {
        self.fraction >= 1.0
    }

    /// Whether this opacity request makes the native window translucent.
    pub fn is_translucent(&self) -> bool {
        self.fraction < 1.0
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window opacity: {}, translucent {}",
            if self.is_opaque() {
                "opaque"
            } else {
                "fractional"
            },
            self.is_translucent()
        )
    }

    /// Validate the opacity before passing it to platform window APIs.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.fraction.is_finite(),
            "window opacity must be a finite fraction"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.fraction),
            "window opacity must be between 0.0 and 1.0"
        );
        Ok(())
    }

    /// Build the checked opacity.
    pub fn build_checked(self) -> Result<WindowOpacity> {
        self.validate()?;
        Ok(WindowOpacity {
            fraction: self.fraction,
        })
    }
}

/// Builder for validated platform window titles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTitleBuilder {
    title: String,
}

impl WindowTitleBuilder {
    /// Create a window title from user-facing text.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }

    /// Return the configured title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Number of Unicode scalar values in the configured title.
    pub fn title_len_chars(&self) -> usize {
        self.title.chars().count()
    }

    /// Whether the configured title is empty or whitespace-only before validation.
    pub fn is_blank(&self) -> bool {
        self.title.trim().is_empty()
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window title: {} chars, blank {}",
            self.title_len_chars(),
            self.is_blank()
        )
    }

    /// Validate the title before passing it to platform chrome.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.title.trim().is_empty(),
            "window title cannot be empty"
        );
        anyhow::ensure!(
            self.title == self.title.trim(),
            "window title cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            !self.title.chars().any(char::is_control),
            "window title cannot contain control characters"
        );
        anyhow::ensure!(
            self.title.chars().count() <= 512,
            "window title cannot be longer than 512 characters"
        );
        Ok(())
    }

    /// Build the validated title.
    pub fn build_checked(self) -> Result<String> {
        self.validate()?;
        Ok(self.title)
    }
}

/// Checked document chrome state for editor and document windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDocumentState {
    title: Option<String>,
    document_path: Option<PathBuf>,
    edited: bool,
}

impl WindowDocumentState {
    /// Return the validated title to apply to the platform window, if any.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Return the optional document path associated with the window.
    pub fn document_path(&self) -> Option<&StdPath> {
        self.document_path.as_deref()
    }

    /// Return whether the window should be marked as having unsaved changes.
    pub fn edited(&self) -> bool {
        self.edited
    }

    /// Whether a title is available without exposing the title text.
    pub fn has_title(&self) -> bool {
        self.title.is_some()
    }

    /// Whether a document path is associated without exposing the path.
    pub fn has_document_path(&self) -> bool {
        self.document_path.is_some()
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "document window state: title {}, path {}, edited {}",
            self.has_title(),
            self.has_document_path(),
            self.edited
        )
    }
}

/// Builder for checked document-window chrome state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDocumentStateBuilder {
    title: Option<String>,
    document_path: Option<PathBuf>,
    edited: bool,
    require_existing_path: bool,
    canonicalize_path: bool,
}

impl WindowDocumentStateBuilder {
    /// Create an empty document-window state builder.
    pub fn new() -> Self {
        Self {
            title: None,
            document_path: None,
            edited: false,
            require_existing_path: false,
            canonicalize_path: false,
        }
    }

    /// Create document-window state from a document path.
    pub fn document(path: impl Into<PathBuf>) -> Self {
        Self::new().document_path(path)
    }

    /// Set a user-facing document title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the path this window represents.
    pub fn document_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.document_path = Some(path.into());
        self
    }

    /// Mark the document as edited or clean.
    pub fn edited(mut self, edited: bool) -> Self {
        self.edited = edited;
        self
    }

    /// Mark the document as having unsaved changes.
    pub fn unsaved_changes(self) -> Self {
        self.edited(true)
    }

    /// Mark the document as clean.
    pub fn clean(self) -> Self {
        self.edited(false)
    }

    /// Require the configured document path to exist.
    pub fn require_existing_path(mut self) -> Self {
        self.require_existing_path = true;
        self
    }

    /// Canonicalize the configured document path before building.
    pub fn canonicalize_path(mut self) -> Self {
        self.canonicalize_path = true;
        self.require_existing_path = true;
        self
    }

    /// Return the configured title, if any.
    pub fn configured_title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Return the configured document path, if any.
    pub fn configured_document_path(&self) -> Option<&StdPath> {
        self.document_path.as_deref()
    }

    /// Return whether the document should be marked edited.
    pub fn is_edited(&self) -> bool {
        self.edited
    }

    /// Whether a title is configured without exposing the title text.
    pub fn has_title(&self) -> bool {
        self.title.is_some()
    }

    /// Whether a document path is configured without exposing the path.
    pub fn has_document_path(&self) -> bool {
        self.document_path.is_some()
    }

    /// Whether the builder requires the path to exist.
    pub fn requires_existing_path(&self) -> bool {
        self.require_existing_path
    }

    /// Whether the builder canonicalizes the document path.
    pub fn canonicalizes_path(&self) -> bool {
        self.canonicalize_path
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "document window state builder: title {}, path {}, edited {}, require existing {}, canonicalize {}",
            self.has_title(),
            self.has_document_path(),
            self.edited,
            self.require_existing_path,
            self.canonicalize_path
        )
    }

    /// Validate the configured state.
    pub fn validate(&self) -> Result<()> {
        if let Some(title) = &self.title {
            WindowTitleBuilder::new(title.clone()).validate()?;
        }
        if let Some(path) = &self.document_path {
            validate_document_state_path(path, self.require_existing_path)?;
            if self.title.is_none() {
                let derived = document_title_from_path(path)?;
                WindowTitleBuilder::new(derived).validate()?;
            }
        }
        anyhow::ensure!(
            self.title.is_some() || self.document_path.is_some(),
            "document window state requires a title or document path"
        );
        Ok(())
    }

    /// Build checked document-window state.
    pub fn build_checked(mut self) -> Result<WindowDocumentState> {
        self.validate()?;
        if self.canonicalize_path
            && let Some(path) = &self.document_path
        {
            self.document_path = Some(path.canonicalize().map_err(|error| {
                anyhow!(
                    "could not canonicalize document window path {}: {error}",
                    path.display()
                )
            })?);
        }
        let title = match self.title {
            Some(title) => Some(WindowTitleBuilder::new(title).build_checked()?),
            None => self
                .document_path
                .as_deref()
                .map(document_title_from_path)
                .transpose()?,
        };
        Ok(WindowDocumentState {
            title,
            document_path: self.document_path,
            edited: self.edited,
        })
    }
}

impl Default for WindowDocumentStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_document_state_path(path: &StdPath, require_existing_path: bool) -> Result<()> {
    anyhow::ensure!(
        !path.as_os_str().is_empty(),
        "document window path cannot be empty"
    );
    if let Some(text) = path.to_str() {
        anyhow::ensure!(
            !text.contains('\0'),
            "document window path cannot contain NUL bytes"
        );
    }
    if require_existing_path {
        std::fs::metadata(path).map_err(|error| {
            anyhow!(
                "document window path does not exist {}: {error}",
                path.display()
            )
        })?;
    }
    Ok(())
}

fn document_title_from_path(path: &StdPath) -> Result<String> {
    let title = path
        .file_name()
        .or_else(|| {
            path.components()
                .next_back()
                .map(|component| component.as_os_str())
        })
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow!(
                "document window path cannot produce a title: {}",
                path.display()
            )
        })?;
    WindowTitleBuilder::new(title).build_checked()
}

/// Builder for validated platform window app identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowAppIdBuilder {
    app_id: String,
}

impl WindowAppIdBuilder {
    /// Create a window app identifier used by platform grouping.
    pub fn new(app_id: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
        }
    }

    /// Return the configured app identifier.
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// Number of bytes in the configured app id.
    pub fn len_bytes(&self) -> usize {
        self.app_id.len()
    }

    /// Whether the configured app id is empty or whitespace-only before validation.
    pub fn is_blank(&self) -> bool {
        self.app_id.trim().is_empty()
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window app id: {} bytes, blank {}",
            self.len_bytes(),
            self.is_blank()
        )
    }

    /// Validate the app identifier before passing it to platform grouping APIs.
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.app_id.trim().is_empty(),
            "window app id cannot be empty"
        );
        anyhow::ensure!(
            self.app_id == self.app_id.trim(),
            "window app id cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            !self
                .app_id
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace()),
            "window app id cannot contain whitespace or control characters"
        );
        Ok(())
    }

    /// Build the validated app identifier.
    pub fn build_checked(self) -> Result<String> {
        self.validate()?;
        Ok(self.app_id)
    }
}

/// Builder for validated platform window tabbing identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTabbingIdentifierBuilder {
    identifier: Option<String>,
}

impl WindowTabbingIdentifierBuilder {
    /// Clear the tabbing identifier.
    pub fn clear() -> Self {
        Self { identifier: None }
    }

    /// Create a tabbing identifier used to group compatible windows.
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: Some(identifier.into()),
        }
    }

    /// Return the configured tabbing identifier, or `None` when clearing it.
    pub fn identifier(&self) -> Option<&str> {
        self.identifier.as_deref()
    }

    /// Return whether this builder clears the tabbing identifier.
    pub fn is_clear(&self) -> bool {
        self.identifier.is_none()
    }

    /// Whether this builder sets a non-clear tabbing identifier.
    pub fn has_identifier(&self) -> bool {
        self.identifier.is_some()
    }

    /// Number of bytes in the configured identifier, or zero when clearing it.
    pub fn len_bytes(&self) -> usize {
        self.identifier.as_ref().map_or(0, String::len)
    }

    /// Content-safe summary for traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window tabbing identifier: clear {}, identifier {}, {} bytes",
            self.is_clear(),
            self.has_identifier(),
            self.len_bytes()
        )
    }

    /// Validate the tabbing identifier before passing it to platform APIs.
    pub fn validate(&self) -> Result<()> {
        let Some(identifier) = &self.identifier else {
            return Ok(());
        };

        anyhow::ensure!(
            !identifier.trim().is_empty(),
            "window tabbing identifier cannot be empty"
        );
        anyhow::ensure!(
            identifier == identifier.trim(),
            "window tabbing identifier cannot have leading or trailing whitespace"
        );
        anyhow::ensure!(
            !identifier
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace()),
            "window tabbing identifier cannot contain whitespace or control characters"
        );
        Ok(())
    }

    /// Build the validated tabbing identifier.
    pub fn build_checked(self) -> Result<Option<String>> {
        self.validate()?;
        Ok(self.identifier)
    }
}

impl Default for WindowTabbingIdentifierBuilder {
    fn default() -> Self {
        Self::clear()
    }
}

/// Desired capture/privacy behavior for a native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowContentProtectionMode {
    /// Allow normal OS capture behavior.
    Disabled,
    /// Request that the OS exclude the window from screenshots and screen capture.
    ExcludeFromCapture,
    /// Request that captured output is obscured when full exclusion is unavailable.
    ObscureWhenCaptured,
}

impl WindowContentProtectionMode {
    /// Whether this mode requests capture protection.
    pub fn is_protected(self) -> bool {
        self != Self::Disabled
    }

    /// Stable key for diagnostics, docs, and generated policies.
    pub fn key(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::ExcludeFromCapture => "exclude-from-capture",
            Self::ObscureWhenCaptured => "obscure-when-captured",
        }
    }
}

/// Checked content-protection policy for a native window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowContentProtection {
    mode: WindowContentProtectionMode,
    reason: Option<String>,
    block_app_window_capture: bool,
}

impl WindowContentProtection {
    /// Requested protection mode.
    pub fn mode(&self) -> WindowContentProtectionMode {
        self.mode
    }

    /// User-facing or diagnostic reason for protected states.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Whether the policy requests protection.
    pub fn is_protected(&self) -> bool {
        self.mode.is_protected()
    }

    /// Whether app-owned window capture should also skip this window.
    pub fn blocks_app_window_capture(&self) -> bool {
        self.block_app_window_capture
    }
}

/// Builder for checked window content-protection policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowContentProtectionBuilder {
    mode: WindowContentProtectionMode,
    reason: Option<String>,
    block_app_window_capture: bool,
}

impl WindowContentProtectionBuilder {
    /// Clear capture protection for the window.
    pub fn disabled() -> Self {
        Self {
            mode: WindowContentProtectionMode::Disabled,
            reason: None,
            block_app_window_capture: false,
        }
    }

    /// Request exclusion from screenshots and screen capture.
    pub fn exclude_from_capture(reason: impl Into<String>) -> Self {
        Self {
            mode: WindowContentProtectionMode::ExcludeFromCapture,
            reason: Some(reason.into()),
            block_app_window_capture: true,
        }
    }

    /// Request obscuring in captured output when full exclusion is unavailable.
    pub fn obscure_when_captured(reason: impl Into<String>) -> Self {
        Self {
            mode: WindowContentProtectionMode::ObscureWhenCaptured,
            reason: Some(reason.into()),
            block_app_window_capture: true,
        }
    }

    /// Override whether app-owned window capture should skip this window.
    pub fn block_app_window_capture(mut self, block: bool) -> Self {
        self.block_app_window_capture = block;
        self
    }

    /// Return the configured mode.
    pub fn mode(&self) -> WindowContentProtectionMode {
        self.mode
    }

    /// Return the configured reason.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the configured content-protection policy.
    pub fn validate(&self) -> Result<()> {
        match self.mode {
            WindowContentProtectionMode::Disabled => {
                anyhow::ensure!(
                    self.reason.is_none(),
                    "disabled window content protection cannot include a reason"
                );
                anyhow::ensure!(
                    !self.block_app_window_capture,
                    "disabled window content protection cannot block app window capture"
                );
            }
            WindowContentProtectionMode::ExcludeFromCapture
            | WindowContentProtectionMode::ObscureWhenCaptured => {
                let reason = self.reason.as_deref().unwrap_or_default();
                validate_window_content_protection_reason(reason)?;
            }
        }
        Ok(())
    }

    /// Build the checked content-protection policy.
    pub fn build_checked(self) -> Result<WindowContentProtection> {
        self.validate()?;
        Ok(WindowContentProtection {
            mode: self.mode,
            reason: self.reason,
            block_app_window_capture: self.block_app_window_capture,
        })
    }
}

fn validate_window_content_protection_reason(reason: &str) -> Result<()> {
    anyhow::ensure!(
        !reason.trim().is_empty(),
        "window content protection reason cannot be empty"
    );
    anyhow::ensure!(
        reason == reason.trim(),
        "window content protection reason cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !reason.chars().any(char::is_control),
        "window content protection reason cannot contain control characters"
    );
    anyhow::ensure!(
        reason.chars().count() <= 256,
        "window content protection reason cannot be longer than 256 characters"
    );
    Ok(())
}

/// Desired presentation behavior for a native window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowPresentationMode {
    /// Normal windowed behavior.
    Windowed,
    /// Fullscreen presentation while preserving normal user exit behavior.
    Fullscreen,
    /// Kiosk-style fullscreen intent for controlled presentation/POS flows.
    Kiosk,
}

impl WindowPresentationMode {
    /// Whether this mode should put the platform window in fullscreen.
    pub fn wants_fullscreen(self) -> bool {
        matches!(self, Self::Fullscreen | Self::Kiosk)
    }

    /// Stable key for diagnostics, docs, and generated policies.
    pub fn key(self) -> &'static str {
        match self {
            Self::Windowed => "windowed",
            Self::Fullscreen => "fullscreen",
            Self::Kiosk => "kiosk",
        }
    }
}

/// Checked presentation/kiosk policy for a native window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowPresentationPolicy {
    mode: WindowPresentationMode,
    reason: Option<String>,
    allow_user_exit: bool,
    hide_chrome: bool,
}

impl WindowPresentationPolicy {
    /// Requested presentation mode.
    pub fn mode(&self) -> WindowPresentationMode {
        self.mode
    }

    /// User-facing or diagnostic reason for fullscreen/kiosk states.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Whether users should retain the normal platform exit gesture.
    pub fn allows_user_exit(&self) -> bool {
        self.allow_user_exit
    }

    /// Whether chrome should be hidden by platform backends where supported.
    pub fn hides_chrome(&self) -> bool {
        self.hide_chrome
    }
}

/// Builder for checked window presentation/kiosk policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowPresentationPolicyBuilder {
    mode: WindowPresentationMode,
    reason: Option<String>,
    allow_user_exit: bool,
    hide_chrome: bool,
}

impl WindowPresentationPolicyBuilder {
    /// Restore normal windowed behavior.
    pub fn windowed() -> Self {
        Self {
            mode: WindowPresentationMode::Windowed,
            reason: None,
            allow_user_exit: true,
            hide_chrome: false,
        }
    }

    /// Request fullscreen presentation.
    pub fn fullscreen(reason: impl Into<String>) -> Self {
        Self {
            mode: WindowPresentationMode::Fullscreen,
            reason: Some(reason.into()),
            allow_user_exit: true,
            hide_chrome: false,
        }
    }

    /// Request kiosk-style fullscreen presentation.
    pub fn kiosk(reason: impl Into<String>) -> Self {
        Self {
            mode: WindowPresentationMode::Kiosk,
            reason: Some(reason.into()),
            allow_user_exit: false,
            hide_chrome: true,
        }
    }

    /// Set whether users retain the normal platform exit gesture.
    pub fn allow_user_exit(mut self, allow: bool) -> Self {
        self.allow_user_exit = allow;
        self
    }

    /// Set whether chrome should be hidden by platform backends where supported.
    pub fn hide_chrome(mut self, hide: bool) -> Self {
        self.hide_chrome = hide;
        self
    }

    /// Return the configured mode.
    pub fn mode(&self) -> WindowPresentationMode {
        self.mode
    }

    /// Return the configured reason.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the configured presentation policy.
    pub fn validate(&self) -> Result<()> {
        match self.mode {
            WindowPresentationMode::Windowed => {
                anyhow::ensure!(
                    self.reason.is_none(),
                    "windowed presentation policy cannot include a reason"
                );
                anyhow::ensure!(
                    self.allow_user_exit,
                    "windowed presentation policy must allow user exit"
                );
                anyhow::ensure!(
                    !self.hide_chrome,
                    "windowed presentation policy cannot hide chrome"
                );
            }
            WindowPresentationMode::Fullscreen | WindowPresentationMode::Kiosk => {
                let reason = self.reason.as_deref().unwrap_or_default();
                validate_window_presentation_reason(reason)?;
            }
        }
        Ok(())
    }

    /// Build the checked presentation policy.
    pub fn build_checked(self) -> Result<WindowPresentationPolicy> {
        self.validate()?;
        Ok(WindowPresentationPolicy {
            mode: self.mode,
            reason: self.reason,
            allow_user_exit: self.allow_user_exit,
            hide_chrome: self.hide_chrome,
        })
    }
}

fn validate_window_presentation_reason(reason: &str) -> Result<()> {
    anyhow::ensure!(
        !reason.trim().is_empty(),
        "window presentation reason cannot be empty"
    );
    anyhow::ensure!(
        reason == reason.trim(),
        "window presentation reason cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !reason.chars().any(char::is_control),
        "window presentation reason cannot contain control characters"
    );
    anyhow::ensure!(
        reason.chars().count() <= 256,
        "window presentation reason cannot be longer than 256 characters"
    );
    Ok(())
}

/// Window-level interaction command for native desktop window show/hide/focus flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowInteractionCommandKind {
    /// Focus and raise the current window.
    Activate,
    /// Minimize the current window.
    Minimize,
    /// Toggle native zoom/maximize behavior for the current window.
    ZoomWindow,
    /// Show the current window.
    Show,
    /// Hide the current window.
    Hide,
    /// Request that the current window close through the platform lifecycle.
    Close,
    /// Enter platform fullscreen if the window is not already fullscreen.
    EnterFullscreen,
    /// Exit platform fullscreen if the window is currently fullscreen.
    ExitFullscreen,
    /// Toggle platform fullscreen.
    ToggleFullscreen,
    /// Enable or disable mouse-event pass-through for overlay windows.
    MousePassthrough {
        /// Whether mouse events should pass through the window.
        enabled: bool,
    },
}

/// Checked window interaction command for visibility, focus, and mouse pass-through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInteractionCommand {
    kind: WindowInteractionCommandKind,
    reason: Option<String>,
}

impl WindowInteractionCommand {
    /// Focus and raise the current window.
    pub fn activate() -> Self {
        Self {
            kind: WindowInteractionCommandKind::Activate,
            reason: None,
        }
    }

    /// Minimize the current window.
    pub fn minimize() -> Self {
        Self {
            kind: WindowInteractionCommandKind::Minimize,
            reason: None,
        }
    }

    /// Toggle native zoom/maximize behavior for the current window.
    pub fn zoom_window() -> Self {
        Self {
            kind: WindowInteractionCommandKind::ZoomWindow,
            reason: None,
        }
    }

    /// Show the current window.
    pub fn show() -> Self {
        Self {
            kind: WindowInteractionCommandKind::Show,
            reason: None,
        }
    }

    /// Hide the current window.
    pub fn hide() -> Self {
        Self {
            kind: WindowInteractionCommandKind::Hide,
            reason: None,
        }
    }

    /// Request that the current window close through the platform lifecycle.
    pub fn close(reason: impl Into<String>) -> Self {
        Self {
            kind: WindowInteractionCommandKind::Close,
            reason: Some(reason.into()),
        }
    }

    /// Enter platform fullscreen if the window is not already fullscreen.
    pub fn enter_fullscreen() -> Self {
        Self {
            kind: WindowInteractionCommandKind::EnterFullscreen,
            reason: None,
        }
    }

    /// Exit platform fullscreen if the window is currently fullscreen.
    pub fn exit_fullscreen() -> Self {
        Self {
            kind: WindowInteractionCommandKind::ExitFullscreen,
            reason: None,
        }
    }

    /// Toggle platform fullscreen.
    pub fn toggle_fullscreen() -> Self {
        Self {
            kind: WindowInteractionCommandKind::ToggleFullscreen,
            reason: None,
        }
    }

    /// Enable mouse-event pass-through for an overlay or click-through window.
    pub fn mouse_passthrough(reason: impl Into<String>) -> Self {
        Self {
            kind: WindowInteractionCommandKind::MousePassthrough { enabled: true },
            reason: Some(reason.into()),
        }
    }

    /// Disable mouse-event pass-through and make the window receive mouse input again.
    pub fn receive_mouse_events() -> Self {
        Self {
            kind: WindowInteractionCommandKind::MousePassthrough { enabled: false },
            reason: None,
        }
    }

    /// Attach a diagnostic reason to the command.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The command kind.
    pub fn kind(&self) -> WindowInteractionCommandKind {
        self.kind
    }

    /// Optional diagnostic reason for the command.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the command before dispatching it to the platform window.
    pub fn validate(&self) -> Result<()> {
        if matches!(
            self.kind,
            WindowInteractionCommandKind::MousePassthrough { enabled: true }
        ) {
            anyhow::ensure!(
                self.reason.is_some(),
                "enabling mouse pass-through requires a reason"
            );
        }
        if matches!(self.kind, WindowInteractionCommandKind::Close) {
            anyhow::ensure!(self.reason.is_some(), "closing a window requires a reason");
        }

        if let Some(reason) = &self.reason {
            validate_window_interaction_reason(reason)?;
        }

        Ok(())
    }
}

fn validate_window_interaction_reason(reason: &str) -> Result<()> {
    anyhow::ensure!(
        !reason.trim().is_empty(),
        "window interaction reason cannot be empty"
    );
    anyhow::ensure!(
        reason == reason.trim(),
        "window interaction reason cannot have leading or trailing whitespace"
    );
    anyhow::ensure!(
        !reason.chars().any(char::is_control),
        "window interaction reason cannot contain control characters"
    );
    anyhow::ensure!(
        reason.chars().count() <= 256,
        "window interaction reason cannot be longer than 256 characters"
    );
    Ok(())
}

/// Checked whole-window cursor style request for generated native UI surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowCursorStyleCommand {
    style: CursorStyle,
    reason: String,
}

impl WindowCursorStyleCommand {
    /// Set a cursor style for the entire window for the upcoming frame.
    pub fn new(style: CursorStyle, reason: impl Into<String>) -> Self {
        Self {
            style,
            reason: reason.into(),
        }
    }

    /// Return the requested cursor style.
    pub fn style(&self) -> CursorStyle {
        self.style
    }

    /// Diagnostic reason for applying a whole-window cursor style.
    pub fn reason_text(&self) -> &str {
        &self.reason
    }

    /// Whether a diagnostic reason is present.
    pub fn has_reason(&self) -> bool {
        !self.reason.is_empty()
    }

    /// Content-safe summary for cursor override traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window cursor: style {:?}, reason {}",
            self.style,
            self.has_reason()
        )
    }

    /// Validate the cursor command before it overrides element cursor styles.
    pub fn validate(&self) -> Result<()> {
        validate_window_interaction_reason(&self.reason)
    }
}

/// Checked native z-order policy for native desktop always-on-top windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowZOrderPolicy {
    always_on_top: bool,
    reason: Option<String>,
}

impl WindowZOrderPolicy {
    /// Return whether the platform window should stay above normal app windows.
    pub fn always_on_top(&self) -> bool {
        self.always_on_top
    }

    /// Optional diagnostic reason for enabling always-on-top behavior.
    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// Builder for checked native z-order policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowZOrderPolicyBuilder {
    always_on_top: bool,
    reason: Option<String>,
}

impl WindowZOrderPolicyBuilder {
    /// Keep this window above normal app windows.
    pub fn always_on_top(reason: impl Into<String>) -> Self {
        Self {
            always_on_top: true,
            reason: Some(reason.into()),
        }
    }

    /// Return this window to normal platform z-order behavior.
    pub fn normal() -> Self {
        Self {
            always_on_top: false,
            reason: None,
        }
    }

    /// Return whether the policy enables always-on-top behavior.
    pub fn is_always_on_top(&self) -> bool {
        self.always_on_top
    }

    /// Optional diagnostic reason for enabling always-on-top behavior.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the policy before changing platform z-order state.
    pub fn validate(&self) -> Result<()> {
        if self.always_on_top {
            let Some(reason) = &self.reason else {
                anyhow::bail!("always-on-top windows require a reason");
            };
            validate_window_interaction_reason(reason)?;
        } else {
            anyhow::ensure!(
                self.reason.is_none(),
                "normal z-order policy cannot include a reason"
            );
        }

        Ok(())
    }

    /// Build the checked policy.
    pub fn build_checked(self) -> Result<WindowZOrderPolicy> {
        self.validate()?;
        Ok(WindowZOrderPolicy {
            always_on_top: self.always_on_top,
            reason: self.reason,
        })
    }
}

/// Native system-UI command for platform window affordances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSystemUiCommandKind {
    /// Open the platform character palette for emoji, symbols, and special characters.
    ShowCharacterPalette,
    /// Perform the platform titlebar double-click action.
    TitlebarDoubleClick,
    /// Toggle platform window zoom/maximize behavior.
    ZoomWindow,
}

/// Checked native system-UI command for editor, custom-titlebar, and desktop flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowSystemUiCommand {
    kind: WindowSystemUiCommandKind,
    reason: Option<String>,
}

impl WindowSystemUiCommand {
    /// Open the platform character palette for emoji, symbols, and special characters.
    pub fn show_character_palette() -> Self {
        Self {
            kind: WindowSystemUiCommandKind::ShowCharacterPalette,
            reason: None,
        }
    }

    /// Perform the platform titlebar double-click action.
    pub fn titlebar_double_click() -> Self {
        Self {
            kind: WindowSystemUiCommandKind::TitlebarDoubleClick,
            reason: None,
        }
    }

    /// Toggle platform window zoom/maximize behavior.
    pub fn zoom_window() -> Self {
        Self {
            kind: WindowSystemUiCommandKind::ZoomWindow,
            reason: None,
        }
    }

    /// Attach a diagnostic reason to the command.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The command kind.
    pub fn kind(&self) -> WindowSystemUiCommandKind {
        self.kind
    }

    /// Optional diagnostic reason for the command.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the command before dispatching it to the platform window.
    pub fn validate(&self) -> Result<()> {
        if let Some(reason) = &self.reason {
            validate_window_interaction_reason(reason)?;
        }

        Ok(())
    }
}

/// Native window-tab command for document and workspace window management.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowTabCommandKind {
    /// Merge compatible app windows into a single tabbed window.
    MergeAllWindows,
    /// Move the current tab into a new containing window.
    MoveTabToNewWindow,
    /// Show or hide the native tab overview.
    ToggleTabOverview,
}

/// Checked native window-tab command for document and workspace flows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTabCommand {
    kind: WindowTabCommandKind,
    reason: Option<String>,
}

impl WindowTabCommand {
    /// Merge compatible app windows into a single tabbed window.
    pub fn merge_all_windows() -> Self {
        Self {
            kind: WindowTabCommandKind::MergeAllWindows,
            reason: None,
        }
    }

    /// Move the current tab into a new containing window.
    pub fn move_tab_to_new_window() -> Self {
        Self {
            kind: WindowTabCommandKind::MoveTabToNewWindow,
            reason: None,
        }
    }

    /// Show or hide the native tab overview.
    pub fn toggle_tab_overview() -> Self {
        Self {
            kind: WindowTabCommandKind::ToggleTabOverview,
            reason: None,
        }
    }

    /// Attach a diagnostic reason to the command.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The command kind.
    pub fn kind(&self) -> WindowTabCommandKind {
        self.kind
    }

    /// Optional diagnostic reason for the command.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the command before dispatching it to the platform window.
    pub fn validate(&self) -> Result<()> {
        if let Some(reason) = &self.reason {
            validate_window_interaction_reason(reason)?;
        }

        Ok(())
    }
}

/// Window-manager/custom-chrome command for native desktop frameless windows.
#[derive(Debug, Clone, PartialEq)]
pub enum WindowChromeCommandKind {
    /// Request server-side or client-side platform decorations.
    RequestDecorations(WindowDecorations),
    /// Show the native titlebar/window context menu at a window-space point.
    ShowWindowMenu(Point<Pixels>),
    /// Ask the compositor to begin moving the window.
    StartMove,
    /// Ask the compositor to begin resizing the window from an edge/corner.
    StartResize(ResizeEdge),
}

impl WindowChromeCommandKind {
    /// Stable command key for diagnostics and generated UI.
    pub fn key(&self) -> &'static str {
        match self {
            Self::RequestDecorations(_) => "request-decorations",
            Self::ShowWindowMenu(_) => "show-window-menu",
            Self::StartMove => "start-move",
            Self::StartResize(_) => "start-resize",
        }
    }

    /// Whether this command includes a window-space menu position.
    pub fn has_position(&self) -> bool {
        matches!(self, Self::ShowWindowMenu(_))
    }

    /// Whether this command includes a resize edge.
    pub fn has_resize_edge(&self) -> bool {
        matches!(self, Self::StartResize(_))
    }

    /// Whether this command requests client-side decorations.
    pub fn requests_client_decorations(&self) -> bool {
        matches!(self, Self::RequestDecorations(WindowDecorations::Client))
    }

    /// Whether this command requests server-side decorations.
    pub fn requests_server_decorations(&self) -> bool {
        matches!(self, Self::RequestDecorations(WindowDecorations::Server))
    }
}

/// Checked custom-window-chrome command for titlebars, menus, move, and resize.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowChromeCommand {
    kind: WindowChromeCommandKind,
    reason: Option<String>,
}

impl WindowChromeCommand {
    /// Request server-side or client-side platform decorations.
    pub fn request_decorations(decorations: WindowDecorations) -> Self {
        Self {
            kind: WindowChromeCommandKind::RequestDecorations(decorations),
            reason: None,
        }
    }

    /// Show the native titlebar/window context menu at a window-space point.
    pub fn show_window_menu(position: Point<Pixels>) -> Self {
        Self {
            kind: WindowChromeCommandKind::ShowWindowMenu(position),
            reason: None,
        }
    }

    /// Ask the compositor to begin moving the window.
    pub fn start_move() -> Self {
        Self {
            kind: WindowChromeCommandKind::StartMove,
            reason: None,
        }
    }

    /// Ask the compositor to begin resizing the window from an edge/corner.
    pub fn start_resize(edge: ResizeEdge) -> Self {
        Self {
            kind: WindowChromeCommandKind::StartResize(edge),
            reason: None,
        }
    }

    /// Attach a diagnostic reason to the command.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The command kind.
    pub fn kind(&self) -> &WindowChromeCommandKind {
        &self.kind
    }

    /// Optional diagnostic reason for the command.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Whether a diagnostic reason is present.
    pub fn has_reason(&self) -> bool {
        self.reason.is_some()
    }

    /// Stable command key for diagnostics and generated UI.
    pub fn key(&self) -> &'static str {
        self.kind.key()
    }

    /// Content-safe summary for custom chrome traces and generated UI.
    pub fn to_text(&self) -> String {
        format!(
            "window chrome command: kind {}, reason {}, position {}, resize-edge {}, client-decorations {}, server-decorations {}",
            self.key(),
            self.has_reason(),
            self.kind.has_position(),
            self.kind.has_resize_edge(),
            self.kind.requests_client_decorations(),
            self.kind.requests_server_decorations()
        )
    }

    /// Validate the command before dispatching it to the platform window.
    pub fn validate(&self) -> Result<()> {
        if let WindowChromeCommandKind::ShowWindowMenu(position) = &self.kind {
            anyhow::ensure!(
                position.x.0.is_finite() && position.y.0.is_finite(),
                "window menu position must use finite values"
            );
        }

        if let Some(reason) = &self.reason {
            validate_window_interaction_reason(reason)?;
        }

        Ok(())
    }
}

const MAX_CHECKED_ATLAS_BUDGET_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Checked memory budget for a window's glyph/sprite atlas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowAtlasBudget {
    max_bytes: Option<u64>,
    reason: Option<String>,
}

impl WindowAtlasBudget {
    /// The atlas byte budget to apply, or `None` to clear the budget.
    pub fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    /// Whether this request clears any existing atlas budget.
    pub fn is_clear(&self) -> bool {
        self.max_bytes.is_none()
    }

    /// Optional diagnostic reason for setting or clearing the budget.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

/// Builder for checked window atlas memory budgets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowAtlasBudgetBuilder {
    max_bytes: Option<u64>,
    reason: Option<String>,
}

impl WindowAtlasBudgetBuilder {
    /// Clear any atlas budget and disable renderer-side atlas eviction.
    pub fn clear() -> Self {
        Self {
            max_bytes: None,
            reason: None,
        }
    }

    /// Bound this window's glyph/sprite atlas to the given number of bytes.
    pub fn bytes(max_bytes: u64) -> Self {
        Self {
            max_bytes: Some(max_bytes),
            reason: None,
        }
    }

    /// Attach a diagnostic reason to the budget change.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The atlas byte budget to apply, or `None` to clear the budget.
    pub fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    /// Optional diagnostic reason for setting or clearing the budget.
    pub fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// Validate the budget before it reaches the renderer backend.
    pub fn validate(&self) -> Result<()> {
        if let Some(max_bytes) = self.max_bytes {
            anyhow::ensure!(
                max_bytes > 0,
                "window atlas byte budget must be greater than zero"
            );
            anyhow::ensure!(
                max_bytes <= MAX_CHECKED_ATLAS_BUDGET_BYTES,
                "window atlas byte budget cannot exceed 8 GiB"
            );
        }

        if let Some(reason) = &self.reason {
            validate_window_interaction_reason(reason)?;
        }

        Ok(())
    }

    /// Validate and build a window atlas budget descriptor.
    pub fn build_checked(self) -> Result<WindowAtlasBudget> {
        self.validate()?;
        Ok(WindowAtlasBudget {
            max_bytes: self.max_bytes,
            reason: self.reason,
        })
    }
}

/// Represents the two different phases when dispatching events.
#[derive(Default, Copy, Clone, Debug, Eq, PartialEq)]
pub enum DispatchPhase {
    /// After the capture phase comes the bubble phase, in which mouse event listeners are
    /// invoked front to back and keyboard event listeners are invoked from the focused element
    /// to the root of the element tree. This is the phase you'll most commonly want to use when
    /// registering event listeners.
    #[default]
    Bubble,
    /// During the initial capture phase, mouse event listeners are invoked back to front, and keyboard
    /// listeners are invoked from the root of the tree downward toward the focused element. This phase
    /// is used for special purposes such as clearing the "pressed" state for click events. If
    /// you stop event propagation during this phase, you need to know what you're doing. Handlers
    /// outside of the immediate region may rely on detecting non-local events during this phase.
    Capture,
}

impl DispatchPhase {
    /// Returns true if this represents the "bubble" phase.
    #[inline]
    pub fn bubble(self) -> bool {
        self == DispatchPhase::Bubble
    }

    /// Returns true if this represents the "capture" phase.
    #[inline]
    pub fn capture(self) -> bool {
        self == DispatchPhase::Capture
    }
}

struct WindowInvalidatorInner {
    pub dirty: bool,
    pub draw_phase: DrawPhase,
    pub dirty_views: FxHashSet<EntityId>,
}

#[derive(Clone)]
pub(crate) struct WindowInvalidator {
    inner: Rc<RefCell<WindowInvalidatorInner>>,
}

#[derive(Default)]
#[cfg(any(feature = "inspector", debug_assertions))]
struct DrawRootsTiming {
    layout_us: u64,
    view_render_us: u64,
    taffy_compute_us: u64,
    layout_nodes: u64,
    layout_measure_count: u64,
    layout_measure_us: u64,
    layout_reused: bool,
    paint_us: u64,
}

#[derive(Default)]
#[cfg(not(any(feature = "inspector", debug_assertions)))]
struct DrawRootsTiming;

impl WindowInvalidator {
    pub fn new() -> Self {
        WindowInvalidator {
            inner: Rc::new(RefCell::new(WindowInvalidatorInner {
                dirty: true,
                draw_phase: DrawPhase::None,
                dirty_views: FxHashSet::default(),
            })),
        }
    }

    pub fn invalidate_view(&self, entity: EntityId, cx: &mut App) -> bool {
        let mut inner = self.inner.borrow_mut();
        inner.dirty_views.insert(entity);
        if inner.draw_phase == DrawPhase::None {
            inner.dirty = true;
            cx.push_effect(Effect::Notify { emitter: entity });
            true
        } else {
            false
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.inner.borrow().dirty
    }

    pub fn set_dirty(&self, dirty: bool) {
        self.inner.borrow_mut().dirty = dirty
    }

    pub fn set_phase(&self, phase: DrawPhase) {
        self.inner.borrow_mut().draw_phase = phase
    }

    pub fn take_views(&self) -> FxHashSet<EntityId> {
        mem::take(&mut self.inner.borrow_mut().dirty_views)
    }

    pub fn replace_views(&self, views: FxHashSet<EntityId>) {
        self.inner.borrow_mut().dirty_views = views;
    }

    pub fn not_drawing(&self) -> bool {
        self.inner.borrow().draw_phase == DrawPhase::None
    }

    #[track_caller]
    pub fn debug_assert_paint(&self) {
        debug_assert!(
            matches!(self.inner.borrow().draw_phase, DrawPhase::Paint),
            "this method can only be called during paint"
        );
    }

    #[track_caller]
    pub fn debug_assert_prepaint(&self) {
        debug_assert!(
            matches!(self.inner.borrow().draw_phase, DrawPhase::Prepaint),
            "this method can only be called during request_layout, or prepaint"
        );
    }

    #[track_caller]
    pub fn debug_assert_paint_or_prepaint(&self) {
        debug_assert!(
            matches!(
                self.inner.borrow().draw_phase,
                DrawPhase::Paint | DrawPhase::Prepaint
            ),
            "this method can only be called during request_layout, prepaint, or paint"
        );
    }
}

type AnyObserver = Box<dyn FnMut(&mut Window, &mut App) -> bool + 'static>;

pub(crate) type AnyWindowFocusListener =
    Box<dyn FnMut(&WindowFocusEvent, &mut Window, &mut App) -> bool + 'static>;

pub(crate) struct WindowFocusEvent {
    pub(crate) previous_focus_path: SmallVec<[FocusId; 8]>,
    pub(crate) current_focus_path: SmallVec<[FocusId; 8]>,
}

impl WindowFocusEvent {
    pub fn is_focus_in(&self, focus_id: FocusId) -> bool {
        !self.previous_focus_path.contains(&focus_id) && self.current_focus_path.contains(&focus_id)
    }

    pub fn is_focus_out(&self, focus_id: FocusId) -> bool {
        self.previous_focus_path.contains(&focus_id) && !self.current_focus_path.contains(&focus_id)
    }
}

/// This is provided when subscribing for `Context::on_focus_out` events.
pub struct FocusOutEvent {
    /// A weak focus handle representing what was blurred.
    pub blurred: WeakFocusHandle,
}

slotmap::new_key_type! {
    /// A globally unique identifier for a focusable element.
    pub struct FocusId;
}

thread_local! {
    pub(crate) static ELEMENT_ARENA: RefCell<Arena> = RefCell::new(Arena::new(1024 * 1024));
}

/// Returned when the element arena has been used and so must be cleared before the next draw.
#[must_use]
pub struct ArenaClearNeeded;

impl ArenaClearNeeded {
    /// Clear the element arena.
    pub fn clear(self) {
        ELEMENT_ARENA.with_borrow_mut(|element_arena| {
            element_arena.clear();
        });
    }
}

pub(crate) type FocusMap = RwLock<SlotMap<FocusId, FocusRef>>;
pub(crate) struct FocusRef {
    pub(crate) ref_count: AtomicUsize,
    pub(crate) tab_index: isize,
    pub(crate) tab_stop: bool,
}

impl FocusId {
    /// Obtains whether the element associated with this handle is currently focused.
    pub fn is_focused(&self, window: &Window) -> bool {
        window.focus == Some(*self)
    }

    /// Obtains whether the element associated with this handle contains the focused
    /// element or is itself focused.
    pub fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        window
            .focused(cx)
            .is_some_and(|focused| self.contains(focused.id, window))
    }

    /// Obtains whether the element associated with this handle is contained within the
    /// focused element or is itself focused.
    pub fn within_focused(&self, window: &Window, cx: &App) -> bool {
        let focused = window.focused(cx);
        focused.is_some_and(|focused| focused.id.contains(*self, window))
    }

    /// Obtains whether this handle contains the given handle in the most recently rendered frame.
    pub(crate) fn contains(&self, other: Self, window: &Window) -> bool {
        window
            .rendered_frame
            .dispatch_tree
            .focus_contains(*self, other)
    }
}

/// A handle which can be used to track and manipulate the focused element in a window.
pub struct FocusHandle {
    pub(crate) id: FocusId,
    handles: Arc<FocusMap>,
    /// The index of this element in the tab order.
    pub tab_index: isize,
    /// Whether this element can be focused by tab navigation.
    pub tab_stop: bool,
}

impl std::fmt::Debug for FocusHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("FocusHandle({:?})", self.id))
    }
}

impl FocusHandle {
    pub(crate) fn new(handles: &Arc<FocusMap>) -> Self {
        let id = handles.write().insert(FocusRef {
            ref_count: AtomicUsize::new(1),
            tab_index: 0,
            tab_stop: false,
        });

        Self {
            id,
            tab_index: 0,
            tab_stop: false,
            handles: handles.clone(),
        }
    }

    pub(crate) fn for_id(id: FocusId, handles: &Arc<FocusMap>) -> Option<Self> {
        let lock = handles.read();
        let focus = lock.get(id)?;
        if atomic_incr_if_not_zero(&focus.ref_count) == 0 {
            return None;
        }
        Some(Self {
            id,
            tab_index: focus.tab_index,
            tab_stop: focus.tab_stop,
            handles: handles.clone(),
        })
    }

    /// Sets the tab index of the element associated with this handle.
    pub fn tab_index(mut self, index: isize) -> Self {
        self.tab_index = index;
        if let Some(focus) = self.handles.write().get_mut(self.id) {
            focus.tab_index = index;
        }
        self
    }

    /// Sets whether the element associated with this handle is a tab stop.
    ///
    /// When `false`, the element will not be included in the tab order.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        if let Some(focus) = self.handles.write().get_mut(self.id) {
            focus.tab_stop = tab_stop;
        }
        self
    }

    /// Converts this focus handle into a weak variant, which does not prevent it from being released.
    pub fn downgrade(&self) -> WeakFocusHandle {
        WeakFocusHandle {
            id: self.id,
            handles: Arc::downgrade(&self.handles),
        }
    }

    /// Moves the focus to the element associated with this handle.
    pub fn focus(&self, window: &mut Window) {
        window.focus(self)
    }

    /// Obtains whether the element associated with this handle is currently focused.
    pub fn is_focused(&self, window: &Window) -> bool {
        self.id.is_focused(window)
    }

    /// Obtains whether the element associated with this handle contains the focused
    /// element or is itself focused.
    pub fn contains_focused(&self, window: &Window, cx: &App) -> bool {
        self.id.contains_focused(window, cx)
    }

    /// Obtains whether the element associated with this handle is contained within the
    /// focused element or is itself focused.
    pub fn within_focused(&self, window: &Window, cx: &mut App) -> bool {
        self.id.within_focused(window, cx)
    }

    /// Obtains whether this handle contains the given handle in the most recently rendered frame.
    pub fn contains(&self, other: &Self, window: &Window) -> bool {
        self.id.contains(other.id, window)
    }

    /// Dispatch an action on the element that rendered this focus handle
    pub fn dispatch_action(&self, action: &dyn Action, window: &mut Window, cx: &mut App) {
        if let Some(node_id) = window
            .rendered_frame
            .dispatch_tree
            .focusable_node_id(self.id)
        {
            window.dispatch_action_on_node(node_id, action, cx)
        }
    }
}

impl Clone for FocusHandle {
    fn clone(&self) -> Self {
        Self::for_id(self.id, &self.handles)
            .unwrap_or_else(|| panic!("focus handle {:?} missing during clone", self.id))
    }
}

impl PartialEq for FocusHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for FocusHandle {}

impl Drop for FocusHandle {
    fn drop(&mut self) {
        let handles = self.handles.read();
        let focus = handles
            .get(self.id)
            .unwrap_or_else(|| panic!("focus handle {:?} missing during drop", self.id));
        focus.ref_count.fetch_sub(1, SeqCst);
    }
}

/// A weak reference to a focus handle.
#[derive(Clone, Debug)]
pub struct WeakFocusHandle {
    pub(crate) id: FocusId,
    pub(crate) handles: Weak<FocusMap>,
}

impl WeakFocusHandle {
    /// Attempts to upgrade the [WeakFocusHandle] to a [FocusHandle].
    pub fn upgrade(&self) -> Option<FocusHandle> {
        let handles = self.handles.upgrade()?;
        FocusHandle::for_id(self.id, &handles)
    }
}

impl PartialEq for WeakFocusHandle {
    fn eq(&self, other: &WeakFocusHandle) -> bool {
        self.id == other.id
    }
}

impl Eq for WeakFocusHandle {}

impl PartialEq<FocusHandle> for WeakFocusHandle {
    fn eq(&self, other: &FocusHandle) -> bool {
        self.id == other.id
    }
}

impl PartialEq<WeakFocusHandle> for FocusHandle {
    fn eq(&self, other: &WeakFocusHandle) -> bool {
        self.id == other.id
    }
}

/// Focusable allows users of your view to easily
/// focus it (using window.focus_view(cx, view))
pub trait Focusable: 'static {
    /// Returns the focus handle associated with this view.
    fn focus_handle(&self, cx: &App) -> FocusHandle;
}

impl<V: Focusable> Focusable for Entity<V> {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.read(cx).focus_handle(cx)
    }
}

/// ManagedView is a view (like a Modal, Popover, Menu, etc.)
/// where the lifecycle of the view is handled by another view.
pub trait ManagedView: Focusable + EventEmitter<DismissEvent> + Render {}

impl<M: Focusable + EventEmitter<DismissEvent> + Render> ManagedView for M {}

/// Emitted by implementers of [`ManagedView`] to indicate the view should be dismissed, such as when a view is presented as a modal.
pub struct DismissEvent;

type FrameCallback = Box<dyn FnOnce(&mut Window, &mut App)>;

pub(crate) type AnyMouseListener =
    Box<dyn FnMut(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static>;

#[derive(Clone)]
pub(crate) struct CursorStyleRequest {
    pub(crate) hitbox_id: Option<HitboxId>,
    pub(crate) style: CursorStyle,
}

#[derive(Default, Eq, PartialEq)]
pub(crate) struct HitTest {
    pub(crate) ids: SmallVec<[HitboxId; 8]>,
    pub(crate) hover_hitbox_count: usize,
}

/// A type of window control area that corresponds to the platform window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowControlArea {
    /// An area that allows dragging of the platform window.
    Drag,
    /// An area that allows closing of the platform window.
    Close,
    /// An area that allows maximizing of the platform window.
    Max,
    /// An area that allows minimizing of the platform window.
    Min,
}

impl WindowControlArea {
    /// Stable label for diagnostics and generated custom titlebar hit regions.
    pub fn to_text(self) -> &'static str {
        match self {
            Self::Drag => "drag",
            Self::Close => "close",
            Self::Max => "max",
            Self::Min => "min",
        }
    }

    /// Whether this area moves the window instead of activating a button.
    pub fn is_drag_region(self) -> bool {
        matches!(self, Self::Drag)
    }

    /// Whether this area represents a native window-control button.
    pub fn is_button(self) -> bool {
        !self.is_drag_region()
    }
}

/// An identifier for a [Hitbox] which also includes [HitboxBehavior].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct HitboxId(u64);

impl HitboxId {
    /// Checks if the hitbox with this ID is currently hovered. Except when handling
    /// `ScrollWheelEvent`, this is typically what you want when determining whether to handle mouse
    /// events or paint hover styles.
    ///
    /// See [`Hitbox::is_hovered`] for details.
    pub fn is_hovered(self, window: &Window) -> bool {
        let hit_test = &window.mouse_hit_test;
        for id in hit_test.ids.iter().take(hit_test.hover_hitbox_count) {
            if self == *id {
                return true;
            }
        }
        false
    }

    /// Checks if the hitbox with this ID contains the mouse and should handle scroll events.
    /// Typically this should only be used when handling `ScrollWheelEvent`, and otherwise
    /// `is_hovered` should be used. See the documentation of `Hitbox::is_hovered` for details about
    /// this distinction.
    pub fn should_handle_scroll(self, window: &Window) -> bool {
        window.mouse_hit_test.ids.contains(&self)
    }

    fn next(mut self) -> HitboxId {
        HitboxId(self.0.wrapping_add(1))
    }
}

/// A rectangular region that potentially blocks hitboxes inserted prior.
/// See [Window::insert_hitbox] for more details.
#[derive(Clone, Debug, Deref)]
pub struct Hitbox {
    /// A unique identifier for the hitbox.
    pub id: HitboxId,
    /// The bounds of the hitbox.
    #[deref]
    pub bounds: Bounds<Pixels>,
    /// The content mask when the hitbox was inserted.
    pub content_mask: ContentMask<Pixels>,
    /// Flags that specify hitbox behavior.
    pub behavior: HitboxBehavior,
}

impl Hitbox {
    /// Checks if the hitbox is currently hovered. Except when handling `ScrollWheelEvent`, this is
    /// typically what you want when determining whether to handle mouse events or paint hover
    /// styles.
    ///
    /// This can return `false` even when the hitbox contains the mouse, if a hitbox in front of
    /// this sets `HitboxBehavior::BlockMouse` (`InteractiveElement::occlude`) or
    /// `HitboxBehavior::BlockMouseExceptScroll` (`InteractiveElement::block_mouse_except_scroll`).
    ///
    /// Handling of `ScrollWheelEvent` should typically use `should_handle_scroll` instead.
    /// Concretely, this is due to use-cases like overlays that cause the elements under to be
    /// non-interactive while still allowing scrolling. More abstractly, this is because
    /// `is_hovered` is about element interactions directly under the mouse - mouse moves, clicks,
    /// hover styling, etc. In contrast, scrolling is about finding the current outer scrollable
    /// container.
    pub fn is_hovered(&self, window: &Window) -> bool {
        self.id.is_hovered(window)
    }

    /// Checks if the hitbox contains the mouse and should handle scroll events. Typically this
    /// should only be used when handling `ScrollWheelEvent`, and otherwise `is_hovered` should be
    /// used. See the documentation of `Hitbox::is_hovered` for details about this distinction.
    ///
    /// This can return `false` even when the hitbox contains the mouse, if a hitbox in front of
    /// this sets `HitboxBehavior::BlockMouse` (`InteractiveElement::occlude`).
    pub fn should_handle_scroll(&self, window: &Window) -> bool {
        self.id.should_handle_scroll(window)
    }
}

/// How the hitbox affects mouse behavior.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum HitboxBehavior {
    /// Normal hitbox mouse behavior, doesn't affect mouse handling for other hitboxes.
    #[default]
    Normal,

    /// All hitboxes behind this hitbox will be ignored and so will have `hitbox.is_hovered() ==
    /// false` and `hitbox.should_handle_scroll() == false`. Typically for elements this causes
    /// skipping of all mouse events, hover styles, and tooltips. This flag is set by
    /// [`InteractiveElement::occlude`].
    ///
    /// For mouse handlers that check those hitboxes, this behaves the same as registering a
    /// bubble-phase handler for every mouse event type:
    ///
    /// ```ignore
    /// window.on_mouse_event(move |_: &EveryMouseEventTypeHere, phase, window, cx| {
    ///     if phase == DispatchPhase::Capture && hitbox.is_hovered(window) {
    ///         cx.stop_propagation();
    ///     }
    /// })
    /// ```
    ///
    /// This has effects beyond event handling - any use of hitbox checking, such as hover
    /// styles and tooltops. These other behaviors are the main point of this mechanism. An
    /// alternative might be to not affect mouse event handling - but this would allow
    /// inconsistent UI where clicks and moves interact with elements that are not considered to
    /// be hovered.
    BlockMouse,

    /// All hitboxes behind this hitbox will have `hitbox.is_hovered() == false`, even when
    /// `hitbox.should_handle_scroll() == true`. Typically for elements this causes all mouse
    /// interaction except scroll events to be ignored - see the documentation of
    /// [`Hitbox::is_hovered`] for details. This flag is set by
    /// [`InteractiveElement::block_mouse_except_scroll`].
    ///
    /// For mouse handlers that check those hitboxes, this behaves the same as registering a
    /// bubble-phase handler for every mouse event type **except** `ScrollWheelEvent`:
    ///
    /// ```ignore
    /// window.on_mouse_event(move |_: &EveryMouseEventTypeExceptScroll, phase, window, cx| {
    ///     if phase == DispatchPhase::Bubble && hitbox.should_handle_scroll(window) {
    ///         cx.stop_propagation();
    ///     }
    /// })
    /// ```
    ///
    /// See the documentation of [`Hitbox::is_hovered`] for details of why `ScrollWheelEvent` is
    /// handled differently than other mouse events. If also blocking these scroll events is
    /// desired, then a `cx.stop_propagation()` handler like the one above can be used.
    ///
    /// This has effects beyond event handling - this affects any use of `is_hovered`, such as
    /// hover styles and tooltops. These other behaviors are the main point of this mechanism.
    /// An alternative might be to not affect mouse event handling - but this would allow
    /// inconsistent UI where clicks and moves interact with elements that are not considered to
    /// be hovered.
    BlockMouseExceptScroll,
}

/// An identifier for a tooltip.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct TooltipId(usize);

impl TooltipId {
    /// Checks if the tooltip is currently hovered.
    pub fn is_hovered(&self, window: &Window) -> bool {
        window
            .tooltip_bounds
            .as_ref()
            .is_some_and(|tooltip_bounds| {
                tooltip_bounds.id == *self
                    && tooltip_bounds.bounds.contains(&window.mouse_position())
            })
    }
}

pub(crate) struct TooltipBounds {
    id: TooltipId,
    bounds: Bounds<Pixels>,
}

#[derive(Clone)]
pub(crate) struct TooltipRequest {
    id: TooltipId,
    tooltip: AnyTooltip,
}

pub(crate) struct DeferredDraw {
    current_view: EntityId,
    priority: usize,
    parent_node: DispatchNodeId,
    element_id_stack: SmallVec<[ElementId; 32]>,
    text_style_stack: SmallVec<[TextStyleRefinement; 4]>,
    element: Option<AnyElement>,
    absolute_offset: Point<Pixels>,
    prepaint_range: Range<PrepaintStateIndex>,
    paint_range: Range<PaintIndex>,
}

pub(crate) struct Frame {
    pub(crate) focus: Option<FocusId>,
    pub(crate) window_active: bool,
    pub(crate) element_states: FxHashMap<(GlobalElementId, TypeId), ElementStateBox>,
    accessed_element_states: Vec<(GlobalElementId, TypeId)>,
    pub(crate) mouse_listeners: Vec<Option<AnyMouseListener>>,
    pub(crate) dispatch_tree: DispatchTree,
    pub(crate) scene: Scene,
    pub(crate) hitboxes: Vec<Hitbox>,
    pub(crate) window_control_hitboxes: Vec<(WindowControlArea, Hitbox)>,
    pub(crate) deferred_draws: Vec<DeferredDraw>,
    pub(crate) input_handlers: Vec<Option<PlatformInputHandler>>,
    pub(crate) tooltip_requests: Vec<Option<TooltipRequest>>,
    pub(crate) cursor_styles: Vec<CursorStyleRequest>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) debug_bounds: FxHashMap<String, Bounds<Pixels>>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) next_inspector_instance_ids: FxHashMap<Rc<crate::InspectorElementPath>, usize>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) inspector_hitboxes: FxHashMap<HitboxId, crate::InspectorElementId>,
    pub(crate) tab_stops: TabStopMap,
    pub(crate) accessibility_nodes: Vec<crate::AccessibilityNode>,
    pub(crate) webviews: Vec<PlatformWebView>,
}

#[derive(Clone, Default)]
pub(crate) struct PrepaintStateIndex {
    hitboxes_index: usize,
    tooltips_index: usize,
    deferred_draws_index: usize,
    dispatch_tree_index: usize,
    accessed_element_states_index: usize,
    line_layout_index: LineLayoutIndex,
    #[cfg(any(feature = "test-support", test))]
    debug_bounds_keys: FxHashSet<String>,
}

#[derive(Clone, Default)]
pub(crate) struct PaintIndex {
    pub(crate) scene_index: usize,
    mouse_listeners_index: usize,
    input_handlers_index: usize,
    cursor_styles_index: usize,
    accessed_element_states_index: usize,
    tab_handle_index: usize,
    line_layout_index: LineLayoutIndex,
    #[cfg(any(feature = "test-support", test))]
    debug_bounds_keys: FxHashSet<String>,
}

impl Frame {
    pub(crate) fn new(dispatch_tree: DispatchTree) -> Self {
        Frame {
            focus: None,
            window_active: false,
            element_states: FxHashMap::default(),
            accessed_element_states: Vec::new(),
            mouse_listeners: Vec::new(),
            dispatch_tree,
            scene: Scene::default(),
            hitboxes: Vec::new(),
            window_control_hitboxes: Vec::new(),
            deferred_draws: Vec::new(),
            input_handlers: Vec::new(),
            tooltip_requests: Vec::new(),
            cursor_styles: Vec::new(),

            #[cfg(any(test, feature = "test-support"))]
            debug_bounds: FxHashMap::default(),

            #[cfg(any(feature = "inspector", debug_assertions))]
            next_inspector_instance_ids: FxHashMap::default(),

            #[cfg(any(feature = "inspector", debug_assertions))]
            inspector_hitboxes: FxHashMap::default(),
            tab_stops: TabStopMap::default(),
            accessibility_nodes: Vec::new(),
            webviews: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.element_states.clear();
        self.accessed_element_states.clear();
        self.mouse_listeners.clear();
        self.dispatch_tree.clear();
        self.scene.clear();
        self.input_handlers.clear();
        self.tooltip_requests.clear();
        self.cursor_styles.clear();
        self.hitboxes.clear();
        self.window_control_hitboxes.clear();
        self.deferred_draws.clear();
        self.tab_stops.clear();
        self.accessibility_nodes.clear();
        self.webviews.clear();
        self.focus = None;

        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            self.next_inspector_instance_ids.clear();
            self.inspector_hitboxes.clear();
        }
    }

    pub(crate) fn cursor_style(&self, window: &Window) -> Option<CursorStyle> {
        self.cursor_styles
            .iter()
            .rev()
            .fold_while(None, |style, request| match request.hitbox_id {
                None => Done(Some(request.style)),
                Some(hitbox_id) => Continue(
                    style.or_else(|| hitbox_id.is_hovered(window).then_some(request.style)),
                ),
            })
            .into_inner()
    }

    pub(crate) fn hit_test(&self, position: Point<Pixels>) -> HitTest {
        let mut set_hover_hitbox_count = false;
        let mut hit_test = HitTest::default();
        for hitbox in self.hitboxes.iter().rev() {
            let bounds = hitbox.bounds.intersect(&hitbox.content_mask.bounds);
            if bounds.contains(&position) {
                hit_test.ids.push(hitbox.id);
                if !set_hover_hitbox_count
                    && hitbox.behavior == HitboxBehavior::BlockMouseExceptScroll
                {
                    hit_test.hover_hitbox_count = hit_test.ids.len();
                    set_hover_hitbox_count = true;
                }
                if hitbox.behavior == HitboxBehavior::BlockMouse {
                    break;
                }
            }
        }
        if !set_hover_hitbox_count {
            hit_test.hover_hitbox_count = hit_test.ids.len();
        }
        hit_test
    }

    pub(crate) fn focus_path(&self) -> SmallVec<[FocusId; 8]> {
        self.focus
            .map(|focus_id| self.dispatch_tree.focus_path(focus_id))
            .unwrap_or_default()
    }

    pub(crate) fn finish(&mut self, prev_frame: &mut Self) {
        for element_state_key in &self.accessed_element_states {
            if let Some((element_state_key, element_state)) =
                prev_frame.element_states.remove_entry(element_state_key)
            {
                self.element_states.insert(element_state_key, element_state);
            }
        }

        self.scene.finish();
    }
}

/// Holds the state for a specific window.
pub struct Window {
    pub(crate) handle: AnyWindowHandle,
    pub(crate) invalidator: WindowInvalidator,
    pub(crate) removed: bool,
    pub(crate) platform_window: Box<dyn PlatformWindow>,
    display_id: Option<DisplayId>,
    sprite_atlas: Arc<dyn PlatformAtlas>,
    text_system: Arc<WindowTextSystem>,
    rem_size: Pixels,
    ui_zoom_factor: f32,
    /// The stack of override values for the window's rem size.
    ///
    /// This is used by `with_rem_size` to allow rendering an element tree with
    /// a given rem size.
    rem_size_override_stack: SmallVec<[Pixels; 8]>,
    pub(crate) viewport_size: Size<Pixels>,
    layout_engine: Option<TaffyLayoutEngine>,
    frame_skip: crate::FrameSkip,
    pub(crate) root: Option<AnyView>,
    pub(crate) element_id_stack: SmallVec<[ElementId; 32]>,
    pub(crate) text_style_stack: SmallVec<[TextStyleRefinement; 4]>,
    pub(crate) rendered_entity_stack: SmallVec<[EntityId; 16]>,
    pub(crate) element_offset_stack: SmallVec<[Point<Pixels>; 16]>,
    pub(crate) element_opacity: f32,
    pub(crate) element_transform: TransformationMatrix,
    pub(crate) element_color_filter: ColorFilter,
    pub(crate) rounded_clip: (Bounds<ScaledPixels>, Corners<ScaledPixels>),
    pub(crate) content_mask_stack: SmallVec<[ContentMask<Pixels>; 16]>,
    pub(crate) requested_autoscroll: Option<Bounds<Pixels>>,
    pub(crate) image_cache_stack: SmallVec<[AnyImageCache; 4]>,
    pub(crate) rendered_frame: Frame,
    pub(crate) next_frame: Frame,
    next_hitbox_id: HitboxId,
    pub(crate) next_tooltip_id: TooltipId,
    pub(crate) tooltip_bounds: Option<TooltipBounds>,
    next_frame_callbacks: Rc<RefCell<Vec<FrameCallback>>>,
    pub(crate) dirty_views: FxHashSet<EntityId>,
    focus_listeners: SubscriberSet<(), AnyWindowFocusListener>,
    pub(crate) focus_lost_listeners: SubscriberSet<(), AnyObserver>,
    undo_manager: Rc<RefCell<UndoRedoManager>>,
    default_prevented: bool,
    mouse_position: Point<Pixels>,
    mouse_hit_test: HitTest,
    modifiers: Modifiers,
    capslock: Capslock,
    scale_factor: f32,
    pub(crate) bounds_observers: SubscriberSet<(), AnyObserver>,
    appearance: WindowAppearance,
    pub(crate) appearance_observers: SubscriberSet<(), AnyObserver>,
    content_protection: Option<WindowContentProtection>,
    presentation_policy: Option<WindowPresentationPolicy>,
    active: Rc<Cell<bool>>,
    hovered: Rc<Cell<bool>>,
    pub(crate) needs_present: Rc<Cell<bool>>,
    pub(crate) last_input_timestamp: Rc<Cell<Instant>>,
    pub(crate) keyboard_navigation_active: bool,
    power_mode: PowerMode,
    reduce_motion: bool,
    last_frame_presented_at: Instant,
    pub(crate) refreshing: bool,
    pub(crate) activation_observers: SubscriberSet<(), AnyObserver>,
    pub(crate) focus: Option<FocusId>,
    focus_enabled: bool,
    pending_input: Option<PendingInput>,
    pending_modifier: ModifierState,
    pub(crate) pending_input_observers: SubscriberSet<(), AnyObserver>,
    prompt: Option<RenderablePromptHandle>,
    pub(crate) client_inset: Option<Pixels>,
    pub(crate) accessibility_tree: crate::AccessibilityTree,
    accessibility_parent_stack: SmallVec<[crate::AccessibilityId; 16]>,
    accessibility_path_ids: FxHashMap<(crate::AccessibilityId, u32), crate::AccessibilityId>,
    accessibility_child_ordinals: FxHashMap<crate::AccessibilityId, u32>,
    pub(crate) accessibility_announcements: Vec<String>,
    accessibility_action_router: crate::AccessibilityActionRouter,
    pending_accessibility_actions: Vec<crate::AccessibilityActionRequest>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    inspector: Option<Entity<Inspector>>,
    #[cfg(any(feature = "inspector", debug_assertions))]
    frame_timeline: crate::FrameTimeline,
    #[cfg(any(feature = "inspector", debug_assertions))]
    frame_counter: u64,
    frame_view_render_us: u64,
    frame_taffy_compute_us: u64,
    frame_layout_nodes: u64,
    frame_layout_measure_count: u64,
    frame_layout_measure_us: u64,
    reuse_layout_on_next_frame: bool,
}

#[derive(Clone, Debug, Default)]
struct ModifierState {
    modifiers: Modifiers,
    saw_keystroke: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DrawPhase {
    None,
    Prepaint,
    Paint,
    Focus,
}

#[derive(Default, Debug)]
struct PendingInput {
    keystrokes: SmallVec<[Keystroke; 1]>,
    focus: Option<FocusId>,
    timer: Option<Task<()>>,
}

pub(crate) struct ElementStateBox {
    pub(crate) inner: Box<dyn Any>,
    #[cfg(debug_assertions)]
    pub(crate) type_name: &'static str,
}

fn default_bounds(display_id: Option<DisplayId>, cx: &mut App) -> Bounds<Pixels> {
    const DEFAULT_WINDOW_OFFSET: Point<Pixels> = point(px(0.), px(35.));

    // TODO, BUG: if you open a window with the currently active window
    // on the stack, this will erroneously select the 'unwrap_or_else'
    // code path
    cx.active_window()
        .and_then(|w| w.update(cx, |_, window, _| window.bounds()).ok())
        .map(|mut bounds| {
            bounds.origin += DEFAULT_WINDOW_OFFSET;
            bounds
        })
        .unwrap_or_else(|| {
            let display = display_id
                .map(|id| cx.find_display(id))
                .unwrap_or_else(|| cx.primary_display());

            display
                .map(|display| display.default_bounds())
                .unwrap_or_else(|| Bounds::new(point(px(0.), px(0.)), DEFAULT_WINDOW_SIZE))
        })
}

impl Window {
    pub(crate) fn new(
        handle: AnyWindowHandle,
        options: WindowOptions,
        cx: &mut App,
    ) -> Result<Self> {
        let WindowOptions {
            window_bounds,
            titlebar,
            focus,
            show,
            kind,
            is_movable,
            is_resizable,
            is_minimizable,
            display_id,
            window_background,
            app_id,
            window_min_size,
            window_decorations,
            #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
            tabbing_identifier,
            mouse_passthrough,
            parent,
        } = options;

        let bounds = window_bounds
            .map(|bounds| bounds.get_bounds())
            .unwrap_or_else(|| default_bounds(display_id, cx));
        let initial_title = titlebar
            .as_ref()
            .and_then(|titlebar| titlebar.title.clone());
        #[allow(clippy::redundant_clone)]
        let initial_tabbing_identifier = tabbing_identifier.clone();
        let mut platform_window = cx.platform.open_window(
            handle,
            WindowParams {
                bounds,
                titlebar,
                kind,
                is_movable,
                is_resizable,
                is_minimizable,
                focus,
                show,
                display_id,
                window_min_size,
                #[cfg(target_os = "macos")]
                tabbing_identifier,
                mouse_passthrough,
                parent,
            },
        )?;
        if let Some(title) = initial_title.as_ref() {
            platform_window.set_title(title.as_ref());
        }
        platform_window.set_tabbing_identifier(initial_tabbing_identifier);

        let tab_bar_visible = platform_window.tab_bar_visible();
        SystemWindowTabController::init_visible(cx, tab_bar_visible);
        if let Some(tabs) = platform_window.tabbed_windows() {
            SystemWindowTabController::add_tab(cx, handle.window_id(), tabs);
        }

        let display_id = platform_window.display().map(|display| display.id());
        let sprite_atlas = platform_window.sprite_atlas();
        let mouse_position = platform_window.mouse_position();
        let modifiers = platform_window.modifiers();
        let capslock = platform_window.capslock();
        let content_size = platform_window.content_size();
        let scale_factor = platform_window.scale_factor();
        let appearance = platform_window.appearance();
        let text_system = Arc::new(WindowTextSystem::new(cx.text_system().clone()));
        let invalidator = WindowInvalidator::new();
        let active = Rc::new(Cell::new(platform_window.is_active()));
        let hovered = Rc::new(Cell::new(platform_window.is_hovered()));
        let needs_present = Rc::new(Cell::new(false));
        let next_frame_callbacks: Rc<RefCell<Vec<FrameCallback>>> = Default::default();
        let last_input_timestamp = Rc::new(Cell::new(Instant::now()));
        let power_mode = cx.power_mode();
        let reduce_motion = cx.reduce_motion();

        platform_window
            .request_decorations(window_decorations.unwrap_or(WindowDecorations::Server));
        platform_window.set_background_appearance(window_background);

        if let Some(ref window_open_state) = window_bounds {
            match window_open_state {
                WindowBounds::Fullscreen(_) => platform_window.toggle_fullscreen(),
                WindowBounds::Maximized(_) => platform_window.zoom(),
                WindowBounds::Windowed(_) => {}
            }
        }

        platform_window.on_close(Box::new({
            let window_id = handle.window_id();
            let mut cx = cx.to_async();
            move || {
                let _ = handle.update(&mut cx, |_, window, _| window.remove_window());
                let _ = cx.update(|cx| {
                    SystemWindowTabController::remove_tab(cx, window_id);
                });
            }
        }));
        platform_window.on_request_frame(Box::new({
            let mut cx = cx.to_async();
            let invalidator = invalidator.clone();
            let active = active.clone();
            let needs_present = needs_present.clone();
            let next_frame_callbacks = next_frame_callbacks.clone();
            let last_input_timestamp = last_input_timestamp.clone();
            move |request_frame_options| {
                let next_frame_callbacks = next_frame_callbacks.take();
                if !next_frame_callbacks.is_empty() {
                    handle
                        .update(&mut cx, |_, window, cx| {
                            for callback in next_frame_callbacks {
                                callback(window, cx);
                            }
                        })
                        .log_err();
                }

                let frame_throttled = handle
                    .update(&mut cx, |_, window, cx| {
                        window.power_mode = cx.power_mode();
                        window.reduce_motion = cx.reduce_motion();
                        window.should_throttle_frame(request_frame_options)
                    })
                    .log_err()
                    .unwrap_or(false);

                // Keep presenting the current scene for 1 extra second since the
                // last input to prevent the display from underclocking the refresh rate.
                let needs_present = request_frame_options.require_presentation
                    || needs_present.get()
                    || (active.get()
                        && last_input_timestamp.get().elapsed() < Duration::from_secs(1));

                if !frame_throttled
                    && (invalidator.is_dirty() || request_frame_options.force_render)
                {
                    crate::tracer::trace_global_duration("window.draw_frame", "render", || {
                        measure("frame duration", || {
                            handle
                                .update(&mut cx, |_, window, cx| {
                                    let arena_clear_needed = window.draw(cx);
                                    window.present();
                                    // drop the arena elements after present to reduce latency
                                    arena_clear_needed.clear();
                                })
                                .log_err();
                        })
                    })
                } else if !frame_throttled && needs_present {
                    crate::tracer::trace_global_duration("window.present", "render", || {
                        handle
                            .update(&mut cx, |_, window, _| window.present())
                            .log_err();
                    });
                }

                handle
                    .update(&mut cx, |_, window, _| {
                        window.complete_frame();
                        window.update_frame_polling();
                    })
                    .log_err();
            }
        }));
        platform_window.on_resize(Box::new({
            let mut cx = cx.to_async();
            move |_, _| {
                handle
                    .update(&mut cx, |_, window, cx| window.bounds_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_moved(Box::new({
            let mut cx = cx.to_async();
            move || {
                handle
                    .update(&mut cx, |_, window, cx| window.bounds_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_appearance_changed(Box::new({
            let mut cx = cx.to_async();
            move || {
                handle
                    .update(&mut cx, |_, window, cx| window.appearance_changed(cx))
                    .log_err();
            }
        }));
        platform_window.on_active_status_change(Box::new({
            let mut cx = cx.to_async();
            move |active| {
                handle
                    .update(&mut cx, |_, window, cx| {
                        window.active.set(active);
                        window.modifiers = window.platform_window.modifiers();
                        window.capslock = window.platform_window.capslock();
                        window
                            .activation_observers
                            .clone()
                            .retain(&(), |callback| callback(window, cx));

                        window.bounds_changed(cx);
                        window.refresh();

                        SystemWindowTabController::update_last_active(cx, window.handle.id);
                    })
                    .log_err();
            }
        }));
        platform_window.on_hover_status_change(Box::new({
            let mut cx = cx.to_async();
            move |active| {
                handle
                    .update(&mut cx, |_, window, _| {
                        window.hovered.set(active);
                        window.refresh();
                    })
                    .log_err();
            }
        }));
        platform_window.on_input({
            let mut cx = cx.to_async();
            Box::new(move |event| {
                handle
                    .update(&mut cx, |_, window, cx| window.dispatch_event(event, cx))
                    .log_err()
                    .unwrap_or(DispatchEventResult::default())
            })
        });
        platform_window.on_hit_test_window_control({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, window, _cx| {
                        for (area, hitbox) in &window.rendered_frame.window_control_hitboxes {
                            if window.mouse_hit_test.ids.contains(&hitbox.id) {
                                return Some(*area);
                            }
                        }
                        None
                    })
                    .log_err()
                    .unwrap_or(None)
            })
        });
        platform_window.on_move_tab_to_new_window({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::move_tab_to_new_window(cx, handle.window_id());
                    })
                    .log_err();
            })
        });
        platform_window.on_merge_all_windows({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::merge_all_windows(cx, handle.window_id());
                    })
                    .log_err();
            })
        });
        platform_window.on_select_next_tab({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::select_next_tab(cx, handle.window_id());
                    })
                    .log_err();
            })
        });
        platform_window.on_select_previous_tab({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, _window, cx| {
                        SystemWindowTabController::select_previous_tab(cx, handle.window_id())
                    })
                    .log_err();
            })
        });
        platform_window.on_toggle_tab_bar({
            let mut cx = cx.to_async();
            Box::new(move || {
                handle
                    .update(&mut cx, |_, window, cx| {
                        let tab_bar_visible = window.platform_window.tab_bar_visible();
                        SystemWindowTabController::set_visible(cx, tab_bar_visible);
                    })
                    .log_err();
            })
        });

        if let Some(app_id) = app_id {
            platform_window.set_app_id(&app_id);
        }

        platform_window
            .map_window()
            .context("failed to map platform window")?;

        Ok(Window {
            handle,
            invalidator,
            removed: false,
            platform_window,
            display_id,
            sprite_atlas,
            text_system,
            rem_size: px(16.),
            ui_zoom_factor: 1.0,
            rem_size_override_stack: SmallVec::new(),
            viewport_size: content_size,
            layout_engine: Some(TaffyLayoutEngine::new()),
            frame_skip: crate::FrameSkip::new(),
            root: None,
            element_id_stack: SmallVec::default(),
            text_style_stack: SmallVec::new(),
            rendered_entity_stack: SmallVec::new(),
            element_offset_stack: SmallVec::new(),
            content_mask_stack: SmallVec::new(),
            element_opacity: 1.0,
            element_transform: TransformationMatrix::unit(),
            element_color_filter: ColorFilter::identity(),
            rounded_clip: (Bounds::default(), Corners::default()),
            requested_autoscroll: None,
            rendered_frame: Frame::new(DispatchTree::new(cx.keymap.clone(), cx.actions.clone())),
            next_frame: Frame::new(DispatchTree::new(cx.keymap.clone(), cx.actions.clone())),
            next_frame_callbacks,
            next_hitbox_id: HitboxId(0),
            next_tooltip_id: TooltipId::default(),
            tooltip_bounds: None,
            dirty_views: FxHashSet::default(),
            focus_listeners: SubscriberSet::new(),
            focus_lost_listeners: SubscriberSet::new(),
            undo_manager: Rc::new(RefCell::new(UndoRedoManager::default())),
            default_prevented: true,
            mouse_position,
            mouse_hit_test: HitTest::default(),
            modifiers,
            capslock,
            scale_factor,
            bounds_observers: SubscriberSet::new(),
            appearance,
            appearance_observers: SubscriberSet::new(),
            content_protection: None,
            presentation_policy: None,
            active,
            hovered,
            needs_present,
            last_input_timestamp,
            keyboard_navigation_active: false,
            power_mode,
            reduce_motion,
            last_frame_presented_at: Instant::now(),
            refreshing: false,
            activation_observers: SubscriberSet::new(),
            focus: None,
            focus_enabled: true,
            pending_input: None,
            pending_modifier: ModifierState::default(),
            pending_input_observers: SubscriberSet::new(),
            prompt: None,
            client_inset: None,
            accessibility_tree: crate::AccessibilityTree::new(crate::AccessibilityNode::new(
                crate::AccessibilityRole::Window,
            )),
            accessibility_parent_stack: SmallVec::new(),
            accessibility_path_ids: FxHashMap::default(),
            accessibility_child_ordinals: FxHashMap::default(),
            accessibility_announcements: Vec::new(),
            accessibility_action_router: crate::AccessibilityActionRouter::new(),
            pending_accessibility_actions: Vec::new(),
            image_cache_stack: SmallVec::new(),
            #[cfg(any(feature = "inspector", debug_assertions))]
            inspector: None,
            #[cfg(any(feature = "inspector", debug_assertions))]
            frame_timeline: crate::FrameTimeline::new(),
            #[cfg(any(feature = "inspector", debug_assertions))]
            frame_counter: 0,
            frame_view_render_us: 0,
            frame_taffy_compute_us: 0,
            frame_layout_nodes: 0,
            frame_layout_measure_count: 0,
            frame_layout_measure_us: 0,
            reuse_layout_on_next_frame: false,
        })
    }

    pub(crate) fn undo_manager(&self) -> Rc<RefCell<UndoRedoManager>> {
        self.undo_manager.clone()
    }

    pub(crate) fn new_focus_listener(
        &self,
        value: AnyWindowFocusListener,
    ) -> (Subscription, impl FnOnce() + use<>) {
        self.focus_listeners.insert((), value)
    }
}

/// Outcome of dispatching a [`PlatformInput`] through [`Window::dispatch_event`].
///
/// Returned so callers that synthesize input (for example, app-level integration
/// tests driving the window directly) can observe whether the event propagated
/// or had its default behavior prevented.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DispatchEventResult {
    /// Whether the event was allowed to continue propagating to other handlers.
    pub propagate: bool,
    /// Whether a handler prevented the platform's default behavior for the event.
    pub default_prevented: bool,
}

/// Indicates which region of the window is visible. Content falling outside of this mask will not be
/// rendered. Currently, only rectangular content masks are supported, but we give the mask its own type
/// to leave room to support more complex shapes in the future.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct ContentMask<P: Clone + Debug + Default + PartialEq> {
    /// The bounds
    pub bounds: Bounds<P>,
}

impl ContentMask<Pixels> {
    /// Scale the content mask's pixel units by the given scaling factor,
    /// snapping to the device pixel grid for crisp clipping boundaries.
    pub fn scale(&self, factor: f32) -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: self.bounds.scale_and_snap_conservative(factor),
        }
    }

    /// Intersect the content mask with the given content mask.
    pub fn intersect(&self, other: &Self) -> Self {
        let bounds = self.bounds.intersect(&other.bounds);
        ContentMask { bounds }
    }
}

impl Window {
    fn mark_view_dirty(&mut self, view_id: EntityId) {
        // Mark ancestor views as dirty. If already in the `dirty_views` set, then all its ancestors
        // should already be dirty.
        for view_id in self
            .rendered_frame
            .dispatch_tree
            .view_path(view_id)
            .into_iter()
            .rev()
        {
            if !self.dirty_views.insert(view_id) {
                break;
            }
        }
    }

    /// Compute dock panel bounds from the given workspace for this window.
    pub fn dock_panels(
        &self,
        workspace: &crate::workspace::Workspace,
    ) -> crate::workspace::DockLayout {
        workspace.compute_dock_layout(self.viewport_size)
    }

    /// Registers a callback to be invoked when the window appearance changes.
    pub fn observe_window_appearance(
        &self,
        mut callback: impl FnMut(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let (subscription, activate) = self.appearance_observers.insert(
            (),
            Box::new(move |window, cx| {
                callback(window, cx);
                true
            }),
        );
        activate();
        subscription
    }

    /// Replaces the root entity of the window with a new one.
    pub fn replace_root<E>(
        &mut self,
        cx: &mut App,
        build_view: impl FnOnce(&mut Window, &mut Context<E>) -> E,
    ) -> Entity<E>
    where
        E: 'static + Render,
    {
        let view = cx.new(|cx| build_view(self, cx));
        self.root = Some(view.clone().into());
        self.refresh();
        view
    }

    /// Returns the root entity of the window, if it has one.
    pub fn root<E>(&self) -> Option<Option<Entity<E>>>
    where
        E: 'static + Render,
    {
        self.root
            .as_ref()
            .map(|view| view.clone().downcast::<E>().ok())
    }

    /// Obtain a handle to the window that belongs to this context.
    pub fn window_handle(&self) -> AnyWindowHandle {
        self.handle
    }

    pub(crate) fn sprite_atlas(&self) -> &Arc<dyn PlatformAtlas> {
        &self.sprite_atlas
    }

    /// Mark the window as dirty, scheduling it to be redrawn on the next frame.
    pub fn refresh(&mut self) {
        if self.invalidator.not_drawing() {
            self.refreshing = true;
            self.invalidator.set_dirty(true);
            self.update_frame_polling();
        }
    }

    /// Schedule a redraw that preserves the retained subtree caches.
    ///
    /// Unlike [`Window::refresh`], this deliberately does **not** set `self.refreshing`. The
    /// `refreshing` flag disables every subtree cache (see `view.rs` and `cached.rs`, both gated
    /// on `!window.refreshing`), so using `refresh()` here would force a full re-render + re-paint
    /// of the entire tree on every scroll event. Scrolling only changes a scroll offset that is
    /// applied at prepaint time, so unchanged sibling subtrees can replay straight from cache.
    ///
    /// It deliberately does **not** request taffy layout-solve reuse either. That fast-path
    /// (`TaffyLayoutEngine::compute_layout` early-return) skips the per-element measure callbacks,
    /// i.e. text shaping. A non-cached view re-renders fresh `StyledText` every frame and then
    /// re-runs its prepaint (view.rs has no cache replay for non-`cached()` views), so reusing the
    /// solve leaves that text unmeasured and panics at prepaint ("measurement has not been
    /// performed"). A full layout is cheap here because only the visible content is in the tree.
    pub(crate) fn refresh_preserving_caches(&mut self) {
        if self.invalidator.not_drawing() {
            self.invalidator.set_dirty(true);
            self.update_frame_polling();
        }
    }

    pub(crate) fn record_view_render_duration(&mut self, duration: Duration) {
        let elapsed_us = duration.as_micros().min(u64::MAX as u128) as u64;
        self.frame_view_render_us = self.frame_view_render_us.saturating_add(elapsed_us);
    }

    fn record_taffy_compute_duration(&mut self, duration: Duration) {
        let elapsed_us = duration.as_micros().min(u64::MAX as u128) as u64;
        self.frame_taffy_compute_us = self.frame_taffy_compute_us.saturating_add(elapsed_us);
    }

    pub(crate) fn record_layout_measure_duration(&mut self, duration: Duration) {
        let elapsed_us = duration.as_micros().min(u64::MAX as u128) as u64;
        self.frame_layout_measure_count = self.frame_layout_measure_count.saturating_add(1);
        self.frame_layout_measure_us = self.frame_layout_measure_us.saturating_add(elapsed_us);
    }

    /// Enable or disable whole-frame damage skipping. When enabled, a frame whose scene
    /// is byte-identical to the previously presented one is not re-rasterized or
    /// re-presented — the compositor keeps the prior contents — which removes redundant
    /// GPU work for mostly-static UIs. Off by default; frames containing live external
    /// surfaces (e.g. video) are never skipped.
    pub fn set_frame_skip_enabled(&mut self, enabled: bool) {
        self.frame_skip.set_enabled(enabled);
    }

    /// Validate and apply window render/performance policy.
    pub fn set_render_policy_checked(
        &mut self,
        policy: WindowRenderPolicyBuilder,
    ) -> Result<WindowRenderPolicy> {
        let policy = policy.build_checked()?;
        self.set_frame_skip_enabled(policy.frame_skip_enabled());
        Ok(policy)
    }

    /// Whether whole-frame damage skipping is currently enabled.
    pub fn frame_skip_enabled(&self) -> bool {
        self.frame_skip.is_enabled()
    }

    fn should_throttle_frame(&self, request_frame_options: crate::RequestFrameOptions) -> bool {
        !request_frame_options.force_render
            && self
                .minimum_frame_interval()
                .is_some_and(|interval| self.last_frame_presented_at.elapsed() < interval)
    }

    fn minimum_frame_interval(&self) -> Option<Duration> {
        (self.power_mode == PowerMode::LowPower).then_some(Duration::from_millis(33))
    }

    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn rendered_scene(&self) -> &Scene {
        &self.rendered_frame.scene
    }

    /// Invalidates retained subtree cache state for elements whose local id matches `element_id`.
    pub fn invalidate_cache(&mut self, element_id: impl Into<ElementId>) {
        fn invalidate_in_frame(frame: &mut Frame, element_id: &ElementId, state_type: TypeId) {
            frame.element_states.retain(|(global_id, type_id), _| {
                *type_id != state_type || global_id.0.last() != Some(element_id)
            });
            frame
                .accessed_element_states
                .retain(|(global_id, type_id)| {
                    *type_id != state_type || global_id.0.last() != Some(element_id)
                });
        }

        let element_id = element_id.into();
        let state_type = TypeId::of::<crate::cache::SubtreeCacheState>();

        invalidate_in_frame(&mut self.rendered_frame, &element_id, state_type);
        invalidate_in_frame(&mut self.next_frame, &element_id, state_type);

        self.invalidator.set_dirty(true);
        if self.invalidator.not_drawing() {
            self.refreshing = true;
        }
        self.update_frame_polling();
    }

    fn should_poll_for_frames(&self) -> bool {
        self.invalidator.is_dirty()
            || !self.next_frame_callbacks.borrow().is_empty()
            || self.needs_present.get()
            || (self.active.get()
                && self.last_input_timestamp.get().elapsed() < Duration::from_secs(1))
    }

    fn update_frame_polling(&self) {
        self.platform_window
            .set_frame_polling(self.should_poll_for_frames());
    }

    /// Close this window.
    pub fn remove_window(&mut self) {
        self.removed = true;
    }

    /// Obtain the currently focused [`FocusHandle`]. If no elements are focused, returns `None`.
    pub fn focused(&self, cx: &App) -> Option<FocusHandle> {
        self.focus
            .and_then(|id| FocusHandle::for_id(id, &cx.focus_handles))
    }

    /// Move focus to the element associated with the given [`FocusHandle`].
    pub fn focus(&mut self, handle: &FocusHandle) {
        if !self.focus_enabled || self.focus == Some(handle.id) {
            return;
        }

        self.focus = Some(handle.id);
        self.clear_pending_keystrokes();
        self.refresh();
    }

    /// Returns whether the currently focused element can undo its next shared-history change.
    pub fn has_undo(&self, cx: &App) -> bool {
        let Some(focus_handle) = self.focused(cx) else {
            return false;
        };

        self.undo_manager
            .borrow()
            .can_undo_for_source(focus_handle.id)
    }

    /// Returns whether the currently focused element can redo its next shared-history change.
    pub fn has_redo(&self, cx: &App) -> bool {
        let Some(focus_handle) = self.focused(cx) else {
            return false;
        };

        self.undo_manager
            .borrow()
            .can_redo_for_source(focus_handle.id)
    }

    /// Returns the label for the currently focused element's next undo action, if any.
    pub fn undo_label(&self, cx: &App) -> Option<SharedString> {
        let focus_handle = self.focused(cx)?;
        let manager = self.undo_manager.borrow();
        if !manager.can_undo_for_source(focus_handle.id) {
            return None;
        }

        let description = manager
            .undo_descriptions()
            .into_iter()
            .next()
            .map(str::to_owned);
        drop(manager);

        description.map(SharedString::from)
    }

    /// Returns the label for the currently focused element's next redo action, if any.
    pub fn redo_label(&self, cx: &App) -> Option<SharedString> {
        let focus_handle = self.focused(cx)?;
        let manager = self.undo_manager.borrow();
        if !manager.can_redo_for_source(focus_handle.id) {
            return None;
        }

        let description = manager
            .redo_descriptions()
            .into_iter()
            .next()
            .map(str::to_owned);
        drop(manager);

        description.map(SharedString::from)
    }

    /// Return a stable identity for an accessible element without an explicit id.
    pub(crate) fn next_anonymous_accessibility_id(&mut self) -> crate::AccessibilityId {
        let parent = self
            .accessibility_parent_stack
            .last()
            .copied()
            .unwrap_or(self.accessibility_tree.root);
        let ordinal = self.accessibility_child_ordinals.entry(parent).or_default();
        let key = (parent, *ordinal);
        *ordinal = ordinal.saturating_add(1);
        *self
            .accessibility_path_ids
            .entry(key)
            .or_insert_with(crate::AccessibilityId::new)
    }

    /// Register an accessibility node for the current frame.
    pub fn register_accessibility_node(&mut self, mut node: crate::AccessibilityNode) {
        if node.parent.is_none() {
            node.parent = self.accessibility_parent_stack.last().copied();
        }
        self.next_frame.accessibility_nodes.push(node);
    }

    pub(crate) fn accessibility_node_index(&self) -> usize {
        self.next_frame.accessibility_nodes.len()
    }

    pub(crate) fn accessibility_nodes_since(&self, index: usize) -> Vec<crate::AccessibilityNode> {
        self.next_frame.accessibility_nodes[index..].to_vec()
    }

    pub(crate) fn replay_accessibility_nodes(&mut self, nodes: &[crate::AccessibilityNode]) {
        self.next_frame.accessibility_nodes.extend_from_slice(nodes);
    }

    /// Register an accessibility node, stamping its screen-space bounds from the
    /// element's laid-out bounds unless the node already carries explicit bounds.
    pub fn register_accessibility_node_at(
        &mut self,
        mut node: crate::AccessibilityNode,
        bounds: crate::Bounds<crate::Pixels>,
    ) {
        if node.bounds.is_none() {
            node.bounds = Some(crate::AccessibilityRect::from_bounds(bounds));
        }
        self.register_accessibility_node(node);
    }

    /// Rebuild the accessibility tree from nodes collected during the frame.
    pub fn update_accessibility_tree(&mut self) {
        // AccessKit adapters retain the consumer tree after activation. The root
        // identifier is therefore a window-lifetime identity, not a frame-lifetime
        // identity. Replacing it on every draw leaves the consumer applying child
        // changes to a root that no longer exists and can abort the host process.
        let root_id = self.accessibility_tree.root;
        let mut root = crate::AccessibilityNode::new(crate::AccessibilityRole::Window);
        root.id = root_id;
        let mut tree = crate::AccessibilityTree::new(root);
        let nodes = self
            .rendered_frame
            .accessibility_nodes
            .drain(..)
            .collect::<Vec<_>>();
        for node in &nodes {
            tree.insert(node.clone());
        }
        for node in nodes {
            tree.set_parent(node.id, node.parent.unwrap_or(root_id));
        }
        self.accessibility_tree = tree;
    }

    /// Return the latest accessibility tree produced for this window.
    ///
    /// This is useful for diagnostics, automated accessibility verification,
    /// and platform integrations that need a read-only semantic snapshot.
    pub fn accessibility_tree(&self) -> &crate::AccessibilityTree {
        &self.accessibility_tree
    }

    /// Focus the given accessibility node in the tree.
    pub fn focus_accessibility_node(&mut self, id: crate::AccessibilityId) {
        if let Some(node) = self.accessibility_tree.get_mut(id) {
            node.states |= crate::AccessibilityState::FOCUSED;
        }
        for (_, node) in self.accessibility_tree.nodes.iter_mut() {
            if node.id != id {
                node.states &= !crate::AccessibilityState::FOCUSED;
            }
        }
    }

    /// Validate and focus an accessibility node in the current tree.
    pub fn focus_accessibility_node_checked(
        &mut self,
        focus: AccessibilityFocusBuilder,
    ) -> Result<crate::AccessibilityId> {
        let id = focus.build_checked(self)?;
        self.focus_accessibility_node(id);
        Ok(id)
    }

    /// Announce a message to assistive technology.
    pub fn announce_accessibility(&mut self, message: &str) {
        self.accessibility_announcements.push(message.to_string());
    }

    /// Validate and announce a message to assistive technology.
    pub fn announce_accessibility_checked(
        &mut self,
        announcement: AccessibilityAnnouncementBuilder,
    ) -> Result<String> {
        let message = announcement.build_checked()?;
        self.announce_accessibility(&message);
        Ok(message)
    }

    /// Register a handler for an assistive-technology action on one node.
    ///
    /// Handlers run after the platform accessibility adapter reports a
    /// normalized action request for the current tree.
    pub fn on_accessibility_action(
        &mut self,
        node_id: crate::AccessibilityId,
        action: crate::AccessibilityAction,
        handler: impl FnMut(crate::AccessibilityActionRequest) + 'static,
    ) {
        self.accessibility_action_router
            .on_action(node_id, action, handler);
    }

    /// Return whether this window has a handler for one accessibility action.
    pub fn has_accessibility_action_handler(
        &self,
        node_id: crate::AccessibilityId,
        action: crate::AccessibilityAction,
    ) -> bool {
        self.accessibility_action_router
            .has_handler(node_id, action)
    }

    /// Drain normalized accessibility action requests delivered since the last drain.
    pub fn drain_accessibility_actions(&mut self) -> Vec<crate::AccessibilityActionRequest> {
        mem::take(&mut self.pending_accessibility_actions)
    }

    fn dispatch_accessibility_actions(&mut self, actions: Vec<crate::AccessibilityActionRequest>) {
        for request in actions {
            self.pending_accessibility_actions.push(request.clone());
            if !self.accessibility_action_router.dispatch(request.clone()) {
                log::debug!(
                    "Unhandled accessibility action request: {:?} on node {}",
                    request.action,
                    request.node_id.0
                );
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn dispatch_accessibility_action_for_test(
        &mut self,
        request: crate::AccessibilityActionRequest,
    ) {
        self.dispatch_accessibility_actions(vec![request]);
    }

    /// Remove focus from all elements within this context's window.
    pub fn blur(&mut self) {
        if !self.focus_enabled {
            return;
        }

        self.focus = None;
        self.refresh();
    }

    /// Blur the window and don't allow anything in it to be focused again.
    pub fn disable_focus(&mut self) {
        self.blur();
        self.focus_enabled = false;
    }

    /// Move focus to next tab stop.
    pub fn focus_next(&mut self) {
        if !self.focus_enabled {
            return;
        }

        if let Some(handle) = self.rendered_frame.tab_stops.next(self.focus.as_ref()) {
            self.focus(&handle)
        }
    }

    /// Move focus to previous tab stop.
    pub fn focus_prev(&mut self) {
        if !self.focus_enabled {
            return;
        }

        if let Some(handle) = self.rendered_frame.tab_stops.prev(self.focus.as_ref()) {
            self.focus(&handle)
        }
    }

    /// Move focus to the next tab stop within the currently focused tab group.
    pub fn focus_next_in_group(&mut self) {
        if !self.focus_enabled {
            return;
        }

        if let Some(handle) = self
            .rendered_frame
            .tab_stops
            .next_in_group(self.focus.as_ref())
        {
            self.focus(&handle)
        }
    }

    /// Move focus to the previous tab stop within the currently focused tab group.
    pub fn focus_prev_in_group(&mut self) {
        if !self.focus_enabled {
            return;
        }

        if let Some(handle) = self
            .rendered_frame
            .tab_stops
            .prev_in_group(self.focus.as_ref())
        {
            self.focus(&handle)
        }
    }

    pub(crate) fn is_keyboard_navigation_active(&self) -> bool {
        self.keyboard_navigation_active
    }

    #[inline]
    pub(crate) fn with_accessibility_parent<R>(
        &mut self,
        parent: crate::AccessibilityId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();
        self.accessibility_parent_stack.push(parent);
        let result = f(self);
        self.accessibility_parent_stack.pop();
        result
    }

    /// Accessor for the text system.
    pub fn text_system(&self) -> &Arc<WindowTextSystem> {
        &self.text_system
    }

    /// The current text style. Which is composed of all the style refinements provided to `with_text_style`.
    pub fn text_style(&self) -> TextStyle {
        let mut style = TextStyle::default();
        for refinement in &self.text_style_stack {
            style.refine(refinement);
        }
        style
    }

    /// Check if the platform window is maximized
    /// On some platforms (namely Windows) this is different than the bounds being the size of the display
    pub fn is_maximized(&self) -> bool {
        self.platform_window.is_maximized()
    }

    /// request a certain window decoration (Wayland)
    pub fn request_decorations(&self, decorations: WindowDecorations) {
        self.platform_window.request_decorations(decorations);
    }

    /// Validate and perform a custom-window-chrome command.
    pub fn perform_window_chrome_command_checked(
        &self,
        command: WindowChromeCommand,
    ) -> Result<WindowChromeCommand> {
        command.validate()?;
        match command.kind() {
            WindowChromeCommandKind::RequestDecorations(decorations) => {
                self.request_decorations(*decorations);
            }
            WindowChromeCommandKind::ShowWindowMenu(position) => {
                self.show_window_menu(*position);
            }
            WindowChromeCommandKind::StartMove => self.start_window_move(),
            WindowChromeCommandKind::StartResize(edge) => self.start_window_resize(*edge),
        }
        Ok(command)
    }

    /// Start a window resize operation (Wayland)
    pub fn start_window_resize(&self, edge: ResizeEdge) {
        self.platform_window.start_window_resize(edge);
    }

    /// Return the `WindowBounds` to indicate that how a window should be opened
    /// after it has been closed
    pub fn window_bounds(&self) -> WindowBounds {
        self.platform_window.window_bounds()
    }

    /// Return the `WindowBounds` excluding insets (Wayland and X11)
    pub fn inner_window_bounds(&self) -> WindowBounds {
        self.platform_window.inner_window_bounds()
    }

    /// Dispatch the given action on the currently focused element.
    pub fn dispatch_action(&mut self, action: Box<dyn Action>, cx: &mut App) {
        let focus_id = self.focused(cx).map(|handle| handle.id);

        let window = self.handle;
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    let node_id = window.focus_node_id_in_rendered_frame(focus_id);
                    window.dispatch_action_on_node(node_id, action.as_ref(), cx);
                })
                .log_err();
        })
    }

    pub(crate) fn dispatch_keystroke_observers(
        &mut self,
        event: &dyn Any,
        action: Option<Box<dyn Action>>,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() else {
            return;
        };

        cx.keystroke_observers.clone().retain(&(), move |callback| {
            (callback)(
                &KeystrokeEvent {
                    keystroke: key_down_event.keystroke.clone(),
                    action: action.as_ref().map(|action| action.boxed_clone()),
                    context_stack: context_stack.clone(),
                },
                self,
                cx,
            )
        });
    }

    pub(crate) fn dispatch_keystroke_interceptors(
        &mut self,
        event: &dyn Any,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() else {
            return;
        };

        cx.keystroke_interceptors
            .clone()
            .retain(&(), move |callback| {
                (callback)(
                    &KeystrokeEvent {
                        keystroke: key_down_event.keystroke.clone(),
                        action: None,
                        context_stack: context_stack.clone(),
                    },
                    self,
                    cx,
                )
            });
    }

    /// Schedules the given function to be run at the end of the current effect cycle, allowing entities
    /// that are currently on the stack to be returned to the app.
    pub fn defer(&self, cx: &mut App, f: impl FnOnce(&mut Window, &mut App) + 'static) {
        let handle = self.handle;
        cx.defer(move |cx| {
            handle.update(cx, |_, window, cx| f(window, cx)).ok();
        });
    }

    /// Subscribe to events emitted by a entity.
    /// The entity to which you're subscribing must implement the [`EventEmitter`] trait.
    /// The callback will be invoked a handle to the emitting entity, the event, and a window context for the current window.
    pub fn observe<T: 'static>(
        &mut self,
        observed: &Entity<T>,
        cx: &mut App,
        mut on_notify: impl FnMut(Entity<T>, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let entity_id = observed.entity_id();
        let observed = observed.downgrade();
        let window_handle = self.handle;
        cx.new_observer(
            entity_id,
            Box::new(move |cx| {
                window_handle
                    .update(cx, |_, window, cx| {
                        if let Some(handle) = observed.upgrade() {
                            on_notify(handle, window, cx);
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false)
            }),
        )
    }

    /// Subscribe to events emitted by a entity.
    /// The entity to which you're subscribing must implement the [`EventEmitter`] trait.
    /// The callback will be invoked a handle to the emitting entity, the event, and a window context for the current window.
    pub fn subscribe<Emitter, Evt>(
        &mut self,
        entity: &Entity<Emitter>,
        cx: &mut App,
        mut on_event: impl FnMut(Entity<Emitter>, &Evt, &mut Window, &mut App) + 'static,
    ) -> Subscription
    where
        Emitter: EventEmitter<Evt>,
        Evt: 'static,
    {
        let entity_id = entity.entity_id();
        let handle = entity.downgrade();
        let window_handle = self.handle;
        cx.new_subscription(
            entity_id,
            (
                TypeId::of::<Evt>(),
                Box::new(move |event, cx| {
                    window_handle
                        .update(cx, |_, window, cx| {
                            if let Some(entity) = handle.upgrade() {
                                let Some(event) = event.downcast_ref() else {
                                    return false;
                                };
                                on_event(entity, event, window, cx);
                                true
                            } else {
                                false
                            }
                        })
                        .unwrap_or(false)
                }),
            ),
        )
    }

    /// Register a callback to be invoked when the given `Entity` is released.
    pub fn observe_release<T>(
        &self,
        entity: &Entity<T>,
        cx: &mut App,
        mut on_release: impl FnOnce(&mut T, &mut Window, &mut App) + 'static,
    ) -> Subscription
    where
        T: 'static,
    {
        let entity_id = entity.entity_id();
        let window_handle = self.handle;
        let (subscription, activate) = cx.release_listeners.insert(
            entity_id,
            Box::new(move |entity, cx| {
                let Some(entity) = entity.downcast_mut() else {
                    return;
                };
                let _ = window_handle.update(cx, |_, window, cx| on_release(entity, window, cx));
            }),
        );
        activate();
        subscription
    }

    /// Creates an [`AsyncWindowContext`], which has a static lifetime and can be held across
    /// await points in async code.
    pub fn to_async(&self, cx: &App) -> AsyncWindowContext {
        AsyncWindowContext::new_context(cx.to_async(), self.handle)
    }

    /// Schedule the given closure to be run directly after the current frame is rendered.
    pub fn on_next_frame(&self, callback: impl FnOnce(&mut Window, &mut App) + 'static) {
        RefCell::borrow_mut(&self.next_frame_callbacks).push(Box::new(callback));
        self.update_frame_polling();
    }

    /// Returns the current system power mode snapshot for this frame.
    pub fn power_mode(&self) -> PowerMode {
        self.power_mode
    }

    /// Returns whether the OS "reduce motion" accessibility preference is active for this frame.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Returns whether animations should run at full fidelity for this frame.
    ///
    /// False when the system is in low-power mode or the user has enabled the
    /// "reduce motion" accessibility preference.
    pub fn animations_enabled(&self) -> bool {
        self.power_mode != PowerMode::LowPower && !self.reduce_motion
    }

    /// Schedule a frame to be drawn on the next animation frame.
    ///
    /// This is useful for elements that need to animate continuously, such as a video player or an animated GIF.
    /// It will cause the window to redraw on the next frame, even if no other changes have occurred.
    ///
    /// If called from within a view, it will notify that view on the next frame. Otherwise, it will refresh the entire window.
    pub fn request_animation_frame(&self) {
        let entity = self.current_view();
        self.on_next_frame(move |_, cx| cx.notify(entity));
    }

    /// Spawn the future returned by the given closure on the application thread pool.
    /// The closure is provided a handle to the current window and an `AsyncWindowContext` for
    /// use within your future.
    #[track_caller]
    pub fn spawn<AsyncFn, R>(&self, cx: &App, f: AsyncFn) -> Task<R>
    where
        R: 'static,
        AsyncFn: AsyncFnOnce(&mut AsyncWindowContext) -> R + 'static,
    {
        let handle = self.handle;
        cx.spawn(async move |app| {
            let mut async_window_cx = AsyncWindowContext::new_context(app.clone(), handle);
            f(&mut async_window_cx).await
        })
    }

    fn bounds_changed(&mut self, cx: &mut App) {
        self.scale_factor = self.platform_window.scale_factor();
        self.viewport_size = self.platform_window.content_size();
        self.display_id = self.platform_window.display().map(|display| display.id());

        // The content checksum can't see the viewport/scale change, so force the next
        // frame to present rather than risk skipping a resize.
        self.frame_skip.invalidate();
        self.refresh();

        self.bounds_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    /// Returns the bounds of the current window in the global coordinate space, which could span across multiple displays.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.platform_window.bounds()
    }

    /// Set the content size of the window.
    pub fn resize(&mut self, size: Size<Pixels>) {
        self.platform_window.resize(size);
    }

    /// Validate and set the content size of the window.
    pub fn resize_checked(&mut self, size: WindowContentSizeBuilder) -> Result<WindowContentSize> {
        let size = size.build_checked()?;
        self.resize(size.size());
        Ok(size)
    }

    /// Returns whether or not the window is currently fullscreen
    pub fn is_fullscreen(&self) -> bool {
        self.platform_window.is_fullscreen()
    }

    /// Capture a runtime snapshot of native window state for diagnostics and generated chrome.
    pub fn runtime_snapshot(&self) -> WindowRuntimeSnapshot {
        WindowRuntimeSnapshot {
            bounds: self.bounds(),
            window_bounds: self.window_bounds(),
            viewport_size: self.viewport_size(),
            display_id: self.display_id,
            scale_factor: self.scale_factor(),
            appearance: self.appearance(),
            active: self.is_window_active(),
            hovered: self.is_window_hovered(),
            visible: self.is_window_visible(),
            fullscreen: self.is_fullscreen(),
            maximized: self.is_maximized(),
            power_mode: self.power_mode(),
            reduce_motion: self.reduce_motion(),
        }
    }

    /// Capture and validate a runtime window snapshot.
    pub fn runtime_snapshot_checked(
        &self,
        query: WindowRuntimeSnapshotQueryBuilder,
    ) -> Result<WindowRuntimeSnapshot> {
        let snapshot = self.runtime_snapshot();
        query.validate_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    pub(crate) fn appearance_changed(&mut self, cx: &mut App) {
        self.appearance = self.platform_window.appearance();

        self.appearance_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    /// Returns the appearance of the current window.
    pub fn appearance(&self) -> WindowAppearance {
        self.appearance
    }

    /// Returns the size of the drawable area within the window.
    pub fn viewport_size(&self) -> Size<Pixels> {
        self.viewport_size
    }

    /// Returns whether this window is focused by the operating system (receiving key events).
    pub fn is_window_active(&self) -> bool {
        self.active.get()
    }

    /// Returns whether this window is considered to be the window
    /// that currently owns the mouse cursor.
    /// On mac, this is equivalent to `is_window_active`.
    pub fn is_window_hovered(&self) -> bool {
        if cfg!(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "freebsd"
        )) {
            self.hovered.get()
        } else {
            self.is_window_active()
        }
    }

    /// Toggle zoom on the window.
    pub fn zoom_window(&self) {
        self.platform_window.zoom();
    }

    /// Opens the native title bar context menu, useful when implementing client side decorations (Wayland and X11)
    pub fn show_window_menu(&self, position: Point<Pixels>) {
        self.platform_window.show_window_menu(position)
    }

    /// Tells the compositor to take control of window movement (Wayland and X11)
    ///
    /// Events may not be received during a move operation.
    pub fn start_window_move(&self) {
        self.platform_window.start_window_move()
    }

    /// When using client side decorations, set this to the width of the invisible decorations (Wayland and X11)
    pub fn set_client_inset(&mut self, inset: Pixels) {
        self.client_inset = Some(inset);
        self.platform_window.set_client_inset(inset);
    }

    /// Validate and set the custom-chrome client inset.
    pub fn set_client_inset_checked(
        &mut self,
        inset: WindowClientInsetBuilder,
    ) -> Result<WindowClientInset> {
        let inset = inset.build_checked()?;
        self.set_client_inset(inset.inset());
        Ok(inset)
    }

    /// Returns the client_inset value by [`Self::set_client_inset`].
    pub fn client_inset(&self) -> Option<Pixels> {
        self.client_inset
    }

    /// Returns whether the title bar window controls need to be rendered by the application (Wayland and X11)
    pub fn window_decorations(&self) -> Decorations {
        self.platform_window.window_decorations()
    }

    /// Returns which window controls are currently visible (Wayland)
    pub fn window_controls(&self) -> WindowControls {
        self.platform_window.window_controls()
    }

    /// Updates the window's title at the platform level.
    pub fn set_window_title(&mut self, title: &str) {
        self.platform_window.set_title(title);
    }

    /// Validate and update the window's title at the platform level.
    pub fn set_window_title_checked(&mut self, title: WindowTitleBuilder) -> Result<()> {
        let title = title.build_checked()?;
        self.set_window_title(&title);
        Ok(())
    }

    /// Sets the application identifier.
    pub fn set_app_id(&mut self, app_id: &str) {
        self.platform_window.set_app_id(app_id);
    }

    /// Validate and set the application identifier.
    pub fn set_app_id_checked(&mut self, app_id: WindowAppIdBuilder) -> Result<()> {
        let app_id = app_id.build_checked()?;
        self.set_app_id(&app_id);
        Ok(())
    }

    /// Sets the window background appearance.
    pub fn set_background_appearance(&self, background_appearance: WindowBackgroundAppearance) {
        self.platform_window
            .set_background_appearance(background_appearance);
    }

    /// Set native window opacity as an unchecked fraction in the inclusive `0.0..=1.0` range.
    pub fn set_opacity(&self, opacity: f32) {
        self.platform_window.set_opacity(opacity);
    }

    /// Validate and set native window opacity.
    pub fn set_opacity_checked(&self, opacity: WindowOpacityBuilder) -> Result<WindowOpacity> {
        let opacity = opacity.build_checked()?;
        self.set_opacity(opacity.fraction());
        Ok(opacity)
    }

    /// Set whether the platform window should stay above normal app windows.
    pub fn set_always_on_top(&self, always_on_top: bool) {
        self.platform_window.set_always_on_top(always_on_top);
    }

    /// Validate and set native z-order policy.
    pub fn set_z_order_policy_checked(
        &self,
        policy: WindowZOrderPolicyBuilder,
    ) -> Result<WindowZOrderPolicy> {
        let policy = policy.build_checked()?;
        self.set_always_on_top(policy.always_on_top());
        Ok(policy)
    }

    /// Mark the window as dirty at the platform level.
    pub fn set_window_edited(&mut self, edited: bool) {
        self.platform_window.set_edited(edited);
    }

    /// Validate and apply document-window chrome state.
    pub fn set_document_state_checked(
        &mut self,
        state: WindowDocumentStateBuilder,
    ) -> Result<WindowDocumentState> {
        let state = state.build_checked()?;
        if let Some(title) = state.title() {
            self.set_window_title(title);
        }
        self.set_window_edited(state.edited());
        Ok(state)
    }

    /// Return the current checked content-protection policy, if enabled.
    pub fn content_protection(&self) -> Option<&WindowContentProtection> {
        self.content_protection.as_ref()
    }

    /// Validate and apply native window content-protection intent.
    ///
    /// This records the checked policy on the window so platform backends,
    /// capture flows, and diagnostics have one authoritative intent to consume.
    pub fn set_content_protection_checked(
        &mut self,
        protection: WindowContentProtectionBuilder,
    ) -> Result<WindowContentProtection> {
        let protection = protection.build_checked()?;
        if protection.is_protected() {
            self.content_protection = Some(protection.clone());
        } else {
            self.content_protection = None;
        }
        Ok(protection)
    }

    /// Clear any checked content-protection policy.
    pub fn clear_content_protection_checked(&mut self) -> Result<WindowContentProtection> {
        self.set_content_protection_checked(WindowContentProtectionBuilder::disabled())
    }

    /// Determine the display on which the window is visible.
    pub fn display(&self, cx: &App) -> Option<Rc<dyn PlatformDisplay>> {
        cx.platform
            .displays()
            .into_iter()
            .find(|display| Some(display.id()) == self.display_id)
    }

    /// Show the platform character palette.
    pub fn show_character_palette(&self) {
        self.platform_window.show_character_palette();
    }

    /// Validate and perform a native system-UI command.
    pub fn perform_window_system_ui_command_checked(
        &self,
        command: WindowSystemUiCommand,
    ) -> Result<WindowSystemUiCommand> {
        command.validate()?;
        match command.kind() {
            WindowSystemUiCommandKind::ShowCharacterPalette => self.show_character_palette(),
            WindowSystemUiCommandKind::TitlebarDoubleClick => self.titlebar_double_click(),
            WindowSystemUiCommandKind::ZoomWindow => self.zoom_window(),
        }
        Ok(command)
    }

    /// The scale factor of the display associated with the window. For example, it could
    /// return 2.0 for a "retina" display, indicating that each logical pixel should actually
    /// be rendered as two pixels on screen.
    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Returns a pixel snap policy configured for this window's scale factor,
    /// enabling application code to snap positions to device pixel boundaries.
    pub fn pixel_snap_policy(&self) -> crate::PixelSnapPolicy {
        crate::PixelSnapPolicy::new(self.scale_factor)
    }

    /// The size of an em for the base font of the application. Adjusting this value allows the
    /// UI to scale, just like zooming a web page.
    pub fn rem_size(&self) -> Pixels {
        self.base_rem_size() * self.ui_zoom_factor
    }

    fn base_rem_size(&self) -> Pixels {
        self.rem_size_override_stack
            .last()
            .copied()
            .unwrap_or(self.rem_size)
    }

    /// Resolve an absolute UI length after applying the user-controlled interface zoom.
    pub fn ui_length_in_pixels(&self, length: AbsoluteLength) -> Pixels {
        length.to_pixels(self.base_rem_size()) * self.ui_zoom_factor
    }

    /// Resolve an absolute length in the layout coordinate system before interface zoom.
    pub(crate) fn unscaled_ui_length_in_pixels(&self, length: AbsoluteLength) -> Pixels {
        length.to_pixels(self.base_rem_size())
    }

    /// Resolve an absolute or font-relative UI length without scaling fractions twice.
    pub fn ui_definite_length_in_pixels(
        &self,
        length: DefiniteLength,
        base_size: Pixels,
    ) -> Pixels {
        match length {
            DefiniteLength::Absolute(length) => self.ui_length_in_pixels(length),
            DefiniteLength::Fraction(fraction) => base_size * fraction,
        }
    }

    /// Resolve absolute edge lengths after applying interface zoom.
    pub fn ui_edges_in_pixels(&self, edges: Edges<AbsoluteLength>) -> Edges<Pixels> {
        edges.map(|length| self.ui_length_in_pixels(*length))
    }

    /// Resolve absolute corner lengths after applying interface zoom.
    pub fn ui_corners_in_pixels(&self, corners: Corners<AbsoluteLength>) -> Corners<Pixels> {
        corners.map(|length| self.ui_length_in_pixels(*length))
    }

    /// Resolve padding-like lengths while preserving parent-relative fractions.
    pub fn ui_definite_edges_in_pixels(
        &self,
        edges: Edges<DefiniteLength>,
        parent_size: Size<Pixels>,
    ) -> Edges<Pixels> {
        Edges {
            top: self.ui_definite_length_in_pixels(edges.top, parent_size.height),
            right: self.ui_definite_length_in_pixels(edges.right, parent_size.width),
            bottom: self.ui_definite_length_in_pixels(edges.bottom, parent_size.height),
            left: self.ui_definite_length_in_pixels(edges.left, parent_size.width),
        }
    }

    fn ui_layout_scale_factor(&self) -> f32 {
        self.scale_factor * self.ui_zoom_factor
    }

    /// Sets the size of an em for the base font of the application. Adjusting this value allows the
    /// UI to scale, just like zooming a web page.
    pub fn set_rem_size(&mut self, rem_size: impl Into<Pixels>) {
        let rem_size = rem_size.into();
        if self.rem_size == rem_size {
            return;
        }
        self.rem_size = rem_size;
        self.frame_skip.invalidate();
        self.refresh();
    }

    /// Return the user-controlled interface zoom factor for this window.
    pub fn ui_zoom_factor(&self) -> f32 {
        self.ui_zoom_factor
    }

    /// Set the user-controlled interface zoom factor, clamped to a readable range.
    pub fn set_ui_zoom_factor(&mut self, factor: f32) {
        if !factor.is_finite() {
            return;
        }
        let factor = factor.clamp(MIN_UI_ZOOM_FACTOR, MAX_UI_ZOOM_FACTOR);
        if (self.ui_zoom_factor - factor).abs() < f32::EPSILON {
            return;
        }
        self.ui_zoom_factor = factor;
        self.frame_skip.invalidate();
        self.refresh();
    }

    /// Increase the interface zoom by one standard step.
    pub fn zoom_in(&mut self) {
        let next = ((self.ui_zoom_factor + UI_ZOOM_STEP) * 10.0).round() / 10.0;
        self.set_ui_zoom_factor(next);
    }

    /// Decrease the interface zoom by one standard step.
    pub fn zoom_out(&mut self) {
        let next = ((self.ui_zoom_factor - UI_ZOOM_STEP) * 10.0).round() / 10.0;
        self.set_ui_zoom_factor(next);
    }

    /// Restore the interface zoom to the application-defined base size.
    pub fn reset_zoom(&mut self) {
        self.set_ui_zoom_factor(1.0);
    }

    /// Validate and set the base font em size for native window UI scaling.
    pub fn set_rem_size_checked(
        &mut self,
        rem_size: WindowRemSizeBuilder,
    ) -> Result<WindowRemSize> {
        let rem_size = rem_size.build_checked()?;
        self.set_rem_size(rem_size.rem_size());
        Ok(rem_size)
    }

    /// Acquire a globally unique identifier for the given ElementId.
    /// Only valid for the duration of the provided closure.
    pub fn with_global_id<R>(
        &mut self,
        element_id: ElementId,
        f: impl FnOnce(&GlobalElementId, &mut Self) -> R,
    ) -> R {
        self.element_id_stack.push(element_id);
        let global_id = GlobalElementId(self.element_id_stack.clone());
        let result = f(&global_id, self);
        self.element_id_stack.pop();
        result
    }

    /// Executes the provided function with the specified rem size.
    ///
    /// This method must only be called as part of element drawing.
    pub fn with_rem_size<F, R>(&mut self, rem_size: Option<impl Into<Pixels>>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        if let Some(rem_size) = rem_size {
            self.rem_size_override_stack.push(rem_size.into());
            let result = f(self);
            self.rem_size_override_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// The line height associated with the current text style.
    pub fn line_height(&self) -> Pixels {
        let style = self.text_style();
        let font_size = self.ui_length_in_pixels(style.font_size);
        self.ui_definite_length_in_pixels(style.line_height, font_size)
            .round()
    }

    /// Call to prevent the default action of an event. Currently only used to prevent
    /// parent elements from becoming focused on mouse down.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    /// Obtain whether default has been prevented for the event currently being dispatched.
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }

    /// Determine whether the given action is available along the dispatch path to the currently focused element.
    pub fn is_action_available(&self, action: &dyn Action, cx: &mut App) -> bool {
        let node_id =
            self.focus_node_id_in_rendered_frame(self.focused(cx).map(|handle| handle.id));
        self.rendered_frame
            .dispatch_tree
            .is_action_available(action, node_id)
    }

    /// The position of the mouse relative to the window.
    pub fn mouse_position(&self) -> Point<Pixels> {
        self.mouse_position
    }

    /// The current state of the keyboard's modifiers
    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    /// The current state of the keyboard's capslock
    pub fn capslock(&self) -> Capslock {
        self.capslock
    }

    fn complete_frame(&self) {
        self.platform_window.completed_frame();
    }

    /// Produces a new frame and assigns it to `rendered_frame`. To actually show
    /// the contents of the new `Scene`, use `present`.
    #[profiling::function]
    pub fn draw(&mut self, cx: &mut App) -> ArenaClearNeeded {
        #[cfg(any(feature = "inspector", debug_assertions))]
        let frame_started_at = Instant::now();
        self.frame_view_render_us = 0;
        self.frame_taffy_compute_us = 0;
        self.frame_layout_nodes = 0;
        self.frame_layout_measure_count = 0;
        self.frame_layout_measure_us = 0;
        self.power_mode = cx.power_mode();
        self.reduce_motion = cx.reduce_motion();
        self.invalidate_entities();
        let reuse_layout = mem::take(&mut self.reuse_layout_on_next_frame);
        self.layout_engine_mut().begin_frame(reuse_layout);
        cx.entities.clear_accessed();
        debug_assert!(self.rendered_entity_stack.is_empty());
        self.invalidator.set_dirty(false);
        self.requested_autoscroll = None;
        self.accessibility_parent_stack.clear();
        self.accessibility_child_ordinals.clear();

        // Restore the previously-used input handler.
        if let Some(input_handler) = self.platform_window.take_input_handler() {
            self.rendered_frame.input_handlers.push(Some(input_handler));
        }
        #[cfg(any(feature = "inspector", debug_assertions))]
        let draw_roots_timing = self.draw_roots(cx);
        #[cfg(not(any(feature = "inspector", debug_assertions)))]
        self.draw_roots(cx);
        self.dirty_views.clear();
        self.next_frame.window_active = self.active.get();

        // Register requested input handler with the platform window.
        if let Some(Some(input_handler)) = self.next_frame.input_handlers.pop() {
            self.platform_window.set_input_handler(input_handler);
        }

        self.layout_engine_mut().clear();
        self.text_system().finish_frame();
        self.next_frame.finish(&mut self.rendered_frame);

        self.invalidator.set_phase(DrawPhase::Focus);
        let previous_focus_path = self.rendered_frame.focus_path();
        let previous_window_active = self.rendered_frame.window_active;
        mem::swap(&mut self.rendered_frame, &mut self.next_frame);
        self.next_frame.clear();
        self.update_accessibility_tree();
        self.accessibility_action_router
            .retain_nodes(self.accessibility_tree.nodes.keys().copied());
        let accessibility_actions = self
            .platform_window
            .update_accessibility_tree(&self.accessibility_tree);
        self.dispatch_accessibility_actions(accessibility_actions);
        self.accessibility_announcements.clear();
        let current_focus_path = self.rendered_frame.focus_path();
        let current_window_active = self.rendered_frame.window_active;

        if previous_focus_path != current_focus_path
            || previous_window_active != current_window_active
        {
            if !previous_focus_path.is_empty() && current_focus_path.is_empty() {
                self.focus_lost_listeners
                    .clone()
                    .retain(&(), |listener| listener(self, cx));
            }

            let event = WindowFocusEvent {
                previous_focus_path: if previous_window_active {
                    previous_focus_path
                } else {
                    Default::default()
                },
                current_focus_path: if current_window_active {
                    current_focus_path
                } else {
                    Default::default()
                },
            };
            self.focus_listeners
                .clone()
                .retain(&(), |listener| listener(&event, self, cx));
        }

        debug_assert!(self.rendered_entity_stack.is_empty());
        self.record_entities_accessed(cx);
        self.reset_cursor_style(cx);
        self.refreshing = false;
        self.invalidator.set_phase(DrawPhase::None);
        self.needs_present.set(true);

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.record_frame_timing(frame_started_at, draw_roots_timing);

        ArenaClearNeeded
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn record_frame_timing(&mut self, frame_started_at: Instant, draw_roots: DrawRootsTiming) {
        let duration_us = frame_started_at.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let start_us = self
            .last_frame_presented_at
            .elapsed()
            .as_micros()
            .min(u64::MAX as u128) as u64;
        let frame_number = self.frame_counter;
        self.frame_counter = self.frame_counter.wrapping_add(1);
        let element_count = self.rendered_frame.hitboxes.len() as u32;
        self.frame_timeline.record(crate::FrameRecord {
            frame_number,
            start_us,
            duration_us,
            layout_us: draw_roots.layout_us,
            paint_us: draw_roots.paint_us,
            gpu_us: 0,
            element_count,
        });

        if crate::scroll_trace_enabled() {
            eprintln!(
                "[kael-scroll:frame] no={} since_present_us={} duration_us={} layout_us={} view_render_us={} taffy_compute_us={} layout_nodes={} measure_count={} measure_us={} layout_reused={} paint_us={} hitboxes={}",
                frame_number,
                start_us,
                duration_us,
                draw_roots.layout_us,
                draw_roots.view_render_us,
                draw_roots.taffy_compute_us,
                draw_roots.layout_nodes,
                draw_roots.layout_measure_count,
                draw_roots.layout_measure_us,
                draw_roots.layout_reused,
                draw_roots.paint_us,
                element_count,
            );
        }
    }

    /// Returns the frame timing timeline recorded for this window.
    ///
    /// Only available with the `inspector` feature or in debug builds. The timeline
    /// is fed automatically at the end of each [`Window::draw`].
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn frame_timeline(&self) -> &crate::FrameTimeline {
        &self.frame_timeline
    }

    fn record_entities_accessed(&mut self, cx: &mut App) {
        let mut entities_ref = cx.entities.accessed_entities.borrow_mut();
        let mut entities = mem::take(entities_ref.deref_mut());
        drop(entities_ref);
        let handle = self.handle;
        cx.record_entities_accessed(
            handle,
            // Try moving window invalidator into the Window
            self.invalidator.clone(),
            &entities,
        );
        let mut entities_ref = cx.entities.accessed_entities.borrow_mut();
        mem::swap(&mut entities, entities_ref.deref_mut());
    }

    fn invalidate_entities(&mut self) {
        let mut views = self.invalidator.take_views();
        for entity in views.drain() {
            self.mark_view_dirty(entity);
        }
        self.invalidator.replace_views(views);
    }

    #[profiling::function]
    fn present(&mut self) {
        self.platform_window
            .sync_webviews(&self.rendered_frame.webviews);

        // Whole-frame damage early-out (opt-in via `set_frame_skip_enabled`): if the
        // scene is byte-identical to the last presented frame, skip the GPU rasterize +
        // present entirely — the compositor retains the previously presented contents.
        // Live external surfaces (video) change without a scene-primitive change, so a
        // frame containing any is never skipped and resets the tracker.
        if self.rendered_frame.scene.has_live_surfaces() {
            self.frame_skip.invalidate();
        } else if self
            .frame_skip
            .should_skip(self.rendered_frame.scene.structural_checksum())
        {
            self.needs_present.set(false);
            return;
        }

        self.platform_window.draw(&self.rendered_frame.scene);
        self.needs_present.set(false);
        self.last_frame_presented_at = Instant::now();
        profiling::finish_frame!();
    }

    fn draw_roots(&mut self, cx: &mut App) -> DrawRootsTiming {
        #[cfg(any(feature = "inspector", debug_assertions))]
        fn elapsed_us(start: Instant) -> u64 {
            start.elapsed().as_micros().min(u64::MAX as u128) as u64
        }

        #[cfg(any(feature = "inspector", debug_assertions))]
        let layout_started_at = Instant::now();
        self.invalidator.set_phase(DrawPhase::Prepaint);
        self.tooltip_bounds.take();

        let _inspector_width: Pixels = rems(30.0).to_pixels(self.rem_size());
        let root_size = {
            #[cfg(any(feature = "inspector", debug_assertions))]
            {
                if self.inspector.is_some() {
                    let mut size = self.viewport_size;
                    size.width = (size.width - _inspector_width).max(px(0.0));
                    size
                } else {
                    self.viewport_size
                }
            }
            #[cfg(not(any(feature = "inspector", debug_assertions)))]
            {
                self.viewport_size
            }
        };

        // Layout all root elements.
        let Some(root) = self.root.as_ref().cloned() else {
            #[cfg(any(feature = "inspector", debug_assertions))]
            return DrawRootsTiming::default();
            #[cfg(not(any(feature = "inspector", debug_assertions)))]
            return DrawRootsTiming;
        };
        let mut root_element = root.into_any();
        root_element.prepaint_as_root(Point::default(), root_size.into(), self, cx);

        #[cfg(any(feature = "inspector", debug_assertions))]
        let inspector_element = self.prepaint_inspector(_inspector_width, cx);

        let mut sorted_deferred_draws =
            (0..self.next_frame.deferred_draws.len()).collect::<SmallVec<[_; 8]>>();
        sorted_deferred_draws.sort_by_key(|ix| self.next_frame.deferred_draws[*ix].priority);
        self.prepaint_deferred_draws(&sorted_deferred_draws, cx);

        let mut prompt_element = None;
        let mut active_drag_element = None;
        let mut tooltip_element = None;
        if let Some(prompt) = self.prompt.take() {
            let mut element = prompt.view.any_view().into_any();
            element.prepaint_as_root(Point::default(), root_size.into(), self, cx);
            prompt_element = Some(element);
            self.prompt = Some(prompt);
        } else if let Some(active_drag) = cx.active_drag.take() {
            let mut element = active_drag.view.clone().into_any();
            let offset = self.mouse_position() - active_drag.cursor_offset;
            element.prepaint_as_root(offset, AvailableSpace::min_size(), self, cx);
            active_drag_element = Some(element);
            cx.active_drag = Some(active_drag);
        } else {
            tooltip_element = self.prepaint_tooltip(cx);
        }

        self.mouse_hit_test = self.next_frame.hit_test(self.mouse_position);
        #[cfg(any(feature = "inspector", debug_assertions))]
        let layout_us = elapsed_us(layout_started_at);

        // Now actually paint the elements.
        #[cfg(any(feature = "inspector", debug_assertions))]
        let paint_started_at = Instant::now();
        self.invalidator.set_phase(DrawPhase::Paint);
        root_element.paint(self, cx);

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.paint_inspector(inspector_element, cx);

        self.paint_deferred_draws(&sorted_deferred_draws, cx);

        if let Some(mut prompt_element) = prompt_element {
            prompt_element.paint(self, cx);
        } else if let Some(mut drag_element) = active_drag_element {
            drag_element.paint(self, cx);
        } else if let Some(mut tooltip_element) = tooltip_element {
            tooltip_element.paint(self, cx);
        }

        #[cfg(any(feature = "inspector", debug_assertions))]
        self.paint_inspector_hitbox(cx);

        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            DrawRootsTiming {
                layout_us,
                view_render_us: self.frame_view_render_us,
                taffy_compute_us: self.frame_taffy_compute_us,
                layout_nodes: self.frame_layout_nodes,
                layout_measure_count: self.frame_layout_measure_count,
                layout_measure_us: self.frame_layout_measure_us,
                layout_reused: self.layout_engine_mut().is_reusing_previous_layout(),
                paint_us: elapsed_us(paint_started_at),
            }
        }
        #[cfg(not(any(feature = "inspector", debug_assertions)))]
        {
            DrawRootsTiming
        }
    }

    fn prepaint_tooltip(&mut self, cx: &mut App) -> Option<AnyElement> {
        // Use indexing instead of iteration to avoid borrowing self for the duration of the loop.
        for tooltip_request_index in (0..self.next_frame.tooltip_requests.len()).rev() {
            let Some(Some(tooltip_request)) = self
                .next_frame
                .tooltip_requests
                .get(tooltip_request_index)
                .cloned()
            else {
                log::error!("Unexpectedly absent TooltipRequest");
                continue;
            };
            let mut element = tooltip_request.tooltip.view.clone().into_any();
            let mouse_position = tooltip_request.tooltip.mouse_position;
            let tooltip_size = element.layout_as_root(AvailableSpace::min_size(), self, cx);

            let window_bounds = Bounds {
                origin: Point::default(),
                size: self.viewport_size(),
            };
            let mut tooltip_bounds = match tooltip_request.tooltip.anchor.as_ref() {
                Some(anchor) => anchored_tooltip_bounds(anchor, tooltip_size, window_bounds),
                None => Bounds::new(mouse_position + point(px(1.), px(1.)), tooltip_size),
            };

            if tooltip_bounds.right() > window_bounds.right() {
                let new_x = mouse_position.x - tooltip_bounds.size.width - px(1.);
                if new_x >= Pixels::ZERO {
                    tooltip_bounds.origin.x = new_x;
                } else {
                    tooltip_bounds.origin.x = cmp::max(
                        Pixels::ZERO,
                        tooltip_bounds.origin.x - tooltip_bounds.right() - window_bounds.right(),
                    );
                }
            }

            if tooltip_bounds.bottom() > window_bounds.bottom() {
                let new_y = mouse_position.y - tooltip_bounds.size.height - px(1.);
                if new_y >= Pixels::ZERO {
                    tooltip_bounds.origin.y = new_y;
                } else {
                    tooltip_bounds.origin.y = cmp::max(
                        Pixels::ZERO,
                        tooltip_bounds.origin.y - tooltip_bounds.bottom() - window_bounds.bottom(),
                    );
                }
            }

            // It's possible for an element to have an active tooltip while not being painted (e.g.
            // via the `visible_on_hover` method). Since mouse listeners are not active in this
            // case, instead update the tooltip's visibility here.
            let is_visible =
                (tooltip_request.tooltip.check_visible_and_update)(tooltip_bounds, self, cx);
            if !is_visible {
                continue;
            }

            self.with_absolute_element_offset(tooltip_bounds.origin, |window| {
                element.prepaint(window, cx)
            });

            self.tooltip_bounds = Some(TooltipBounds {
                id: tooltip_request.id,
                bounds: tooltip_bounds,
            });
            return Some(element);
        }
        None
    }

    fn prepaint_deferred_draws(&mut self, deferred_draw_indices: &[usize], cx: &mut App) {
        assert_eq!(self.element_id_stack.len(), 0);

        let mut deferred_draws = mem::take(&mut self.next_frame.deferred_draws);
        for deferred_draw_ix in deferred_draw_indices {
            let deferred_draw = &mut deferred_draws[*deferred_draw_ix];
            self.element_id_stack
                .clone_from(&deferred_draw.element_id_stack);
            self.text_style_stack
                .clone_from(&deferred_draw.text_style_stack);
            self.next_frame
                .dispatch_tree
                .set_active_node(deferred_draw.parent_node);

            let prepaint_start = self.prepaint_index();
            if let Some(element) = deferred_draw.element.as_mut() {
                self.with_rendered_view(deferred_draw.current_view, |window| {
                    window.with_absolute_element_offset(deferred_draw.absolute_offset, |window| {
                        element.prepaint(window, cx)
                    });
                })
            } else {
                self.reuse_prepaint(deferred_draw.prepaint_range.clone());
            }
            let prepaint_end = self.prepaint_index();
            deferred_draw.prepaint_range = prepaint_start..prepaint_end;
        }
        assert_eq!(
            self.next_frame.deferred_draws.len(),
            0,
            "cannot call defer_draw during deferred drawing"
        );
        self.next_frame.deferred_draws = deferred_draws;
        self.element_id_stack.clear();
        self.text_style_stack.clear();
    }

    fn paint_deferred_draws(&mut self, deferred_draw_indices: &[usize], cx: &mut App) {
        assert_eq!(self.element_id_stack.len(), 0);

        let mut deferred_draws = mem::take(&mut self.next_frame.deferred_draws);
        for deferred_draw_ix in deferred_draw_indices {
            let mut deferred_draw = &mut deferred_draws[*deferred_draw_ix];
            self.element_id_stack
                .clone_from(&deferred_draw.element_id_stack);
            self.next_frame
                .dispatch_tree
                .set_active_node(deferred_draw.parent_node);

            let paint_start = self.paint_index();
            if let Some(element) = deferred_draw.element.as_mut() {
                self.with_rendered_view(deferred_draw.current_view, |window| {
                    element.paint(window, cx);
                })
            } else {
                self.reuse_paint(deferred_draw.paint_range.clone());
            }
            let paint_end = self.paint_index();
            deferred_draw.paint_range = paint_start..paint_end;
        }
        self.next_frame.deferred_draws = deferred_draws;
        self.element_id_stack.clear();
    }

    pub(crate) fn prepaint_index(&self) -> PrepaintStateIndex {
        PrepaintStateIndex {
            hitboxes_index: self.next_frame.hitboxes.len(),
            tooltips_index: self.next_frame.tooltip_requests.len(),
            deferred_draws_index: self.next_frame.deferred_draws.len(),
            dispatch_tree_index: self.next_frame.dispatch_tree.len(),
            accessed_element_states_index: self.next_frame.accessed_element_states.len(),
            line_layout_index: self.text_system.layout_index(),
            #[cfg(any(feature = "test-support", test))]
            debug_bounds_keys: self.next_frame.debug_bounds.keys().cloned().collect(),
        }
    }

    pub(crate) fn reuse_prepaint(&mut self, range: Range<PrepaintStateIndex>) {
        #[cfg(any(feature = "test-support", test))]
        self.next_frame.debug_bounds.extend(
            range
                .end
                .debug_bounds_keys
                .difference(&range.start.debug_bounds_keys)
                .filter_map(|key| {
                    self.rendered_frame
                        .debug_bounds
                        .get(key)
                        .copied()
                        .map(|bounds| (key.clone(), bounds))
                }),
        );
        self.next_frame.hitboxes.extend(
            self.rendered_frame.hitboxes[range.start.hitboxes_index..range.end.hitboxes_index]
                .iter()
                .cloned(),
        );
        self.next_frame.tooltip_requests.extend(
            self.rendered_frame.tooltip_requests
                [range.start.tooltips_index..range.end.tooltips_index]
                .iter_mut()
                .map(|request| request.take()),
        );
        self.next_frame.accessed_element_states.extend(
            self.rendered_frame.accessed_element_states[range.start.accessed_element_states_index
                ..range.end.accessed_element_states_index]
                .iter()
                .map(|(id, type_id)| (GlobalElementId(id.0.clone()), *type_id)),
        );
        self.text_system
            .reuse_layouts(range.start.line_layout_index..range.end.line_layout_index);

        let reused_subtree = self.next_frame.dispatch_tree.reuse_subtree(
            range.start.dispatch_tree_index..range.end.dispatch_tree_index,
            &mut self.rendered_frame.dispatch_tree,
            self.focus,
        );

        if reused_subtree.contains_focus() {
            self.next_frame.focus = self.focus;
        }

        self.next_frame.deferred_draws.extend(
            self.rendered_frame.deferred_draws
                [range.start.deferred_draws_index..range.end.deferred_draws_index]
                .iter()
                .map(|deferred_draw| DeferredDraw {
                    current_view: deferred_draw.current_view,
                    parent_node: reused_subtree.refresh_node_id(deferred_draw.parent_node),
                    element_id_stack: deferred_draw.element_id_stack.clone(),
                    text_style_stack: deferred_draw.text_style_stack.clone(),
                    priority: deferred_draw.priority,
                    element: None,
                    absolute_offset: deferred_draw.absolute_offset,
                    prepaint_range: deferred_draw.prepaint_range.clone(),
                    paint_range: deferred_draw.paint_range.clone(),
                }),
        );
    }

    pub(crate) fn paint_index(&self) -> PaintIndex {
        PaintIndex {
            scene_index: self.next_frame.scene.len(),
            mouse_listeners_index: self.next_frame.mouse_listeners.len(),
            input_handlers_index: self.next_frame.input_handlers.len(),
            cursor_styles_index: self.next_frame.cursor_styles.len(),
            accessed_element_states_index: self.next_frame.accessed_element_states.len(),
            tab_handle_index: self.next_frame.tab_stops.paint_index(),
            line_layout_index: self.text_system.layout_index(),
            #[cfg(any(feature = "test-support", test))]
            debug_bounds_keys: self.next_frame.debug_bounds.keys().cloned().collect(),
        }
    }

    pub(crate) fn reuse_paint(&mut self, range: Range<PaintIndex>) {
        self.reuse_paint_impl(range, true);
    }

    pub(crate) fn reuse_paint_without_scene(&mut self, range: Range<PaintIndex>) {
        self.reuse_paint_impl(range, false);
    }

    fn reuse_paint_impl(&mut self, range: Range<PaintIndex>, replay_scene: bool) {
        #[cfg(any(feature = "test-support", test))]
        self.next_frame.debug_bounds.extend(
            range
                .end
                .debug_bounds_keys
                .difference(&range.start.debug_bounds_keys)
                .filter_map(|key| {
                    self.rendered_frame
                        .debug_bounds
                        .get(key)
                        .copied()
                        .map(|bounds| (key.clone(), bounds))
                }),
        );
        self.next_frame.cursor_styles.extend(
            self.rendered_frame.cursor_styles
                [range.start.cursor_styles_index..range.end.cursor_styles_index]
                .iter()
                .cloned(),
        );
        self.next_frame.input_handlers.extend(
            self.rendered_frame.input_handlers
                [range.start.input_handlers_index..range.end.input_handlers_index]
                .iter_mut()
                .map(|handler| handler.take()),
        );
        self.next_frame.mouse_listeners.extend(
            self.rendered_frame.mouse_listeners
                [range.start.mouse_listeners_index..range.end.mouse_listeners_index]
                .iter_mut()
                .map(|listener| listener.take()),
        );
        self.next_frame.accessed_element_states.extend(
            self.rendered_frame.accessed_element_states[range.start.accessed_element_states_index
                ..range.end.accessed_element_states_index]
                .iter()
                .map(|(id, type_id)| (GlobalElementId(id.0.clone()), *type_id)),
        );
        self.next_frame.tab_stops.replay(
            &self.rendered_frame.tab_stops.insertion_history
                [range.start.tab_handle_index..range.end.tab_handle_index],
        );

        self.text_system
            .reuse_layouts(range.start.line_layout_index..range.end.line_layout_index);
        if replay_scene {
            self.next_frame.scene.replay(
                range.start.scene_index..range.end.scene_index,
                &self.rendered_frame.scene,
            );
        }
    }

    pub(crate) fn request_cached_surface_snapshot(
        &mut self,
        paint_operations: Range<usize>,
        cached_surface: &crate::cache::CachedSurface,
    ) {
        self.next_frame
            .scene
            .request_cached_surface_snapshot(crate::CachedSurfaceSnapshot {
                paint_operations,
                source_bounds: cached_surface.device_bounds,
                target: cached_surface.tile.clone(),
            });
    }

    pub(crate) fn paint_cached_surface(&mut self, cached_surface: &crate::cache::CachedSurface) {
        self.next_frame
            .scene
            .insert_primitive(crate::PolychromeSprite {
                order: 0,
                rounded_clip_bounds: self.rounded_clip.0,
                rounded_clip_radii: self.rounded_clip.1,
                color_filter: self.element_color_filter,
                pad: 0,
                grayscale: false,
                opacity: 1.0,
                bounds: cached_surface.bounds,
                content_mask: self.content_mask().scale(self.scale_factor()),
                corner_radii: Corners::default(),
                tile: cached_surface.tile.clone(),
                sprite_kind: POLYCHROME_SPRITE_KIND_PREMULTIPLIED,
                color: transparent_black(),
                pad3: 0,
                transformation: self.element_transform,
                blur_radius: 0.0,
                pad2: 0,
            });
    }

    pub(crate) fn paint_effect_surface(
        &mut self,
        cached_surface: &crate::cache::CachedSurface,
        content_blur: Pixels,
        drop_shadow: Option<&BoxShadow>,
    ) {
        let scale_factor = self.scale_factor();
        let content_mask = self.content_mask().scale(scale_factor);

        if let Some(shadow) = drop_shadow {
            let offset = shadow.offset.scale(scale_factor);
            let shadow_bounds = Bounds {
                origin: cached_surface.bounds.origin + offset,
                size: cached_surface.bounds.size,
            };
            self.next_frame
                .scene
                .insert_primitive(crate::PolychromeSprite {
                    order: 0,
                    rounded_clip_bounds: self.rounded_clip.0,
                    rounded_clip_radii: self.rounded_clip.1,
                    color_filter: self.element_color_filter,
                    pad: 0,
                    grayscale: false,
                    opacity: 1.0,
                    bounds: shadow_bounds,
                    content_mask: content_mask.clone(),
                    corner_radii: Corners::default(),
                    tile: cached_surface.tile.clone(),
                    sprite_kind: POLYCHROME_SPRITE_KIND_CONTENT_SHADOW,
                    color: shadow.color,
                    pad3: 0,
                    transformation: self.element_transform,
                    blur_radius: shadow.blur_radius.scale(scale_factor).0,
                    pad2: 0,
                });
        }

        let blur_radius = content_blur.scale(scale_factor).0;
        let sprite_kind = if blur_radius > 0.0 {
            POLYCHROME_SPRITE_KIND_CONTENT_BLURRED
        } else {
            POLYCHROME_SPRITE_KIND_PREMULTIPLIED
        };

        self.next_frame
            .scene
            .insert_primitive(crate::PolychromeSprite {
                order: 0,
                rounded_clip_bounds: self.rounded_clip.0,
                rounded_clip_radii: self.rounded_clip.1,
                color_filter: self.element_color_filter,
                pad: 0,
                grayscale: false,
                opacity: 1.0,
                bounds: cached_surface.bounds,
                content_mask,
                corner_radii: Corners::default(),
                tile: cached_surface.tile.clone(),
                sprite_kind,
                color: transparent_black(),
                pad3: 0,
                transformation: self.element_transform,
                blur_radius,
                pad2: 0,
            });
    }

    /// Push a text style onto the stack, and call a function with that style active.
    /// Use [`Window::text_style`] to get the current, combined text style. This method
    /// should only be called as part of element drawing.
    pub fn with_text_style<F, R>(&mut self, style: Option<TextStyleRefinement>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.invalidator.debug_assert_paint_or_prepaint();
        if let Some(style) = style {
            self.text_style_stack.push(style);
            let result = f(self);
            self.text_style_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// Updates the cursor style at the platform level. This method should only be called
    /// during the prepaint phase of element drawing.
    pub fn set_cursor_style(&mut self, style: CursorStyle, hitbox: &Hitbox) {
        self.invalidator.debug_assert_paint();
        self.next_frame.cursor_styles.push(CursorStyleRequest {
            hitbox_id: Some(hitbox.id),
            style,
        });
    }

    /// Updates the cursor style for the entire window at the platform level. A cursor
    /// style using this method will have precedence over any cursor style set using
    /// `set_cursor_style`. This method should only be called during the prepaint
    /// phase of element drawing.
    pub fn set_window_cursor_style(&mut self, style: CursorStyle) {
        self.invalidator.debug_assert_paint();
        self.next_frame.cursor_styles.push(CursorStyleRequest {
            hitbox_id: None,
            style,
        })
    }

    /// Validate and set a cursor style for the entire window for the upcoming frame.
    pub fn set_window_cursor_style_checked(
        &mut self,
        command: WindowCursorStyleCommand,
    ) -> Result<WindowCursorStyleCommand> {
        command.validate()?;
        self.set_window_cursor_style(command.style());
        Ok(command)
    }

    /// Sets a tooltip to be rendered for the upcoming frame. This method should only be called
    /// during the paint phase of element drawing.
    pub fn set_tooltip(&mut self, tooltip: AnyTooltip) -> TooltipId {
        self.invalidator.debug_assert_prepaint();
        let id = TooltipId(post_inc(&mut self.next_tooltip_id.0));
        self.next_frame
            .tooltip_requests
            .push(Some(TooltipRequest { id, tooltip }));
        id
    }

    /// Invoke the given function with the given content mask after intersecting it
    /// with the current mask. This method should only be called during element drawing.
    pub fn with_content_mask<R>(
        &mut self,
        mask: Option<ContentMask<Pixels>>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();
        if let Some(mask) = mask {
            let mask = mask.intersect(&self.content_mask());
            self.content_mask_stack.push(mask);
            let result = f(self);
            self.content_mask_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// Updates the global element offset relative to the current offset. This is used to implement
    /// scrolling. This method should only be called during the prepaint phase of element drawing.
    pub fn with_element_offset<R>(
        &mut self,
        offset: Point<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_prepaint();

        if offset.is_zero() {
            return f(self);
        };

        let abs_offset = self.element_offset() + offset;
        self.with_absolute_element_offset(abs_offset, f)
    }

    /// Updates the global element offset based on the given offset. This is used to implement
    /// drag handles and other manual painting of elements. This method should only be called during
    /// the prepaint phase of element drawing.
    pub fn with_absolute_element_offset<R>(
        &mut self,
        offset: Point<Pixels>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_prepaint();
        self.element_offset_stack.push(offset);
        let result = f(self);
        self.element_offset_stack.pop();
        result
    }

    pub(crate) fn with_element_opacity<R>(
        &mut self,
        opacity: Option<f32>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint_or_prepaint();

        let Some(opacity) = opacity else {
            return f(self);
        };

        let previous_opacity = self.element_opacity;
        self.element_opacity = previous_opacity * opacity;
        let result = f(self);
        self.element_opacity = previous_opacity;
        result
    }

    /// Compose a transformation onto the current element transform for the duration of
    /// the given closure. Primitives painted inside inherit the composed transform:
    /// quads, paths, SVGs, and text follow it. Box shadows, backdrop blurs, underlines,
    /// images, emoji, and video surfaces do not yet transform. The transformation is
    /// expressed in scaled (device) pixels. Hitboxes are unaffected.
    ///
    /// This method should only be called during the paint phase of element drawing.
    pub fn with_element_transform<R>(
        &mut self,
        transform: Option<TransformationMatrix>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        let Some(transform) = transform else {
            return f(self);
        };

        let previous_transform = self.element_transform;
        self.element_transform = previous_transform.compose(transform);
        let result = f(self);
        self.element_transform = previous_transform;
        result
    }

    /// Obtain the current composed element transform in scaled (device) pixels.
    ///
    /// This method should only be called during the paint phase of element drawing.
    #[inline]
    pub fn element_transform(&self) -> TransformationMatrix {
        self.invalidator.debug_assert_paint();
        self.element_transform
    }

    /// Compose a color filter onto the current element color filter for the duration of
    /// the given closure. Primitives painted inside (quads, sprites, shadows, underlines)
    /// inherit the composed filter, so a filter on a div applies to its whole subtree.
    /// Multiplicative factors multiply; grayscale combines so that fully gray wins.
    ///
    /// This method should only be called during the paint phase of element drawing.
    pub fn with_color_filter<R>(
        &mut self,
        color_filter: Option<ColorFilter>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        let Some(color_filter) = color_filter else {
            return f(self);
        };

        let previous = self.element_color_filter;
        self.element_color_filter = previous.compose(color_filter);
        let result = f(self);
        self.element_color_filter = previous;
        result
    }

    /// Clip primitives painted inside the given closure to a rounded rectangle.
    /// Quads, sprites (text, icons, images), shadows, underlines, and backdrop
    /// blurs honor the clip; paths and video surfaces do not yet. When rounded
    /// clips nest, the innermost clip's corners win; rectangular clipping still
    /// applies through the regular content mask. The clip is evaluated in screen
    /// space and does not follow element transforms.
    ///
    /// This method should only be called during the paint phase of element drawing.
    pub fn with_rounded_clip<R>(
        &mut self,
        clip: Option<(Bounds<Pixels>, Corners<Pixels>)>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        let Some((bounds, corner_radii)) = clip else {
            return f(self);
        };

        let scale_factor = self.scale_factor();
        let previous = self.rounded_clip;
        self.rounded_clip = (
            bounds.scale_and_snap(scale_factor),
            corner_radii.scale_and_snap(scale_factor),
        );
        let result = f(self);
        self.rounded_clip = previous;
        result
    }

    /// Clip primitives painted inside the closure to an arbitrary [`crate::ClipShape`]
    /// (circle, ellipse, or convex polygon).
    ///
    /// Circles (and equal-radius ellipses) clip exactly through the existing shader-backed
    /// rounded-clip path. Shapes that path cannot express yet — true ellipses and convex
    /// polygons — clip to the shape's bounding box via the content mask as a conservative
    /// fallback until the per-shape mask sample is fused into the pipeline.
    ///
    /// This method should only be called during the paint phase of element drawing.
    pub fn with_clip_path<R>(
        &mut self,
        shape: &crate::ClipShape,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.invalidator.debug_assert_paint();

        if let Some((bounds, corner_radii)) = shape.as_rounded_clip() {
            self.with_rounded_clip(Some((bounds, corner_radii)), f)
        } else {
            let bbox = shape.bounding_box();
            self.with_content_mask(Some(ContentMask { bounds: bbox }), f)
        }
    }

    /// Perform prepaint on child elements in a "retryable" manner, so that any side effects
    /// of prepaints can be discarded before prepainting again. This is used to support autoscroll
    /// where we need to prepaint children to detect the autoscroll bounds, then adjust the
    /// element offset and prepaint again. See [`crate::List`] for an example. This method should only be
    /// called during the prepaint phase of element drawing.
    pub fn transact<T, U>(&mut self, f: impl FnOnce(&mut Self) -> Result<T, U>) -> Result<T, U> {
        self.invalidator.debug_assert_prepaint();
        let index = self.prepaint_index();
        let result = f(self);
        if result.is_err() {
            self.next_frame.hitboxes.truncate(index.hitboxes_index);
            self.next_frame
                .tooltip_requests
                .truncate(index.tooltips_index);
            self.next_frame
                .deferred_draws
                .truncate(index.deferred_draws_index);
            self.next_frame
                .dispatch_tree
                .truncate(index.dispatch_tree_index);
            self.next_frame
                .accessed_element_states
                .truncate(index.accessed_element_states_index);
            self.text_system.truncate_layouts(index.line_layout_index);
        }
        result
    }

    /// When you call this method during [`Element::prepaint`], containing elements will attempt to
    /// scroll to cause the specified bounds to become visible. When they decide to autoscroll, they will call
    /// [`Element::prepaint`] again with a new set of bounds. See [`crate::List`] for an example of an element
    /// that supports this method being called on the elements it contains. This method should only be
    /// called during the prepaint phase of element drawing.
    pub fn request_autoscroll(&mut self, bounds: Bounds<Pixels>) {
        self.invalidator.debug_assert_prepaint();
        self.requested_autoscroll = Some(bounds);
    }

    /// Validate and request autoscroll for drag, selection, and editor surfaces.
    pub fn request_autoscroll_checked(
        &mut self,
        request: WindowAutoscrollRequestBuilder,
    ) -> Result<WindowAutoscrollRequest> {
        let request = request.build_checked()?;
        self.request_autoscroll(request.bounds());
        Ok(request)
    }

    /// This method can be called from a containing element such as [`crate::List`] to support the autoscroll behavior
    /// described in [`Self::request_autoscroll`].
    pub fn take_autoscroll(&mut self) -> Option<Bounds<Pixels>> {
        self.invalidator.debug_assert_prepaint();
        self.requested_autoscroll.take()
    }

    /// Asynchronously load an asset, if the asset hasn't finished loading this will return None.
    /// Your view will be re-drawn once the asset has finished loading.
    ///
    /// Note that the multiple calls to this method will only result in one `Asset::load` call at a
    /// time.
    pub fn use_asset<A: Asset>(&mut self, source: &A::Source, cx: &mut App) -> Option<A::Output> {
        let (task, is_first) = cx.fetch_asset::<A>(source);
        task.clone().now_or_never().or_else(|| {
            if is_first {
                let entity_id = self.current_view();
                self.spawn(cx, {
                    let task = task.clone();
                    async move |cx| {
                        task.await;

                        cx.on_next_frame(move |_, cx| {
                            cx.notify(entity_id);
                        });
                    }
                })
                .detach();
            }

            None
        })
    }

    /// Asynchronously load an asset, if the asset hasn't finished loading or doesn't exist this will return None.
    /// Your view will not be re-drawn once the asset has finished loading.
    ///
    /// Note that the multiple calls to this method will only result in one `Asset::load` call at a
    /// time.
    pub fn get_asset<A: Asset>(&mut self, source: &A::Source, cx: &mut App) -> Option<A::Output> {
        let (task, _) = cx.fetch_asset::<A>(source);
        task.now_or_never()
    }
    /// Obtain the current element offset. This method should only be called during the
    /// prepaint phase of element drawing.
    pub fn element_offset(&self) -> Point<Pixels> {
        self.invalidator.debug_assert_prepaint();
        self.element_offset_stack
            .last()
            .copied()
            .unwrap_or_default()
    }

    /// Obtain the current element opacity. This method should only be called during the
    /// prepaint phase of element drawing.
    #[inline]
    pub(crate) fn element_opacity(&self) -> f32 {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.element_opacity
    }

    /// Obtain the current content mask. This method should only be called during element drawing.
    pub fn content_mask(&self) -> ContentMask<Pixels> {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.content_mask_stack
            .last()
            .cloned()
            .unwrap_or_else(|| ContentMask {
                bounds: Bounds {
                    origin: Point::default(),
                    size: self.viewport_size,
                },
            })
    }

    /// Provide elements in the called function with a new namespace in which their identifiers must be unique.
    /// This can be used within a custom element to distinguish multiple sets of child elements.
    pub fn with_element_namespace<R>(
        &mut self,
        element_id: impl Into<ElementId>,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.element_id_stack.push(element_id.into());
        let result = f(self);
        self.element_id_stack.pop();
        result
    }

    /// Use a piece of state that exists as long this element is being rendered in consecutive frames.
    pub fn use_keyed_state<S: 'static>(
        &mut self,
        key: impl Into<ElementId>,
        cx: &mut App,
        init: impl FnOnce(&mut Self, &mut Context<S>) -> S,
    ) -> Entity<S> {
        let current_view = self.current_view();
        self.with_global_id(key.into(), |global_id, window| {
            window.with_element_state(global_id, |state: Option<Entity<S>>, window| {
                if let Some(state) = state {
                    (state.clone(), state)
                } else {
                    let new_state = cx.new(|cx| init(window, cx));
                    cx.observe(&new_state, move |_, cx| {
                        cx.notify(current_view);
                    })
                    .detach();
                    (new_state.clone(), new_state)
                }
            })
        })
    }

    /// Immediately push an element ID onto the stack. Useful for simplifying IDs in lists
    pub fn with_id<R>(&mut self, id: impl Into<ElementId>, f: impl FnOnce(&mut Self) -> R) -> R {
        self.with_global_id(id.into(), |_, window| f(window))
    }

    /// Use a piece of state that exists as long this element is being rendered in consecutive frames, without needing to specify a key
    ///
    /// NOTE: This method uses the location of the caller to generate an ID for this state.
    ///       If this is not sufficient to identify your state (e.g. you're rendering a list item),
    ///       you can provide a custom ElementID using the `use_keyed_state` method.
    #[track_caller]
    pub fn use_state<S: 'static>(
        &mut self,
        cx: &mut App,
        init: impl FnOnce(&mut Self, &mut Context<S>) -> S,
    ) -> Entity<S> {
        self.use_keyed_state(
            ElementId::CodeLocation(*core::panic::Location::caller()),
            cx,
            init,
        )
    }

    /// Updates or initializes state for an element with the given id that lives across multiple
    /// frames. If an element with this ID existed in the rendered frame, its state will be passed
    /// to the given closure. The state returned by the closure will be stored so it can be referenced
    /// when drawing the next frame. This method should only be called as part of element drawing.
    pub fn with_element_state<S, R>(
        &mut self,
        global_id: &GlobalElementId,
        f: impl FnOnce(Option<S>, &mut Self) -> (R, S),
    ) -> R
    where
        S: 'static,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        let key = (GlobalElementId(global_id.0.clone()), TypeId::of::<S>());
        self.next_frame
            .accessed_element_states
            .push((GlobalElementId(key.0.clone()), TypeId::of::<S>()));

        if let Some(any) = self
            .next_frame
            .element_states
            .remove(&key)
            .or_else(|| self.rendered_frame.element_states.remove(&key))
        {
            let ElementStateBox {
                inner,
                #[cfg(debug_assertions)]
                type_name,
            } = any;
            // Using the extra inner option to avoid needing to reallocate a new box.
            let mut state_box = match inner.downcast::<Option<S>>() {
                Ok(state_box) => state_box,
                Err(_) => {
                    #[cfg(debug_assertions)]
                    panic!(
                        "invalid element state type for id, requested {:?}, actual: {:?}",
                        std::any::type_name::<S>(),
                        type_name
                    );

                    #[cfg(not(debug_assertions))]
                    panic!(
                        "invalid element state type for id, requested {:?}",
                        std::any::type_name::<S>(),
                    );
                }
            };

            let state = state_box.take().unwrap_or_else(|| {
                panic!(
                    "reentrant call to with_element_state for the same state type and element id"
                )
            });
            let (result, state) = f(Some(state), self);
            state_box.replace(state);
            self.next_frame.element_states.insert(
                key,
                ElementStateBox {
                    inner: state_box,
                    #[cfg(debug_assertions)]
                    type_name,
                },
            );
            result
        } else {
            let (result, state) = f(None, self);
            self.next_frame.element_states.insert(
                key,
                ElementStateBox {
                    inner: Box::new(Some(state)),
                    #[cfg(debug_assertions)]
                    type_name: std::any::type_name::<S>(),
                },
            );
            result
        }
    }

    /// A variant of `with_element_state` that allows the element's id to be optional. This is a convenience
    /// method for elements where the element id may or may not be assigned. Prefer using `with_element_state`
    /// when the element is guaranteed to have an id.
    ///
    /// The first option means 'no ID provided'
    /// The second option means 'not yet initialized'
    pub fn with_optional_element_state<S, R>(
        &mut self,
        global_id: Option<&GlobalElementId>,
        f: impl FnOnce(Option<Option<S>>, &mut Self) -> (R, Option<S>),
    ) -> R
    where
        S: 'static,
    {
        self.invalidator.debug_assert_paint_or_prepaint();

        if let Some(global_id) = global_id {
            self.with_element_state(global_id, |state, cx| {
                let (result, state) = f(Some(state), cx);
                let state = state.unwrap_or_else(|| {
                    panic!("you must return some state when you pass some element id")
                });
                (result, state)
            })
        } else {
            let (result, state) = f(None, self);
            debug_assert!(
                state.is_none(),
                "you must not return an element state when passing None for the global id"
            );
            result
        }
    }

    /// Executes the given closure within the context of a tab group.
    #[inline]
    pub fn with_tab_group<R>(&mut self, index: Option<isize>, f: impl FnOnce(&mut Self) -> R) -> R {
        if let Some(index) = index {
            self.next_frame.tab_stops.begin_group(index);
            let result = f(self);
            self.next_frame.tab_stops.end_group();
            result
        } else {
            f(self)
        }
    }

    /// Defers the drawing of the given element, scheduling it to be painted on top of the currently-drawn tree
    /// at a later time. The `priority` parameter determines the drawing order relative to other deferred elements,
    /// with higher values being drawn on top.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn defer_draw(
        &mut self,
        element: AnyElement,
        absolute_offset: Point<Pixels>,
        priority: usize,
    ) {
        self.invalidator.debug_assert_prepaint();
        let parent_node = self
            .next_frame
            .dispatch_tree
            .active_node_id()
            .unwrap_or_else(|| panic!("deferred draw requested without an active dispatch node"));
        self.next_frame.deferred_draws.push(DeferredDraw {
            current_view: self.current_view(),
            parent_node,
            element_id_stack: self.element_id_stack.clone(),
            text_style_stack: self.text_style_stack.clone(),
            priority,
            element: Some(element),
            absolute_offset,
            prepaint_range: PrepaintStateIndex::default()..PrepaintStateIndex::default(),
            paint_range: PaintIndex::default()..PaintIndex::default(),
        });
    }

    /// Creates a new painting layer for the specified bounds. A "layer" is a batch
    /// of geometry that are non-overlapping and have the same draw order. This is typically used
    /// for performance reasons.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_layer<R>(&mut self, bounds: Bounds<Pixels>, f: impl FnOnce(&mut Self) -> R) -> R {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let content_mask = self.content_mask();
        let clipped_bounds = bounds.intersect(&content_mask.bounds);
        if !clipped_bounds.is_empty() {
            self.next_frame
                .scene
                .push_layer(clipped_bounds.scale(scale_factor));
        }

        let result = f(self);

        if !clipped_bounds.is_empty() {
            self.next_frame.scene.pop_layer();
        }

        result
    }

    /// Paint one or more drop shadows into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_shadows(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        shadows: &[BoxShadow],
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let content_mask = self.content_mask();
        let opacity = self.element_opacity();
        let ui_zoom_factor = self.ui_zoom_factor();
        for shadow in shadows {
            let offset = shadow.offset * ui_zoom_factor;
            let spread_radius = shadow.spread_radius * ui_zoom_factor;
            let shadow_bounds = if shadow.inset {
                bounds.contract(spread_radius)
            } else {
                (bounds + offset).dilate(spread_radius)
            };

            let scaled_bounds = shadow_bounds.scale_and_snap_conservative(scale_factor);
            if scaled_bounds.is_empty() {
                continue;
            }

            self.next_frame.scene.insert_primitive(Shadow {
                order: 0,
                rounded_clip_bounds: self.rounded_clip.0,
                rounded_clip_radii: self.rounded_clip.1,
                color_filter: self.element_color_filter,
                blur_radius: (shadow.blur_radius * ui_zoom_factor).scale(scale_factor),
                bounds: scaled_bounds,
                corner_radii: corner_radii.scale_and_snap(scale_factor),
                content_mask: if shadow.inset {
                    ContentMask {
                        bounds: bounds.scale_and_snap(scale_factor),
                    }
                } else {
                    content_mask.scale(scale_factor)
                },
                color: shadow.color.opacity(opacity),
                inset: shadow.inset as u32,
                pad: 0,
            });
        }
    }

    /// Paint a backdrop blur into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_blur(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        blur_radius: Pixels,
        tint: Hsla,
        saturation: f32,
    ) {
        self.invalidator.debug_assert_paint();

        if blur_radius <= Pixels::ZERO {
            return;
        }

        if self.power_mode == PowerMode::LowPower {
            if !tint.is_transparent() {
                self.paint_quad(crate::quad(
                    bounds,
                    corner_radii,
                    tint,
                    px(0.),
                    transparent_black(),
                    BorderStyle::Solid,
                ));
            }
            return;
        }

        let scale_factor = self.scale_factor();
        let content_mask = self.content_mask();
        let opacity = self.element_opacity();
        self.next_frame.scene.insert_primitive(crate::BlurRect {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            blur_radius: blur_radius.scale(scale_factor),
            bounds: bounds.scale_and_snap_conservative(scale_factor),
            content_mask: content_mask.scale(scale_factor),
            corner_radii: corner_radii.scale_and_snap(scale_factor),
            tint: tint.opacity(opacity),
            saturation: saturation.max(0.0),
        });
    }

    /// Paint one or more quads into the scene for the next frame at the current stacking context.
    /// Quads are colored rectangular regions with an optional background, border, and corner radius.
    /// see [`fill`], [`outline`], and [`quad`] to construct this type.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    ///
    /// Note that the `quad.corner_radii` are allowed to exceed the bounds, creating sharp corners
    /// where the circular arcs meet. This will not display well when combined with dashed borders.
    /// Use `Corners::clamp_radii_for_quad_size` if the radii should fit within the bounds.
    pub fn paint_quad(&mut self, quad: PaintQuad) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let content_mask = self.content_mask();
        let opacity = self.element_opacity();
        let corner_radii = platform_adjusted_corner_radii(
            quad.corner_radii,
            quad.bounds.size,
            quad.continuous_corners,
        );
        self.next_frame.scene.insert_primitive(Quad {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            color_filter: self.element_color_filter,
            bounds: quad.bounds.scale_and_snap(scale_factor),
            content_mask: content_mask.scale(scale_factor),
            background: quad.background.opacity(opacity),
            border_color: quad.border_color.opacity(opacity),
            corner_radii: corner_radii.scale_and_snap(scale_factor),
            border_widths: quad.border_widths.scale_and_snap_widths(scale_factor),
            border_style: quad.border_style,
            continuous_corners: if quad.continuous_corners { 1 } else { 0 },
            transform: self.element_transform.compose(quad.transform),
            blend_mode: quad.blend_mode as u32,
            pad: 0,
            pad2: 0,
        });
    }

    /// Paint the given `Path` into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_path(&mut self, mut path: Path<Pixels>, color: impl Into<Background>) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let content_mask = self.content_mask();
        let opacity = self.element_opacity();
        path.content_mask = content_mask;
        let color: Background = color.into();
        path.color = color.opacity(opacity);
        let transform = self.element_transform;
        let path = if transform == TransformationMatrix::unit() {
            path
        } else {
            let mut logical_transform = transform;
            logical_transform.translation[0] /= scale_factor;
            logical_transform.translation[1] /= scale_factor;
            path.transformed(logical_transform)
        };
        self.next_frame
            .scene
            .insert_primitive(path.scale(scale_factor));
    }

    /// Paint an underline into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_underline(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &UnderlineStyle,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let height = if style.wavy {
            style.thickness * 3.
        } else {
            style.thickness
        };
        let bounds = Bounds {
            origin,
            size: size(width, height),
        };
        let content_mask = self.content_mask();
        let element_opacity = self.element_opacity();

        self.next_frame.scene.insert_primitive(Underline {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            color_filter: self.element_color_filter,
            pad: 0,
            bounds: bounds.scale_and_snap(scale_factor),
            content_mask: content_mask.scale(scale_factor),
            color: style.color.unwrap_or_default().opacity(element_opacity),
            thickness: style.thickness.scale(scale_factor),
            wavy: if style.wavy { 1 } else { 0 },
        });
    }

    /// Paint a strikethrough into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_strikethrough(
        &mut self,
        origin: Point<Pixels>,
        width: Pixels,
        style: &StrikethroughStyle,
    ) {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let height = style.thickness;
        let bounds = Bounds {
            origin,
            size: size(width, height),
        };
        let content_mask = self.content_mask();
        let opacity = self.element_opacity();

        self.next_frame.scene.insert_primitive(Underline {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            color_filter: self.element_color_filter,
            pad: 0,
            bounds: bounds.scale_and_snap(scale_factor),
            content_mask: content_mask.scale(scale_factor),
            thickness: style.thickness.scale(scale_factor),
            color: style.color.unwrap_or_default().opacity(opacity),
            wavy: 0,
        });
    }

    /// Paints a monochrome (non-emoji) glyph into the scene for the next frame at the current z-index.
    ///
    /// The y component of the origin is the baseline of the glyph.
    /// You should generally prefer to use the [`ShapedLine::paint`](crate::ShapedLine::paint) or
    /// [`WrappedLine::paint`](crate::WrappedLine::paint) methods in the [`TextSystem`](crate::TextSystem).
    /// This method is only useful if you need to paint a single glyph that has already been shaped.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_glyph(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
        color: Hsla,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        self.paint_glyph_with_transformation(
            origin,
            font_id,
            glyph_id,
            font_size,
            color,
            TransformationMatrix::unit(),
        )
    }

    pub(crate) fn paint_glyph_with_transformation(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
        color: Hsla,
        transformation: TransformationMatrix,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let transformation = self.element_transform.compose(transformation);
        let element_opacity = self.element_opacity();
        let scale_factor = self.scale_factor();
        let glyph_origin = origin.scale(scale_factor);
        let device_x = glyph_origin.x.0;
        let device_y = glyph_origin.y.0;
        let fract_x = device_x - device_x.floor();
        let fract_y = device_y - device_y.floor();

        let subpixel_variant = Point {
            x: (fract_x * SUBPIXEL_VARIANTS_X as f32).floor() as u8,
            y: (fract_y * SUBPIXEL_VARIANTS_Y as f32).floor() as u8,
        };
        let raster_mode = if self.text_system().supports_subpixel_glyphs()
            && color.a >= 1.0
            && element_opacity >= 1.0
            && transformation == TransformationMatrix::unit()
        {
            GlyphRasterMode::Subpixel
        } else {
            GlyphRasterMode::Grayscale
        };
        let params = RenderGlyphParams {
            font_id,
            glyph_id,
            font_size,
            subpixel_variant,
            scale_factor,
            is_emoji: false,
            raster_mode,
        };

        let raster_bounds = self.text_system().raster_bounds(&params)?;
        if !raster_bounds.is_zero() {
            let Some(tile) =
                self.sprite_atlas
                    .get_or_insert_with(&params.clone().into(), &mut || {
                        let (size, bytes) = self.text_system().rasterize_glyph(&params)?;
                        Ok(Some((size, Cow::Owned(bytes))))
                    })?
            else {
                return Ok(());
            };
            let floored_origin = point(
                ScaledPixels(device_x.floor()),
                ScaledPixels(device_y.floor()),
            );
            let bounds = Bounds {
                origin: floored_origin + raster_bounds.origin.map(Into::into),
                size: tile.bounds.size.map(Into::into),
            };
            let content_mask = self.content_mask().scale(scale_factor);
            match raster_mode {
                GlyphRasterMode::Subpixel => {
                    self.next_frame.scene.insert_primitive(PolychromeSprite {
                        order: 0,
                        rounded_clip_bounds: self.rounded_clip.0,
                        rounded_clip_radii: self.rounded_clip.1,
                        color_filter: self.element_color_filter,
                        pad: 0,
                        grayscale: false,
                        opacity: 1.0,
                        bounds,
                        content_mask,
                        corner_radii: Default::default(),
                        tile,
                        sprite_kind: POLYCHROME_SPRITE_KIND_SUBPIXEL_TEXT,
                        color: color.opacity(element_opacity),
                        pad3: 0,
                        transformation,
                        blur_radius: 0.0,
                        pad2: 0,
                    });
                }
                GlyphRasterMode::Grayscale => {
                    self.next_frame.scene.insert_primitive(MonochromeSprite {
                        order: 0,
                        rounded_clip_bounds: self.rounded_clip.0,
                        rounded_clip_radii: self.rounded_clip.1,
                        color_filter: self.element_color_filter,
                        pad: 0,
                        bounds,
                        content_mask,
                        color: color.opacity(element_opacity),
                        tile,
                        transformation,
                    });
                }
            }
        }
        Ok(())
    }

    /// Paints an emoji glyph into the scene for the next frame at the current z-index.
    ///
    /// The y component of the origin is the baseline of the glyph.
    /// You should generally prefer to use the [`ShapedLine::paint`](crate::ShapedLine::paint) or
    /// [`WrappedLine::paint`](crate::WrappedLine::paint) methods in the [`TextSystem`](crate::TextSystem).
    /// This method is only useful if you need to paint a single emoji that has already been shaped.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_emoji(
        &mut self,
        origin: Point<Pixels>,
        font_id: FontId,
        glyph_id: GlyphId,
        font_size: Pixels,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let glyph_origin = origin.scale(scale_factor);
        let device_x = glyph_origin.x.0;
        let device_y = glyph_origin.y.0;
        let params = RenderGlyphParams {
            font_id,
            glyph_id,
            font_size,
            // We don't render emojis with subpixel variants.
            subpixel_variant: Default::default(),
            scale_factor,
            is_emoji: true,
            raster_mode: GlyphRasterMode::Grayscale,
        };

        let raster_bounds = self.text_system().raster_bounds(&params)?;
        if !raster_bounds.is_zero() {
            let Some(tile) =
                self.sprite_atlas
                    .get_or_insert_with(&params.clone().into(), &mut || {
                        let (size, bytes) = self.text_system().rasterize_glyph(&params)?;
                        Ok(Some((size, Cow::Owned(bytes))))
                    })?
            else {
                return Ok(());
            };

            let bounds = Bounds {
                origin: point(
                    ScaledPixels(device_x.floor()),
                    ScaledPixels(device_y.floor()),
                ) + raster_bounds.origin.map(Into::into),
                size: tile.bounds.size.map(Into::into),
            };
            let content_mask = self.content_mask().scale(scale_factor);
            let opacity = self.element_opacity();

            self.next_frame.scene.insert_primitive(PolychromeSprite {
                order: 0,
                rounded_clip_bounds: self.rounded_clip.0,
                rounded_clip_radii: self.rounded_clip.1,
                color_filter: self.element_color_filter,
                pad: 0,
                grayscale: false,
                bounds,
                corner_radii: Default::default(),
                content_mask,
                tile,
                opacity,
                sprite_kind: POLYCHROME_SPRITE_KIND_COLOR,
                color: transparent_black(),
                pad3: 0,
                transformation: self.element_transform,
                blur_radius: 0.0,
                pad2: 0,
            });
        }
        Ok(())
    }

    /// Paint a monochrome SVG into the scene for the next frame at the current stacking context.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_svg(
        &mut self,
        bounds: Bounds<Pixels>,
        path: SharedString,
        transformation: TransformationMatrix,
        color: Hsla,
        cx: &App,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let transformation = self.element_transform.compose(transformation);
        let element_opacity = self.element_opacity();
        let scale_factor = self.scale_factor();

        let bounds = bounds.scale_and_snap(scale_factor);
        let params = RenderSvgParams {
            path,
            size: bounds.size.map(|pixels| {
                DevicePixels::from((pixels.0 * SMOOTH_SVG_SCALE_FACTOR).ceil() as i32)
            }),
        };

        let Some(tile) =
            self.sprite_atlas
                .get_or_insert_with(&params.clone().into(), &mut || {
                    let Some((size, bytes)) = cx.svg_renderer.render(&params)? else {
                        return Ok(None);
                    };
                    Ok(Some((size, Cow::Owned(bytes))))
                })?
        else {
            return Ok(());
        };
        let content_mask = self.content_mask().scale(scale_factor);
        let svg_bounds = Bounds {
            origin: bounds.center()
                - Point::new(
                    ScaledPixels(tile.bounds.size.width.0 as f32 / SMOOTH_SVG_SCALE_FACTOR / 2.),
                    ScaledPixels(tile.bounds.size.height.0 as f32 / SMOOTH_SVG_SCALE_FACTOR / 2.),
                ),
            size: tile
                .bounds
                .size
                .map(|value| ScaledPixels(value.0 as f32 / SMOOTH_SVG_SCALE_FACTOR)),
        };

        self.next_frame.scene.insert_primitive(MonochromeSprite {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            color_filter: self.element_color_filter,
            pad: 0,
            bounds: svg_bounds
                .map_origin(|origin| origin.round())
                .map_size(|size| size.ceil()),
            content_mask,
            color: color.opacity(element_opacity),
            tile,
            transformation,
        });

        Ok(())
    }

    pub(crate) fn paint_icon(
        &mut self,
        bounds: Bounds<Pixels>,
        name: SharedString,
        color: Hsla,
        cx: &App,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let fallback_path = crate::icons::icon_asset_path(name.as_ref());

        let element_opacity = self.element_opacity();
        let scale_factor = self.scale_factor();
        let bounds = bounds.scale_and_snap(scale_factor);
        let requested_size = bounds.size.map(|value| DevicePixels(value.0.ceil() as i32));
        let Some(icon) = crate::icons::resolve_generated_icon(name.as_ref(), requested_size) else {
            return self.paint_svg(
                bounds.map(|value| Pixels(value.0 / scale_factor)),
                fallback_path,
                TransformationMatrix::default(),
                color,
                cx,
            );
        };

        let Some(atlas_tile) = self.sprite_atlas.get_or_insert_with(
            &crate::AtlasKey::IconAtlas(icon.atlas_params.clone()),
            &mut || Ok(Some((icon.atlas_size, Cow::Borrowed(icon.bytes)))),
        )?
        else {
            return Ok(());
        };

        let content_mask = self.content_mask().scale(scale_factor);
        self.next_frame.scene.insert_primitive(MonochromeSprite {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            color_filter: self.element_color_filter,
            pad: 0,
            bounds: bounds
                .map_origin(|origin| origin.floor())
                .map_size(|size| size.ceil()),
            content_mask,
            color: color.opacity(element_opacity),
            tile: crate::icons::icon_subtile(atlas_tile, icon.rect),
            transformation: TransformationMatrix::default(),
        });

        Ok(())
    }

    /// Paint an image into the scene for the next frame at the current z-index.
    /// This method returns an error if the frame index is not valid.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn paint_image(
        &mut self,
        bounds: Bounds<Pixels>,
        corner_radii: Corners<Pixels>,
        data: Arc<RenderImage>,
        frame_index: usize,
        grayscale: bool,
    ) -> Result<()> {
        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let bounds = bounds.scale_and_snap(scale_factor);
        let params = RenderImageParams {
            image_id: data.id,
            frame_index,
        };

        let Some(tile) = self
            .sprite_atlas
            .get_or_insert_with(&params.into(), &mut || {
                let bytes = data
                    .as_bytes(frame_index)
                    .with_context(|| format!("invalid image frame index {frame_index}"))?;
                Ok(Some((data.size(frame_index), Cow::Borrowed(bytes))))
            })?
        else {
            return Ok(());
        };
        let content_mask = self.content_mask().scale(scale_factor);
        let corner_radii = corner_radii.scale_and_snap(scale_factor);
        let opacity = self.element_opacity();

        self.next_frame.scene.insert_primitive(PolychromeSprite {
            order: 0,
            rounded_clip_bounds: self.rounded_clip.0,
            rounded_clip_radii: self.rounded_clip.1,
            color_filter: self.element_color_filter,
            pad: 0,
            grayscale,
            bounds: bounds
                .map_origin(|origin| origin.floor())
                .map_size(|size| size.ceil()),
            content_mask,
            corner_radii,
            tile,
            opacity,
            sprite_kind: POLYCHROME_SPRITE_KIND_COLOR,
            color: transparent_black(),
            pad3: 0,
            transformation: self.element_transform,
            blur_radius: 0.0,
            pad2: 0,
        });
        Ok(())
    }

    /// Paint a surface into the scene for the next frame at the current z-index.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    #[cfg(target_os = "macos")]
    pub fn paint_surface(&mut self, bounds: Bounds<Pixels>, image_buffer: CVPixelBuffer) {
        use crate::PaintSurface;

        self.invalidator.debug_assert_paint();

        let scale_factor = self.scale_factor();
        let bounds = bounds.scale(scale_factor);
        let content_mask = self.content_mask().scale(scale_factor);
        self.next_frame.scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask,
            image_buffer,
        });
    }

    /// Register a native WebView for the next frame at the given bounds.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub(crate) fn paint_webview(&mut self, webview: PlatformWebView) {
        self.invalidator.debug_assert_paint();
        self.next_frame.webviews.push(webview);
    }

    /// Navigate the WebView with the given identifier to a new URL.
    pub fn navigate_webview(
        &mut self,
        id: impl Into<SharedString>,
        url: impl Into<SharedString>,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::Navigate {
                id: id.into(),
                url: url.into(),
            })
    }

    /// Navigate the WebView with the given identifier to a new URL with additional request headers.
    pub fn navigate_webview_with_headers(
        &mut self,
        id: impl Into<SharedString>,
        url: impl Into<SharedString>,
        headers: http_client::http::HeaderMap,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::NavigateWithHeaders {
                id: id.into(),
                url: url.into(),
                headers,
            })
    }

    /// Load an HTML string into the WebView with the given identifier.
    pub fn load_webview_html(
        &mut self,
        id: impl Into<SharedString>,
        html: impl Into<SharedString>,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::LoadHtml {
                id: id.into(),
                html: html.into(),
            })
    }

    /// Evaluate JavaScript in the WebView with the given identifier.
    pub fn evaluate_webview_javascript(
        &mut self,
        id: impl Into<SharedString>,
        script: impl Into<SharedString>,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::EvaluateJavaScript {
                id: id.into(),
                script: script.into(),
            })
    }

    /// Evaluate JavaScript in the WebView with the given identifier and receive its serialized result.
    pub fn evaluate_webview_javascript_with_result(
        &mut self,
        id: impl Into<SharedString>,
        script: impl Into<SharedString>,
        callback: impl Fn(Result<SharedString, SharedString>) + Send + Sync + 'static,
    ) -> Result<()> {
        self.platform_window.dispatch_webview_command(
            PlatformWebViewCommand::EvaluateJavaScriptWithResult {
                id: id.into(),
                script: script.into(),
                callback: std::sync::Arc::new(callback),
            },
        )
    }

    /// Post a structured message into the WebView with the given identifier.
    pub fn post_webview_message(
        &mut self,
        id: impl Into<SharedString>,
        message: serde_json::Value,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::PostMessage {
                id: id.into(),
                message,
            })
    }

    /// Reload the WebView with the given identifier.
    pub fn reload_webview(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::Reload { id: id.into() })
    }

    /// Stop loading resources in the WebView with the given identifier.
    pub fn stop_loading_webview(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.evaluate_webview_javascript(id, "window.stop && window.stop();")
    }

    /// Pause every browser media element in the WebView with the given identifier.
    pub fn pause_webview_media(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.evaluate_webview_javascript(
            id,
            "(() => { for (const element of Array.from(document.querySelectorAll('audio,video'))) { if (typeof element.pause === 'function') element.pause(); } })();",
        )
    }

    /// Mute or unmute every browser media element in the WebView with the given identifier.
    pub fn set_webview_media_muted(
        &mut self,
        id: impl Into<SharedString>,
        muted: bool,
    ) -> Result<()> {
        let script = if muted {
            "(() => { for (const element of Array.from(document.querySelectorAll('audio,video'))) { element.muted = true; } })();"
        } else {
            "(() => { for (const element of Array.from(document.querySelectorAll('audio,video'))) { element.muted = false; } })();"
        };
        self.evaluate_webview_javascript(id, script)
    }

    /// Navigate the WebView with the given identifier backward if possible.
    pub fn go_back_webview(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::GoBack { id: id.into() })
    }

    /// Navigate the WebView with the given identifier forward if possible.
    pub fn go_forward_webview(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::GoForward { id: id.into() })
    }

    /// Open developer tools for the WebView with the given identifier when supported.
    pub fn open_webview_devtools(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::OpenDevTools { id: id.into() })
    }

    /// Close developer tools for the WebView with the given identifier when supported.
    pub fn close_webview_devtools(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::CloseDevTools { id: id.into() })
    }

    /// Read whether developer tools are open for the WebView with the given identifier.
    pub fn is_webview_devtools_open(
        &mut self,
        id: impl Into<SharedString>,
        callback: impl Fn(Result<bool, SharedString>) + 'static,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::IsDevToolsOpen {
                id: id.into(),
                callback: Rc::new(callback),
            })
    }

    /// Open the platform print dialog for the WebView with the given identifier.
    pub fn print_webview(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::Print { id: id.into() })
    }

    /// Set the browser zoom factor for the WebView with the given identifier.
    pub fn set_webview_zoom_factor(
        &mut self,
        id: impl Into<SharedString>,
        factor: f64,
    ) -> Result<()> {
        anyhow::ensure!(
            factor.is_finite() && (0.25..=5.0).contains(&factor),
            "WebView zoom factor must be finite and between 0.25 and 5.0"
        );
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::SetZoomFactor {
                id: id.into(),
                factor,
            })
    }

    /// Move focus into the WebView with the given identifier when supported.
    pub fn focus_webview(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::Focus { id: id.into() })
    }

    /// Move focus from the WebView back to the parent window when supported.
    pub fn focus_webview_parent(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::FocusParent { id: id.into() })
    }

    /// Clear cookies, cache, local storage, and other browsing data for the WebView profile.
    pub fn clear_webview_browsing_data(&mut self, id: impl Into<SharedString>) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::ClearBrowsingData { id: id.into() })
    }

    /// Read the current URL reported by the WebView with the given identifier.
    pub fn read_webview_url(
        &mut self,
        id: impl Into<SharedString>,
        callback: impl Fn(Result<SharedString, SharedString>) + 'static,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::ReadUrl {
                id: id.into(),
                callback: Rc::new(callback),
            })
    }

    /// Read all cookies visible to the WebView with the given identifier.
    pub fn read_webview_cookies(
        &mut self,
        id: impl Into<SharedString>,
        callback: impl Fn(Result<Vec<crate::WebViewCookie>, SharedString>) + 'static,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::ReadCookies {
                id: id.into(),
                url: None,
                callback: Rc::new(callback),
            })
    }

    /// Read cookies for a URL from the WebView with the given identifier.
    pub fn read_webview_cookies_for_url(
        &mut self,
        id: impl Into<SharedString>,
        url: impl Into<SharedString>,
        callback: impl Fn(Result<Vec<crate::WebViewCookie>, SharedString>) + 'static,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::ReadCookies {
                id: id.into(),
                url: Some(url.into()),
                callback: Rc::new(callback),
            })
    }

    /// Set a cookie in the WebView with the given identifier.
    pub fn set_webview_cookie(
        &mut self,
        id: impl Into<SharedString>,
        cookie: crate::WebViewCookie,
        callback: impl Fn(Result<(), SharedString>) + 'static,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::SetCookie {
                id: id.into(),
                cookie,
                callback: Rc::new(callback),
            })
    }

    /// Delete a cookie from the WebView with the given identifier.
    pub fn delete_webview_cookie(
        &mut self,
        id: impl Into<SharedString>,
        cookie: crate::WebViewCookie,
        callback: impl Fn(Result<(), SharedString>) + 'static,
    ) -> Result<()> {
        self.platform_window
            .dispatch_webview_command(PlatformWebViewCommand::DeleteCookie {
                id: id.into(),
                cookie,
                callback: Rc::new(callback),
            })
    }

    /// Sends a print job directly to the platform print system without showing the native print dialog.
    pub fn print(&mut self, job: PrintJob, cx: &mut App) -> Result<()> {
        self.platform_window.print(job.into_platform_job(cx)?)
    }

    /// Shows the native print dialog for a print job.
    pub fn show_print_dialog(&mut self, job: PrintJob, cx: &mut App) -> Result<()> {
        self.platform_window
            .show_print_dialog(job.into_platform_job(cx)?)
    }

    /// Validate and dispatch a native or WebView print request.
    pub fn print_checked(&mut self, request: PrintRequest, cx: &mut App) -> Result<()> {
        request.validate()?;
        match request {
            PrintRequest::NativeJob { job, mode } => match mode {
                PrintDialogMode::ShowDialog => self.show_print_dialog(job, cx),
                PrintDialogMode::Silent => self.print(job, cx),
            },
            PrintRequest::WebView { id } => self.print_webview(id),
        }
    }

    /// Removes an image from the sprite atlas.
    pub fn drop_image(&mut self, data: Arc<RenderImage>) -> Result<()> {
        for frame_index in 0..data.frame_count() {
            let params = RenderImageParams {
                image_id: data.id,
                frame_index,
            };

            self.sprite_atlas.remove(&params.clone().into());
        }

        Ok(())
    }

    fn layout_engine_mut(&mut self) -> &mut TaffyLayoutEngine {
        self.layout_engine
            .as_mut()
            .unwrap_or_else(|| panic!("window layout engine missing"))
    }

    fn take_layout_engine(&mut self) -> TaffyLayoutEngine {
        self.layout_engine
            .take()
            .unwrap_or_else(|| panic!("window layout engine missing"))
    }

    /// Add a node to the layout tree for the current frame. Takes the `Style` of the element for which
    /// layout is being requested, along with the layout ids of any children. This method is called during
    /// calls to the [`Element::request_layout`] trait method and enables any element to participate in layout.
    ///
    /// This method should only be called as part of the request_layout or prepaint phase of element drawing.
    #[must_use]
    pub fn request_layout(
        &mut self,
        style: Style,
        children: impl IntoIterator<Item = LayoutId>,
        cx: &mut App,
    ) -> LayoutId {
        self.invalidator.debug_assert_prepaint();

        cx.layout_id_buffer.clear();
        cx.layout_id_buffer.extend(children);
        let rem_size = self.base_rem_size();
        let scale_factor = self.ui_layout_scale_factor();
        self.frame_layout_nodes = self.frame_layout_nodes.saturating_add(1);

        self.layout_engine_mut()
            .request_layout(style, rem_size, scale_factor, &cx.layout_id_buffer)
    }

    /// Add a node to the layout tree for the current frame. Instead of taking a `Style` and children,
    /// this variant takes a function that is invoked during layout so you can use arbitrary logic to
    /// determine the element's size. One place this is used internally is when measuring text.
    ///
    /// The given closure is invoked at layout time with the known dimensions and available space and
    /// returns a `Size`.
    ///
    /// This method should only be called as part of the request_layout or prepaint phase of element drawing.
    pub fn request_measured_layout<
        F: FnMut(Size<Option<Pixels>>, Size<AvailableSpace>, &mut Window, &mut App) -> Size<Pixels>
            + 'static,
    >(
        &mut self,
        style: Style,
        measure: F,
    ) -> LayoutId {
        self.invalidator.debug_assert_prepaint();

        let rem_size = self.base_rem_size();
        let scale_factor = self.ui_layout_scale_factor();
        self.frame_layout_nodes = self.frame_layout_nodes.saturating_add(1);
        self.layout_engine_mut()
            .request_measured_layout(style, rem_size, scale_factor, measure)
    }

    /// Compute the layout for the given id within the given available space.
    /// This method is called for its side effect, typically by the framework prior to painting.
    /// After calling it, you can request the bounds of the given layout node id or any descendant.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn compute_layout(
        &mut self,
        layout_id: LayoutId,
        available_space: Size<AvailableSpace>,
        cx: &mut App,
    ) {
        self.invalidator.debug_assert_prepaint();

        let mut layout_engine = self.take_layout_engine();
        let started_at = Instant::now();
        layout_engine.compute_layout(layout_id, available_space, self, cx);
        self.record_taffy_compute_duration(started_at.elapsed());
        self.layout_engine = Some(layout_engine);
    }

    /// Obtain the bounds computed for the given LayoutId relative to the window. This method will usually be invoked by
    /// GPUI itself automatically in order to pass your element its `Bounds` automatically.
    ///
    /// This method should only be called as part of element drawing.
    pub fn layout_bounds(&mut self, layout_id: LayoutId) -> Bounds<Pixels> {
        self.invalidator.debug_assert_prepaint();

        let scale_factor = self.scale_factor();
        let mut bounds = self
            .layout_engine_mut()
            .layout_bounds(layout_id, scale_factor)
            .map(Into::into);
        bounds.origin += self.element_offset();
        bounds
    }

    /// This method should be called during `prepaint`. You can use
    /// the returned [Hitbox] during `paint` or in an event handler
    /// to determine whether the inserted hitbox was the topmost.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn insert_hitbox(&mut self, bounds: Bounds<Pixels>, behavior: HitboxBehavior) -> Hitbox {
        self.invalidator.debug_assert_prepaint();

        let content_mask = self.content_mask();
        let mut id = self.next_hitbox_id;
        self.next_hitbox_id = self.next_hitbox_id.next();
        let hitbox = Hitbox {
            id,
            bounds,
            content_mask,
            behavior,
        };
        self.next_frame.hitboxes.push(hitbox.clone());
        hitbox
    }

    /// Set a hitbox which will act as a control area of the platform window.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn insert_window_control_hitbox(&mut self, area: WindowControlArea, hitbox: Hitbox) {
        self.invalidator.debug_assert_paint();
        self.next_frame.window_control_hitboxes.push((area, hitbox));
    }

    /// Sets the key context for the current element. This context will be used to translate
    /// keybindings into actions.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn set_key_context(&mut self, context: KeyContext) {
        self.invalidator.debug_assert_paint();
        self.next_frame.dispatch_tree.set_key_context(context);
    }

    /// Sets the focus handle for the current element. This handle will be used to manage focus state
    /// and keyboard event dispatch for the element.
    ///
    /// This method should only be called as part of the prepaint phase of element drawing.
    pub fn set_focus_handle(&mut self, focus_handle: &FocusHandle, _: &App) {
        self.invalidator.debug_assert_prepaint();
        if focus_handle.is_focused(self) {
            self.next_frame.focus = Some(focus_handle.id);
        }
        self.next_frame.dispatch_tree.set_focus_id(focus_handle.id);
    }

    /// Sets the view id for the current element, which will be used to manage view caching.
    ///
    /// This method should only be called as part of element prepaint. We plan on removing this
    /// method eventually when we solve some issues that require us to construct editor elements
    /// directly instead of always using editors via views.
    pub fn set_view_id(&mut self, view_id: EntityId) {
        self.invalidator.debug_assert_prepaint();
        self.next_frame.dispatch_tree.set_view_id(view_id);
    }

    /// Get the entity ID for the currently rendering view
    pub fn current_view(&self) -> EntityId {
        self.invalidator.debug_assert_paint_or_prepaint();
        self.rendered_entity_stack
            .last()
            .copied()
            .unwrap_or_else(|| panic!("current_view called without a rendered entity"))
    }

    pub(crate) fn with_rendered_view<R>(
        &mut self,
        id: EntityId,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.rendered_entity_stack.push(id);
        let result = f(self);
        self.rendered_entity_stack.pop();
        result
    }

    /// Executes the provided function with the specified image cache.
    pub fn with_image_cache<F, R>(&mut self, image_cache: Option<AnyImageCache>, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        if let Some(image_cache) = image_cache {
            self.image_cache_stack.push(image_cache);
            let result = f(self);
            self.image_cache_stack.pop();
            result
        } else {
            f(self)
        }
    }

    /// Sets an input handler, such as [`ElementInputHandler`][element_input_handler], which interfaces with the
    /// platform to receive textual input with proper integration with concerns such
    /// as IME interactions. This handler will be active for the upcoming frame until the following frame is
    /// rendered.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    ///
    /// [element_input_handler]: crate::ElementInputHandler
    pub fn handle_input(
        &mut self,
        focus_handle: &FocusHandle,
        input_handler: impl InputHandler,
        cx: &App,
    ) {
        self.invalidator.debug_assert_paint();

        if focus_handle.is_focused(self) {
            let cx = self.to_async(cx);
            self.next_frame
                .input_handlers
                .push(Some(PlatformInputHandler::new(cx, Box::new(input_handler))));
        }
    }

    /// Register a mouse event listener on the window for the next frame. The type of event
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_mouse_event<Event: MouseEvent>(
        &mut self,
        mut handler: impl FnMut(&Event, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame.mouse_listeners.push(Some(Box::new(
            move |event: &dyn Any, phase: DispatchPhase, window: &mut Window, cx: &mut App| {
                if let Some(event) = event.downcast_ref() {
                    handler(event, phase, window, cx)
                }
            },
        )));
    }

    /// Register a key event listener on the window for the next frame. The type of event
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This is a fairly low-level method, so prefer using event handlers on elements unless you have
    /// a specific need to register a global listener.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_key_event<Event: KeyEvent>(
        &mut self,
        listener: impl Fn(&Event, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame.dispatch_tree.on_key_event(Rc::new(
            move |event: &dyn Any, phase, window: &mut Window, cx: &mut App| {
                if let Some(event) = event.downcast_ref::<Event>() {
                    listener(event, phase, window, cx)
                }
            },
        ));
    }

    /// Register a modifiers changed event listener on the window for the next frame.
    ///
    /// This is a fairly low-level method, so prefer using event handlers on elements unless you have
    /// a specific need to register a global listener.
    ///
    /// This method should only be called as part of the paint phase of element drawing.
    pub fn on_modifiers_changed(
        &mut self,
        listener: impl Fn(&ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) {
        self.invalidator.debug_assert_paint();

        self.next_frame.dispatch_tree.on_modifiers_changed(Rc::new(
            move |event: &ModifiersChangedEvent, window: &mut Window, cx: &mut App| {
                listener(event, window, cx)
            },
        ));
    }

    /// Register a listener to be called when the given focus handle or one of its descendants receives focus.
    /// This does not fire if the given focus handle - or one of its descendants - was previously focused.
    /// Returns a subscription and persists until the subscription is dropped.
    pub fn on_focus_in(
        &mut self,
        handle: &FocusHandle,
        cx: &mut App,
        mut listener: impl FnMut(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let focus_id = handle.id;
        let (subscription, activate) =
            self.new_focus_listener(Box::new(move |event, window, cx| {
                if event.is_focus_in(focus_id) {
                    listener(window, cx);
                }
                true
            }));
        cx.defer(move |_| activate());
        subscription
    }

    /// Register a listener to be called when the given focus handle or one of its descendants loses focus.
    /// Returns a subscription and persists until the subscription is dropped.
    pub fn on_focus_out(
        &mut self,
        handle: &FocusHandle,
        cx: &mut App,
        mut listener: impl FnMut(FocusOutEvent, &mut Window, &mut App) + 'static,
    ) -> Subscription {
        let focus_id = handle.id;
        let (subscription, activate) =
            self.new_focus_listener(Box::new(move |event, window, cx| {
                if let Some(blurred_id) = event.previous_focus_path.last().copied()
                    && event.is_focus_out(focus_id)
                {
                    let event = FocusOutEvent {
                        blurred: WeakFocusHandle {
                            id: blurred_id,
                            handles: Arc::downgrade(&cx.focus_handles),
                        },
                    };
                    listener(event, window, cx)
                }
                true
            }));
        cx.defer(move |_| activate());
        subscription
    }

    fn reset_cursor_style(&self, cx: &mut App) {
        // Set the cursor only if we're the active window.
        if self.is_window_hovered() {
            let style = self
                .rendered_frame
                .cursor_style(self)
                .unwrap_or(CursorStyle::Arrow);
            cx.platform.set_cursor_style(style);
        }
    }

    /// Dispatch a given keystroke as though the user had typed it.
    /// You can create a keystroke with Keystroke::parse("").
    pub fn dispatch_keystroke(&mut self, keystroke: Keystroke, cx: &mut App) -> bool {
        let keystroke = keystroke.with_simulated_ime();
        let result = self.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
            }),
            cx,
        );
        if !result.propagate {
            return true;
        }

        if let Some(input) = keystroke.key_char
            && let Some(mut input_handler) = self.platform_window.take_input_handler()
        {
            input_handler.dispatch_input(&input, self, cx);
            self.platform_window.set_input_handler(input_handler);
            return true;
        }

        false
    }

    /// Return a key binding string for an action, to display in the UI. Uses the highest precedence
    /// binding for the action (last binding added to the keymap).
    pub fn keystroke_text_for(&self, action: &dyn Action) -> String {
        self.highest_precedence_binding_for_action(action)
            .map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|| action.name().to_string())
    }

    /// Dispatch a mouse or keyboard event on the window.
    #[profiling::function]
    pub fn dispatch_event(&mut self, event: PlatformInput, cx: &mut App) -> DispatchEventResult {
        crate::tracer::trace_global_duration("window.dispatch_event", "input", || {
            self.last_input_timestamp.set(Instant::now());
            // Handlers may set this to false by calling `stop_propagation`.
            cx.propagate_event = true;
            // Handlers may set this to true by calling `prevent_default`.
            self.default_prevented = false;

            let event = match event {
                // Track the mouse position with our own state, since accessing the platform
                // API for the mouse position can only occur on the main thread.
                PlatformInput::MouseMove(mouse_move) => {
                    self.mouse_position = mouse_move.position;
                    self.modifiers = mouse_move.modifiers;
                    PlatformInput::MouseMove(mouse_move)
                }
                PlatformInput::MouseDown(mouse_down) => {
                    self.mouse_position = mouse_down.position;
                    self.modifiers = mouse_down.modifiers;
                    self.keyboard_navigation_active = false;
                    PlatformInput::MouseDown(mouse_down)
                }
                PlatformInput::MouseUp(mouse_up) => {
                    self.mouse_position = mouse_up.position;
                    self.modifiers = mouse_up.modifiers;
                    PlatformInput::MouseUp(mouse_up)
                }
                PlatformInput::MouseExited(mouse_exited) => {
                    self.modifiers = mouse_exited.modifiers;
                    PlatformInput::MouseExited(mouse_exited)
                }
                PlatformInput::ModifiersChanged(modifiers_changed) => {
                    self.modifiers = modifiers_changed.modifiers;
                    self.capslock = modifiers_changed.capslock;
                    PlatformInput::ModifiersChanged(modifiers_changed)
                }
                PlatformInput::ScrollWheel(scroll_wheel) => {
                    self.mouse_position = scroll_wheel.position;
                    self.modifiers = scroll_wheel.modifiers;
                    PlatformInput::ScrollWheel(scroll_wheel)
                }
                PlatformInput::Magnify(magnify) => {
                    self.mouse_position = magnify.position;
                    self.modifiers = magnify.modifiers;
                    PlatformInput::Magnify(magnify)
                }
                // Translate dragging and dropping of external files from the operating system
                // to internal drag and drop events.
                PlatformInput::FileDrop(file_drop) => match file_drop {
                    FileDropEvent::Entered { position, paths } => {
                        self.mouse_position = position;
                        if cx.active_drag.is_none() {
                            cx.active_drag = Some(AnyDrag {
                                value: Arc::new(paths.clone()),
                                view: cx.new(|_| paths).into(),
                                cursor_offset: position,
                                cursor_style: None,
                            });
                        }
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::default(),
                        })
                    }
                    FileDropEvent::DataEntered { position, data } => {
                        self.mouse_position = position;
                        if cx.active_drag.is_none() {
                            cx.active_drag = Some(AnyDrag {
                                value: Arc::new(data.clone()),
                                view: cx.new(|_| data).into(),
                                cursor_offset: position,
                                cursor_style: None,
                            });
                        }
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::default(),
                        })
                    }
                    FileDropEvent::Pending { position } => {
                        self.mouse_position = position;
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: Modifiers::default(),
                        })
                    }
                    FileDropEvent::Submit { position } => {
                        cx.activate(true);
                        self.mouse_position = position;
                        PlatformInput::MouseUp(MouseUpEvent {
                            button: MouseButton::Left,
                            position,
                            modifiers: Modifiers::default(),
                            click_count: 1,
                        })
                    }
                    FileDropEvent::Exited => {
                        cx.active_drag.take();
                        PlatformInput::FileDrop(FileDropEvent::Exited)
                    }
                },
                PlatformInput::KeyDown(key_down) => {
                    self.keyboard_navigation_active = true;
                    PlatformInput::KeyDown(key_down)
                }
                PlatformInput::KeyUp(key_up) => PlatformInput::KeyUp(key_up),
            };

            if let Some(any_mouse_event) = event.mouse_event() {
                self.dispatch_mouse_event(any_mouse_event, cx);
            } else if let Some(any_key_event) = event.keyboard_event() {
                self.dispatch_key_event(any_key_event, cx);
            }

            self.update_frame_polling();

            DispatchEventResult {
                propagate: cx.propagate_event,
                default_prevented: self.default_prevented,
            }
        })
    }

    fn dispatch_mouse_event(&mut self, event: &dyn Any, cx: &mut App) {
        let hit_test = self.rendered_frame.hit_test(self.mouse_position());
        if hit_test != self.mouse_hit_test {
            self.mouse_hit_test = hit_test;
            self.reset_cursor_style(cx);
        }

        #[cfg(any(feature = "inspector", debug_assertions))]
        if self.is_inspector_picking(cx) {
            self.handle_inspector_mouse_event(event, cx);
            // When inspector is picking, all other mouse handling is skipped.
            return;
        }

        let mut mouse_listeners = mem::take(&mut self.rendered_frame.mouse_listeners);

        // Capture phase, events bubble from back to front. Handlers for this phase are used for
        // special purposes, such as detecting events outside of a given Bounds.
        for listener in &mut mouse_listeners {
            let Some(listener) = listener.as_mut() else {
                continue;
            };
            listener(event, DispatchPhase::Capture, self, cx);
            if !cx.propagate_event {
                break;
            }
        }

        // Bubble phase, where most normal handlers do their work.
        if cx.propagate_event {
            for listener in mouse_listeners.iter_mut().rev() {
                let Some(listener) = listener.as_mut() else {
                    continue;
                };
                listener(event, DispatchPhase::Bubble, self, cx);
                if !cx.propagate_event {
                    break;
                }
            }
        }

        self.rendered_frame.mouse_listeners = mouse_listeners;

        if cx.has_active_drag() {
            if event.is::<MouseMoveEvent>() {
                // If this was a mouse move event, redraw the window so that the
                // active drag can follow the mouse cursor.
                self.refresh();
            } else if event.is::<MouseUpEvent>() {
                // If this was a mouse up event, cancel the active drag and redraw
                // the window.
                cx.active_drag = None;
                self.refresh();
            }
        }
    }

    fn dispatch_key_event(&mut self, event: &dyn Any, cx: &mut App) {
        if self.invalidator.is_dirty() {
            self.draw(cx).clear();
        }

        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);

        let mut keystroke: Option<Keystroke> = None;

        if let Some(event) = event.downcast_ref::<ModifiersChangedEvent>() {
            if event.modifiers.number_of_modifiers() == 0
                && self.pending_modifier.modifiers.number_of_modifiers() == 1
                && !self.pending_modifier.saw_keystroke
            {
                let key = match self.pending_modifier.modifiers {
                    modifiers if modifiers.shift => Some("shift"),
                    modifiers if modifiers.control => Some("control"),
                    modifiers if modifiers.alt => Some("alt"),
                    modifiers if modifiers.platform => Some("platform"),
                    modifiers if modifiers.function => Some("function"),
                    _ => None,
                };
                if let Some(key) = key {
                    keystroke = Some(Keystroke {
                        key: key.to_string(),
                        key_char: None,
                        modifiers: Modifiers::default(),
                    });
                }
            }

            if self.pending_modifier.modifiers.number_of_modifiers() == 0
                && event.modifiers.number_of_modifiers() == 1
            {
                self.pending_modifier.saw_keystroke = false
            }
            self.pending_modifier.modifiers = event.modifiers
        } else if let Some(key_down_event) = event.downcast_ref::<KeyDownEvent>() {
            self.pending_modifier.saw_keystroke = true;
            keystroke = Some(key_down_event.keystroke.clone());
        }

        let Some(keystroke) = keystroke else {
            self.finish_dispatch_key_event(event, dispatch_path, self.context_stack(), cx);
            return;
        };

        cx.propagate_event = true;
        self.dispatch_keystroke_interceptors(event, self.context_stack(), cx);
        if !cx.propagate_event {
            self.finish_dispatch_key_event(event, dispatch_path, self.context_stack(), cx);
            return;
        }

        let mut currently_pending = self.pending_input.take().unwrap_or_default();
        if currently_pending.focus.is_some() && currently_pending.focus != self.focus {
            currently_pending = PendingInput::default();
        }

        let match_result = self.rendered_frame.dispatch_tree.dispatch_key(
            currently_pending.keystrokes,
            keystroke,
            &dispatch_path,
        );

        if !match_result.to_replay.is_empty() {
            self.replay_pending_input(match_result.to_replay, cx);
            cx.propagate_event = true;
        }

        if !match_result.pending.is_empty() {
            currently_pending.keystrokes = match_result.pending;
            currently_pending.focus = self.focus;
            currently_pending.timer = Some(self.spawn(cx, async move |cx| {
                cx.background_executor.timer(Duration::from_secs(1)).await;
                cx.update(move |window, cx| {
                    let Some(currently_pending) = window
                        .pending_input
                        .take()
                        .filter(|pending| pending.focus == window.focus)
                    else {
                        return;
                    };

                    let node_id = window.focus_node_id_in_rendered_frame(window.focus);
                    let dispatch_path = window.rendered_frame.dispatch_tree.dispatch_path(node_id);

                    let to_replay = window
                        .rendered_frame
                        .dispatch_tree
                        .flush_dispatch(currently_pending.keystrokes, &dispatch_path);

                    window.pending_input_changed(cx);
                    window.replay_pending_input(to_replay, cx)
                })
                .log_err();
            }));
            self.pending_input = Some(currently_pending);
            self.pending_input_changed(cx);
            cx.propagate_event = false;
            return;
        }

        for binding in match_result.bindings {
            self.dispatch_action_on_node(node_id, binding.action.as_ref(), cx);
            if !cx.propagate_event {
                self.dispatch_keystroke_observers(
                    event,
                    Some(binding.action),
                    match_result.context_stack,
                    cx,
                );
                self.pending_input_changed(cx);
                return;
            }
        }

        self.finish_dispatch_key_event(event, dispatch_path, match_result.context_stack, cx);
        self.pending_input_changed(cx);
    }

    fn finish_dispatch_key_event(
        &mut self,
        event: &dyn Any,
        dispatch_path: SmallVec<[DispatchNodeId; 32]>,
        context_stack: Vec<KeyContext>,
        cx: &mut App,
    ) {
        self.dispatch_key_down_up_event(event, &dispatch_path, cx);
        if !cx.propagate_event {
            return;
        }

        self.dispatch_modifiers_changed_event(event, &dispatch_path, cx);
        if !cx.propagate_event {
            return;
        }

        self.handle_default_keyboard_navigation(event, cx);
        if !cx.propagate_event {
            return;
        }

        self.dispatch_keystroke_observers(event, None, context_stack, cx);
    }

    fn handle_default_keyboard_navigation(&mut self, event: &dyn Any, cx: &mut App) {
        let Some(event) = event.downcast_ref::<KeyDownEvent>() else {
            return;
        };

        let modifiers = event.keystroke.modifiers;
        let secondary_only = modifiers.secondary() && modifiers.number_of_modifiers() == 1;
        let secondary_with_shift =
            modifiers.secondary() && modifiers.shift && modifiers.number_of_modifiers() == 2;
        let handled_zoom = match event.keystroke.key.as_str() {
            "+" | "=" if secondary_only || secondary_with_shift => {
                self.zoom_in();
                true
            }
            "-" if secondary_only => {
                self.zoom_out();
                true
            }
            "0" if secondary_only => {
                self.reset_zoom();
                true
            }
            _ => false,
        };
        if handled_zoom {
            self.default_prevented = true;
            cx.propagate_event = false;
            return;
        }

        let next_focus = if event.keystroke.key == "tab"
            && event.keystroke.modifiers.number_of_modifiers() == 0
        {
            self.rendered_frame.tab_stops.next(self.focus.as_ref())
        } else if event.keystroke.key == "tab"
            && event.keystroke.modifiers.shift
            && event.keystroke.modifiers.number_of_modifiers() == 1
        {
            self.rendered_frame.tab_stops.prev(self.focus.as_ref())
        } else if matches!(event.keystroke.key.as_str(), "left" | "up")
            && event.keystroke.modifiers.number_of_modifiers() == 0
        {
            self.rendered_frame
                .tab_stops
                .prev_in_group(self.focus.as_ref())
        } else if matches!(event.keystroke.key.as_str(), "right" | "down")
            && event.keystroke.modifiers.number_of_modifiers() == 0
        {
            self.rendered_frame
                .tab_stops
                .next_in_group(self.focus.as_ref())
        } else {
            None
        };

        if let Some(handle) = next_focus {
            self.focus(&handle);
            self.default_prevented = true;
            cx.propagate_event = false;
        }
    }

    fn pending_input_changed(&mut self, cx: &mut App) {
        self.pending_input_observers
            .clone()
            .retain(&(), |callback| callback(self, cx));
    }

    fn dispatch_key_down_up_event(
        &mut self,
        event: &dyn Any,
        dispatch_path: &SmallVec<[DispatchNodeId; 32]>,
        cx: &mut App,
    ) {
        // Capture phase
        for node_id in dispatch_path {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);

            for key_listener in node.key_listeners.clone() {
                key_listener(event, DispatchPhase::Capture, self, cx);
                if !cx.propagate_event {
                    return;
                }
            }
        }

        // Bubble phase
        for node_id in dispatch_path.iter().rev() {
            // Handle low level key events
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for key_listener in node.key_listeners.clone() {
                key_listener(event, DispatchPhase::Bubble, self, cx);
                if !cx.propagate_event {
                    return;
                }
            }
        }
    }

    fn dispatch_modifiers_changed_event(
        &mut self,
        event: &dyn Any,
        dispatch_path: &SmallVec<[DispatchNodeId; 32]>,
        cx: &mut App,
    ) {
        let Some(event) = event.downcast_ref::<ModifiersChangedEvent>() else {
            return;
        };
        for node_id in dispatch_path.iter().rev() {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for listener in node.modifiers_changed_listeners.clone() {
                listener(event, self, cx);
                if !cx.propagate_event {
                    return;
                }
            }
        }
    }

    /// Determine whether a potential multi-stroke key binding is in progress on this window.
    pub fn has_pending_keystrokes(&self) -> bool {
        self.pending_input.is_some()
    }

    pub(crate) fn clear_pending_keystrokes(&mut self) {
        self.pending_input.take();
    }

    /// Returns the currently pending input keystrokes that might result in a multi-stroke key binding.
    pub fn pending_input_keystrokes(&self) -> Option<&[Keystroke]> {
        self.pending_input
            .as_ref()
            .map(|pending_input| pending_input.keystrokes.as_slice())
    }

    fn replay_pending_input(&mut self, replays: SmallVec<[Replay; 1]>, cx: &mut App) {
        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);

        'replay: for replay in replays {
            let event = KeyDownEvent {
                keystroke: replay.keystroke.clone(),
                is_held: false,
            };

            cx.propagate_event = true;
            for binding in replay.bindings {
                self.dispatch_action_on_node(node_id, binding.action.as_ref(), cx);
                if !cx.propagate_event {
                    self.dispatch_keystroke_observers(
                        &event,
                        Some(binding.action),
                        Vec::default(),
                        cx,
                    );
                    continue 'replay;
                }
            }

            self.dispatch_key_down_up_event(&event, &dispatch_path, cx);
            if !cx.propagate_event {
                continue 'replay;
            }
            if let Some(input) = replay.keystroke.key_char.as_ref().cloned()
                && let Some(mut input_handler) = self.platform_window.take_input_handler()
            {
                input_handler.dispatch_input(&input, self, cx);
                self.platform_window.set_input_handler(input_handler)
            }
        }
    }

    fn focus_node_id_in_rendered_frame(&self, focus_id: Option<FocusId>) -> DispatchNodeId {
        focus_id
            .and_then(|focus_id| {
                self.rendered_frame
                    .dispatch_tree
                    .focusable_node_id(focus_id)
            })
            .unwrap_or_else(|| self.rendered_frame.dispatch_tree.root_node_id())
    }

    fn dispatch_action_on_node(
        &mut self,
        node_id: DispatchNodeId,
        action: &dyn Action,
        cx: &mut App,
    ) {
        let dispatch_path = self.rendered_frame.dispatch_tree.dispatch_path(node_id);

        // Capture phase for global actions.
        cx.propagate_event = true;
        if let Some(mut global_listeners) = cx
            .global_action_listeners
            .remove(&action.as_any().type_id())
        {
            for listener in &global_listeners {
                listener(action.as_any(), DispatchPhase::Capture, cx);
                if !cx.propagate_event {
                    break;
                }
            }

            global_listeners.extend(
                cx.global_action_listeners
                    .remove(&action.as_any().type_id())
                    .unwrap_or_default(),
            );

            cx.global_action_listeners
                .insert(action.as_any().type_id(), global_listeners);
        }

        if !cx.propagate_event {
            return;
        }

        // Capture phase for window actions.
        for node_id in &dispatch_path {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for DispatchActionListener {
                action_type,
                listener,
            } in node.action_listeners.clone()
            {
                let any_action = action.as_any();
                if action_type == any_action.type_id() {
                    listener(any_action, DispatchPhase::Capture, self, cx);

                    if !cx.propagate_event {
                        return;
                    }
                }
            }
        }

        // Bubble phase for window actions.
        for node_id in dispatch_path.iter().rev() {
            let node = self.rendered_frame.dispatch_tree.node(*node_id);
            for DispatchActionListener {
                action_type,
                listener,
            } in node.action_listeners.clone()
            {
                let any_action = action.as_any();
                if action_type == any_action.type_id() {
                    cx.propagate_event = false; // Actions stop propagation by default during the bubble phase
                    listener(any_action, DispatchPhase::Bubble, self, cx);

                    if !cx.propagate_event {
                        return;
                    }
                }
            }
        }

        // Bubble phase for global actions.
        if let Some(mut global_listeners) = cx
            .global_action_listeners
            .remove(&action.as_any().type_id())
        {
            for listener in global_listeners.iter().rev() {
                cx.propagate_event = false; // Actions stop propagation by default during the bubble phase

                listener(action.as_any(), DispatchPhase::Bubble, cx);
                if !cx.propagate_event {
                    break;
                }
            }

            global_listeners.extend(
                cx.global_action_listeners
                    .remove(&action.as_any().type_id())
                    .unwrap_or_default(),
            );

            cx.global_action_listeners
                .insert(action.as_any().type_id(), global_listeners);
        }
    }

    /// Register the given handler to be invoked whenever the global of the given type
    /// is updated.
    pub fn observe_global<G: Global>(
        &mut self,
        cx: &mut App,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Subscription {
        let window_handle = self.handle;
        let (subscription, activate) = cx.global_observers.insert(
            TypeId::of::<G>(),
            Box::new(move |cx| {
                window_handle
                    .update(cx, |_, window, cx| f(window, cx))
                    .is_ok()
            }),
        );
        cx.defer(move |_| activate());
        subscription
    }

    /// Focus the current window and bring it to the foreground at the platform level.
    pub fn activate_window(&self) {
        self.platform_window.activate();
    }

    /// Minimize the current window at the platform level.
    pub fn minimize_window(&self) {
        self.platform_window.minimize();
    }

    /// Show the current window at the platform level.
    pub fn show_window(&self) {
        self.platform_window.show();
    }

    /// Hide the current window at the platform level.
    pub fn hide_window(&self) {
        self.platform_window.hide();
    }

    /// Request that the current window close through the platform lifecycle.
    pub fn close_window(&self) {
        self.platform_window.close();
    }

    /// Returns whether the current window is visible at the platform level.
    pub fn is_window_visible(&self) -> bool {
        self.platform_window.is_visible()
    }

    /// Set whether mouse events should pass through the current window.
    pub fn set_mouse_passthrough(&self, passthrough: bool) {
        self.platform_window.set_mouse_passthrough(passthrough);
    }

    /// Validate and perform a window-level visibility, focus, or interaction command.
    pub fn perform_window_interaction_checked(
        &self,
        command: WindowInteractionCommand,
    ) -> Result<WindowInteractionCommand> {
        command.validate()?;
        match command.kind() {
            WindowInteractionCommandKind::Activate => self.activate_window(),
            WindowInteractionCommandKind::Minimize => self.minimize_window(),
            WindowInteractionCommandKind::ZoomWindow => self.zoom_window(),
            WindowInteractionCommandKind::Show => self.show_window(),
            WindowInteractionCommandKind::Hide => self.hide_window(),
            WindowInteractionCommandKind::Close => self.close_window(),
            WindowInteractionCommandKind::EnterFullscreen => {
                if !self.is_fullscreen() {
                    self.toggle_fullscreen();
                }
            }
            WindowInteractionCommandKind::ExitFullscreen => {
                if self.is_fullscreen() {
                    self.toggle_fullscreen();
                }
            }
            WindowInteractionCommandKind::ToggleFullscreen => self.toggle_fullscreen(),
            WindowInteractionCommandKind::MousePassthrough { enabled } => {
                self.set_mouse_passthrough(enabled);
            }
        }
        Ok(command)
    }

    /// Set a soft byte budget for this window's glyph/sprite atlas. When set, the renderer
    /// evicts least-recently-used atlas tiles down to the budget at the end of each frame,
    /// bounding glyph-atlas growth on long-running, text-churning UIs. `None` (the default)
    /// disables eviction. Currently honored on the Metal backend.
    pub fn set_atlas_byte_budget(&self, budget: Option<u64>) {
        self.platform_window.set_atlas_byte_budget(budget);
    }

    /// Validate and set a soft byte budget for this window's glyph/sprite atlas.
    pub fn set_atlas_byte_budget_checked(
        &self,
        budget: WindowAtlasBudgetBuilder,
    ) -> Result<WindowAtlasBudget> {
        let budget = budget.build_checked()?;
        self.set_atlas_byte_budget(budget.max_bytes());
        Ok(budget)
    }

    /// Toggle full screen status on the current window at the platform level.
    pub fn toggle_fullscreen(&self) {
        self.platform_window.toggle_fullscreen();
    }

    /// Return the current checked presentation/kiosk policy, if any.
    pub fn presentation_policy(&self) -> Option<&WindowPresentationPolicy> {
        self.presentation_policy.as_ref()
    }

    /// Validate and apply fullscreen/kiosk presentation intent.
    ///
    /// This toggles platform fullscreen to match the requested mode and records
    /// the checked policy so platform backends can enforce stronger kiosk
    /// behavior where supported.
    pub fn set_presentation_policy_checked(
        &mut self,
        policy: WindowPresentationPolicyBuilder,
    ) -> Result<WindowPresentationPolicy> {
        let policy = policy.build_checked()?;
        let wants_fullscreen = policy.mode().wants_fullscreen();
        if self.platform_window.is_fullscreen() != wants_fullscreen {
            self.platform_window.toggle_fullscreen();
        }
        if policy.mode() == WindowPresentationMode::Windowed {
            self.presentation_policy = None;
        } else {
            self.presentation_policy = Some(policy.clone());
        }
        Ok(policy)
    }

    /// Restore normal windowed presentation behavior.
    pub fn clear_presentation_policy_checked(&mut self) -> Result<WindowPresentationPolicy> {
        self.set_presentation_policy_checked(WindowPresentationPolicyBuilder::windowed())
    }

    /// Set the progress bar state for this window's taskbar/dock representation.
    pub fn set_progress_bar(&self, state: ProgressBarState) {
        self.platform_window.set_progress_bar(state);
    }

    /// Validate and set the progress bar state for this window's taskbar/dock representation.
    pub fn set_progress_bar_checked(
        &self,
        progress: impl Into<WindowProgressBuilder>,
    ) -> Result<()> {
        let state = progress.into().build_checked()?;
        self.set_progress_bar(state);
        Ok(())
    }

    /// Capture the current window state for save/restore.
    pub fn window_state(&self) -> WindowState {
        let bounds = self.platform_window.window_bounds();
        let display_id = self.platform_window.display().map(|d| d.id());
        let fullscreen = self.platform_window.is_fullscreen();
        WindowState {
            bounds,
            display_id,
            fullscreen,
        }
    }

    /// Restore a previously captured window state.
    ///
    /// This restores the fullscreen state. Window bounds are set at creation
    /// time via `WindowOptions`/`WindowParams`, so this primarily handles
    /// toggling fullscreen to match the saved state.
    pub fn restore_window_state(&self, state: &WindowState) {
        let is_fullscreen = self.platform_window.is_fullscreen();
        if state.fullscreen != is_fullscreen {
            self.platform_window.toggle_fullscreen();
        }
    }

    /// Updates the IME panel position suggestions for languages like japanese, chinese.
    pub fn invalidate_character_coordinates(&self) {
        self.on_next_frame(|window, cx| {
            if let Some(mut input_handler) = window.platform_window.take_input_handler() {
                if let Some(bounds) = input_handler.selected_bounds(window, cx) {
                    window.platform_window.update_ime_position(bounds);
                }
                window.platform_window.set_input_handler(input_handler);
            }
        });
    }

    /// Present a platform dialog.
    /// The provided message will be presented, along with buttons for each answer.
    /// When a button is clicked, the returned Receiver will receive the index of the clicked button.
    pub fn prompt<T>(
        &mut self,
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        answers: &[T],
        cx: &mut App,
    ) -> oneshot::Receiver<usize>
    where
        T: Clone + Into<PromptButton>,
    {
        let prompt_builder = cx.prompt_builder.take();
        let Some(prompt_builder) = prompt_builder else {
            unreachable!("Re-entrant window prompting is not supported by GPUI");
        };

        let answers = answers
            .iter()
            .map(|answer| answer.clone().into())
            .collect::<Vec<_>>();

        let receiver = match &prompt_builder {
            PromptBuilder::Default => self
                .platform_window
                .prompt(level, message, detail, &answers)
                .unwrap_or_else(|| {
                    self.build_custom_prompt(&prompt_builder, level, message, detail, &answers, cx)
                }),
            PromptBuilder::Custom(_) => {
                self.build_custom_prompt(&prompt_builder, level, message, detail, &answers, cx)
            }
        };

        cx.prompt_builder = Some(prompt_builder);

        receiver
    }

    fn build_custom_prompt(
        &mut self,
        prompt_builder: &PromptBuilder,
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        answers: &[PromptButton],
        cx: &mut App,
    ) -> oneshot::Receiver<usize> {
        let (sender, receiver) = oneshot::channel();
        let handle = PromptHandle::new(sender);
        let handle = (prompt_builder)(level, message, detail, answers, handle, self, cx);
        self.prompt = Some(handle);
        receiver
    }

    /// Returns the current context stack.
    pub fn context_stack(&self) -> Vec<KeyContext> {
        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        dispatch_tree
            .dispatch_path(node_id)
            .iter()
            .filter_map(move |&node_id| dispatch_tree.node(node_id).context.clone())
            .collect()
    }

    /// Returns all available actions for the focused element.
    pub fn available_actions(&self, cx: &App) -> Vec<Box<dyn Action>> {
        let node_id = self.focus_node_id_in_rendered_frame(self.focus);
        let mut actions = self.rendered_frame.dispatch_tree.available_actions(node_id);
        for action_type in cx.global_action_listeners.keys() {
            if let Err(ix) = actions.binary_search_by_key(action_type, |a| a.as_any().type_id()) {
                let action = cx.actions.build_action_type(action_type).ok();
                if let Some(action) = action {
                    actions.insert(ix, action);
                }
            }
        }
        actions
    }

    /// Returns key bindings that invoke an action on the currently focused element. Bindings are
    /// returned in the order they were added. For display, the last binding should take precedence.
    pub fn bindings_for_action(&self, action: &dyn Action) -> Vec<KeyBinding> {
        self.rendered_frame
            .dispatch_tree
            .bindings_for_action(action, &self.rendered_frame.dispatch_tree.context_stack)
    }

    /// Returns the highest precedence key binding that invokes an action on the currently focused
    /// element. This is more efficient than getting the last result of `bindings_for_action`.
    pub fn highest_precedence_binding_for_action(&self, action: &dyn Action) -> Option<KeyBinding> {
        self.rendered_frame
            .dispatch_tree
            .highest_precedence_binding_for_action(
                action,
                &self.rendered_frame.dispatch_tree.context_stack,
            )
    }

    /// Returns the key bindings for an action in a context.
    pub fn bindings_for_action_in_context(
        &self,
        action: &dyn Action,
        context: KeyContext,
    ) -> Vec<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        dispatch_tree.bindings_for_action(action, &[context])
    }

    /// Returns the highest precedence key binding for an action in a context. This is more
    /// efficient than getting the last result of `bindings_for_action_in_context`.
    pub fn highest_precedence_binding_for_action_in_context(
        &self,
        action: &dyn Action,
        context: KeyContext,
    ) -> Option<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        dispatch_tree.highest_precedence_binding_for_action(action, &[context])
    }

    /// Returns any bindings that would invoke an action on the given focus handle if it were
    /// focused. Bindings are returned in the order they were added. For display, the last binding
    /// should take precedence.
    pub fn bindings_for_action_in(
        &self,
        action: &dyn Action,
        focus_handle: &FocusHandle,
    ) -> Vec<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        let Some(context_stack) = self.context_stack_for_focus_handle(focus_handle) else {
            return vec![];
        };
        dispatch_tree.bindings_for_action(action, &context_stack)
    }

    /// Returns the highest precedence key binding that would invoke an action on the given focus
    /// handle if it were focused. This is more efficient than getting the last result of
    /// `bindings_for_action_in`.
    pub fn highest_precedence_binding_for_action_in(
        &self,
        action: &dyn Action,
        focus_handle: &FocusHandle,
    ) -> Option<KeyBinding> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        let context_stack = self.context_stack_for_focus_handle(focus_handle)?;
        dispatch_tree.highest_precedence_binding_for_action(action, &context_stack)
    }

    fn context_stack_for_focus_handle(
        &self,
        focus_handle: &FocusHandle,
    ) -> Option<Vec<KeyContext>> {
        let dispatch_tree = &self.rendered_frame.dispatch_tree;
        let node_id = dispatch_tree.focusable_node_id(focus_handle.id)?;
        let context_stack: Vec<_> = dispatch_tree
            .dispatch_path(node_id)
            .into_iter()
            .filter_map(|node_id| dispatch_tree.node(node_id).context.clone())
            .collect();
        Some(context_stack)
    }

    /// Returns a generic event listener that invokes the given listener with the view and context associated with the given view handle.
    pub fn listener_for<V: Render, E>(
        &self,
        view: &Entity<V>,
        f: impl Fn(&mut V, &E, &mut Window, &mut Context<V>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let view = view.downgrade();
        move |e: &E, window: &mut Window, cx: &mut App| {
            view.update(cx, |view, cx| f(view, e, window, cx)).ok();
        }
    }

    /// Returns a generic handler that invokes the given handler with the view and context associated with the given view handle.
    pub fn handler_for<V: Render, Callback: Fn(&mut V, &mut Window, &mut Context<V>) + 'static>(
        &self,
        view: &Entity<V>,
        f: Callback,
    ) -> impl Fn(&mut Window, &mut App) + use<V, Callback> {
        let view = view.downgrade();
        move |window: &mut Window, cx: &mut App| {
            view.update(cx, |view, cx| f(view, window, cx)).ok();
        }
    }

    /// Register a callback that can interrupt the closing of the current window based the returned boolean.
    /// If the callback returns false, the window won't be closed.
    pub fn on_window_should_close(
        &self,
        cx: &App,
        f: impl Fn(&mut Window, &mut App) -> bool + 'static,
    ) {
        let mut cx = self.to_async(cx);
        self.platform_window.on_should_close(Box::new(move || {
            cx.update(|window, cx| f(window, cx)).unwrap_or(true)
        }))
    }

    /// Register an action listener on the window for the next frame. The type of action
    /// is determined by the first parameter of the given listener. When the next frame is rendered
    /// the listener will be cleared.
    ///
    /// This is a fairly low-level method, so prefer using action handlers on elements unless you have
    /// a specific need to register a global listener.
    pub fn on_action(
        &mut self,
        action_type: TypeId,
        listener: impl Fn(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        self.next_frame
            .dispatch_tree
            .on_action(action_type, Rc::new(listener));
    }

    /// Register an action listener on the window for the next frame if the condition is true.
    /// The type of action is determined by the first parameter of the given listener.
    /// When the next frame is rendered the listener will be cleared.
    ///
    /// This is a fairly low-level method, so prefer using action handlers on elements unless you have
    /// a specific need to register a global listener.
    pub fn on_action_when(
        &mut self,
        condition: bool,
        action_type: TypeId,
        listener: impl Fn(&dyn Any, DispatchPhase, &mut Window, &mut App) + 'static,
    ) {
        if condition {
            self.next_frame
                .dispatch_tree
                .on_action(action_type, Rc::new(listener));
        }
    }

    /// Read information about the GPU backing this window.
    /// Currently returns None on Mac and Windows.
    pub fn gpu_specs(&self) -> Option<GpuSpecs> {
        self.platform_window.gpu_specs()
    }

    /// Perform titlebar double-click action.
    /// This is macOS specific.
    pub fn titlebar_double_click(&self) {
        self.platform_window.titlebar_double_click();
    }

    /// Gets the window's title at the platform level.
    /// This is macOS specific.
    pub fn window_title(&self) -> String {
        self.platform_window.get_title()
    }

    /// Returns a list of all tabbed windows and their titles.
    /// This is macOS specific.
    pub fn tabbed_windows(&self) -> Option<Vec<SystemWindowTab>> {
        self.platform_window.tabbed_windows()
    }

    /// Returns the tab bar visibility.
    /// This is macOS specific.
    pub fn tab_bar_visible(&self) -> bool {
        self.platform_window.tab_bar_visible()
    }

    /// Merges all open windows into a single tabbed window.
    /// This is macOS specific.
    pub fn merge_all_windows(&self) {
        self.platform_window.merge_all_windows()
    }

    /// Moves the tab to a new containing window.
    /// This is macOS specific.
    pub fn move_tab_to_new_window(&self) {
        self.platform_window.move_tab_to_new_window()
    }

    /// Shows or hides the window tab overview.
    /// This is macOS specific.
    pub fn toggle_window_tab_overview(&self) {
        self.platform_window.toggle_window_tab_overview()
    }

    /// Validate and perform a native window-tab command.
    /// This is macOS specific.
    pub fn perform_window_tab_command_checked(
        &self,
        command: WindowTabCommand,
    ) -> Result<WindowTabCommand> {
        command.validate()?;
        match command.kind() {
            WindowTabCommandKind::MergeAllWindows => self.merge_all_windows(),
            WindowTabCommandKind::MoveTabToNewWindow => self.move_tab_to_new_window(),
            WindowTabCommandKind::ToggleTabOverview => self.toggle_window_tab_overview(),
        }
        Ok(command)
    }

    /// Sets the tabbing identifier for the window.
    /// This is macOS specific.
    pub fn set_tabbing_identifier(&self, tabbing_identifier: Option<String>) {
        self.platform_window
            .set_tabbing_identifier(tabbing_identifier)
    }

    /// Validate and set the tabbing identifier for the window.
    /// This is macOS specific.
    pub fn set_tabbing_identifier_checked(
        &self,
        tabbing_identifier: WindowTabbingIdentifierBuilder,
    ) -> Result<()> {
        self.set_tabbing_identifier(tabbing_identifier.build_checked()?);
        Ok(())
    }

    /// Toggles the inspector mode on this window.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn toggle_inspector(&mut self, cx: &mut App) {
        self.inspector = match self.inspector {
            None => Some(cx.new(|_| Inspector::new())),
            Some(_) => None,
        };
        self.refresh();
    }

    /// Returns true if the window is in inspector mode.
    pub fn is_inspector_picking(&self, _cx: &App) -> bool {
        #[cfg(any(feature = "inspector", debug_assertions))]
        {
            if let Some(inspector) = &self.inspector {
                return inspector.read(_cx).is_picking();
            }
        }
        false
    }

    /// Executes the provided function with mutable access to an inspector state.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn with_inspector_state<T: 'static, R>(
        &mut self,
        _inspector_id: Option<&crate::InspectorElementId>,
        cx: &mut App,
        f: impl FnOnce(&mut Option<T>, &mut Self) -> R,
    ) -> R {
        if let Some(inspector_id) = _inspector_id
            && let Some(inspector) = &self.inspector
        {
            let inspector = inspector.clone();
            let active_element_id = inspector.read(cx).active_element_id();
            if Some(inspector_id) == active_element_id {
                return inspector.update(cx, |inspector, _cx| {
                    inspector.with_active_element_state(self, f)
                });
            }
        }
        f(&mut None, self)
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    pub(crate) fn build_inspector_element_id(
        &mut self,
        path: crate::InspectorElementPath,
    ) -> crate::InspectorElementId {
        self.invalidator.debug_assert_paint_or_prepaint();
        let path = Rc::new(path);
        let next_instance_id = self
            .next_frame
            .next_inspector_instance_ids
            .entry(path.clone())
            .or_insert(0);
        let instance_id = *next_instance_id;
        *next_instance_id += 1;
        crate::InspectorElementId { path, instance_id }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn prepaint_inspector(&mut self, inspector_width: Pixels, cx: &mut App) -> Option<AnyElement> {
        if let Some(inspector) = self.inspector.take() {
            let mut inspector_element = AnyView::from(inspector.clone()).into_any_element();
            inspector_element.prepaint_as_root(
                point(self.viewport_size.width - inspector_width, px(0.0)),
                size(inspector_width, self.viewport_size.height).into(),
                self,
                cx,
            );
            self.inspector = Some(inspector);
            Some(inspector_element)
        } else {
            None
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn paint_inspector(&mut self, mut inspector_element: Option<AnyElement>, cx: &mut App) {
        if let Some(mut inspector_element) = inspector_element {
            inspector_element.paint(self, cx);
        };
    }

    /// Registers a hitbox that can be used for inspector picking mode, allowing users to select and
    /// inspect UI elements by clicking on them.
    #[cfg(any(feature = "inspector", debug_assertions))]
    pub fn insert_inspector_hitbox(
        &mut self,
        hitbox_id: HitboxId,
        inspector_id: Option<&crate::InspectorElementId>,
        cx: &App,
    ) {
        self.invalidator.debug_assert_paint_or_prepaint();
        if !self.is_inspector_picking(cx) {
            return;
        }
        if let Some(inspector_id) = inspector_id {
            self.next_frame
                .inspector_hitboxes
                .insert(hitbox_id, inspector_id.clone());
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn paint_inspector_hitbox(&mut self, cx: &App) {
        if let Some(inspector) = self.inspector.as_ref() {
            let inspector = inspector.read(cx);
            if let Some((hitbox_id, _)) = self.hovered_inspector_hitbox(inspector, &self.next_frame)
                && let Some(hitbox) = self
                    .next_frame
                    .hitboxes
                    .iter()
                    .find(|hitbox| hitbox.id == hitbox_id)
            {
                self.paint_quad(crate::fill(hitbox.bounds, crate::rgba(0x61afef4d)));
            }
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn handle_inspector_mouse_event(&mut self, event: &dyn Any, cx: &mut App) {
        let Some(inspector) = self.inspector.clone() else {
            return;
        };
        if event.downcast_ref::<MouseMoveEvent>().is_some() {
            inspector.update(cx, |inspector, _cx| {
                if let Some((_, inspector_id)) =
                    self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                {
                    inspector.hover(inspector_id, self);
                }
            });
        } else if event.downcast_ref::<crate::MouseDownEvent>().is_some() {
            inspector.update(cx, |inspector, _cx| {
                if let Some((_, inspector_id)) =
                    self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                {
                    inspector.select(inspector_id, self);
                }
            });
        } else if let Some(event) = event.downcast_ref::<crate::ScrollWheelEvent>() {
            // This should be kept in sync with SCROLL_LINES in x11 platform.
            const SCROLL_LINES: f32 = 3.0;
            const SCROLL_PIXELS_PER_LAYER: f32 = 36.0;
            let delta_y = event
                .delta
                .pixel_delta(px(SCROLL_PIXELS_PER_LAYER / SCROLL_LINES))
                .y;
            if let Some(inspector) = self.inspector.clone() {
                inspector.update(cx, |inspector, _cx| {
                    if let Some(depth) = inspector.pick_depth.as_mut() {
                        *depth += f32::from(delta_y) / SCROLL_PIXELS_PER_LAYER;
                        let max_depth = self.mouse_hit_test.ids.len() as f32 - 0.5;
                        if *depth < 0.0 {
                            *depth = 0.0;
                        } else if *depth > max_depth {
                            *depth = max_depth;
                        }
                        if let Some((_, inspector_id)) =
                            self.hovered_inspector_hitbox(inspector, &self.rendered_frame)
                        {
                            inspector.set_active_element_id(inspector_id, self);
                        }
                    }
                });
            }
        }
    }

    #[cfg(any(feature = "inspector", debug_assertions))]
    fn hovered_inspector_hitbox(
        &self,
        inspector: &Inspector,
        frame: &Frame,
    ) -> Option<(HitboxId, crate::InspectorElementId)> {
        if let Some(pick_depth) = inspector.pick_depth {
            let depth = (pick_depth as i64).try_into().unwrap_or(0);
            let max_skipped = self.mouse_hit_test.ids.len().saturating_sub(1);
            let skip_count = (depth as usize).min(max_skipped);
            for hitbox_id in self.mouse_hit_test.ids.iter().skip(skip_count) {
                if let Some(inspector_id) = frame.inspector_hitboxes.get(hitbox_id) {
                    return Some((*hitbox_id, inspector_id.clone()));
                }
            }
        }
        None
    }

    /// For testing: set the current modifier keys state.
    /// This does not generate any events.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }
}

// #[derive(Clone, Copy, Eq, PartialEq, Hash)]
slotmap::new_key_type! {
    /// A unique identifier for a window.
    pub struct WindowId;
}

impl WindowId {
    /// Converts this window ID to a `u64`.
    pub fn as_u64(&self) -> u64 {
        self.0.as_ffi()
    }
}

impl From<u64> for WindowId {
    fn from(value: u64) -> Self {
        WindowId(slotmap::KeyData::from_ffi(value))
    }
}

/// A handle to a window with a specific root view type.
/// Note that this does not keep the window alive on its own.
#[derive(Deref, DerefMut)]
pub struct WindowHandle<V> {
    #[deref]
    #[deref_mut]
    pub(crate) any_handle: AnyWindowHandle,
    state_type: PhantomData<fn(V) -> V>,
}

impl<V> Debug for WindowHandle<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowHandle")
            .field("any_handle", &self.any_handle.id.as_u64())
            .finish()
    }
}

impl<V: 'static + Render> WindowHandle<V> {
    /// Creates a new handle from a window ID.
    /// This does not check if the root type of the window is `V`.
    pub fn new(id: WindowId) -> Self {
        WindowHandle {
            any_handle: AnyWindowHandle {
                id,
                state_type: TypeId::of::<V>(),
            },
            state_type: PhantomData,
        }
    }

    /// Get the root view out of this window.
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn root<C>(&self, cx: &mut C) -> Result<Entity<V>>
    where
        C: AppContext,
    {
        crate::Flatten::flatten(cx.update_window(self.any_handle, |root_view, _, _| {
            root_view
                .downcast::<V>()
                .map_err(|_| anyhow!("the type of the window's root view has changed"))
        }))
    }

    /// Updates the root view of this window.
    ///
    /// This will fail if the window has been closed or if the root view's type does not match
    pub fn update<C, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Result<R>
    where
        C: AppContext,
    {
        cx.update_window(self.any_handle, |root_view, window, cx| {
            let view = root_view
                .downcast::<V>()
                .map_err(|_| anyhow!("the type of the window's root view has changed"))?;

            Ok(view.update(cx, |view, cx| update(view, window, cx)))
        })?
    }

    /// Read the root view out of this window.
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    pub fn read<'a>(&self, cx: &'a App) -> Result<&'a V> {
        let x = cx
            .windows
            .get(self.id)
            .and_then(|window| {
                window
                    .as_ref()
                    .and_then(|window| window.root.clone())
                    .map(|root_view| root_view.downcast::<V>())
            })
            .context("window not found")?
            .map_err(|_| anyhow!("the type of the window's root view has changed"))?;

        Ok(x.read(cx))
    }

    /// Read the root view out of this window, with a callback
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    pub fn read_with<C, R>(&self, cx: &C, read_with: impl FnOnce(&V, &App) -> R) -> Result<R>
    where
        C: AppContext,
    {
        cx.read_window(self, |root_view, cx| read_with(root_view.read(cx), cx))
    }

    /// Read the root view pointer off of this window.
    ///
    /// This will fail if the window is closed or if the root view's type does not match `V`.
    pub fn entity<C>(&self, cx: &C) -> Result<Entity<V>>
    where
        C: AppContext,
    {
        cx.read_window(self, |root_view, _cx| root_view)
    }

    /// Check if this window is 'active'.
    ///
    /// Will return `None` if the window is closed or currently
    /// borrowed.
    pub fn is_active(&self, cx: &mut App) -> Option<bool> {
        cx.update_window(self.any_handle, |_, window, _| window.is_window_active())
            .ok()
    }
}

impl<V> Copy for WindowHandle<V> {}

impl<V> Clone for WindowHandle<V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<V> PartialEq for WindowHandle<V> {
    fn eq(&self, other: &Self) -> bool {
        self.any_handle == other.any_handle
    }
}

impl<V> Eq for WindowHandle<V> {}

impl<V> Hash for WindowHandle<V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.any_handle.hash(state);
    }
}

impl<V: 'static> From<WindowHandle<V>> for AnyWindowHandle {
    fn from(val: WindowHandle<V>) -> Self {
        val.any_handle
    }
}

/// A handle to a window with any root view type, which can be downcast to a window with a specific root view type.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct AnyWindowHandle {
    pub(crate) id: WindowId,
    state_type: TypeId,
}

impl AnyWindowHandle {
    /// Get the ID of this window.
    pub fn window_id(&self) -> WindowId {
        self.id
    }

    /// Attempt to convert this handle to a window handle with a specific root view type.
    /// If the types do not match, this will return `None`.
    pub fn downcast<T: 'static>(&self) -> Option<WindowHandle<T>> {
        if TypeId::of::<T>() == self.state_type {
            Some(WindowHandle {
                any_handle: *self,
                state_type: PhantomData,
            })
        } else {
            None
        }
    }

    /// Updates the state of the root view of this window.
    ///
    /// This will fail if the window has been closed.
    pub fn update<C, R>(
        self,
        cx: &mut C,
        update: impl FnOnce(AnyView, &mut Window, &mut App) -> R,
    ) -> Result<R>
    where
        C: AppContext,
    {
        cx.update_window(self, update)
    }

    /// Read the state of the root view of this window.
    ///
    /// This will fail if the window has been closed.
    pub fn read<T, C, R>(self, cx: &C, read: impl FnOnce(Entity<T>, &App) -> R) -> Result<R>
    where
        C: AppContext,
        T: 'static,
    {
        let view = self
            .downcast::<T>()
            .context("the type of the window's root view has changed")?;

        cx.read_window(&view, read)
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<raw_window_handle::WindowHandle<'_>, HandleError> {
        self.platform_window.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(
        &self,
    ) -> std::result::Result<raw_window_handle::DisplayHandle<'_>, HandleError> {
        self.platform_window.display_handle()
    }
}

/// An identifier for an [`Element`].
///
/// Can be constructed with a string, a number, or both, as well
/// as other internal representations.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum ElementId {
    /// The ID of a View element
    View(EntityId),
    /// An integer ID.
    Integer(u64),
    /// A string based ID.
    Name(SharedString),
    /// A UUID.
    Uuid(Uuid),
    /// An ID that's equated with a focus handle.
    FocusHandle(FocusId),
    /// A combination of a name and an integer.
    NamedInteger(SharedString, u64),
    /// A path.
    Path(Arc<std::path::Path>),
    /// A code location.
    CodeLocation(core::panic::Location<'static>),
    /// A labeled child of an element.
    NamedChild(Box<ElementId>, SharedString),
}

impl ElementId {
    /// Constructs an `ElementId::NamedInteger` from a name and `usize`.
    pub fn named_usize(name: impl Into<SharedString>, integer: usize) -> ElementId {
        Self::NamedInteger(name.into(), integer as u64)
    }
}

impl Display for ElementId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ElementId::View(entity_id) => write!(f, "view-{}", entity_id)?,
            ElementId::Integer(ix) => write!(f, "{}", ix)?,
            ElementId::Name(name) => write!(f, "{}", name)?,
            ElementId::FocusHandle(_) => write!(f, "FocusHandle")?,
            ElementId::NamedInteger(s, i) => write!(f, "{}-{}", s, i)?,
            ElementId::Uuid(uuid) => write!(f, "{}", uuid)?,
            ElementId::Path(path) => write!(f, "{}", path.display())?,
            ElementId::CodeLocation(location) => write!(f, "{}", location)?,
            ElementId::NamedChild(id, name) => write!(f, "{}-{}", id, name)?,
        }

        Ok(())
    }
}

impl TryInto<SharedString> for ElementId {
    type Error = anyhow::Error;

    fn try_into(self) -> anyhow::Result<SharedString> {
        if let ElementId::Name(name) = self {
            Ok(name)
        } else {
            anyhow::bail!("element id is not string")
        }
    }
}

impl From<usize> for ElementId {
    fn from(id: usize) -> Self {
        ElementId::Integer(id as u64)
    }
}

impl From<i32> for ElementId {
    fn from(id: i32) -> Self {
        Self::Integer(id as u64)
    }
}

impl From<SharedString> for ElementId {
    fn from(name: SharedString) -> Self {
        ElementId::Name(name)
    }
}

impl From<Arc<std::path::Path>> for ElementId {
    fn from(path: Arc<std::path::Path>) -> Self {
        ElementId::Path(path)
    }
}

impl From<&'static str> for ElementId {
    fn from(name: &'static str) -> Self {
        ElementId::Name(name.into())
    }
}

impl<'a> From<&'a FocusHandle> for ElementId {
    fn from(handle: &'a FocusHandle) -> Self {
        ElementId::FocusHandle(handle.id)
    }
}

impl From<(&'static str, EntityId)> for ElementId {
    fn from((name, id): (&'static str, EntityId)) -> Self {
        ElementId::NamedInteger(name.into(), id.as_u64())
    }
}

impl From<(&'static str, usize)> for ElementId {
    fn from((name, id): (&'static str, usize)) -> Self {
        ElementId::NamedInteger(name.into(), id as u64)
    }
}

impl From<(SharedString, usize)> for ElementId {
    fn from((name, id): (SharedString, usize)) -> Self {
        ElementId::NamedInteger(name, id as u64)
    }
}

impl From<(&'static str, u64)> for ElementId {
    fn from((name, id): (&'static str, u64)) -> Self {
        ElementId::NamedInteger(name.into(), id)
    }
}

impl From<Uuid> for ElementId {
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl From<(&'static str, u32)> for ElementId {
    fn from((name, id): (&'static str, u32)) -> Self {
        ElementId::NamedInteger(name.into(), id.into())
    }
}

impl<T: Into<SharedString>> From<(ElementId, T)> for ElementId {
    fn from((id, name): (ElementId, T)) -> Self {
        ElementId::NamedChild(Box::new(id), name.into())
    }
}

impl From<&'static core::panic::Location<'static>> for ElementId {
    fn from(location: &'static core::panic::Location<'static>) -> Self {
        ElementId::CodeLocation(*location)
    }
}

fn platform_adjusted_corner_radii(
    corner_radii: Corners<Pixels>,
    size: Size<Pixels>,
    continuous_corners: bool,
) -> Corners<Pixels> {
    #[cfg(target_os = "macos")]
    {
        if !continuous_corners {
            return Corners {
                top_left: corner_radii.top_left * 1.5,
                top_right: corner_radii.top_right * 1.5,
                bottom_right: corner_radii.bottom_right * 1.5,
                bottom_left: corner_radii.bottom_left * 1.5,
            }
            .clamp_radii_for_quad_size(size);
        }
    }

    let _ = size;
    let _ = continuous_corners;
    corner_radii
}

/// A rectangle to be rendered in the window at the given position and size.
/// Passed as an argument [`Window::paint_quad`].
#[derive(Clone)]
pub struct PaintQuad {
    /// The bounds of the quad within the window.
    pub bounds: Bounds<Pixels>,
    /// The radii of the quad's corners.
    pub corner_radii: Corners<Pixels>,
    /// The background color of the quad.
    pub background: Background,
    /// The widths of the quad's borders.
    pub border_widths: Edges<Pixels>,
    /// The color of the quad's borders. Accepts a solid color or a gradient.
    pub border_color: Background,
    /// The style of the quad's borders.
    pub border_style: BorderStyle,
    /// Whether to use continuous (squircle) corner rounding.
    pub continuous_corners: bool,
    /// The 2D affine transform applied to this quad.
    pub transform: TransformationMatrix,
    /// The blend mode to apply when rendering this quad.
    pub blend_mode: BlendMode,
}

impl PaintQuad {
    /// Sets the corner radii of the quad.
    pub fn corner_radii(self, corner_radii: impl Into<Corners<Pixels>>) -> Self {
        PaintQuad {
            corner_radii: corner_radii.into(),
            ..self
        }
    }

    /// Sets the border widths of the quad.
    pub fn border_widths(self, border_widths: impl Into<Edges<Pixels>>) -> Self {
        PaintQuad {
            border_widths: border_widths.into(),
            ..self
        }
    }

    /// Sets the border color of the quad. Accepts a solid color or a gradient.
    pub fn border_color(self, border_color: impl Into<Background>) -> Self {
        PaintQuad {
            border_color: border_color.into(),
            ..self
        }
    }

    /// Sets the background color of the quad.
    pub fn background(self, background: impl Into<Background>) -> Self {
        PaintQuad {
            background: background.into(),
            ..self
        }
    }
}

/// Creates a quad with the given parameters.
pub fn quad(
    bounds: Bounds<Pixels>,
    corner_radii: impl Into<Corners<Pixels>>,
    background: impl Into<Background>,
    border_widths: impl Into<Edges<Pixels>>,
    border_color: impl Into<Background>,
    border_style: BorderStyle,
) -> PaintQuad {
    PaintQuad {
        bounds,
        corner_radii: corner_radii.into(),
        background: background.into(),
        border_widths: border_widths.into(),
        border_color: border_color.into(),
        border_style,
        continuous_corners: true,
        transform: TransformationMatrix::unit(),
        blend_mode: BlendMode::Normal,
    }
}

/// Creates a filled quad with the given bounds and background color.
pub fn fill(bounds: impl Into<Bounds<Pixels>>, background: impl Into<Background>) -> PaintQuad {
    PaintQuad {
        bounds: bounds.into(),
        corner_radii: (0.).into(),
        background: background.into(),
        border_widths: (0.).into(),
        border_color: transparent_black().into(),
        border_style: BorderStyle::default(),
        continuous_corners: true,
        transform: TransformationMatrix::unit(),
        blend_mode: BlendMode::Normal,
    }
}

/// Creates a rectangle outline with the given bounds, border color, and a 1px border width
pub fn outline(
    bounds: impl Into<Bounds<Pixels>>,
    border_color: impl Into<Background>,
    border_style: BorderStyle,
) -> PaintQuad {
    PaintQuad {
        bounds: bounds.into(),
        corner_radii: (0.).into(),
        background: transparent_black().into(),
        border_widths: (1.).into(),
        border_color: border_color.into(),
        border_style,
        continuous_corners: true,
        transform: TransformationMatrix::unit(),
        blend_mode: BlendMode::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccessibilityAnnouncementBuilder, AccessibilityFocusBuilder, WindowAppIdBuilder,
        WindowAtlasBudgetBuilder, WindowAutoscrollRequestBuilder, WindowChromeCommand,
        WindowChromeCommandKind, WindowClientInsetBuilder, WindowContentProtectionBuilder,
        WindowContentProtectionMode, WindowContentSizeBuilder, WindowCursorStyleCommand,
        WindowDocumentStateBuilder, WindowInteractionCommand, WindowInteractionCommandKind,
        WindowOpacityBuilder, WindowPresentationMode, WindowPresentationPolicyBuilder,
        WindowProgressBuilder, WindowRemSizeBuilder, WindowRenderPolicyBuilder,
        WindowRuntimeSnapshot, WindowRuntimeSnapshotQueryBuilder, WindowSystemUiCommand,
        WindowSystemUiCommandKind, WindowTabCommand, WindowTabCommandKind,
        WindowTabbingIdentifierBuilder, WindowTitleBuilder, WindowZOrderPolicyBuilder,
    };
    use crate::{
        AccessibilityId, AccessibilityNode, AccessibilityRole, AccessibilityState,
        AccessibilityTree, Bounds, CursorStyle, DisplayId, PowerMode, ProgressBarState, ResizeEdge,
        WindowAppearance, WindowBounds, WindowDecorations, point, px, size,
    };

    #[test]
    fn window_title_builder_validates_platform_chrome_text() {
        let title = WindowTitleBuilder::new("Project - Report.md");
        assert!(title.validate().is_ok());
        assert_eq!(title.title(), "Project - Report.md");
        assert_eq!(title.title_len_chars(), 19);
        assert!(!title.is_blank());
        assert_eq!(title.to_text(), "window title: 19 chars, blank false");
        assert!(!title.to_text().contains("Project"));
        assert_eq!(
            title.build_checked().unwrap(),
            "Project - Report.md".to_string()
        );
        let blank = WindowTitleBuilder::new(" ");
        assert!(blank.is_blank());
        assert_eq!(blank.to_text(), "window title: 1 chars, blank true");

        assert!(WindowTitleBuilder::new("").validate().is_err());
        assert!(WindowTitleBuilder::new(" ").validate().is_err());
        assert!(WindowTitleBuilder::new(" Project").validate().is_err());
        assert!(WindowTitleBuilder::new("Project ").validate().is_err());
        assert!(
            WindowTitleBuilder::new("Project\nDraft")
                .validate()
                .is_err()
        );
        assert!(WindowTitleBuilder::new("x".repeat(513)).validate().is_err());
    }

    #[test]
    fn window_app_id_builder_validates_platform_grouping_id() {
        let app_id = WindowAppIdBuilder::new("com.example.app");
        assert!(app_id.validate().is_ok());
        assert_eq!(app_id.app_id(), "com.example.app");
        assert_eq!(app_id.len_bytes(), 15);
        assert!(!app_id.is_blank());
        assert_eq!(app_id.to_text(), "window app id: 15 bytes, blank false");
        assert!(!app_id.to_text().contains("com.example.app"));
        assert_eq!(app_id.build_checked().unwrap(), "com.example.app");

        let blank = WindowAppIdBuilder::new(" ");
        assert!(blank.is_blank());
        assert_eq!(blank.to_text(), "window app id: 1 bytes, blank true");
        assert!(WindowAppIdBuilder::new("").validate().is_err());
        assert!(WindowAppIdBuilder::new(" ").validate().is_err());
        assert!(
            WindowAppIdBuilder::new(" com.example.app")
                .validate()
                .is_err()
        );
        assert!(
            WindowAppIdBuilder::new("com.example.app ")
                .validate()
                .is_err()
        );
        assert!(
            WindowAppIdBuilder::new("com.example app")
                .validate()
                .is_err()
        );
        assert!(
            WindowAppIdBuilder::new("com.example\napp")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn accessibility_announcement_builder_validates_live_region_text() {
        let announcement = AccessibilityAnnouncementBuilder::new("Upload complete");
        assert!(announcement.validate().is_ok());
        assert_eq!(announcement.message(), "Upload complete");
        assert_eq!(
            announcement.build_checked().unwrap(),
            "Upload complete".to_string()
        );

        assert!(
            AccessibilityAnnouncementBuilder::new("")
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAnnouncementBuilder::new(" Upload complete")
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAnnouncementBuilder::new("Upload\ncomplete")
                .validate()
                .is_err()
        );
        assert!(
            AccessibilityAnnouncementBuilder::new("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn accessibility_focus_builder_validates_current_tree_target() {
        let root = AccessibilityNode::new(AccessibilityRole::Window);
        let mut tree = AccessibilityTree::new(root);
        let button = AccessibilityNode::new(AccessibilityRole::Button).with_label("Save");
        let button_id = button.id;
        tree.insert(button);
        let hidden = AccessibilityNode::new(AccessibilityRole::Button)
            .with_label("Hidden")
            .with_states(AccessibilityState::HIDDEN);
        let hidden_id = hidden.id;
        tree.insert(hidden);

        let focus = AccessibilityFocusBuilder::new(button_id);
        assert_eq!(focus.node_id(), button_id);
        assert!(focus.validate_tree(&tree).is_ok());

        assert!(
            AccessibilityFocusBuilder::new(AccessibilityId::new())
                .validate_tree(&tree)
                .is_err()
        );
        assert!(
            AccessibilityFocusBuilder::new(hidden_id)
                .validate_tree(&tree)
                .is_err()
        );
    }

    #[test]
    fn window_content_size_builder_validates_runtime_resize() {
        let builder = WindowContentSizeBuilder::new(size(px(640.0), px(480.0)));
        assert!(builder.is_landscape());
        assert!(!builder.is_portrait());
        assert!(!builder.is_square());
        assert_eq!(
            builder.to_text(),
            "window content size builder: orientation landscape"
        );
        assert!(!builder.to_text().contains("640"));
        let request = builder.build_checked().unwrap();
        assert_eq!(request.size(), size(px(640.0), px(480.0)));
        assert!(request.is_landscape());
        assert_eq!(
            request.to_text(),
            "window content size: orientation landscape"
        );
        assert!(!request.to_text().contains("480"));

        assert!(
            WindowContentSizeBuilder::dimensions(px(0.0), px(480.0))
                .validate()
                .is_err()
        );
        assert!(
            WindowContentSizeBuilder::dimensions(px(640.0), px(-1.0))
                .validate()
                .is_err()
        );
        assert!(
            WindowContentSizeBuilder::dimensions(px(f32::NAN), px(480.0))
                .validate()
                .is_err()
        );
        assert!(
            WindowContentSizeBuilder::dimensions(px(640.0), px(f32::INFINITY))
                .validate()
                .is_err()
        );
        assert!(
            WindowContentSizeBuilder::dimensions(px(32769.0), px(480.0))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_progress_builder_validates_taskbar_progress() {
        let normal = WindowProgressBuilder::normal_percent(37);
        assert_eq!(normal.kind(), "normal");
        assert!(normal.is_determinate());
        assert!(!normal.is_clear());
        assert_eq!(
            normal.to_text(),
            "window progress: kind normal, determinate true"
        );
        assert!(!normal.to_text().contains("0.37"));
        assert_eq!(
            normal.build_checked().unwrap(),
            ProgressBarState::Normal(0.37)
        );
        assert_eq!(
            WindowProgressBuilder::paused_percent(100)
                .build_checked()
                .unwrap(),
            ProgressBarState::Paused(1.0)
        );
        assert_eq!(
            WindowProgressBuilder::error_percent(0)
                .build_checked()
                .unwrap(),
            ProgressBarState::Error(0.0)
        );
        let indeterminate = WindowProgressBuilder::indeterminate();
        assert_eq!(indeterminate.kind(), "indeterminate");
        assert!(!indeterminate.is_determinate());
        assert!(!indeterminate.is_clear());
        assert_eq!(
            indeterminate.to_text(),
            "window progress: kind indeterminate, determinate false"
        );
        assert_eq!(
            indeterminate.build_checked().unwrap(),
            ProgressBarState::Indeterminate
        );
        let clear = WindowProgressBuilder::none();
        assert_eq!(clear.kind(), "none");
        assert!(!clear.is_determinate());
        assert!(clear.is_clear());
        assert_eq!(
            clear.to_text(),
            "window progress: kind none, determinate false"
        );
        assert_eq!(clear.build_checked().unwrap(), ProgressBarState::None);
        assert_eq!(
            WindowProgressBuilder::from(ProgressBarState::Normal(0.5)).state(),
            ProgressBarState::Normal(0.5)
        );
        let error_state = ProgressBarState::Error(0.5);
        assert_eq!(error_state.kind(), "error");
        assert!(error_state.is_determinate());
        assert!(!error_state.is_clear());
        assert_eq!(
            error_state.to_text(),
            "window progress: kind error, determinate true"
        );
        assert!(!error_state.to_text().contains("0.5"));

        for value in [-0.1, 1.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(WindowProgressBuilder::normal(value).validate().is_err());
            assert!(WindowProgressBuilder::paused(value).validate().is_err());
            assert!(WindowProgressBuilder::error(value).validate().is_err());
        }
    }

    #[test]
    fn window_runtime_snapshot_query_validates_required_state() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(640.0), px(480.0)));
        let snapshot = WindowRuntimeSnapshot {
            bounds,
            window_bounds: WindowBounds::Windowed(bounds),
            viewport_size: size(px(640.0), px(480.0)),
            display_id: Some(DisplayId(7)),
            scale_factor: 2.0,
            appearance: WindowAppearance::Dark,
            active: true,
            hovered: true,
            visible: true,
            fullscreen: false,
            maximized: false,
            power_mode: PowerMode::Performance,
            reduce_motion: false,
        };

        let query = WindowRuntimeSnapshotQueryBuilder::new()
            .require_visible()
            .require_active()
            .require_display();
        assert!(query.requires_visible());
        assert!(query.requires_active());
        assert!(query.requires_display());
        assert!(query.validate_snapshot(&snapshot).is_ok());
        assert_eq!(snapshot.bounds(), bounds);
        assert_eq!(snapshot.window_bounds(), WindowBounds::Windowed(bounds));
        assert_eq!(snapshot.viewport_size(), size(px(640.0), px(480.0)));
        assert_eq!(snapshot.display_id(), Some(DisplayId(7)));
        assert_eq!(snapshot.scale_factor(), 2.0);
        assert_eq!(snapshot.appearance(), WindowAppearance::Dark);
        assert!(snapshot.is_active());
        assert!(snapshot.is_hovered());
        assert!(snapshot.is_visible());
        assert!(!snapshot.is_fullscreen());
        assert!(!snapshot.is_maximized());
        assert_eq!(snapshot.power_mode(), PowerMode::Performance);
        assert!(!snapshot.reduce_motion());
        assert!(snapshot.animations_enabled());

        let hidden = WindowRuntimeSnapshot {
            visible: false,
            ..snapshot.clone()
        };
        assert!(query.validate_snapshot(&hidden).is_err());

        let inactive = WindowRuntimeSnapshot {
            active: false,
            ..snapshot.clone()
        };
        assert!(query.validate_snapshot(&inactive).is_err());

        let displayless = WindowRuntimeSnapshot {
            display_id: None,
            ..snapshot
        };
        assert!(query.validate_snapshot(&displayless).is_err());
    }

    #[test]
    fn window_opacity_builder_validates_native_opacity_fraction() {
        let opacity_builder = WindowOpacityBuilder::fraction(0.72);
        assert_eq!(opacity_builder.value(), 0.72);
        assert!(!opacity_builder.is_opaque());
        assert!(opacity_builder.is_translucent());
        assert_eq!(
            opacity_builder.to_text(),
            "window opacity: fractional, translucent true"
        );
        assert!(!opacity_builder.to_text().contains("0.72"));
        let opacity = opacity_builder.build_checked().unwrap();
        assert_eq!(opacity.fraction(), 0.72);
        assert!(!opacity.is_opaque());
        assert!(opacity.is_translucent());
        assert_eq!(
            opacity.to_text(),
            "window opacity: fractional, translucent true"
        );

        let opaque_builder = WindowOpacityBuilder::opaque();
        assert!(opaque_builder.is_opaque());
        assert!(!opaque_builder.is_translucent());
        assert_eq!(
            opaque_builder.to_text(),
            "window opacity: opaque, translucent false"
        );
        let opaque = opaque_builder.build_checked().unwrap();
        assert_eq!(opaque.fraction(), 1.0);
        assert!(opaque.is_opaque());
        assert_eq!(
            opaque.to_text(),
            "window opacity: opaque, translucent false"
        );

        assert!(WindowOpacityBuilder::fraction(-0.01).validate().is_err());
        assert!(WindowOpacityBuilder::fraction(1.01).validate().is_err());
        assert!(WindowOpacityBuilder::fraction(f32::NAN).validate().is_err());
        assert!(
            WindowOpacityBuilder::fraction(f32::INFINITY)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_z_order_policy_builder_validates_always_on_top_intent() {
        let always_on_top =
            WindowZOrderPolicyBuilder::always_on_top("Keep video call controls visible")
                .build_checked()
                .unwrap();
        assert!(always_on_top.always_on_top());
        assert_eq!(
            always_on_top.reason(),
            Some("Keep video call controls visible")
        );

        let normal = WindowZOrderPolicyBuilder::normal().build_checked().unwrap();
        assert!(!normal.always_on_top());
        assert_eq!(normal.reason(), None);

        assert!(
            WindowZOrderPolicyBuilder::always_on_top("")
                .validate()
                .is_err()
        );
        assert!(
            WindowZOrderPolicyBuilder::always_on_top(" Keep visible")
                .validate()
                .is_err()
        );
        assert!(
            WindowZOrderPolicyBuilder::always_on_top("Keep\nvisible")
                .validate()
                .is_err()
        );
        assert!(
            WindowZOrderPolicyBuilder::always_on_top("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_render_policy_builder_validates_frame_skip_intent() {
        let policy = WindowRenderPolicyBuilder::frame_skip("Static settings panel")
            .build_checked()
            .unwrap();
        assert!(policy.frame_skip_enabled());
        assert_eq!(policy.reason(), Some("Static settings panel"));

        let disabled = WindowRenderPolicyBuilder::no_frame_skip()
            .build_checked()
            .unwrap();
        assert!(!disabled.frame_skip_enabled());
        assert_eq!(disabled.reason(), None);

        assert!(
            WindowRenderPolicyBuilder::frame_skip("")
                .validate()
                .is_err()
        );
        assert!(
            WindowRenderPolicyBuilder::frame_skip(" Static panel")
                .validate()
                .is_err()
        );
        assert!(
            WindowRenderPolicyBuilder::frame_skip("Static\npanel")
                .validate()
                .is_err()
        );
        assert!(
            WindowRenderPolicyBuilder::frame_skip("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_tabbing_identifier_builder_validates_tab_group_id() {
        let identifier = WindowTabbingIdentifierBuilder::new("workspace.main");
        assert!(identifier.validate().is_ok());
        assert_eq!(identifier.identifier(), Some("workspace.main"));
        assert!(!identifier.is_clear());
        assert!(identifier.has_identifier());
        assert_eq!(identifier.len_bytes(), 14);
        assert_eq!(
            identifier.to_text(),
            "window tabbing identifier: clear false, identifier true, 14 bytes"
        );
        assert!(!identifier.to_text().contains("workspace"));
        assert_eq!(
            identifier.build_checked().unwrap(),
            Some("workspace.main".to_string())
        );

        let clear = WindowTabbingIdentifierBuilder::clear();
        assert!(clear.validate().is_ok());
        assert!(clear.is_clear());
        assert!(!clear.has_identifier());
        assert_eq!(clear.len_bytes(), 0);
        assert_eq!(
            clear.to_text(),
            "window tabbing identifier: clear true, identifier false, 0 bytes"
        );
        assert_eq!(clear.build_checked().unwrap(), None);

        assert!(WindowTabbingIdentifierBuilder::new("").validate().is_err());
        assert!(WindowTabbingIdentifierBuilder::new(" ").validate().is_err());
        assert!(
            WindowTabbingIdentifierBuilder::new(" workspace.main")
                .validate()
                .is_err()
        );
        assert!(
            WindowTabbingIdentifierBuilder::new("workspace.main ")
                .validate()
                .is_err()
        );
        assert!(
            WindowTabbingIdentifierBuilder::new("workspace main")
                .validate()
                .is_err()
        );
        assert!(
            WindowTabbingIdentifierBuilder::new("workspace\nmain")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_document_state_builder_derives_document_titles() {
        let builder =
            WindowDocumentStateBuilder::document("/Users/example/Report.md").unsaved_changes();
        assert!(!builder.has_title());
        assert!(builder.has_document_path());
        assert!(builder.is_edited());
        assert!(!builder.requires_existing_path());
        assert!(!builder.canonicalizes_path());
        assert_eq!(
            builder.to_text(),
            "document window state builder: title false, path true, edited true, require existing false, canonicalize false"
        );
        assert!(!builder.to_text().contains("Report.md"));
        let state = builder.build_checked().unwrap();

        assert_eq!(state.title(), Some("Report.md"));
        assert_eq!(
            state.document_path().unwrap(),
            std::path::Path::new("/Users/example/Report.md")
        );
        assert!(state.edited());
        assert!(state.has_title());
        assert!(state.has_document_path());
        assert_eq!(
            state.to_text(),
            "document window state: title true, path true, edited true"
        );
        assert!(!state.to_text().contains("Report.md"));

        let titled = WindowDocumentStateBuilder::new()
            .title("Project Notes")
            .clean()
            .build_checked()
            .unwrap();
        assert_eq!(titled.title(), Some("Project Notes"));
        assert_eq!(titled.document_path(), None);
        assert!(!titled.edited());
        assert!(titled.has_title());
        assert!(!titled.has_document_path());
        assert_eq!(
            titled.to_text(),
            "document window state: title true, path false, edited false"
        );
        assert!(!titled.to_text().contains("Project Notes"));
    }

    #[test]
    fn window_document_state_builder_rejects_invalid_generated_state() {
        assert!(WindowDocumentStateBuilder::new().validate().is_err());
        assert!(
            WindowDocumentStateBuilder::new()
                .title(" Project")
                .validate()
                .is_err()
        );
        assert!(WindowDocumentStateBuilder::document("").validate().is_err());
        assert!(
            WindowDocumentStateBuilder::document("/tmp/report\0.md")
                .validate()
                .is_err()
        );
        assert!(
            WindowDocumentStateBuilder::document("/tmp/kael-missing-document-state-file.md")
                .require_existing_path()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_document_state_builder_canonicalizes_existing_paths() {
        let state = WindowDocumentStateBuilder::document(std::env::temp_dir())
            .canonicalize_path()
            .build_checked()
            .unwrap();

        assert_eq!(
            state.title(),
            std::env::temp_dir()
                .file_name()
                .and_then(|name| name.to_str())
        );
        assert!(state.document_path().unwrap().is_absolute());
    }

    #[test]
    fn window_content_protection_builder_validates_capture_policy() {
        let protection =
            WindowContentProtectionBuilder::exclude_from_capture("Protect checkout secrets")
                .build_checked()
                .unwrap();

        assert_eq!(
            protection.mode(),
            WindowContentProtectionMode::ExcludeFromCapture
        );
        assert_eq!(protection.mode().key(), "exclude-from-capture");
        assert_eq!(protection.reason(), Some("Protect checkout secrets"));
        assert!(protection.is_protected());
        assert!(protection.blocks_app_window_capture());

        let obscure =
            WindowContentProtectionBuilder::obscure_when_captured("Hide unreleased designs")
                .block_app_window_capture(false)
                .build_checked()
                .unwrap();
        assert_eq!(
            obscure.mode(),
            WindowContentProtectionMode::ObscureWhenCaptured
        );
        assert_eq!(obscure.mode().key(), "obscure-when-captured");
        assert!(!obscure.blocks_app_window_capture());

        let disabled = WindowContentProtectionBuilder::disabled()
            .build_checked()
            .unwrap();
        assert_eq!(disabled.mode(), WindowContentProtectionMode::Disabled);
        assert_eq!(disabled.mode().key(), "disabled");
        assert!(!disabled.is_protected());
        assert!(!disabled.blocks_app_window_capture());
    }

    #[test]
    fn window_content_protection_builder_rejects_generated_footguns() {
        assert!(
            WindowContentProtectionBuilder::exclude_from_capture("")
                .validate()
                .is_err()
        );
        assert!(
            WindowContentProtectionBuilder::exclude_from_capture(" Protect secrets")
                .validate()
                .is_err()
        );
        assert!(
            WindowContentProtectionBuilder::obscure_when_captured("Protect\nsecrets")
                .validate()
                .is_err()
        );
        assert!(
            WindowContentProtectionBuilder::exclude_from_capture("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_presentation_policy_builder_validates_modes() {
        let fullscreen = WindowPresentationPolicyBuilder::fullscreen("Present onboarding")
            .build_checked()
            .unwrap();
        assert_eq!(fullscreen.mode(), WindowPresentationMode::Fullscreen);
        assert_eq!(fullscreen.mode().key(), "fullscreen");
        assert!(fullscreen.mode().wants_fullscreen());
        assert_eq!(fullscreen.reason(), Some("Present onboarding"));
        assert!(fullscreen.allows_user_exit());
        assert!(!fullscreen.hides_chrome());

        let kiosk = WindowPresentationPolicyBuilder::kiosk("Point of sale checkout")
            .build_checked()
            .unwrap();
        assert_eq!(kiosk.mode(), WindowPresentationMode::Kiosk);
        assert_eq!(kiosk.mode().key(), "kiosk");
        assert!(kiosk.mode().wants_fullscreen());
        assert!(!kiosk.allows_user_exit());
        assert!(kiosk.hides_chrome());

        let windowed = WindowPresentationPolicyBuilder::windowed()
            .build_checked()
            .unwrap();
        assert_eq!(windowed.mode(), WindowPresentationMode::Windowed);
        assert_eq!(windowed.mode().key(), "windowed");
        assert!(!windowed.mode().wants_fullscreen());
        assert!(windowed.allows_user_exit());
    }

    #[test]
    fn window_presentation_policy_builder_rejects_generated_footguns() {
        assert!(
            WindowPresentationPolicyBuilder::fullscreen("")
                .validate()
                .is_err()
        );
        assert!(
            WindowPresentationPolicyBuilder::fullscreen(" Present")
                .validate()
                .is_err()
        );
        assert!(
            WindowPresentationPolicyBuilder::kiosk("Line one\nLine two")
                .validate()
                .is_err()
        );
        assert!(
            WindowPresentationPolicyBuilder::kiosk("x".repeat(257))
                .validate()
                .is_err()
        );
        assert!(
            WindowPresentationPolicyBuilder::windowed()
                .allow_user_exit(false)
                .validate()
                .is_err()
        );
        assert!(
            WindowPresentationPolicyBuilder::windowed()
                .hide_chrome(true)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_interaction_command_validates_visibility_and_mouse_passthrough() {
        let show = WindowInteractionCommand::show().reason("Restore project window");
        assert!(show.validate().is_ok());
        assert_eq!(show.kind(), WindowInteractionCommandKind::Show);
        assert_eq!(show.reason_text(), Some("Restore project window"));

        let click_through =
            WindowInteractionCommand::mouse_passthrough("Heads-up overlay should not block clicks");
        assert!(click_through.validate().is_ok());
        assert_eq!(
            click_through.kind(),
            WindowInteractionCommandKind::MousePassthrough { enabled: true }
        );

        let receive_mouse = WindowInteractionCommand::receive_mouse_events();
        assert!(receive_mouse.validate().is_ok());
        assert_eq!(
            receive_mouse.kind(),
            WindowInteractionCommandKind::MousePassthrough { enabled: false }
        );

        assert!(WindowInteractionCommand::activate().validate().is_ok());
        assert!(WindowInteractionCommand::minimize().validate().is_ok());
        assert!(
            WindowInteractionCommand::zoom_window()
                .reason("Toolbar zoom button")
                .validate()
                .is_ok()
        );
        assert_eq!(
            WindowInteractionCommand::zoom_window().kind(),
            WindowInteractionCommandKind::ZoomWindow
        );
        assert!(WindowInteractionCommand::hide().validate().is_ok());
        assert!(
            WindowInteractionCommand::enter_fullscreen()
                .reason("Start presentation")
                .validate()
                .is_ok()
        );
        assert!(
            WindowInteractionCommand::exit_fullscreen()
                .reason("Presentation ended")
                .validate()
                .is_ok()
        );
        assert!(
            WindowInteractionCommand::toggle_fullscreen()
                .reason("Toolbar fullscreen button")
                .validate()
                .is_ok()
        );
        assert_eq!(
            WindowInteractionCommand::enter_fullscreen().kind(),
            WindowInteractionCommandKind::EnterFullscreen
        );
        assert_eq!(
            WindowInteractionCommand::exit_fullscreen().kind(),
            WindowInteractionCommandKind::ExitFullscreen
        );
        assert_eq!(
            WindowInteractionCommand::toggle_fullscreen().kind(),
            WindowInteractionCommandKind::ToggleFullscreen
        );
        let close = WindowInteractionCommand::close("User confirmed close");
        assert!(close.validate().is_ok());
        assert_eq!(close.kind(), WindowInteractionCommandKind::Close);
        assert_eq!(close.reason_text(), Some("User confirmed close"));
    }

    #[test]
    fn window_interaction_command_rejects_generated_footguns() {
        assert!(
            WindowInteractionCommand::mouse_passthrough("")
                .validate()
                .is_err()
        );
        assert!(
            WindowInteractionCommand::mouse_passthrough(" Overlay")
                .validate()
                .is_err()
        );
        assert!(
            WindowInteractionCommand::show()
                .reason("Restore\nwindow")
                .validate()
                .is_err()
        );
        assert!(
            WindowInteractionCommand::hide()
                .reason("x".repeat(257))
                .validate()
                .is_err()
        );
        assert!(WindowInteractionCommand::close("").validate().is_err());
        assert!(
            WindowInteractionCommand::close("Unsaved changes\nclose")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_cursor_style_command_validates_generated_window_cursor_overrides() {
        let command =
            WindowCursorStyleCommand::new(CursorStyle::Crosshair, "Canvas drawing tool active");
        assert!(command.validate().is_ok());
        assert_eq!(command.style(), CursorStyle::Crosshair);
        assert_eq!(command.reason_text(), "Canvas drawing tool active");
        assert!(command.has_reason());
        assert_eq!(
            command.to_text(),
            "window cursor: style Crosshair, reason true"
        );
        assert!(!command.to_text().contains("Canvas"));

        assert!(
            WindowCursorStyleCommand::new(CursorStyle::PointingHand, "")
                .validate()
                .is_err()
        );
        assert!(
            WindowCursorStyleCommand::new(CursorStyle::PointingHand, " Cursor")
                .validate()
                .is_err()
        );
        assert!(
            WindowCursorStyleCommand::new(CursorStyle::PointingHand, "Line one\nLine two")
                .validate()
                .is_err()
        );
        assert!(
            WindowCursorStyleCommand::new(CursorStyle::PointingHand, "x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_client_inset_builder_validates_custom_chrome_insets() {
        let builder = WindowClientInsetBuilder::new(px(12.0));
        assert!(!builder.is_zero());
        assert_eq!(builder.to_text(), "window client inset builder: zero false");
        assert!(!builder.to_text().contains("12"));
        let inset = builder.build_checked().unwrap();
        assert_eq!(inset.inset(), px(12.0));
        assert!(!inset.is_zero());
        assert_eq!(inset.to_text(), "window client inset: zero false");
        let zero = WindowClientInsetBuilder::new(px(0.0))
            .build_checked()
            .unwrap();
        assert!(zero.is_zero());
        assert_eq!(zero.to_text(), "window client inset: zero true");
        assert!(
            WindowClientInsetBuilder::new(px(f32::NAN))
                .validate()
                .is_err()
        );
        assert!(
            WindowClientInsetBuilder::new(px(f32::INFINITY))
                .validate()
                .is_err()
        );
        assert!(WindowClientInsetBuilder::new(px(-1.0)).validate().is_err());
        assert!(
            WindowClientInsetBuilder::new(px(1024.0))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_rem_size_builder_validates_generated_ui_scale() {
        let builder = WindowRemSizeBuilder::new(px(18.0));
        assert_eq!(builder.size_class(), "standard");
        assert_eq!(builder.to_text(), "window rem size builder: class standard");
        assert!(!builder.to_text().contains("18"));
        let rem_size = builder.build_checked().unwrap();
        assert_eq!(rem_size.rem_size(), px(18.0));
        assert_eq!(rem_size.size_class(), "standard");
        assert_eq!(rem_size.to_text(), "window rem size: class standard");
        assert_eq!(WindowRemSizeBuilder::new(px(8.0)).size_class(), "compact");
        assert_eq!(WindowRemSizeBuilder::new(px(32.0)).size_class(), "large");
        assert!(WindowRemSizeBuilder::new(px(4.0)).validate().is_ok());
        assert!(WindowRemSizeBuilder::new(px(128.0)).validate().is_ok());
        assert!(WindowRemSizeBuilder::new(px(f32::NAN)).validate().is_err());
        assert!(
            WindowRemSizeBuilder::new(px(f32::INFINITY))
                .validate()
                .is_err()
        );
        assert!(WindowRemSizeBuilder::new(px(0.0)).validate().is_err());
        assert!(WindowRemSizeBuilder::new(px(3.0)).validate().is_err());
        assert!(WindowRemSizeBuilder::new(px(129.0)).validate().is_err());
    }

    #[test]
    fn window_autoscroll_request_builder_validates_generated_bounds() {
        let bounds = Bounds::new(point(px(10.0), px(20.0)), size(px(30.0), px(40.0)));
        let builder = WindowAutoscrollRequestBuilder::new(bounds);
        assert!(!builder.is_empty());
        assert_eq!(builder.to_text(), "window autoscroll builder: empty false");
        assert!(!builder.to_text().contains("10"));
        let request = builder.build_checked().unwrap();
        assert_eq!(request.bounds(), bounds);
        assert!(!request.is_empty());
        assert_eq!(request.to_text(), "window autoscroll: empty false");
        assert!(!request.to_text().contains("40"));
        assert!(
            WindowAutoscrollRequestBuilder::new(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(0.0), px(0.0)),
            ))
            .validate()
            .is_ok()
        );
        assert!(
            WindowAutoscrollRequestBuilder::new(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(0.0), px(0.0)),
            ))
            .build_checked()
            .unwrap()
            .is_empty()
        );
        assert!(
            WindowAutoscrollRequestBuilder::new(Bounds::new(
                point(px(f32::NAN), px(0.0)),
                size(px(1.0), px(1.0)),
            ))
            .validate()
            .is_err()
        );
        assert!(
            WindowAutoscrollRequestBuilder::new(Bounds::new(
                point(px(0.0), px(f32::INFINITY)),
                size(px(1.0), px(1.0)),
            ))
            .validate()
            .is_err()
        );
        assert!(
            WindowAutoscrollRequestBuilder::new(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(-1.0), px(1.0)),
            ))
            .validate()
            .is_err()
        );
        assert!(
            WindowAutoscrollRequestBuilder::new(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(40000.0), px(1.0)),
            ))
            .validate()
            .is_err()
        );
    }

    #[test]
    fn window_system_ui_command_validates_native_commands() {
        let palette =
            WindowSystemUiCommand::show_character_palette().reason("Editor symbol picker");
        assert!(palette.validate().is_ok());
        assert_eq!(
            palette.kind(),
            WindowSystemUiCommandKind::ShowCharacterPalette
        );
        assert_eq!(palette.reason_text(), Some("Editor symbol picker"));

        let double_click = WindowSystemUiCommand::titlebar_double_click();
        assert!(double_click.validate().is_ok());
        assert_eq!(
            double_click.kind(),
            WindowSystemUiCommandKind::TitlebarDoubleClick
        );

        let zoom = WindowSystemUiCommand::zoom_window();
        assert!(zoom.validate().is_ok());
        assert_eq!(zoom.kind(), WindowSystemUiCommandKind::ZoomWindow);
    }

    #[test]
    fn window_system_ui_command_rejects_generated_footguns() {
        assert!(
            WindowSystemUiCommand::show_character_palette()
                .reason("")
                .validate()
                .is_err()
        );
        assert!(
            WindowSystemUiCommand::titlebar_double_click()
                .reason(" Titlebar")
                .validate()
                .is_err()
        );
        assert!(
            WindowSystemUiCommand::zoom_window()
                .reason("Zoom\nwindow")
                .validate()
                .is_err()
        );
        assert!(
            WindowSystemUiCommand::zoom_window()
                .reason("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_tab_command_validates_native_tab_commands() {
        let merge = WindowTabCommand::merge_all_windows().reason("Collect project windows");
        assert!(merge.validate().is_ok());
        assert_eq!(merge.kind(), WindowTabCommandKind::MergeAllWindows);
        assert_eq!(merge.reason_text(), Some("Collect project windows"));

        let detach = WindowTabCommand::move_tab_to_new_window();
        assert!(detach.validate().is_ok());
        assert_eq!(detach.kind(), WindowTabCommandKind::MoveTabToNewWindow);

        let overview = WindowTabCommand::toggle_tab_overview();
        assert!(overview.validate().is_ok());
        assert_eq!(overview.kind(), WindowTabCommandKind::ToggleTabOverview);
    }

    #[test]
    fn window_tab_command_rejects_generated_footguns() {
        assert!(
            WindowTabCommand::merge_all_windows()
                .reason("")
                .validate()
                .is_err()
        );
        assert!(
            WindowTabCommand::move_tab_to_new_window()
                .reason(" Detach tab")
                .validate()
                .is_err()
        );
        assert!(
            WindowTabCommand::toggle_tab_overview()
                .reason("Toggle\noverview")
                .validate()
                .is_err()
        );
        assert!(
            WindowTabCommand::toggle_tab_overview()
                .reason("x".repeat(257))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_chrome_command_validates_custom_titlebar_commands() {
        let decorations = WindowChromeCommand::request_decorations(WindowDecorations::Client)
            .reason("Custom titlebar owns drag regions");
        assert!(decorations.validate().is_ok());
        assert_eq!(
            decorations.kind(),
            &WindowChromeCommandKind::RequestDecorations(WindowDecorations::Client)
        );
        assert_eq!(decorations.key(), "request-decorations");
        assert!(decorations.has_reason());
        assert_eq!(
            decorations.to_text(),
            "window chrome command: kind request-decorations, reason true, position false, resize-edge false, client-decorations true, server-decorations false"
        );
        assert!(!decorations.to_text().contains("Custom titlebar"));
        assert!(decorations.kind().requests_client_decorations());
        assert!(!decorations.kind().requests_server_decorations());
        assert_eq!(
            decorations.reason_text(),
            Some("Custom titlebar owns drag regions")
        );

        let menu = WindowChromeCommand::show_window_menu(point(px(10.0), px(20.0)));
        assert!(menu.validate().is_ok());
        assert_eq!(
            menu.kind(),
            &WindowChromeCommandKind::ShowWindowMenu(point(px(10.0), px(20.0)))
        );
        assert_eq!(menu.key(), "show-window-menu");
        assert!(menu.kind().has_position());
        assert_eq!(
            menu.to_text(),
            "window chrome command: kind show-window-menu, reason false, position true, resize-edge false, client-decorations false, server-decorations false"
        );
        assert!(!menu.to_text().contains("10"));

        let resize = WindowChromeCommand::start_resize(ResizeEdge::BottomRight);
        assert!(resize.validate().is_ok());
        assert_eq!(
            resize.kind(),
            &WindowChromeCommandKind::StartResize(ResizeEdge::BottomRight)
        );
        assert_eq!(resize.key(), "start-resize");
        assert!(resize.kind().has_resize_edge());
        assert_eq!(
            resize.to_text(),
            "window chrome command: kind start-resize, reason false, position false, resize-edge true, client-decorations false, server-decorations false"
        );

        let move_command = WindowChromeCommand::start_move();
        assert!(move_command.validate().is_ok());
        assert_eq!(move_command.key(), "start-move");
    }

    #[test]
    fn window_chrome_command_rejects_generated_footguns() {
        assert!(
            WindowChromeCommand::show_window_menu(point(px(f32::NAN), px(0.0)))
                .validate()
                .is_err()
        );
        assert!(
            WindowChromeCommand::show_window_menu(point(px(0.0), px(f32::INFINITY)))
                .validate()
                .is_err()
        );
        assert!(
            WindowChromeCommand::start_move()
                .reason(" Drag")
                .validate()
                .is_err()
        );
        assert!(
            WindowChromeCommand::request_decorations(WindowDecorations::Server)
                .reason("Line one\nLine two")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn window_atlas_budget_builder_validates_memory_caps() {
        let budget = WindowAtlasBudgetBuilder::bytes(128 * 1024 * 1024)
            .reason("Large text editor churns glyphs")
            .build_checked()
            .unwrap();
        assert_eq!(budget.max_bytes(), Some(128 * 1024 * 1024));
        assert_eq!(
            budget.reason_text(),
            Some("Large text editor churns glyphs")
        );
        assert!(!budget.is_clear());

        let cleared = WindowAtlasBudgetBuilder::clear().build_checked().unwrap();
        assert_eq!(cleared.max_bytes(), None);
        assert!(cleared.is_clear());
    }

    #[test]
    fn window_atlas_budget_builder_rejects_generated_footguns() {
        assert!(WindowAtlasBudgetBuilder::bytes(0).validate().is_err());
        assert!(
            WindowAtlasBudgetBuilder::bytes(9 * 1024 * 1024 * 1024)
                .validate()
                .is_err()
        );
        assert!(
            WindowAtlasBudgetBuilder::bytes(64 * 1024 * 1024)
                .reason(" generated")
                .validate()
                .is_err()
        );
        assert!(
            WindowAtlasBudgetBuilder::clear()
                .reason("Line one\nLine two")
                .validate()
                .is_err()
        );
    }
}
/// Position a tooltip relative to its anchor bounds, flipping to the
/// opposite side when the window does not have room and clamping the final
/// bounds into the window.
fn anchored_tooltip_bounds(
    anchor: &TooltipAnchor,
    tooltip_size: Size<Pixels>,
    window_bounds: Bounds<Pixels>,
) -> Bounds<Pixels> {
    const GAP: Pixels = px(4.);

    let side = match anchor.side {
        TooltipSide::Top
            if anchor.bounds.top() - GAP - tooltip_size.height < window_bounds.top() =>
        {
            TooltipSide::Bottom
        }
        TooltipSide::Bottom
            if anchor.bounds.bottom() + GAP + tooltip_size.height > window_bounds.bottom() =>
        {
            TooltipSide::Top
        }
        TooltipSide::Left
            if anchor.bounds.left() - GAP - tooltip_size.width < window_bounds.left() =>
        {
            TooltipSide::Right
        }
        TooltipSide::Right
            if anchor.bounds.right() + GAP + tooltip_size.width > window_bounds.right() =>
        {
            TooltipSide::Left
        }
        side => side,
    };

    let origin = match side {
        TooltipSide::Top => point(
            aligned_tooltip_x(anchor.align, anchor.bounds, tooltip_size),
            anchor.bounds.top() - GAP - tooltip_size.height,
        ),
        TooltipSide::Bottom => point(
            aligned_tooltip_x(anchor.align, anchor.bounds, tooltip_size),
            anchor.bounds.bottom() + GAP,
        ),
        TooltipSide::Left => point(
            anchor.bounds.left() - GAP - tooltip_size.width,
            aligned_tooltip_y(anchor.align, anchor.bounds, tooltip_size),
        ),
        TooltipSide::Right => point(
            anchor.bounds.right() + GAP,
            aligned_tooltip_y(anchor.align, anchor.bounds, tooltip_size),
        ),
    };

    let max_x = (window_bounds.right() - tooltip_size.width).max(window_bounds.left());
    let max_y = (window_bounds.bottom() - tooltip_size.height).max(window_bounds.top());
    Bounds::new(
        point(
            origin.x.clamp(window_bounds.left(), max_x),
            origin.y.clamp(window_bounds.top(), max_y),
        ),
        tooltip_size,
    )
}

fn aligned_tooltip_x(
    align: TooltipAlign,
    anchor_bounds: Bounds<Pixels>,
    tooltip_size: Size<Pixels>,
) -> Pixels {
    match align {
        TooltipAlign::Start => anchor_bounds.left(),
        TooltipAlign::Center => anchor_bounds.center().x - tooltip_size.width / 2.,
        TooltipAlign::End => anchor_bounds.right() - tooltip_size.width,
    }
}

fn aligned_tooltip_y(
    align: TooltipAlign,
    anchor_bounds: Bounds<Pixels>,
    tooltip_size: Size<Pixels>,
) -> Pixels {
    match align {
        TooltipAlign::Start => anchor_bounds.top(),
        TooltipAlign::Center => anchor_bounds.center().y - tooltip_size.height / 2.,
        TooltipAlign::End => anchor_bounds.bottom() - tooltip_size.height,
    }
}
